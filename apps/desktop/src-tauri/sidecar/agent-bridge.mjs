#!/usr/bin/env node

/**
 * doxus Agent Bridge — Multi-agent sidecar
 * stdin/stdout JSONL 프로토콜로 Tauri 앱과 통신
 * stdout = JSONL 프로토콜 전용 / stderr = 디버그 로그
 */

import { createInterface } from "readline";
import { rmSync } from "fs";
import { tmpdir } from "os";
import { ClaudeAdapter } from "./adapters/claude.mjs";
import { GeminiAdapter } from "./adapters/gemini.mjs";

const log = (...args) => process.stderr.write(`[bridge] ${args.join(" ")}\n`);

// .serena/ 등 프로젝트 MCP 서버 자동 탐지 차단
// Agent SDK가 process.cwd() 기준으로 프로젝트를 탐지하므로 /tmp로 변경
process.chdir(tmpdir());
log(`CWD set to: ${process.cwd()}`);

function emit(response) {
  process.stdout.write(JSON.stringify(response) + "\n");
}

const adapters = {
  claude: new ClaudeAdapter(),
  gemini: new GeminiAdapter(),
};

/** Active sessions: Map<sessionId, { adapter, options, sdkSessionId?, abort? }> */
const sessions = new Map();

async function handleStart(req) {
  const { sessionId, cliType } = req;

  const adapter = adapters[cliType];
  if (!adapter) {
    emit({
      type: "error",
      sessionId,
      code: "unknown_cli_type",
      message: `Unknown CLI type: '${cliType}'. Supported: ${Object.keys(adapters).join(", ")}`,
      retryable: false,
    });
    return;
  }

  log(`Registering session ${sessionId} (cliType: ${cliType}, model: ${req.model})`);

  const options = adapter.buildOptions(req);
  sessions.set(sessionId, { adapter, options, sdkSessionId: null, abort: null });

  emit({
    type: "init",
    sessionId,
    model: req.model,
    mcpServers: Object.keys(req.mcpServers || {}),
  });

  log(`Session ${sessionId} registered`);
}

async function handleMessage(req) {
  const { sessionId, content } = req;
  const entry = sessions.get(sessionId);

  if (!entry) {
    emit({
      type: "error",
      sessionId,
      code: "session_not_found",
      message: `Session ${sessionId} not found. Send a "start" request first.`,
      retryable: false,
    });
    return;
  }

  await entry.adapter.query(sessionId, content, entry, emit);
}

function handleCancel(req) {
  const { sessionId } = req;
  const entry = sessions.get(sessionId);
  if (entry?.abort) {
    entry.abort.abort();
    log(`Cancelled session ${sessionId}`);
  }
}

function handleClose(req) {
  const { sessionId } = req;
  const entry = sessions.get(sessionId);
  if (entry) {
    if (entry.abort) entry.abort.abort();
    // 세션 단위 격리 config 디렉토리 정리
    const configDir = entry.options?.configDir;
    if (configDir) {
      try { rmSync(configDir, { recursive: true, force: true }); } catch {}
    }
    sessions.delete(sessionId);
    log(`Closed session ${sessionId}`);
  }
}

async function main() {
  await Promise.all(
    Object.entries(adapters).map(async ([name, adapter]) => {
      try {
        await adapter.loadSDK();
      } catch (err) {
        log(`Warning: ${name} adapter SDK load failed: ${err.message}`);
      }
    })
  );

  const rl = createInterface({ input: process.stdin });

  rl.on("line", async (line) => {
    const trimmed = line.trim();
    if (!trimmed) return;

    let req;
    try {
      req = JSON.parse(trimmed);
    } catch {
      log(`Invalid JSON: ${trimmed}`);
      return;
    }

    switch (req.type) {
      case "start":   await handleStart(req);   break;
      case "message": await handleMessage(req); break;
      case "cancel":  handleCancel(req);         break;
      case "close":   handleClose(req);          break;
      default: log(`Unknown request type: ${req.type}`);
    }
  });

  rl.on("close", () => {
    log("stdin closed, shutting down");
    for (const [, entry] of sessions) {
      if (entry.abort) entry.abort.abort();
    }
    sessions.clear();
    process.exit(0);
  });

  log("doxus agent bridge ready");
}

main().catch((err) => {
  log(`Fatal error: ${err.message}`);
  process.exit(1);
});
