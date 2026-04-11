---
title: "ABI 버전 검증, SecretBackend trait, Confluence OAuth 콜백 TDD 구현"
aliases:
  - abi-secret-oauth-tdd
  - ABI 시크릿 OAuth TDD
tags:
  - devlog
  - feature
  - tdd
  - plugin
  - confluence
  - security
created: "2026-04-11"
updated: "2026-04-11"
---

<!-- docsmith: auto-generated 2026-04-11 -->

# ABI 버전 검증, SecretBackend trait, Confluence OAuth 콜백 TDD 구현

## 배경

`.omc/plans/items-1-4.md` 계획에서 파생된 작업. Critic 리뷰 2회를 거쳐 최종 실행 순서를 항목 3 → 1 → 4로 확정했다.

- 항목 2(content_transform 고도화)는 host function 역할 오남용으로 판단해 제거
- agent 관련 3개 항목(도구 실행 브릿지, 세션 상태 머신, ChatDrawer IPC)은 P6으로 분리

1차 Critic WARN: `from_bytes` 호출부 28+ 곳 일괄 수정 비용, OAuth에 raw HTTP 사용.
2차 Critic WARN: `from_bytes` 29곳, `fetch_all` expand 파라미터 누락.

## 변경 내용

### 주요 변경사항

#### 항목 3 — ABI 버전 런타임 검증

- `crates/core/src/plugin/manager.rs`에 `pub(crate) const SUPPORTED_ABI_VERSION: u32 = 1;` 추가
- `get_source()`에서 ABI 버전 불일치 시 `None` 반환 + `tracing::warn!` 출력
- `wasm_adapter.rs`의 하드코딩된 `1` → 동일 상수로 교체
- 신규 테스트: `test_get_source_rejects_unsupported_abi_version`, `test_get_source_accepts_supported_abi_version`

#### 항목 1 — SecretBackend trait 추출

`secrets_get` host function의 Keychain 의존성을 trait으로 분리해 테스트 가능성 확보.

```rust
trait SecretBackend: Send + Sync {
    fn get_secret(&self, service: &str, key: &str) -> Option<String>;
}

// 프로덕션: keyring::Entry 래핑
struct KeyringBackend;

// 테스트: HashMap 기반 mock
#[cfg(test)]
struct MemoryBackend(HashMap<(String, String), String>);
```

`WasmDocSourceAdapter::from_bytes`에 `secret_backend: Option<Arc<dyn SecretBackend>>` 파라미터 추가. 총 29개 호출부를 일괄 업데이트(manager.rs 4곳 + wasm_adapter.rs 테스트 25곳).

신규 테스트: `test_secrets_get_uses_injected_backend`

#### 항목 4 — Confluence OAuth 콜백 HTTP 서버

`crates/plugins/confluence/src/oauth_server.rs` 신규 파일.

```rust
// OS 임의 포트로 바인딩
let listener = TcpListener::bind("127.0.0.1:0")?;

// partial read 루프 — \r\n\r\n 헤더 끝 탐지
// CSRF state 파라미터 검증 (expected_state 대조)
pub fn wait_for_callback(
    timeout: Duration,
    expected_state: &str,
) -> Result<OAuthCode, PluginError>
```

`confluence/lib.rs` 변경:
- `oauth_start()`에서 동적 포트 redirect_uri 적용
- `wait_oauth_callback()` 추가

신규 테스트 4개: port binding, callback receipt, missing-code rejection, timeout

#### Validation 이슈 수정 (보안/코드리뷰 반영)

| 등급 | 이슈 | 수정 |
|------|------|------|
| Security REJECT | OAuth state validation 미구현 | `wait_for_callback`에 `expected_state` 파라미터 + CSRF 검증 |
| Code-review CRITICAL | partial read 미처리 | read 루프로 교체 |
| Code-review HIGH | `local_addr()` expect() | `Result<SocketAddr, PluginError>` 반환으로 변경 |
| Code-review HIGH | `new_with_domains` expect() | `#[cfg(test)]` 애너테이션 |
| Code-review MEDIUM | `oauth_start` getrandom expect() | `.ok()?` 변환 |

#### 버그 수정 (커밋 51f5cc3)

- `confluence/lib.rs` fetch_all에 `("expand", "body.storage")` 추가 — content가 항상 빈값으로 반환되던 버그 수정
- `wasm_adapter.rs` 테스트 5개에 `#[serial_test::serial]` 추가 — env var race condition 수정

### 영향 범위

- `crates/core/src/plugin/manager.rs` — ABI 검증 로직
- `crates/core/src/plugin/wasm_adapter.rs` — SecretBackend trait, from_bytes 시그니처 변경
- `crates/plugins/confluence/src/oauth_server.rs` — 신규 파일
- `crates/plugins/confluence/src/lib.rs` — OAuth 흐름, expand 버그 수정

## 결과

- 커밋: `cbbacf5` (구현), `51f5cc3` (버그 수정)
- 전체 테스트: 435+ passed, 0 failed
- Architect ✅ / Security ✅ / Code-review ✅

## 교훈

- **content_transform 고도화는 host function 역할 오남용**: 문서 소스에서 마크다운 파싱 책임을 core에 위임하는 것은 분리 원칙 위반. 항목 제거 결정이 옳았다.
- **Critic 2회 사이클 효과**: 1차에서 발견 못한 fetch_all expand 누락을 2차에서 잡아냄. 복잡한 플러그인 변경에서 Critic 2회는 유효한 투자다.
- **from_bytes 시그니처 변경 비용**: trait 도입 시 29곳 호출부 수정이 필요했다. 향후 유사 변경을 대비해 빌더 패턴 또는 `WasmAdapterConfig` struct 도입을 검토할 것.
- **OS 임의 포트 바인딩**: OAuth 콜백 서버에서 고정 포트 대신 `bind("127.0.0.1:0")` 사용이 충돌 없는 표준 패턴임을 재확인.

## 관련 문서

- [[002-doxus-1순위-구현-설계결정]]
- [[doxus-project-summary]]
