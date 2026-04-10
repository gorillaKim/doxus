---
title: "macOS 서명된 앱 미서명 바이너리 SIGKILL — doxus-mcp 등록 패턴"
aliases:
  - doxus-mcp-codesign
  - doxus-mcp-macos-sigkill
  - doxus-mcp-settings-json
tags:
  - troubleshooting
  - devlog
  - doxus
  - mcp
  - macos
  - codesign
created: "2026-04-06"
updated: "2026-04-06"
---

<!-- docsmith: auto-generated 2026-04-06 -->

## 문제 상황

doxus 데스크톱 앱(Tauri, 서명됨)에서 `doxus-mcp` 바이너리를 Claude Agent SDK를 통해 실행하려 했다. MCP 상태가 항상 "pending"으로 표시되고 도구가 연결되지 않았다. `mcp__doxus__doxus_search` 같은 도구가 호출 불가 상태였다.

## 근본 원인

macOS Gatekeeper/Security 정책: **서명된 앱이 미서명 바이너리를 서브프로세스로 실행하면 SIGKILL로 즉시 종료**된다.

- Claude CLI(서명됨)가 `doxus-mcp`(dev 빌드, 미서명)를 MCP 서버로 spawning 시도
- macOS가 SIGKILL 발동 → MCP 연결 불가 → 도구 "pending" 상태 지속
- `target/debug/doxus-mcp`는 `cargo build`로만 빌드되어 ad-hoc 서명 없음

이 실패는 로그 없이 무음으로 발생한다. 프로세스 자체가 기록을 남기기 전에 종료되므로 stderr에 아무것도 찍히지 않는다.

## 해결책 1: Ad-hoc 코드서명 (개발 중)

```bash
codesign --sign - --force --preserve-metadata=entitlements target/debug/doxus-mcp
```

| 플래그 | 설명 |
|--------|------|
| `--sign -` | ad-hoc identity (Apple 개발자 계정 불필요) |
| `--force` | 기존 서명 덮어씀 |
| `--preserve-metadata=entitlements` | 기존 entitlements 유지 |

빌드할 때마다 재실행이 필요하다. `cargo build` 후 자동 실행되도록 빌드 스크립트 또는 Makefile 타겟에 연동하는 것을 권장한다.

## 해결책 2: ~/.claude/settings.json MCP 등록 (권장)

obsidian-nexus에서 발견한 패턴이다. Claude CLI가 시작할 때 `~/.claude/settings.json`의 `mcpServers`를 읽어 MCP 서버를 로드한다.

```json
{
  "mcpServers": {
    "doxus": {
      "command": "/Users/madup/gorillaProject/doxus/target/debug/doxus-mcp",
      "args": [],
      "type": "stdio"
    }
  }
}
```

Agent SDK의 세션별 `mcpServers` 옵션보다 이 방식이 더 안정적이다. Claude CLI 자체가 MCP 서버를 관리하므로 서명 문제를 우회할 수 있다.

## 향후 개선 방향

`apps/desktop/src-tauri/src/commands/agent.rs`의 `chat_start_session` 커맨드에서 앱 시작 시 `~/.claude/settings.json`에 자동 등록하는 `register_mcp_server()` 함수를 구현한다.

```rust
// 구현 예정 위치: apps/desktop/src-tauri/src/commands/agent.rs
fn register_mcp_server(mcp_bin_path: &Path) -> Result<(), String> {
    // ~/.claude/settings.json 읽기
    // mcpServers.doxus 항목 upsert
    // 저장
}
```

- 릴리즈 번들: 번들 디렉토리의 `doxus-mcp` 경로로 등록
- 개발 모드: `target/debug/doxus-mcp`로 등록 (빌드 후 codesign 스크립트와 연동)

`find_doxus_mcp()` 함수는 이미 dev 모드 폴백 로직을 포함하고 있으므로 (`apps/desktop/src-tauri/src/commands/agent.rs`) 이를 확장하면 된다.

## 교훈

- macOS에서 서명된 앱이 미서명 바이너리를 실행하면 **무음으로 SIGKILL** — 로그 없이 실패
- `doxus-mcp`가 "pending" 상태로 멈추면 먼저 codesign 여부를 확인할 것
- obsidian-nexus 소스 주석에서 이 문제 발견: "macOS SIGKILL them when spawned as subprocesses by a signed app"
- SDK의 `mcpServers` 옵션보다 `~/.claude/settings.json` 등록이 더 신뢰성 높음

## 관련 파일

- `apps/desktop/src-tauri/sidecar/adapters/claude.mjs` — `canUseTool`에서 `mcp__doxus__doxus_*` 허용 패턴
- `apps/desktop/src-tauri/src/commands/agent.rs` — `find_doxus_mcp()` dev 모드 폴백
- `~/.claude/settings.json` — MCP 서버 전역 등록

## 관련 문서

- [[2026-04-04-doxus-agent-chat-ipc-indexing]]
- [[2026-04-04-tdd-security-hardening]]
