---
title: "doxus 채팅에서 Serena MCP 서버 자동 실행 차단"
aliases:
  - doxus-serena-mcp-block
  - serena-plugin-block
  - doxus-chat-serena
  - doxus-serena-mcp-차단
  - serena-plugin-차단
  - doxus-채팅-serena
tags:
  - devlog
  - troubleshooting
  - doxus
  - serena
  - mcp
  - agent-sdk
created: "2026-04-05"
updated: "2026-04-06"
---

<!-- docsmith: auto-generated 2026-04-05 -->

# doxus 채팅에서 Serena MCP 서버 자동 실행 차단

## 배경

doxus 데스크톱 앱은 Node.js sidecar(`apps/desktop/src-tauri/sidecar/agent-bridge.mjs`)로 AI 채팅을 처리한다. sidecar는 `@anthropic-ai/claude-agent-sdk`를 사용해 claude CLI를 실행하는데, 매 채팅 세션마다 Serena MCP 서버가 자동으로 시작되고 브라우저에서 대시보드(`http://127.0.0.1:24287/dashboard/`)가 열리는 문제가 발생했다.

원인은 전역 `~/.claude/settings.json`에 `enabledPlugins: { "serena@claude-plugins-official": true }` 설정이 있었기 때문이다. Serena는 프로젝트 수준 MCP 서버가 아닌 **전역 Claude Code 플러그인**으로 등록되어 있어 모든 claude CLI 세션에서 자동으로 로드된다.

## 시도한 접근법과 실패 원인

### 1. CLAUDE_CONFIG_DIR 격리 (실패)

빈 `mcpServers`를 가진 임시 디렉토리를 생성하고 `sessions/` symlink를 만드는 방식을 시도했다. 실패 원인은 인증 파일(`~/.claude.json`)의 auth token이 Keychain에 있으며, `CLAUDE_CONFIG_DIR` 변경 시 "Not logged in" 에러가 발생한다. `~/.claude.json`을 복사하거나 symlink해도 동일하게 실패한다.

### 2. --disallowed-tools 플래그 (실패)

Serena의 모든 도구명(`mcp__plugin_serena_serena__*`)을 명시적으로 차단했다. 도구 호출 자체는 막을 수 있지만 MCP 서버 초기화(서버 프로세스 시작) 자체를 막지 못한다. 대시보드 자동 오픈은 서버 시작 시점에 발생하므로 효과가 없었다.

### 3. CWD 격리 (실패)

Agent SDK의 `cwd` 옵션으로 `/tmp` 지정, Node.js 프로세스의 CWD를 `/tmp`로 변경, Rust spawn 시 `.current_dir(temp_dir())` 지정 등을 시도했다. Serena는 `.serena/` 프로젝트 탐지 기반이 아닌 `enabledPlugins` 전역 플러그인으로 등록된 방식이라 CWD와 무관하게 모든 claude 세션에서 로드된다.

### 4. canUseTool 콜백 (부분 성공)

Agent SDK의 `canUseTool`로 `docnx_*`와 기본 읽기 도구 외 모두 차단했다. 도구 호출은 차단되지만 Serena MCP 서버 자체는 여전히 시작되어 대시보드 오픈이 발생한다.

## 변경 내용

### 핵심 발견

Serena 로드 메커니즘이 전역 플러그인 등록 방식임을 확인했다.

```json
// ~/.claude/settings.json
{
  "enabledPlugins": {
    "serena@claude-plugins-official": true
  }
}
```

대시보드 자동 오픈은 Serena 설정에서 기인한다.

```yaml
# ~/.serena/serena_config.yml
web_dashboard_open_on_launch: true
```

### 최종 해결책

Agent SDK의 `settings` 옵션을 사용해 세션별로만 Serena를 비활성화한다. 이 옵션은 claude CLI의 `--settings` 플래그로 전달되어 전역 설정을 변경하지 않고 해당 세션에만 적용된다.

