/**
 * Claude Agent SDK Adapter for doxus
 * obsidian-nexus와 동일한 구현 방식
 */

import { tmpdir } from "os";

const log = (...args) => process.stderr.write(`[claude] ${args.join(" ")}\n`);

export class ClaudeAdapter {
  #sdk = null;

  async loadSDK() {
    try {
      this.#sdk = await import("@anthropic-ai/claude-agent-sdk");
      log("Claude Agent SDK loaded");
    } catch (err) {
      log("Failed to load Claude Agent SDK:", err.message);
      throw err;
    }
  }

  buildOptions(req) {
    const { model, systemPrompt, mcpServers, cliPath } = req;
    log("mcpServers received:", JSON.stringify(mcpServers));

    // GUI 앱(Tauri)은 shell PATH를 상속하지 않음 → claude binary 디렉토리를 PATH에 추가
    if (cliPath && cliPath.includes("/")) {
      const parentDir = cliPath.substring(0, cliPath.lastIndexOf("/"));
      const current = process.env.PATH || "";
      if (parentDir && !current.split(":").includes(parentDir)) {
        process.env.PATH = `${parentDir}:${current}`;
        log(`PATH enriched with: ${parentDir}`);
      }
    }

    const mcpServersConfig = JSON.parse(JSON.stringify(mcpServers || {}));
    if (mcpServersConfig.doxus && req.bridgeToken) {
      if (!mcpServersConfig.doxus.env) mcpServersConfig.doxus.env = {};
      mcpServersConfig.doxus.env.DOXUS_BRIDGE_TOKEN = req.bridgeToken;
      log("Injected DOXUS_BRIDGE_TOKEN for doxus mcp");
    }

    return {
      model,
      systemPrompt,
      mcpServers: mcpServersConfig,
      permissionMode: "bypassPermissions",
      allowDangerouslySkipPermissions: true,
      pathToClaudeCodeExecutable: cliPath,
      settings: {
        enabledPlugins: { "serena@claude-plugins-official": false },
        mcpServers: mcpServersConfig
      },
      // Serena 등 허가되지 않은 도구 차단
      canUseTool: async (toolName, _input, opts) => {
        // doxus_* 또는 mcp__doxus__doxus_* 형태 허용
        const allowed =
          toolName.startsWith("doxus_") ||
          toolName.includes("__doxus_") ||
          ["Read", "LS", "Glob", "Grep", "WebSearch", "WebFetch"].includes(toolName);
        if (allowed) {
          log(`Allowed tool: ${toolName}`);
          return { behavior: "allow", updatedPermissions: opts.suggestions };
        }
        log(`Blocked tool: ${toolName}`);
        return { behavior: "deny", message: `Tool '${toolName}' is not permitted in doxus chat.` };
      },
    };
  }

  async query(sessionId, prompt, entry, emit) {
    const abort = new AbortController();
    entry.abort = abort;

    const queryOpts = {
      ...entry.options,
      abortController: abort,
    };

    if (entry.sdkSessionId) {
      queryOpts.resume = entry.sdkSessionId;
      log(`Resuming session: ${entry.sdkSessionId}`);
    }

    log(`Sending message to session ${sessionId} (${entry.sdkSessionId ? "resume" : "new"})`);

    try {
      for await (const msg of this.#sdk.query({ prompt, options: queryOpts })) {
        if (msg.type === "system" && msg.subtype === "init" && msg.session_id) {
          entry.sdkSessionId = msg.session_id;
          log(`SDK session ID captured: ${msg.session_id}`);
          log(`Init tools: ${JSON.stringify(msg.tools?.map(t => t.name ?? t) ?? [])}`);
          log(`Init mcp_servers: ${JSON.stringify(msg.mcp_servers ?? [])}`);
        }
        this.#processMessage(sessionId, msg, emit);
      }
    } catch (err) {
      if (err.name === "AbortError") {
        log(`Session ${sessionId} cancelled`);
        emit({ type: "cancelled", sessionId });
        return;
      }
      emit({
        type: "error",
        sessionId,
        code: "execution_error",
        message: err.message,
        retryable: true,
      });
    } finally {
      entry.abort = null;
    }
  }

  #processMessage(sessionId, msg, emit) {
    switch (msg.type) {
      case "assistant": {
        if (!msg.message?.content) break;
        for (const block of msg.message.content) {
          if (block.type === "text" && block.text) {
            emit({ type: "text", sessionId, content: block.text, done: false });
          } else if (block.type === "thinking" && block.thinking) {
            emit({ type: "thought", sessionId, content: block.thinking });
          } else if (block.type === "tool_use") {
            emit({
              type: "tool_use",
              sessionId,
              toolName: block.name,
              input: block.input,
              status: "running",
            });
          } else if (block.type === "tool_result") {
            emit({ type: "tool_use", sessionId, toolName: block.tool_use_id || "unknown", status: "done" });
          }
        }
        break;
      }
      case "result": {
        emit({
          type: "result",
          sessionId,
          content: msg.result || "",
          cost: msg.total_cost_usd,
          duration: msg.duration_ms,
        });
        break;
      }
      default:
        break;
    }
  }
}
