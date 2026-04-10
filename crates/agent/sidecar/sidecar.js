#!/usr/bin/env node
/**
 * doxus Agent Sidecar — JSONL stdio bridge
 *
 * Uses the locally installed Claude CLI or Gemini CLI — no API key required.
 * Falls back to Anthropic SDK (ANTHROPIC_API_KEY) if no CLI is detected.
 *
 * Protocol (matches Rust protocol.rs):
 *   stdin:  HostMessage  { type: 'start'|'message'|'cancel'|'close', ... }
 *   stdout: AgentMessage { type: 'init'|'thought'|'text'|'result'|'error'|'cancelled', ... }
 *   stderr: logs only (never protocol messages)
 *
 * Env vars (set by Rust SidecarManager before spawning):
 *   DOXUS_CLI_KIND  — 'claude' | 'gemini'
 *   DOXUS_CLI_PATH  — absolute path to the CLI binary
 */

import { spawn } from 'child_process';
import readline from 'readline';
import { statSync } from 'fs';

// ── Logging (stderr only) ──────────────────────────────────────────────────
function log(msg) {
  process.stderr.write(`[sidecar] ${msg}\n`);
}

// ── Send AgentMessage to Rust host ─────────────────────────────────────────
function send(msg) {
  process.stdout.write(JSON.stringify(msg) + '\n');
}

// ── CLI detection ──────────────────────────────────────────────────────────
function findBinarySync(name) {
  const PATH = process.env.PATH || '';
  for (const dir of PATH.split(':')) {
    try {
      const p = `${dir}/${name}`;
      statSync(p);
      return p;
    } catch { /* not found in this dir */ }
  }
  return null;
}

const CLI = (() => {
  // Prefer env vars set by Rust host (most explicit)
  const kind = process.env.DOXUS_CLI_KIND;
  const path = process.env.DOXUS_CLI_PATH;
  if (kind && path) return { kind, path };

  // Auto-detect from PATH
  const claudePath = findBinarySync('claude');
  if (claudePath) return { kind: 'claude', path: claudePath };
  const geminiPath = findBinarySync('gemini');
  if (geminiPath) return { kind: 'gemini', path: geminiPath };
  return null;
})();

// SDK fallback when no CLI found but API key is set
const API_KEY = process.env.ANTHROPIC_API_KEY;
const USE_SDK_FALLBACK = !CLI && !!API_KEY;

log(`CLI: ${CLI ? `${CLI.kind} at ${CLI.path}` : 'not found'}`);
log(`SDK fallback: ${USE_SDK_FALLBACK}`);

// ── Session state ──────────────────────────────────────────────────────────
const sessions = new Map();
let currentSessionId = null;

