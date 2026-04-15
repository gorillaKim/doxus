# Doxus 플러그인 개발 가이드

Doxus는 WASM(Extism) 기반의 플러그인 시스템을 통해 데이터 소스를 확장합니다. 이 가이드는 새로운 플러그인을 개발하기 위한 규격과 절차를 설명합니다.

---

## 1. 프로젝트 설정 (Rust 기준)

WASM 플러그인은 `cdylib` 라이브러리로 빌드되어야 합니다.

### Cargo.toml 템플릿
```toml
[package]
name = "doxus-plugin-example"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
doxus-plugin-sdk = { path = "../../plugin-sdk", default-features = false }
extism-pdk = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

---

## 2. 필수 구현 함수 (Exports)

모든 플러그인은 다음 함수를 반드시 내보내야 합니다 (`#[plugin_fn]` 사용).

| 함수명 | 인자 타입 | 반환 타입 | 설명 |
|--------|-----------|-----------|------|
| `initialize` | `Json<InitOpts>` | `FnResult<()>` | 시크릿 초기화 및 플러그인 설정 |
| `fetch_all` | `Json<FetchAllOptsWasm>` | `Json<DocumentStreamWasm>` | 전체 문서 목록 및 내용 조회 |
| `fetch_document` | `Json<FetchDocumentOptsWasm>` | `Json<RawDocumentWasm>` | 특정 ID의 문서 단건 조회 |
| `health_check` | 없음 | `FnResult<String>` | 플러그인 상태 확인 (정상이면 "OK") |

---

## 3. 선택 구현 함수 (Exports)

기능에 따라 다음 함수들을 추가로 구현할 수 있습니다.

| 함수명 | 인자 타입 | 반환 타입 | 설명 |
|--------|-----------|-----------|------|
| `fetch_changes` | `Json<FetchChangesOptsWasm>` | `Json<ChangeSetWasm>` | 증분 동기화 지원 |
| `create_document` | `Json<CreateDocumentOptsWasm>` | `Json<CreateDocumentResultWasm>` | 새로운 문서 생성 |
| `update_document` | `Json<UpdateDocumentOptsWasm>` | `FnResult<()>` | 기존 문서 수정 |
| `delete_document` | `Json<DeleteDocumentOptsWasm>` | `FnResult<()>` | 문서 삭제 |

---

## 4. 호스트 함수 (Host Functions)

플러그인에서 시스템 기능을 사용하기 위해 호스트 함수를 호출할 수 있습니다.

### 시크릿 저장 (`__doxus_set_secret`)
인증 토큰 등을 호스트의 보안 저장소(키체인)에 저장합니다.
```rust
#[host_fn]
extern "ExtismHost" {
    fn __doxus_set_secret(key: String, value: String);
}
```

### HTTP 요청 (`extism_pdk::http`)
`extism_pdk`에서 제공하는 HTTP 클라이언트를 사용합니다.
*   **주의**: `manifest.json`의 `http_domains`에 명시된 도메인만 접근 가능합니다.

---

## 5. 설치 및 배포

### 플러그인 설치 경로
Doxus 데스크탑 앱 및 MCP 서버는 다음 경로에서 플러그인을 탐색합니다.

*   **macOS/Linux**: `~/.doxus/plugins/`
*   **Windows**: `%USERPROFILE%\.doxus\plugins\`

### 배포 파일 구성
플러그인은 다음 두 파일로 구성되어야 하며, 파일명은 `{plugin_id}`와 일치해야 합니다.

1.  `{plugin_id}.wasm`: 빌드된 WASM 바이너리
2.  `{plugin_id}.manifest.toml`: 플러그인 설정 및 메타데이터

**예시 (`com.doxus.confluence` 기준)**:
*   `~/.doxus/plugins/com.doxus.confluence.wasm`
*   `~/.doxus/plugins/com.doxus.confluence.manifest.toml`

### manifest.toml 예시
```toml
plugin_id = "com.doxus.confluence"
display_name = "Confluence"
version = "0.1.0"
abi_version = 1
http_domains = ["*.atlassian.net", "api.atlassian.com"]
kv_namespaces = ["settings"]
secrets = ["api_token", "access_token", "refresh_token", "expires_at"]
```

---

## 6. 빌드 및 테스트

### 빌드
```bash
cargo build --target wasm32-unknown-unknown --release
```

### 테스트 가이드
1.  `plugin-sdk`에 정의된 mock을 활용하여 Rust 유닛 테스트 작성.
2.  `doxus-core`의 `WasmDocSourceAdapter`를 통한 통합 테스트 수행.
