---
title: "Agent Chat IPC + 실제 인덱싱 파이프라인 구현"
aliases:
  - agent-chat-ipc-indexing-2026-04-04
  - 에이전트 채팅 IPC 인덱싱 구현
tags:
  - devlog
  - feature
  - tauri
  - rust
  - tdd
  - doxus
created: "2026-04-04"
updated: "2026-04-04"
---

<!-- docsmith: auto-generated 2026-04-04 -->

# Agent Chat IPC + 실제 인덱싱 파이프라인 구현

## 배경

ChatDrawer 컴포넌트가 에이전트에게 메시지를 보내는 기능이 `setTimeout` echo stub으로 막혀 있었고, `trigger_reindex` 커맨드도 항상 `"status": "ok"`만 반환하는 stub 상태였다. Phase 3 에이전트 sidecar 연결 전에 Rust에서 Claude/Gemini API를 직접 호출하는 방식으로 채팅을 실용화하고, 실제 vault 파일을 읽어 DB에 색인하는 파이프라인도 완성했다.

## 변경 내용

### 주요 변경사항

#### 1. Agent Chat IPC (`apps/desktop/src-tauri/src/commands/agent.rs` 신규)

`agent_send_message` 커맨드는 `provider` 파라미터로 Claude/Gemini를 구분하여 각 API를 직접 호출한다.

- Claude: `x-api-key` 헤더, `ANTHROPIC_API_KEY` 환경변수
- Gemini: `x-goog-api-key` 헤더, `GEMINI_API_KEY` 환경변수 (URL query string 노출 방식 제거)
- `reqwest::Client`에 30초 timeout 적용
- model 파라미터는 `CLAUDE_MODELS` / `GEMINI_MODELS` allowlist로 검증하여 URL path injection 방지

`agent_status` 커맨드는 환경변수 감지 결과를 `ok` / `warn`으로 반환한다.

#### 2. Frontend ChatDrawer 연결

`useChatStore.ts`에 `sendMessage(content)` 액션 추가 — `invoke('agent_send_message', ...)` 호출, `isLoading` 상태 관리, 에러 시 assistant 역할로 에러 메시지 표시.

`ChatDrawer.tsx`에서 echo stub(`setTimeout`) 완전 제거 → `await sendMessage(content)` 호출, submit 버튼 `disabled={isLoading}` 처리.

#### 3. 실제 인덱싱 파이프라인 (`apps/desktop/src-tauri/src/commands/search.rs` 수정)

`trigger_reindex` stub을 `run_reindex(&conn)` 실제 구현으로 교체:

1. `projects` 테이블에서 `status = 'active'` 프로젝트 조회
2. 각 프로젝트에 `ObsidianPlugin` 초기화 + `fetch_all` 페이지네이션으로 문서 수집
3. `SearchEngine::index_document` 호출로 FTS5 + sqlite-vec 색인

`Cargo.toml`에 `doxus-plugin-sdk`, `doxus-plugin-obsidian`, `tokio`, `tempfile`(dev) 의존성 추가.

#### 4. 보안 수정 (리뷰 지적사항)

| 항목 | Before | After |
|------|--------|-------|
| Gemini API key 전달 | URL query string `?key=...` | `x-goog-api-key` 헤더 |
| model 파라미터 검증 | 없음 | allowlist 검증 |
| HTTP timeout | 없음 | 30초 |

#### 5. 병렬 테스트 환경변수 경쟁 해결

`agent_status` 테스트에서 `set_var` / `remove_var` 병렬 실행 충돌 발생 → `static ENV_LOCK: Mutex<()>` 직렬화로 해결.

### 영향 범위

- `apps/desktop/src-tauri/src/commands/agent.rs` (신규)
- `apps/desktop/src-tauri/src/commands/mod.rs` (`pub mod agent;` 추가)
- `apps/desktop/src-tauri/src/main.rs` (invoke_handler에 2개 커맨드 등록)
- `apps/desktop/src-tauri/src/commands/search.rs` (stub → 실제 구현)
- `apps/desktop/src/stores/useChatStore.ts`
- `apps/desktop/src/components/layout/ChatDrawer.tsx`

## 결과

- 테스트: 239개 → 251개 (신규 12개, 전체 통과)
  - agent 단위 테스트 7개 (payload 직렬화, 응답 역직렬화, API key 감지, model 검증)
  - 인덱싱 통합 테스트 1개 (임시 vault → DB 색인 확인)
  - 보안 검증 테스트 2개 (unknown model 거부, known model 수락)
- ChatDrawer가 실제 Claude/Gemini API와 연결됨
- `trigger_reindex`가 active 프로젝트의 vault를 실제로 읽어 DB에 색인함
- 커밋 2개:
  - `87218fd` — `feat(desktop): agent chat IPC — Claude/Gemini API integration (TDD)`
  - `b83bc00` — `feat(desktop): real indexing pipeline — trigger_reindex connects Obsidian vault (TDD)`

## 교훈

- **Node.js sidecar는 나중에**: AgentManager가 sidecar를 관리하는 설계지만, 채팅 기능은 Rust에서 직접 API를 호출하는 것이 단순하고 테스트하기 쉽다. sidecar 연결은 별도 Phase에서 진행.
- **API key는 항상 헤더로**: query string에 secret을 넣으면 서버 로그, 브라우저 히스토리, 프록시에 노출된다. Gemini도 `x-goog-api-key` 헤더를 지원한다.
- **model 파라미터는 반드시 검증**: frontend에서 넘어오는 값을 그대로 URL path에 삽입하면 path injection이 가능하다. allowlist가 가장 단순하고 확실한 방어.
- **환경변수 테스트는 직렬화 필수**: Rust의 `std::env::set_var`는 프로세스 전역에 영향을 주므로 병렬 테스트에서 반드시 Mutex로 직렬화해야 한다.

## 관련 문서

- [[module-map]]
- [[data-flow]]
- [[getting-started]]
