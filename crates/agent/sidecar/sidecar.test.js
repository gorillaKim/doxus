'use strict';

import { spawn } from 'child_process';
import path from 'path';
import assert from 'assert';
import { test } from 'node:test';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const SIDECAR = path.join(__dirname, 'sidecar.js');

/**
 * Spawn the sidecar without ANTHROPIC_API_KEY so it runs in echo mode.
 * Sends `messages` one-by-one (50 ms apart), then sends `close` and
 * collects all AgentMessage objects from stdout.
 *
 * @param {object[]} messages  HostMessages to send (excluding the final close)
 * @param {number}   [timeout=3000]  hard kill timeout in ms
 * @returns {Promise<object[]>}  parsed AgentMessages
 */
function runSidecar(messages, timeout = 3000) {
  return new Promise((resolve, reject) => {
    const proc = spawn('node', [SIDECAR], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env, ANTHROPIC_API_KEY: '' }, // force echo mode
    });

    const outputs = [];
    proc.stdout.on('data', (d) => {
      d.toString()
        .split('\n')
        .filter(Boolean)
        .forEach((line) => {
          try {
            outputs.push(JSON.parse(line));
          } catch {}
        });
    });
    proc.stderr.on('data', () => {}); // discard log lines

    let i = 0;
    function sendNext() {
      if (i >= messages.length) {
        proc.stdin.write(JSON.stringify({ type: 'close' }) + '\n');
        return;
      }
      proc.stdin.write(JSON.stringify(messages[i++]) + '\n');
      setTimeout(sendNext, 50);
    }
    setTimeout(sendNext, 100);

    const timer = setTimeout(() => {
      proc.kill();
      resolve(outputs);
    }, timeout);

    proc.on('close', () => {
      clearTimeout(timer);
      resolve(outputs);
    });
    proc.on('error', reject);
  });
}

// ── Original 5 tests (echo mode) ──────────────────────────────────────────

test('sidecar sends init on startup', async () => {
  const outputs = await runSidecar([]);
  const init = outputs.find((m) => m.type === 'init');
  assert.ok(init, 'should receive init message');
  assert.strictEqual(typeof init.model, 'string', 'init.model should be a string');
});

test('sidecar handles start message', async () => {
  const outputs = await runSidecar([
    { type: 'start', session_id: 'test-123', prompt: 'hello' },
  ]);
  const result = outputs.find((m) => m.type === 'result');
  assert.ok(result, 'should receive result');
  assert.strictEqual(typeof result.content, 'string', 'result.content should be a string');
});

test('sidecar handles cancel message', async () => {
  const outputs = await runSidecar([
    { type: 'start', session_id: 'test-456', prompt: 'hi' },
    { type: 'cancel' },
  ]);
  const cancelled = outputs.find((m) => m.type === 'cancelled');
  assert.ok(cancelled, 'should receive cancelled');
});

test('sidecar handles message after start', async () => {
  const outputs = await runSidecar([
    { type: 'start', session_id: 'test-789', prompt: 'init' },
    { type: 'message', content: 'ping' },
  ]);
  const results = outputs.filter((m) => m.type === 'result');
  assert.ok(results.length >= 1, 'should receive at least one result');
});

test('sidecar returns error for invalid JSON', async () => {
  return new Promise((resolve) => {
    const proc = spawn('node', [SIDECAR], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env, ANTHROPIC_API_KEY: '' },
    });
    const outputs = [];
    proc.stdout.on('data', (d) => {
      d.toString()
        .split('\n')
        .filter(Boolean)
        .forEach((line) => {
          try {
            outputs.push(JSON.parse(line));
          } catch {}
        });
    });
    proc.stderr.on('data', () => {});

    setTimeout(() => {
      proc.stdin.write('not valid json\n');
      setTimeout(() => {
        proc.kill();
        const err = outputs.find((m) => m.type === 'error');
        assert.ok(err, 'should receive error for invalid JSON');
        assert.ok(
          err.message.includes('invalid JSON'),
          'error message should describe the problem',
        );
        resolve();
      }, 200);
    }, 100);
  });
});

// ── New 4 tests ────────────────────────────────────────────────────────────

test('falls back to echo when ANTHROPIC_API_KEY not set', async () => {
  const outputs = await runSidecar([
    { type: 'start', session_id: 'echo-1', prompt: 'hello world' },
  ]);

  // Should emit an error message advertising echo mode
  const echoError = outputs.find(
    (m) => m.type === 'error' && m.message && m.message.includes('echo mode'),
  );
  assert.ok(echoError, 'should emit error noting echo mode');

  // Should still produce a result with echoed content
  const result = outputs.find((m) => m.type === 'result');
  assert.ok(result, 'should receive a result in echo mode');
  assert.ok(
    result.content.includes('hello world'),
    'echo result should contain the original prompt',
  );
});

test('start initializes empty session history', async () => {
  // In echo mode the session map is populated on 'start'.
  // We verify indirectly: a subsequent 'message' after 'start' succeeds
  // (no "No active session" error), which means the session was created.
  const outputs = await runSidecar([
    { type: 'start', session_id: 'hist-1', prompt: 'init' },
    { type: 'message', content: 'follow-up' },
  ]);

  const noSessionError = outputs.find(
    (m) => m.type === 'error' && m.message && m.message.includes('No active session'),
  );
  assert.ok(!noSessionError, 'should not get "No active session" error after start');

  const results = outputs.filter((m) => m.type === 'result');
  assert.ok(results.length >= 2, 'should receive results for both start and message');
});

test('message appends to session history', async () => {
  // Send two messages and verify both produce results (history maintained).
  const outputs = await runSidecar([
    { type: 'start', session_id: 'hist-2', prompt: 'first' },
    { type: 'message', content: 'second' },
    { type: 'message', content: 'third' },
  ]);

  const results = outputs.filter((m) => m.type === 'result');
  assert.ok(results.length >= 3, 'should receive a result for each message turn');
});

test('cancel stops processing', async () => {
  const outputs = await runSidecar([
    { type: 'start', session_id: 'cancel-1', prompt: 'start' },
    { type: 'cancel' },
  ]);

  const cancelled = outputs.find((m) => m.type === 'cancelled');
  assert.ok(cancelled, 'should receive cancelled after cancel message');

  // No further result/text messages should arrive after cancelled
  const cancelIdx = outputs.indexOf(cancelled);
  const afterCancel = outputs.slice(cancelIdx + 1).filter(
    (m) => m.type === 'result' || m.type === 'text',
  );
  assert.strictEqual(afterCancel.length, 0, 'no result/text messages should follow cancelled');
});
