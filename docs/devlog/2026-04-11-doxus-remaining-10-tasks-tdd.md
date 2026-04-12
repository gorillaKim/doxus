---
title: "doxus 미구현 항목 재검증 + 잔여 10개 Task TDD 구현 완료"
aliases:
  - doxus-remaining-10-tasks-tdd
  - doxus 잔여 태스크 TDD 구현
tags:
  - devlog
  - feature
  - tdd
  - doxus
created: "2026-04-11"
updated: "2026-04-11"
---

<!-- docsmith: auto-generated 2026-04-11 -->

# doxus 미구현 항목 재검증 + 잔여 10개 Task TDD 구현 완료

## 배경

이전 세션에서 ABI 검증 / SecretBackend / OAuth 콜백을 구현한 뒤, 이번 세션에서 `docs/unimplemented.md`에 "미구현"으로 남아 있던 항목 전체를 재검증하였다. 검증 결과 6개 항목이 이미 코드베이스에 구현된 상태임을 확인하고 목록을 20개 → 10개로 축소한 뒤, 잔여 10개를 TDD로 모두 구현하였다.

## 변경 내용

### 주요 변경사항

#### 0. 미구현 항목 재검증

아래 6개 항목이 이미 구현됨을 확인하여 목록에서 제거:

| 항목 | 확인 위치 |
|------|-----------|
| `secrets_get` Keychain 연동 | `secrets.rs:20-37` + `KeyringBackend` |
| 레지스트리 API 연동 | `registry.rs:13-70` |
| GitHub PAT 인증 | `github/lib.rs:100-130` |
| Agent 도구 실행 브릿지 | `tool_bridge.rs` |
| Agent 세션 상태 머신 | `session.rs` |
| ChatDrawer IPC | `agent.rs:100-130` |

#### 1. Confluence 토큰 자동 갱신 (Task 1)

`&mut self`로는 `DocSource` trait (`&self`) 구현이 불가하다는 Critic 지적 반영. `oauth_token` 필드를 `Arc<RwLock<Option<OAuthToken>>>`으로 변경하고 `ensure_valid_token(&self)`에서 double-checked locking으로 refresh 수행. thundering herd 방지 포함.

```rust
// 핵심 패턴
async fn ensure_valid_token(&self) -> Result<String, ConfluenceError> {
    {
        let guard = self.oauth_token.read().await;
        if let Some(t) = guard.as_ref() {
            if !t.is_expired_with_margin(60) {
                return Ok(t.access_token.clone());
            }
        }
    }
    // write lock 획득 후 재확인 (thundering herd 방지)
    let mut guard = self.oauth_token.write().await;
    // ... refresh 요청
}
```

테스트 6개: expired refresh, thundering herd, no refresh_token, api_token bypass 등.

#### 2. Retry + exponential backoff (Task 2)

`RetryPolicy` 구조체와 `retry_with_backoff` 함수 신규 추가. jitter 포함, `max_delay` cap 적용.

```rust
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter: bool,
}
```

테스트 5개: transient retry, max retries 초과, 지수 간격, max_delay cap, jitter 범위.

#### 3. Rate limit 핸들링 (Task 3)

`handle_rate_limited` 함수: `tokio::select!`로 shutdown 취소 가능. `MAX_RATE_LIMIT_WAIT_SECS = 300` 상한 추가 (Security 리뷰 반영). rate limit 응답은 retry 카운터를 소모하지 않는 `retry_with_backoff_rate_aware` 구현.

#### 4. semver range 지원 (Task 4)

`semver = "1"` 의존성 추가. `matches_version` + `find_best_match` 구현. bare version(`1.0.0`)을 `=1.0.0`으로 정규화하는 로직을 두 함수에서 동일하게 처리 (Code 리뷰 지적 반영).

#### 5. 코드 서명 생성 (Task 5)

`generate_keypair`, `sign_plugin`, `save_keypair`(raw 32B), `load_keypair` 구현. 기존 `ed25519-dalek` 재사용, 신규 의존성 없음. `save_keypair`의 파일 권한을 0644 → **0o600**으로 수정 (Security 리뷰 CRITICAL 반영, `#[cfg(unix)]`).

#### 6. 동기화 충돌 해결 (Task 6)

`ConflictResolver` trait 과설계 지양. 단순 함수 `resolve_conflict` (content_hash 비교, last-indexed-wins)와 `record_conflict` (audit_log 기록) 구현. JSON injection 방어를 위해 문자열 포맷 대신 `serde_json::json!` 매크로 사용 (Security 리뷰 CRITICAL 반영).

