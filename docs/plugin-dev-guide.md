# Doxus 플러그인 개발 가이드 (WASM)

Doxus는 Extism 기반의 WASM 플러그인 시스템을 통해 외부 데이터 소스를 확장할 수 있습니다. 이 문서는 WASM 플러그인을 개발하는 데 필요한 정보를 제공합니다.

## 1. 개요

Doxus 플러그인은 아래의 역할을 수행합니다.
- 외부 소스(Confluence, GitHub 등)로부터 문서 목록 및 내용을 가져옴.
- (옵션) 문서 생성, 수정, 삭제 작업을 수행.
- 스페이스 및 폴더 계층 구조를 Doxus의 상대 경로 체계로 변환.

## 2. 개발 환경 설정

- **Rust**: `wasm32-unknown-unknown` 타겟이 필요합니다.
  ```bash
  rustup target add wasm32-unknown-unknown
  ```
- **SDK**: `doxus-plugin-sdk`를 의존성에 추가해야 합니다.

## 3. 필수 구현 인터페이스

플러그인은 아래의 전역 함수들을 `#[plugin_fn]`으로 내보내야 합니다.

### `initialize(Json<InitOpts>) -> FnResult<()>`
플러그인 인스턴스가 생성될 때 호출됩니다. 설정 값과 시크릿(토큰 등)을 전달받아 초기화합니다.

### `fetch_all(Json<FetchAllOptsWasm>) -> FnResult<Json<DocumentStreamWasm>>`
전체 문서를 동기화할 때 호출됩니다. 페이징 처리를 지원해야 합니다.

### `fetch_changes(Json<FetchChangesOptsWasm>) -> FnResult<Json<ChangeSetWasm>>`
마지막 동기화 시점 이후의 변경 사항만 가져올 때 호출됩니다.

### `fetch_document(Json<FetchDocumentOptsWasm>) -> FnResult<Json<RawDocumentWasm>>`
특정 ID의 문서 상세 내용을 가져올 때 호출됩니다.

## 4. 선택적 인터페이스 (Write Support)

플러그인이 `supports_write`를 구현하고 쓰기 함수들을 내보내면 Doxus에서 문서 수정을 지원합니다.

- `create_document`
- `update_document`
- `delete_document`

## 5. 호스트 함수 (Host Functions)

WASM 샌드박스 내부에서 호스트의 기능을 사용할 수 있는 인터페이스입니다.

- `__doxus_set_secret(key, value)`: 영구 저장소에 시크릿을 저장합니다. (예: OAuth 토큰 갱신)
- `__doxus_get_secret(key) -> String`: 저장된 시크릿을 가져옵니다.
- `__doxus_get_time() -> i64`: 현재 시스템 시간(Unix timestamp)을 가져옵니다.

## 6. 성능 최적화 팁

- **계층 구조 캐시**: 컨플루언스와 같이 계층 구조가 복잡한 경우, `PluginState`를 사용하여 폴더 정보를 캐싱하면 인덱싱 속도를 크게 높일 수 있습니다.
- **배치 처리**: 가능하면 API 호출 횟수를 최소화하고, SDK에서 제공하는 타입들을 활용하세요.

## 7. 빌드 및 배포

```bash
cargo build --target wasm32-unknown-unknown --release
```
빌드된 `.wasm` 파일과 `manifest.toml` 파일을 `.doxus/plugins` 디렉토리에 배치하면 Doxus가 자동으로 인식합니다.
