'use strict';

const { spawn } = require('child_process');
const path = require('path');
const assert = require('assert');
const { test } = require('node:test');

const SIDECAR = path.join(__dirname, 'sidecar.js');

/**
 * Spawn the sidecar, send `messages` one-by-one (50 ms apart), then send
 * `close` and collect all AgentMessage objects from stdout.
 *
 * @param {object[]} messages  HostMessages to send (excluding the final close)
 * @param {number}   [timeout=3000]  hard kill timeout in ms
 * @returns {Promise<object[]>}  parsed AgentMessages
 */
function runSidecar(messages, timeout = 3000) {
  return new Promise((resolve, reject) => {
    const proc = spawn('node', [SIDECAR], {
      stdio: ['pipe', 'pipe', 'pipe'],
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

// ── Tests ──────────────────────────────────────────────────────────────────

test('sidecar sends init on startup', async () => {
  const outputs = await runSidecar([]);
  const init = outputs.find((m) => m.type === 'init');
  assert.ok(init, 'should receive init message');
  // AgentMessage::Init { model } — no tools field in the Rust protocol
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
    const proc = spawn('node', [SIDECAR], { stdio: ['pipe', 'pipe', 'pipe'] });
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