```js
// apps/desktop/src-tauri/sidecar/adapters/claude.mjs
return {
  model,
  settings: { enabledPlugins: { "serena@claude-plugins-official": false } },
  canUseTool: async (toolName, _input, opts) => {
    const allowed =
      toolName.startsWith("docnx_") ||
      ["Read", "LS", "Glob", "Grep", "WebSearch", "WebFetch"].includes(toolName);
    if (allowed) return { behavior: "allow", updatedPermissions: opts.suggestions };
    return { behavior: "deny", message: `Tool '${toolName}' is not permitted in doxus chat.` };
  },
};
```

### 주요 변경사항

- `apps/desktop/src-tauri/sidecar/adapters/claude.mjs` — Agent SDK `settings` 옵션 추가로 세션별 Serena 비활성화
- `apps/desktop/src-tauri/sidecar/agent-bridge.mjs` — `process.chdir(tmpdir())` 추가 (CWD 격리 보조)
- `apps/desktop/src/stores/useChatStore.ts` — 스트리밍 누적 로직 수정

### 영향 범위

- doxus 데스크톱 앱의 채팅 기능에만 적용
- 전역 `~/.claude/settings.json` 변경 없음 — 사용자의 다른 Claude Code 세션에 영향 없음
- `canUseTool`을 통한 2중 차단으로 허용되지 않은 도구 호출도 차단

## 결과

Agent SDK `settings: { enabledPlugins: { "serena@claude-plugins-official": false } }` 적용 후 doxus 채팅 세션에서 Serena MCP 서버가 더 이상 시작되지 않는다. 대시보드 자동 오픈 문제가 해소된다.

## 교훈

- Claude Code 전역 플러그인(`enabledPlugins`)은 CWD나 `--disallowed-tools`로는 로드 자체를 막을 수 없다. 반드시 `settings` 옵션으로 세션 수준에서 비활성화해야 한다.
- MCP 서버의 부작용(대시보드 오픈, 외부 프로세스 시작 등)은 도구 차단이 아니라 서버 로드 차단으로만 막을 수 있다.
- `CLAUDE_CONFIG_DIR` 격리는 인증(Keychain 연동) 때문에 실용적이지 않다. Agent SDK의 `settings` 옵션이 올바른 세션별 격리 방법이다.

## 관련 문서

- [[doxus-agent-chat-ipc-indexing]]
- [[doxus-tdd-security-hardening]]

---

## 2026-04-06 추가 작업

### 1. doxus_ 도구 prefix 통일

기존 `docnx_` prefix로 되어있던 MCP 도구 이름을 모두 `doxus_`로 rename했다.

- 파일: `crates/mcp-server/src/main.rs`
- `canUseTool` 콜백도 `doxus_`로 업데이트

### 2. 마크다운 렌더링 개선

`react-markdown` + `remark-gfm` + `rehype-highlight` 조합으로 채팅 메시지 렌더링을 개선했다.

- Tailwind CSS `[&_selector]` 패턴으로 테이블, 코드블록, 헤딩 스타일링
- `@tailwindcss/typography` 플러그인 추가 (`apps/desktop/src/index.css`)
- 파일: `apps/desktop/src/components/layout/ChatDrawer.tsx`

### 3. canUseTool MCP 도구명 형식 수정

핵심 발견: Claude Agent SDK는 MCP 도구를 `mcp__<서버>__<도구>` 형식으로 전달한다.

- 기존: `toolName.startsWith("doxus_")` — MCP 도구(`mcp__doxus__doxus_search`)를 차단했음
- 수정: `toolName.includes("__doxus_")` 조건 추가
- 파일: `apps/desktop/src-tauri/sidecar/adapters/claude.mjs`

### 4. Serena 재출현 버그 수정

`settings` 옵션이 실수로 제거되어 Serena가 다시 로드되는 문제가 재발했다. `settings` 블록을 복구했다.

```js
settings: { enabledPlugins: { "serena@claude-plugins-official": false } }
```

### 5. 시스템 프롬프트 개선

`~/.doxus/agents/librarian/system.md` 및 `crates/agent/resources/librarian/system.md`를 업데이트했다.

- `nexus_*` 도구 사용 금지 명시 (obsidian-nexus와 혼동 방지)
- `doxus_*` 도구 테이블 명시
