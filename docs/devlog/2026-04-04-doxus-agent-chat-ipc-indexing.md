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
  - streaming
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

---

## 세션 2: CLI 감지 & 채팅 UX 개선

### 작업 배경

데스크톱 앱에서 claude CLI가 설치되어 있음에도 "No AI CLI found" 오류 발생. macOS GUI 앱(Tauri)은 Finder에서 실행 시 사용자의 shell PATH를 상속받지 않아 `~/.local/bin/claude`, `~/.nvm/*/bin/gemini` 등을 찾지 못하는 문제.

### 해결: login shell 기반 CLI 감지

obsidian-nexus 프로젝트의 `cli_detector.rs` 패턴을 참조하여 포팅:

```rust
// crates/agent/src/cli_detector.rs
fn try_which_login_shell(name: &str) -> Option<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let mut cmd = Command::new(&shell);
    cmd.args(["-l", "-c", &format!("which -a {}", name)]);
    let output = command_output_timeout(cmd, Duration::from_secs(5))?;
    // ...
}
```

`-l` 플래그로 login shell 실행 → `~/.zshrc` 로드 → nvm, volta, `~/.local/bin` 포함한 전체 PATH 탐색.

추가로 glob 패턴으로 `~/.nvm/versions/node/*/bin/<name>` 도 탐색.

### 변경 파일

| 파일 | 변경 내용 |
|------|-----------|
| `crates/agent/src/cli_detector.rs` | login shell `which -a` + 폴백 경로 + glob + shebang 검증 전체 재작성 |
| `crates/agent/sidecar/sidecar.js` | `@anthropic-ai/sdk` → 로컬 CLI spawn (`claude -p`) + API Key fallback |
| `apps/desktop/src-tauri/src/commands/agent.rs` | 직접 API 호출 → `detect_cli()` 감지 후 `tokio::process::Command` spawn |
| `apps/desktop/src-tauri/src/commands/market.rs` | `check_claude_status`/`check_gemini_status` → `detect_cli()` / `find_binary()` 사용 |
| `apps/desktop/src-tauri/Cargo.toml` | `doxus-agent` 의존성 추가 |
| `apps/desktop/src/stores/useChatStore.ts` | Zustand `persist` 미들웨어 추가 + thought 버블 |

### 채팅 UX 개선

1. **답변중 표시**: 메시지 전송 즉시 `role: 'thought'` 버블 표시 → 응답 수신 후 제거
2. **세션 영구 저장**: Zustand `persist` 미들웨어로 `localStorage('doxus-chat-sessions')`에 세션/메시지 저장 → 앱 재시작 후에도 세션 유지

### 발생한 문제들

- **Rust match non-exhaustive**: `check_claude_status`에서 CliKind 패턴 누락 → arm 추가
- **DMG 빌드 실패**: 이전 임시 DMG 파일(`rw.*.dmg`)이 마운트된 채 남아 재빌드 시 충돌 → `hdiutil detach` + `rm`으로 해결
- **테스트 실패**: `detect_cli_returns_gemini_from_env` — login shell이 실제 claude를 찾아버려 PATH 격리 무효화 → 테스트 시나리오를 env var 우선순위 검증으로 변경

### 학습

- macOS GUI 앱의 PATH 문제는 login shell 실행(`zsh -l -c`)으로만 완전히 해결 가능
- nvm 설치 바이너리의 shebang(`#!/usr/bin/env node`)은 같은 bin/ 디렉토리에 node가 있어야 실행 가능 → `is_valid_executable`로 shebang 검증 필요
- Tauri 앱에 `doxus-agent` 의존성을 추가하면 CLI 감지 로직을 단일 크레이트에서 관리 가능

---

## 2026-04-04 (Session 3): 실시간 스트리밍 & 상태 뱃지 버그 수정

### 작업 배경

설정 페이지의 Claude/Gemini 상태 뱃지가 항상 "? 미확인"으로 표시되는 버그 발견. 동시에 채팅 응답이 전부 완료된 후에야 표시되는 UX 문제를 개선하기 위해 Tauri 이벤트 기반 실시간 스트리밍을 구현했다.

### 변경 내용

