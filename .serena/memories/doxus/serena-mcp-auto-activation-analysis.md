# doxus × Serena MCP 자동 활성화 원인 분석

## 핵심 원인

**Root Cause**: Claude Agent SDK의 `enableAllProjectMcpServers: true` 전역 설정 + doxus의 sidecar 아키텍처 차이

### 문제 메커니즘

```
doxus chat_start_session
  → sidecar.send_request({ type: "start", mcpServers: { "doxus": {...} } })
    → sidecar.js: SDK 로드 (@anthropic-ai/claude-agent-sdk)
      → SDK 내부: ~/.claude/settings.json 읽음
        → enableAllProjectMcpServers: true 발견
          → ~/.claude/projects/ 스캔 → doxus 발견
            → /Users/madup/gorillaProject/doxus/.serena/mcp.json 로드
              → Serena MCP 서버 초기화 시작 🚀
```

### 왜 `canUseTool`로 차단 안 되는가?

1. **도구 호출**: ✓ 차단됨 (canUseTool 작동)
2. **MCP 서버 초기화**: ❌ 차단 안 됨 (SDK 내부에서 이미 로드됨)

---

## obsidian-nexus vs doxus 비교

### obsidian-nexus: 정상 작동

**파일**: `/Users/madup/gorillaProject/obsidian-nexus/apps/desktop/sidecar/adapters/claude.mjs`

```javascript
export class ClaudeAdapter {
  buildOptions(req) {
    return {
      ...
      canUseTool: async (toolName) => {
        const allowed = 
          toolName.startsWith("nexus_") ||
          ["Read", "LS", "Glob", "Grep", "WebSearch", "WebFetch"].includes(toolName);
        if (allowed) return { behavior: "allow", ... };
        return { behavior: "deny", ... };
      },
      ...
    };
  }

  async query(sessionId, prompt, entry, emit) {
    for await (const msg of this.#sdk.query({ prompt, options: entry.options })) {
      // ...
    }
  }
}
```

**Key Points**:
- Adapter 패턴으로 SDK를 감싼다
- `buildOptions`에서 canUseTool을 명시적으로 정의
- Serena 도구(activate_project, find_symbol 등)는 모두 거부

### doxus: 문제 있음

**파일**: `/Users/madup/gorillaProject/doxus/apps/desktop/src-tauri/src/commands/agent.rs`

```rust
#[tauri::command]
pub async fn chat_start_session(...) -> Result<(), String> {
    let mcp_servers = find_doxus_mcp()
        .map(|p| serde_json::json!({ "doxus": { "command": p.to_string_lossy() } }))
        .unwrap_or(serde_json::json!({}));

    let start_req = serde_json::json!({
        "type": "start",
        "mcpServers": mcp_servers,
        ...
    });

    state.sidecar.send_request(&start_req)  // ← Rust에서 전달
}
```

**파일**: `/Users/madup/gorillaProject/doxus/crates/agent/sidecar/sidecar.js`

```javascript
// sidecar.js는 CLI를 직접 스폰
async function spawnCli(message, session) {
  const proc = spawn(CLI.path, args, {
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  // SDK 없음, canUseTool 제어 없음
}
```

**문제**:
- sidecar.js가 SDK를 사용하지 않는다 (단순 CLI 스폰)
- Rust에서 START 메시지에 mcpServers를 포함
- SDK가 전역 settings.json의 enableAllProjectMcpServers를 읽음
- 결과: 불필요한 MCP가 로드됨

---

## 파일 경로 정리

### obsidian-nexus (정상)
- `/Users/madup/gorillaProject/obsidian-nexus/apps/desktop/src-tauri/src/main.rs` - chat_start_session
- `/Users/madup/gorillaProject/obsidian-nexus/apps/desktop/sidecar/adapters/claude.mjs` - canUseTool 정의

### doxus (문제)
- `/Users/madup/gorillaProject/doxus/apps/desktop/src-tauri/src/commands/agent.rs` - chat_start_session
- `/Users/madup/gorillaProject/doxus/crates/agent/sidecar/sidecar.js` - SDK 없음
- `~/.claude/settings.json` - enableAllProjectMcpServers: true

---

## 수정 방법

### 방법 1: 빠른 해결 (전역 설정 변경)

```bash
# ~/.claude/settings.json에서
{
  "mcp": {
    "enableAllProjectMcpServers": false
  }
}
```

### 방법 2: 근본 해결 (obsidian-nexus 패턴)

doxus의 sidecar를 Adapter 패턴으로 변경:

1. `crates/agent/sidecar/adapters/claude.mjs` 생성
2. ClaudeAdapter 구현 (canUseTool로 docnx_* 만 허용)
3. `crates/agent/src/sidecar.rs`에서 adapter 호출
4. SDK를 adapter 내부에서만 사용
