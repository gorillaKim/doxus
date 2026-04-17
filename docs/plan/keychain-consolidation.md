# Keychain Access Consolidation Plan: Single Prompt Solution

## Problem Overview
현재 Doxus는 각 플러그인과 설정 항목마다 별도의 키체인 항목(Item)을 생성하고 접근합니다. macOS에서는 새로운 키체인 항목에 접근할 때마다 사용자에게 허용 요청 팝업을 띄우므로, 사용자는 앱 실행 시 여러 차례 권한 승인을 해야 하는 불편함이 있습니다. 또한, 코드 곳곳에 중복된 `SecretStore` 트레이트와 서로 다른 서비스 명명 규칙이 흩어져 있어 관리가 어렵습니다.

### Diagnosis
- **서비스 이름 불일치**: `doxus.{id}`, `doxus-{id}`, `doxus` 등 코드 곳곳에서 서로 다른 서비스명을 사용하여 호출함.
- **중복된 추상화**: `secrets.rs`, `auth.rs`, `wasm_adapter.rs`에 각각 서로 다른 키체인 접근 로직과 트레이트가 정의됨.
- **개별 항목 관리**: `email`, `api_token`, `github_token` 등이 모두 별도의 키체인 항목으로 저장됨.
- **결과**: N개의 시크릿을 로드할 때 최대 N번의 팝업 발생.

## Proposed Changes

핵심 전략은 모든 시크릿 데이터를 **하나의 통합 키체인 항목**에 JSON 형태로 묶어서 저장하고, 시스템 전반의 인증 인터페이스를 하나로 통합하는 것입니다.

### 1. `doxus-core` crate: 아키텍처 통합

#### [MODIFY] [secrets.rs](file:///Users/madup/gorillaProject/doxus/crates/core/src/secrets.rs)
- **`UnifiedKeychainStore` 구현**:
    - 전용 키체인 항목 사용 (Service: `"doxus"`, Account: `"com.doxus.secrets.v1"`)
    - 데이터를 `HashMap<String, String>` 형태의 JSON으로 저장.
    - **네임스페이스 전략**: 키 저장 시 `"service:key"` 형식을 사용하여 충돌 방지.
    - **스레드 안전성**: `RwLock`과 내부 캐싱을 통해 멀티스레드 환경에서 안전하고 빠른 접근 보장.
- **트레이트 표준화**: `SecretStore` 트레이트를 시스템의 유일한 시크릿 인터페이스로 설정.

#### [MODIFY] [auth.rs](file:///Users/madup/gorillaProject/doxus/crates/core/src/auth.rs)
- 중복된 `SecretStore` 트레이트 및 `KeyringSecretStore` 제거.
- `secrets.rs`의 `SecretStore`를 사용하도록 리팩토링.
- `inject_keychain_auth` 함수가 `UnifiedKeychainStore`를 통해 한 번의 팝업으로 모든 필요한 토큰을 가져오도록 수정.

#### [MODIFY] [wasm_adapter.rs](file:///Users/madup/gorillaProject/doxus/crates/core/src/plugin/wasm_adapter.rs)
- `SecretBackend` 트레이트를 제거하고 `secrets::SecretStore`로 교체.
- WASM 호스트 함수(`__doxus_get_secret` 등)가 통합 저장소를 사용하도록 변경.

### 2. Migration: 기존 데이터 이전
- 앱 시작 시 `UnifiedKeychainStore` 초기화 과정에서 다음 패턴의 기존 키체인 항목을 탐색:
    - `doxus.{plugin_id}`
    - `doxus-{plugin_id}`
    - Service: `doxus` / Account: `doxus:{plugin_id}:{key}`
- 발견된 데이터를 통합 JSON으로 이전한 후, 기존 개별 항목들을 안전하게 삭제.

---

## User Review Required

> [!IMPORTANT]
> **TDD 방식 적용**: `UnifiedKeychainStore` 구현 시 먼저 실패하는 테스트 케이스(JSON 직렬화, 네임스페이스 충돌 테스트 등)를 작성하고 이를 해결하는 방식으로 진행합니다.

> [!WARNING]
> **첫 실행 시 팝업**: 마이그레이션 도중 기존 데이터를 읽어오기 위해 처음 한 번은 여러 번의 팝업이 뜰 수 있습니다. 이전이 완료된 이후부터는 단 1회로 고정됩니다.

## Verification Plan

### Automated Tests
- `cargo test -p doxus-core --lib secrets::tests`: 
    - `test_unified_store_namespace_isolation`: 네임스페이스 간 격리 확인.
    - `test_migration_logic`: 기존 데이터 자동 이전 검증.
    - `test_concurrent_access`: 멀티스레드 환경에서 데이터 정합성 확인.

### Manual Verification
- macOS 키체인 접근(Keychain Access.app)에서 `com.doxus.secrets.v1` 항목 하나만 유지되는지 확인.
- 앱 재시작 시 팝업 발생 횟수가 1회로 단축되었는지 실제 확인.