#### 1. 설정 페이지 상태 뱃지 버그 수정

- **파일**: `apps/desktop/src/pages/SettingsPage.tsx`
- **문제**: `statusLevel()` 함수가 `'ok'`, `'warn'` 문자열을 인식하지 못해 Claude/Gemini 상태가 항상 "? 미확인"으로 표시됨
- **원인**: `statusLevel()`이 `'running'|'connected'|'installed'`만 'ok'로 매핑. `'ok'` 자체는 매핑 없음
- **수정**:
  - `'ok' | 'running' | 'connected' | 'installed'` → ok
  - `'warn' | 'not started'` → warn
  - `'error' | 'not found' | 'not installed'` → error

#### 2. 에이전트 사이드카 상태 실제 감지로 교체

- **파일**: `apps/desktop/src-tauri/src/commands/market.rs`
- **변경**: `get_system_status()`에서 하드코딩된 `"status": "not started"` / "Phase 3에서 구현됩니다" 제거
- `detect_cli()` 호출 → ClaudeCode/GeminiCli 감지 시 `"status": "connected"` + 경로, 없으면 `"status": "warn"`

#### 3. 실시간 스트리밍 구현 (Tauri 이벤트)

obsidian-nexus의 `chat-stream:{sessionId}` 패턴을 참고하여 구현.

**파일**: `apps/desktop/src-tauri/src/commands/agent.rs`

- `agent_send_message`에 `app: tauri::AppHandle` 추가
- Claude: `--output-format stream-json --verbose --include-partial-messages` 플래그로 JSONL 스트리밍
- 각 JSON 라인 파싱:
  - `assistant.message.content[].text` → `text` 이벤트 emit (누적)
  - `tool_use` 블록 감지 → `tool_use` 이벤트 emit (toolName, input)
  - `result` 타입 → `result` 이벤트
- Gemini: raw byte 스트리밍 → 누적 텍스트로 `text` 이벤트
- 이벤트명: `chat-stream:{session_id}`

**파일**: `apps/desktop/src/stores/useChatStore.ts`

- `toolInfo: string | null` 상태 추가
- `sendMessage` 흐름:
  1. 빈 placeholder assistant 메시지 먼저 추가
  2. `listen('chat-stream:...')` 구독
  3. `invoke` 호출
  4. 이벤트로 메시지 in-place 업데이트
  5. `finally` 블록에서 구독 해제
- `thought` role 제거

#### 4. ChatDrawer UI 개선

- **파일**: `apps/desktop/src/components/layout/ChatDrawer.tsx`
- `key={activeSessionId}` 추가 → 세션 전환 시 DOM 강제 재마운트 (메시지 갱신 안 되는 버그 수정)
- `StatusIndicator` 컴포넌트 추가: 스피너 애니메이션 + `toolInfo` 텍스트 표시
- `useRef` + `scrollIntoView` 자동 스크롤
- 전송 중 버튼 ⏳ 표시

### 발생한 문제와 해결

| 문제 | 원인 | 해결 |
|------|------|------|
| Claude CLI exit 1 | `--output-format stream-json` 단독 사용 불가 | `--verbose` 플래그 추가 |
| TypeScript 빌드 에러 | `ChatDrawer.tsx`에서 제거된 `'thought'` role 비교 잔존 | `filter((m) => m.role !== 'thought')` 제거 |
| `json!` 매크로 내 클로저 불가 | Rust `serde_json::json!`은 클로저 표현식 미지원 | 변수로 먼저 추출 후 매크로에 전달 |

### 학습

- Claude CLI `--output-format stream-json`은 반드시 `--verbose` 필요. 진짜 스트리밍은 `--include-partial-messages` 추가
- stream-json 포맷 흐름: `system(init)` → `assistant(content[])` 반복 → `result(subtype:success)`
- Zustand + persist: `partialize`로 임시 상태(`isLoading`, `toolInfo`) 제외 필수
- Tauri `key={activeSessionId}` on 스크롤 컨테이너 → 세션 전환 시 DOM 재마운트로 stale 렌더링 방지

## 관련 문서

- [[module-map]]
- [[data-flow]]
- [[getting-started]]
