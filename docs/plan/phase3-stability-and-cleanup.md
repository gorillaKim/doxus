# Phase 3 안정화 및 정리 계획 (Stability & Cleanup) - v1.1

이 문서는 Phase 3 코드 리뷰 이후 발견된 빌드 오류를 해결하고, 누락된 기능 및 테스트를 보완하며, 시스템의 실제 작동을 검증하기 위한 실행 계획을 담고 있습니다.

---

## 1. P0 — 데스크톱 앱(Tauri) 빌드 복구

현재 `apps/desktop/src-tauri`에서 `ConfluencePlugin`을 직접 참조하여 발생하는 컴파일 오류를 해결합니다.

### 1-1. 정적 참조 제거
*   **대상**: `apps/desktop/src-tauri/src/commands/search.rs`
*   **수정**: `use doxus_plugin_confluence::ConfluencePlugin` 제거.
*   **대안**: 플러그인 인스턴스 생성을 하드코딩 대신 `PluginManager`의 `get_source()`를 사용하도록 변경.

### 1-2. 타입 추론 오류 해결
*   **대상**: `search.rs` 내 `plugin.initialize` 및 `fetch_all` 호출부.
*   **수정**: 플러그인 변수에 명시적 타입(`Box<dyn DocSource>`)을 부여하여 컴파일러가 타입을 추론할 수 있도록 지원.

### 1-3. WASM 바이너리 배치 및 탐색 경로 확인 (추가)
*   **위험**: 빌드 성공 후에도 플러그인이 로드되지 않아 런타임 에러 발생 가능.
*   **조치**: `PluginManager`가 플러그인을 찾는 기본 경로(예: `~/.doxus/plugins/`)를 확인하고, 빌드된 `.wasm` 파일이 해당 위치에 배치되었는지 검증하는 체크리스트 추가.

---

## 2. P1 — MCP 서버 및 WASM 어댑터 보완

### 2-1. `apply_template` 메타데이터 전달 구현
*   **대상**: `crates/mcp-server/src/tools/workspace.rs`
*   **수정**: 템플릿 변수(`variables`) 중 메타데이터와 관련된 항목을 추출하여 `create_document`의 3번째 인자로 전달.

### 2-2. WASM 쓰기 경로 검증 테스트 추가
*   **대상**: `crates/core/src/plugin/wasm_adapter.rs` (테스트 모듈)
*   **수정**: `create_document` 등을 모방하는 WASM 함수를 호출하여 직렬화된 데이터가 올바르게 전달되는지 확인하는 통합 테스트 작성.

---

## 3. P2 — 코드 품질 및 경고 정리

### 3-1. 미사용 코드 정리 (Dead Code Cleanup)
*   **대상**: `crates/core/src/plugin/wasm_adapter.rs`
*   **수정**: `allowed_domains`, `http_client` 필드 제거에 따른 생성자 및 관련 코드 완전 삭제.
*   **대상**: `crates/core/src/secrets.rs` 등
*   **수정**: 사용되지 않는 `Arc`, `json` 등의 임포트 제거.

---

## 4. P3 — 통합 검증 (Smoke Test) (추가)

수정된 모든 모듈이 실제 환경에서 유기적으로 작동하는지 확인합니다.

*   **시나리오**: `MCP 도구 호출 -> Core (WasmAdapter) -> Plugin (Wasm Binary)`
*   **방법**: MCP 서버를 실행하고 `create_document` 도구를 호출하여 실제 WASM 플러그인(Confluence 등)이 문서를 생성하려고 시도하는지 로그 및 결과 확인.

---

## 작업 순서 및 예상 소요

1.  **Tauri 빌드 복구**: `search.rs` 리팩토링 및 정적 참조 제거 (1시간)
2.  **바이너리 배치 확인**: 플러그인 탐색 경로 및 파일 존재 여부 점검 (30분)
3.  **MCP 기능 보완**: `apply_template` 메타데이터 로직 추가 (30분)
4.  **WASM 검증 테스트**: 직렬화 테스트 케이스 추가 (1시간)
5.  **경고 정리**: 전체 워크스페이스 경고 제거 (30분)
6.  **통합 스모크 테스트**: 전체 흐름 검증 (30분)

**총 예상 소요**: 약 4시간
