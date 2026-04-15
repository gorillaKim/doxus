---
title: Doxus Phase 3 Tech Spec - Confluence WASM 레퍼런스 및 토큰 갱신 파이프라인
date: 2026-04-14
status: draft
---

# Doxus Phase 3 기술 스펙: WASM 레퍼런스 구현 및 토큰 갱신 파이프라인

본 문서는 써드파티(3rd-party) 개발자 생태계를 고려하여, Doxus 플러그인의 **표준 보안 모델(WASM Sandbox)**과 **복합 인증(OAuth) 처리 방식**에 대한 공식 레퍼런스 구현을 정의합니다. 

> **작업 목표**: 기존 Native 기반 Confluence 플러그인을 `extism-pdk` 기반 WASM으로 전면 전환하고, 샌드박스 내부에서 안전하게 OAuth 토큰을 갱신 및 호스트 DB에 영속화하는 표준 파이프라인 수립.

---

## 1. 아키텍처 원칙 (Reference Implementation)

미래의 플러그인 마켓플레이스 생태계를 위해 아래 3가지 원칙을 준수합니다.

### 1.1 강제 격리 (WASM Sandbox)
*   외부 소스(Confluence, GitHub 등) 연동 플러그인은 반드시 **Extism 샌드박스** 내부에서 실행됩니다.
*   플러그인은 호스트의 파일 시스템이나 네트워크에 직접 접근할 수 없으며, 오직 허용된 **Host Function(`http_request`, `kv_set/get`)**을 통해서만 외부와 소통합니다.

### 1.2 단방향 상태 동기화 (Unidirectional Commits)
*   WASM 플러그인은 비상태성(Stateless)을 유지합니다. 
*   갱신된 토큰 등 영속화가 필요한 정보는 `kv_set`을 통해 호스트에 전달하며, 호스트 어댑터(`WasmDocSourceAdapter`)가 호출 종료 시 이를 감지하여 **SQLite DB(`source_instances.secrets_json`)**에 안전하게 반영합니다.

### 1.3 테스트 자동화 표준 (Mock-first TDD)
*   써드파티 개발자가 실제 자격 증명 없이도 CI/CD에서 검증할 수 있도록, **Mock 서버(`wiremock`)** 기반의 테스트 블루프린트를 제공합니다.

---

## 2. 세부 구현 설계

### 2.1 플러그인 리팩토링 (`crates/plugins/confluence`)
*   **의존성 정리**: `tokio`, `reqwest` 등 Native I/O 라이브러리를 제거하고 `extism-pdk`로 교체합니다.
*   **Shared Client Logic**: `ensure_valid_token` 로직을 WASM 환경에 맞게 수정합니다.
    *   API 호출 전 `kv_get`으로 로컬 캐시(WASM 내부 변수)와 Host KV Store의 토큰 상태를 대조합니다.
    *   만료 시(혹은 10분 내 만료 임박 시) 호스트의 `http_request`를 통해 Atlassian OAuth 서버에 리프레시 요청을 보냅니다.
    *   새 토큰 수령 시 즉시 `kv_set`으로 호스트에 상태 변경을 통보합니다.

### 2.2 호스트 커밋 로직 (`crates/core/src/plugin/wasm_adapter.rs`)
*   **Secret Persistence**: 플러그인 메서드(`fetch_all`, `fetch_changes`) 호출이 반환될 때, 어댑터는 플러그인 전용 KV 네임스페이스를 스캔합니다.
*   **DB 반영**: 특정 키(예: `__updated_secrets`)가 존재할 경우, 해당 정보를 파싱하여 SQLite DB의 `source_instances` 테이블을 업데이트합니다. 이를 통해 다음 실행 시 플러그인이 항상 최신 토큰 정보를 주입받게 됩니다.

---

## 3. 보안 및 안정성

*   **SSRF 방지**: 모든 OAuth 갱신 및 API 요청은 호스트의 `PluginManifest` 도메인 허용 목록(Allowlist) 검사를 통과해야 합니다.
*   **최소 권한 원칙**: 플러그인은 자신의 샌드박스 외부의 KV 영역을 볼 수 없으며, 오직 할당된 네임스페이스 내에서만 데이터 교환이 가능합니다.
*   **경합 방지 (Thundering Herd)**: 백그라운드 동기화 런타임에서 중복 갱신이 발생하지 않도록, 호스트 측 어댑터에서 세션당 갱신 여부를 캐싱합니다.

---

## 4. 검증 계획 (Blueprinting)

1.  **Mock Pipeline 테스트**: `wiremock`을 통해 인증 만료 시나리오를 재현하고, 자동으로 토큰이 바뀌며 통신이 재개되는지 코드 레벨에서 검증합니다.
2.  **DB 영속화 확인**: 플러그인 실행 후 SQLite 내부의 `secrets_json` 컬럼이 새 토큰 값으로 자동 업데이트되었는지 SQL 쿼리로 확인합니다.
3.  **WASM 빌드 검증**: `wasm32-unknown-unknown` 타겟으로 컴파일이 완벽하게 수행되는지 확인합니다.