```rust
// last-indexed-wins: 원격 수정 시각이 아닌 로컬 인덱싱 시각 기준임을 주석으로 명시
pub fn resolve_conflict(local: &Document, remote: &Document) -> ConflictResolution { ... }
```

#### 7. SettingsPage 영속화 (Task 7)

Tauri 커맨드 `save_settings` / `load_settings` 구현. `~/.doxus/config.toml`에 저장. embedding_model 허용 목록 검증 + `load_settings` 반환 전 `validate()` 호출 추가 (Code 리뷰 반영). `SettingsPage.tsx`에서 invoke 연결.

#### 8. MarketPage UI 연결 (Task 8)

하드코딩된 `MOCK_PLUGINS` 제거 → `market_fetch_registry` invoke로 대체. `market_install_plugin`에 trust anchor 검증 추가:

```rust
pub fn verify_plugin_with_anchor(plugin: &[u8], trusted_key: &[u8; 32]) -> Result<(), SignError> {
    // public_key_hex 대조 후 서명 검증
}
```

Security 리뷰에서 "trust anchor 없이 서명 검증은 의미 없음" 지적 → `trusted_key` 파라미터 필수화.

#### 9. WorkspacePage 백엔드 연결 (Task 9)

기존 MCP 기반 커맨드 확인 후 누락된 `delete_workspace_document` 추가. `useWorkspaceStore`에 CRUD 액션 추가.

#### 10. DashboardPage 실시간 업데이트 (Task 10)

`AppHandle` 직접 의존성 대신 `EventSink` trait 도입:

```rust
pub trait EventSink: Send + Sync {
    fn emit_progress(&self, payload: SyncProgress);
    fn emit_complete(&self, payload: SyncComplete);
    fn emit_error(&self, payload: SyncError);
}
```

| 구현체 | 용도 |
|--------|------|
| `TauriEventSink` | Desktop 프로덕션 |
| `NoopEventSink` | CLI 모드 |
| `RecordingEventSink` | 테스트 (실제 이벤트 발행 없이 검증) |

`spawn_sync_loop` → `spawn_sync_loop_with_sink` 위임 패턴으로 ~150줄 중복 제거 (Code 리뷰 반영). DashboardPage.tsx에서 `listen("sync:progress/complete/error")` + `unlisten` cleanup 구현.

### 영향 범위

- `crates/plugins/confluence/src/`: oauth, retry, rate_limit
- `crates/core/src/plugin/market/`: semver, signing, verify
- `crates/core/src/sync/`: conflict, runner, event_sink
- `crates/core/src/settings.rs`: save/load + validate
- `apps/desktop/src-tauri/src/commands/`: market, settings, workspace
- `apps/desktop/src/pages/`: SettingsPage, MarketPage, WorkspacePage, DashboardPage
- `apps/desktop/src/stores/useWorkspaceStore.ts`

## 결과

| 지표 | 값 |
|------|-----|
| 변경 파일 수 | 32개 |
| 추가/삭제 줄 | +2,637 / -193 |
| 신규 테스트 | 34개 |
| 커밋 | `af1a79d` |
| 잔여 미구현 항목 | **0개** (P1-P5 전체 완료) |

## 교훈

**trait 경계에서의 가변성 설계**: `DocSource` trait이 `&self`를 요구하는 상황에서 내부 상태 변경이 필요할 때는 `Arc<RwLock<T>>`로 내부 가변성을 제공해야 한다. `&mut self`로 설계하면 async_trait 구현이 불가하다.

**trust anchor 없는 서명 검증은 무의미**: 공개 키를 플러그인 번들 자체에서 읽어오면 변조된 플러그인이 조작된 키를 함께 제공할 수 있다. 반드시 별도 신뢰 앵커(hardcoded 또는 설정 파일)와 대조해야 한다.

**`EventSink` 추상화의 테스트 가치**: Tauri `AppHandle`에 직접 의존하면 단위 테스트 작성이 불가하다. `EventSink` trait 한 겹을 추가하면 `RecordingEventSink`로 이벤트 발행 내용을 검증할 수 있어 테스트 가능성이 크게 향상된다.

**과설계 경계**: `ConflictResolver` trait을 만들고 싶은 욕구를 억제하고 단순 함수로 구현. 전략 교체 필요성이 명확해지기 전까지는 단순함을 유지한다.

## 관련 문서

- [[doxus-sqlite-vec-wasm-bridge-mcp-extraction]]
- [[002-doxus-1순위-구현-설계결정]]
