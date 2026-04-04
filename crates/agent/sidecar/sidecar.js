#!/usr/bin/env node
/**
 * doxus Agent Sidecar — JSONL stdio bridge
 *
 * Protocol (matches Rust protocol.rs):
 *   stdin:  HostMessage  { type: 'start'|'message'|'cancel'|'close', ... }
 *   stdout: AgentMessage { type: 'init'|'thought'|'text'|'result'|'error'|'cancelled', ... }
 *   stderr: logs only (never protocol messages)
 */

import readline from 'readline';
import Anthropic from '@anthropic-ai/sdk';

// ── Logging (stderr only) ──────────────────────────────────────────────────
function log(msg) {
  process.stderr.write(`[sidecar] ${msg}\n`);
}

// ── Send AgentMessage to Rust host ─────────────────────────────────────────
function send(msg) {
  process.stdout.write(JSON.stringify(msg) + '\n');
}

// ── API key check ──────────────────────────────────────────────────────────
const API_KEY = process.env.ANTHROPIC_API_KEY;
const USE_ECHO = !API_KEY;

let client = null;
if (!USE_ECHO) {
  client = new Anthropic({ apiKey: API_KEY });
}

// ── Session state ──────────────────────────────────────────────────────────
// sessions: Map<session_id, { messages: Array, cancelled: boolean }>
const sessions = new Map();
let currentSessionId = null;

// ── Echo fallback ──────────────────────────────────────────────────────────
function echoReply(text) {
  send({ type: 'thought', content: 'Processing (echo mode)...' });
  send({ type: 'text', content: `Echo: ${text}` });
  send({ type: 'result', content: `Echo: ${text}` });
}

// ── Claude streaming call ──────────────────────────────────────────────────
async function claudeReply(sessionId, userMessage, systemPrompt) {
  const session = sessions.get(sessionId);
  if (!session) return;

  session.messages.push({ role: 'user', content: userMessage });

  send({ type: 'thought', content: 'Calling Claude API...' });

  const requestMessages = session.messages.slice();

  let stream;
  try {
    stream = client.messages.stream({
      model: 'claude-haiku-4-5',
      max_tokens: 2048,
      system: systemPrompt,
      messages: requestMessages,
    });
  } catch (err) {
    send({ type: 'error', message: `API error: ${err.message}` });
    return;
  }

  let fullText = '';
  try {
    for await (const chunk of stream) {
      if (session.cancelled) {
        stream.abort();
        send({ type: 'cancelled' });
        return;
      }
      if (
        chunk.type === 'content_block_delta' &&
        chunk.delta.type === 'text_delta'
      ) {
        send({ type: 'text', content: chunk.delta.text });
        fullText += chunk.delta.text;
      }
    }
  } catch (err) {
    if (!session.cancelled) {
      send({ type: 'error', message: `Stream error: ${err.message}` });
      return;
    }
    send({ type: 'cancelled' });
    return;
  }

  const finalMsg = await stream.finalMessage();
  const finalText =
    finalMsg.content.find((b) => b.type === 'text')?.text ?? fullText;

  session.messages.push({ role: 'assistant', content: finalText });
  send({ type: 'result', content: finalText });
}

// ── Handle a single HostMessage ────────────────────────────────────────────
async function handleMessage(msg) {
  switch (msg.type) {
    case 'start': {
      currentSessionId = msg.session_id;
      sessions.set(currentSessionId, { messages: [], cancelled: false });
      log(`Session started: ${currentSessionId}`);

      if (USE_ECHO) {
        send({
          type: 'error',
          message: 'ANTHROPIC_API_KEY not set, using echo mode',
        });
        echoReply(msg.prompt);
      } else {
        await claudeReply(
          currentSessionId,
          msg.prompt,
          'You are a helpful document search assistant for the doxus application.',
        );
      }
      break;
    }

    case 'message': {
      log(`User message: ${msg.content}`);

      if (!currentSessionId || !sessions.has(currentSessionId)) {
        send({ type: 'error', message: 'No active session' });
        return;
      }

      if (USE_ECHO) {
        echoReply(msg.content);
      } else {
        await claudeReply(
          currentSessionId,
          msg.content,
          'You are a helpful document search assistant for the doxus application.',
        );
      }
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
      log('Session closed');
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
const rl = readline.createInterface({
  input: process.stdin,
  terminal: false,
});

// Send init immediately so the Rust host knows we are ready.
const modelName = USE_ECHO ? 'echo' : 'claude-haiku-4-5';
send({ type: 'init', model: modelName });
log('doxus sidecar ready');

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

rl.on('close', () => {
  log('stdin closed, exiting');
  process.exit(0);
});

process.on('SIGTERM', () => {
  log('SIGTERM received');
  process.exit(0);
});