// ── Spawn CLI and stream output ────────────────────────────────────────────
async function spawnCli(message, session) {
  return new Promise((resolve) => {
    const args = CLI.kind === 'claude'
      ? ['-p', message, '--output-format', 'text']
      : [message]; // gemini CLI interface

    send({ type: 'thought', content: `Calling ${CLI.kind} CLI...` });
    log(`Spawning: ${CLI.path} ${args.join(' ')}`);

    const proc = spawn(CLI.path, args, {
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    let fullText = '';

    proc.stdout.on('data', (chunk) => {
      if (session.cancelled) return;
      const text = chunk.toString();
      fullText += text;
      send({ type: 'text', content: text });
    });

    proc.stderr.on('data', (data) => {
      log(`CLI stderr: ${data.toString().trim()}`);
    });

    proc.on('close', (code) => {
      if (session.cancelled) {
        send({ type: 'cancelled' });
        resolve();
        return;
      }
      if (code === 0) {
        const trimmed = fullText.trim();
        session.messages.push({ role: 'assistant', content: trimmed });
        send({ type: 'result', content: trimmed });
      } else {
        send({ type: 'error', message: `${CLI.kind} CLI exited with code ${code}` });
      }
      resolve();
    });

    proc.on('error', (err) => {
      send({ type: 'error', message: `CLI spawn error: ${err.message}` });
      resolve();
    });

    if (session.cancelled) {
      proc.kill('SIGTERM');
    }
  });
}

// ── SDK fallback (requires ANTHROPIC_API_KEY) ─────────────────────────────
async function sdkReply(sessionId, userMessage) {
  const session = sessions.get(sessionId);
  if (!session) return;
  try {
    const { default: Anthropic } = await import('@anthropic-ai/sdk');
    const client = new Anthropic({ apiKey: API_KEY });

    session.messages.push({ role: 'user', content: userMessage });
    send({ type: 'thought', content: 'Calling Claude API (SDK fallback)...' });

    const stream = client.messages.stream({
      model: 'claude-haiku-4-5-20251001',
      max_tokens: 2048,
      system: 'You are a helpful document search assistant for the doxus application.',
      messages: session.messages.slice(),
    });

    let fullText = '';
    for await (const chunk of stream) {
      if (session.cancelled) { stream.abort(); send({ type: 'cancelled' }); return; }
      if (chunk.type === 'content_block_delta' && chunk.delta.type === 'text_delta') {
        send({ type: 'text', content: chunk.delta.text });
        fullText += chunk.delta.text;
      }
    }

    session.messages.push({ role: 'assistant', content: fullText });
    send({ type: 'result', content: fullText });
  } catch (err) {
    send({ type: 'error', message: `SDK error: ${err.message}` });
  }
}

// ── Dispatch reply ─────────────────────────────────────────────────────────
async function reply(sessionId, message) {
  const session = sessions.get(sessionId);
  if (!session) { send({ type: 'error', message: 'No active session' }); return; }

  if (CLI) {
    session.messages.push({ role: 'user', content: message });
    await spawnCli(message, session);
  } else if (USE_SDK_FALLBACK) {
    await sdkReply(sessionId, message);
  } else {
    send({
      type: 'error',
      message:
        'No AI CLI found. Install Claude Code (https://claude.ai/code) or Gemini CLI.',
    });
  }
}

// ── Handle a single HostMessage ────────────────────────────────────────────
async function handleMessage(msg) {
  switch (msg.type) {
    case 'start': {
      currentSessionId = msg.session_id;
      sessions.set(currentSessionId, { messages: [], cancelled: false });
      log(`Session started: ${currentSessionId}`);
      await reply(currentSessionId, msg.prompt);
      break;
    }

    case 'message': {
      if (!currentSessionId || !sessions.has(currentSessionId)) {
        send({ type: 'error', message: 'No active session. Send a start message first.' });
        return;
      }
      await reply(currentSessionId, msg.content);
      break;
    }

    case 'cancel': {
      log('Cancelled by host');
      if (currentSessionId && sessions.has(currentSessionId)) {
        sessions.get(currentSessionId).cancelled = true;
      }
      send({ type: 'cancelled' });
      currentSessionId = null;
      break;
    }

    case 'close': {
      log('Session closed by host');
      currentSessionId = null;
      process.exit(0);
      break;
    }

    default:
      log(`Unknown message type: ${msg.type}`);
      send({ type: 'error', message: `unknown message type: ${msg.type}` });
  }
}

// ── JSONL stdio loop ───────────────────────────────────────────────────────
const rl = readline.createInterface({ input: process.stdin, terminal: false });

const modelName = CLI ? `${CLI.kind}-cli` : USE_SDK_FALLBACK ? 'claude-sdk-fallback' : 'none';
send({ type: 'init', model: modelName });
log(`doxus sidecar ready (mode: ${modelName})`);

rl.on('line', (line) => {
  line = line.trim();
  if (!line) return;
  try {
    const msg = JSON.parse(line);
    handleMessage(msg).catch((err) => {
      log(`Error handling message: ${err.message}`);
      send({ type: 'error', message: err.message });
    });
  } catch (err) {
    log(`JSON parse error: ${err.message}`);
    send({ type: 'error', message: `invalid JSON: ${err.message}` });
  }
});

rl.on('close', () => { log('stdin closed, exiting'); process.exit(0); });
process.on('SIGTERM', () => { log('SIGTERM received'); process.exit(0); });
