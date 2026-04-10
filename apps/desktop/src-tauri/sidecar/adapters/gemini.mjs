/**
 * Gemini CLI Adapter for doxus
 */

import { spawn } from "child_process";
import { dirname } from "path";
import { writeFileSync, unlinkSync, mkdtempSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";

const log = (...args) => process.stderr.write(`[gemini] ${args.join(" ")}\n`);

const QUERY_TIMEOUT_MS = 5 * 60 * 1000;

export class GeminiAdapter {
  async loadSDK() {
    log("Gemini adapter ready");
  }

  buildOptions(req) {
    return {
      cliPath: req.cliPath,
      model: req.model,
      systemPrompt: req.systemPrompt || "",
    };
  }

  async query(sessionId, prompt, entry, emit) {
    const { cliPath, model, systemPrompt } = entry.options;

    let systemMdDir = null;
    let systemMdPath = null;
    if (systemPrompt) {
      systemMdDir = mkdtempSync(join(tmpdir(), "doxus-gemini-"));
      systemMdPath = join(systemMdDir, "system.md");
      writeFileSync(systemMdPath, systemPrompt, { encoding: "utf8", mode: 0o600 });
    }

    const cleanup = () => {
      if (systemMdPath) {
        try { unlinkSync(systemMdPath); } catch (e) { log("temp file cleanup failed:", e.message); }
        systemMdPath = null;
      }
    };

    const args = ["-p", prompt, "--output-format", "stream-json", "--approval-mode", "yolo"];
    if (model) args.push("-m", model);

    const cliDir = dirname(cliPath);
    const enrichedPath = cliDir + ":" + (process.env.PATH || "");

    const env = {
      ...process.env,
      PATH: enrichedPath,
      ...(systemMdPath ? { GEMINI_SYSTEM_MD: systemMdPath } : {}),
    };

    const child = spawn(cliPath, args, { stdio: ["ignore", "pipe", "pipe"], env });
    entry.abort = { abort: () => child.kill("SIGTERM") };

    let fullText = "";
    let lineBuffer = "";

    child.stdout.on("data", (chunk) => {
      lineBuffer += chunk.toString();
      const lines = lineBuffer.split("\n");
      lineBuffer = lines.pop();

      for (const line of lines) {
        const trimmed = line.trim();
        if (!trimmed) continue;
        let event;
        try { event = JSON.parse(trimmed); } catch {
          fullText += trimmed + "\n";
          emit({ type: "text", sessionId, content: trimmed + "\n", done: false });
          continue;
        }
        if (event.type === "message" && (event.role === "model" || event.role === "assistant")) {
          const text = event.content ?? "";
          fullText += text;
          emit({ type: "text", sessionId, content: text, done: false });
        }
      }
    });

    child.stderr.on("data", (chunk) => log(`stderr: ${chunk.toString().trim()}`));

    await new Promise((resolve) => {
      let settled = false;
      const timer = setTimeout(() => {
        if (!settled) { log("query timeout"); child.kill("SIGTERM"); }
      }, QUERY_TIMEOUT_MS);

      const finish = (emitFn) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        entry.abort = null;
        cleanup();
        emitFn();
        resolve();
      };

      child.on("close", (code) => {
        finish(() => {
          if (code === 0 || code === null) {
            emit({ type: "result", sessionId, content: fullText });
          } else {
            emit({ type: "error", sessionId, code: "execution_error", message: `gemini exited with code ${code}`, retryable: true });
          }
        });
      });

      child.on("error", (err) => {
        finish(() => emit({ type: "error", sessionId, code: "spawn_error", message: `Gemini CLI 실행 실패: ${err.message}`, retryable: false }));
      });
    });
  }
}
