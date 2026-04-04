#!/usr/bin/env node
/**
 * doxus Agent Sidecar — JSONL stdio bridge
 *
 * Protocol (matches Rust protocol.rs):
 *   stdin:  HostMessage  { type: 'start'|'message'|'cancel'|'close', ... }
 *   stdout: AgentMessage { type: 'init'|'thought'|'text'|'result'|'error'|'cancelled', ... }
 *   stderr: logs only (never protocol messages)
 */

'use strict';

const readline = require('readline');

// ── Logging (stderr only) ──────────────────────────────────────────────────
function log(msg) {
  process.stderr.write(`[sidecar] ${msg}\n`);
}

// ── Send AgentMessage to Rust host ─────────────────────────────────────────
function send(msg) {
  process.stdout.write(JSON.stringify(msg) + '\n');
}

// ── Session state ──────────────────────────────────────────────────────────
let currentSessionId = null;

// ── Handle a single HostMessage ────────────────────────────────────────────
async function handleMessage(msg) {
  switch (msg.type) {
    case 'start': {
      currentSessionId = msg.session_id;
      log(`Session started: ${currentSessionId}`);
      // Emit text and result for the initial prompt (stub — replace with real
      // Claude API call when @anthropic-ai/sdk is wired in).
      send({ type: 'thought', content: `Processing prompt: ${msg.prompt}` });
      send({ type: 'text', content: `Received prompt: ${msg.prompt}` });
      send({
        type: 'result',
        content: `Session ${currentSessionId} initialized. Ready to assist.`,
      });
      break;
    }

    case 'message': {
      log(`User message: ${msg.content}`);
      send({ type: 'thought', content: 'Thinking...' });
      send({ type: 'text', content: `Echo: ${msg.content}` });
      send({ type: 'result', content: msg.content });
      break;
    }

    case 'cancel': {
      log('Cancelled by host');
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
// Matches AgentMessage::Init { model } in protocol.rs.
send({ type: 'init', model: 'stub' });
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
