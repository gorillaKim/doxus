# Keychain Access Consolidation Plan: Single Prompt Solution

## Problem Overview
현재 Doxus는 각 플러그인과 설정 항목마다 별도의 키체인 항목(Item)을 생성하고 접근합니다. macOS에서는 새로운 키체인 항목에 접근할 때마다 사용자에게 허용 요청 팝업을 띄우므로, 사용자는 앱 실행 시 여러 차례 권한 승인을 해야 하는 불편함이 있습니다.

### Diagnosis
- **서비스 이름 불일치**: `doxus.{id}`, `doxus-{id}`, `doxus` 등 코드 곳곳에서 서로 다른 서비스명을 사용하여 호출함.
- **개별 항목 관리**: `email`, `api_token`, `github_token` 등이 모두 별도의 키체인 항목으로 저장됨.
- **결과**: N개의 시크릿을 로드할 때 최대 N번의 팝업 발생.

## Proposed Changes

핵심 전략은 모든 시크릿 데이터를 **하나의 통합 키체인 항목**에 JSON 형태로 묶어서 저장하는 것입니다. 이렇게 하면 macOS는 해당 통합 항목에 대한 접근 권한을 단 한 번만 묻게 됩니다.

### 1. `doxus-core` crate

#### [MODIFY] [secrets.rs](file:///Users/madup/gorillaProject/doxus/crates/core/src/secrets.rs)
- `UnifiedKeychainStore` 구조체 구현:
    - 전용 키체인 항목 사용 (Service: `"doxus"`, Account: `"com.doxus.secrets.v1"`)
    - 데이터를 `HashMap<String, String>` 형태의 JSON으로 직렬화/역직렬화하여 관리.
    - 메모리 캐싱을 통해 잦은 읽기 시 성능 최적화.
- 기존 `SystemKeychain`을 `UnifiedKeychainStore`로 점진적 교체하거나 내부 로직을 통합.

#### [MODIFY] [auth.rs](file:///Users/madup/gorillaProject/doxus/crates/core/src/auth.rs)
- `inject_keychain_auth` 함수가 개별 `keyring::Entry`를 호출하지 않고, `UnifiedKeychainStore`를 통해 한 번에 시크릿을 가져오도록 수정.

#### [MODIFY] [wasm_adapter.rs](file:///Users/madup/gorillaProject/doxus/crates/core/src/plugin/wasm_adapter.rs)
- WASM 플러그인 호스트 함수들(`__doxus_get_secret`, `__doxus_set_secret`)이 사용하는 `KeyringBackend`를 `UnifiedKeychainStore` 기반으로 변경.

### 2. Migration Strategy (Optional but Recommended)
- 새로운 시스템으로 전환 시, 기존에 개별적으로 저장되어 있던 데이터들을 읽어와서 통합 JSON에 저장하고 기존 항목을 삭제하는 마이그레이션 로직 추가.

---

## User Review Required

> [!IMPORTANT]
> **데이터 통합 방식**: 모든 시크릿이 하나의 JSON 문자열로 키체인에 저장됩니다. 이는 macOS 기준 "암호" 항목 하나로 관리됨을 의미합니다. 보안 수준은 기존과 동일(키체인 암호화 적용)하지만, 항목 관리가 단순해집니다.

> [!WARNING]
> **마이그레이션**: 시스템 변경 직후 첫 실행 시 한 번 더 여러 번의 팝업이 뜰 수 있습니다 (기존 데이터를 읽어오기 위함). 이후에는 하나로 통합됩니다.

## Open Questions
- 기존에 저장된 데이터가 많은가요? (마이그레이션 필수 여부 판단)
- 특정한 시크릿만 별도의 보안 정책을 적용해야 하는 경우가 있나요? (현재는 모두 일반 텍스트 토큰/비밀번호로 보임)

## Verification Plan

### Automated Tests
- `cargo test -p doxus-core --lib secrets::tests`: 통합 저장소의 읽기/쓰기/삭제 로직 검증.
- JSON 직렬화/역직렬화 오버헤드 확인.

### Manual Verification
- macOS 시스템 키체인 접근 앱(Keychain Access.app)에서 `com.doxus.secrets.v1` 항목이 정상 생성되는지 확인.
- 앱 재시작 후 팝업 발생 횟수가 1회로 줄어드는지 확인.
