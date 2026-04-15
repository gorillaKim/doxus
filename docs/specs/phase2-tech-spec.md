---
title: Doxus Phase 2 Tech Spec - 문서 쓰기(Write) 권한 제어 연동
date: 2026-04-14
status: draft
---

# Doxus Phase 2 기술 스펙 (Tech Spec)

본 문서는 Doxus의 외부 데이터 소스(Obsidian 등)에 대한 **양방향 연동(Write-back)** 기능을 안전하게 제공하기 위해, **문서 쓰기(Create/Update/Delete) 권한 제어 및 처리 파이프라인**을 구현하는 Phase 2 작업의 세부 설계 사양입니다.

> **작업 목표**: 단순 읽기(Read-only)를 넘어, 외부 플러그인 소스로 문서를 직접 생성/수정/삭제할 때 권한 여부를 런타임에 안전하게 검증하고, 허용된 플러그인(Obsidian)에 한하여 실제 파일 시스템이나 API에 반영하는 기반 구조 확립.

---

## 1. 개요 및 요구사항

기존 Doxus의 플러그인 구조(`DocSource` Trait)는 데이터 패치(`fetch_all`, `fetch_changes`) 등 단방향(Read) 동기화 기능만 정의되어 있었습니다. 워크스페이스(Workspace) 환경에서 템플릿을 생성하거나 기존 옵시디언 노트를 수정/삭제했을 때, 이를 외부 프로젝트에 직접 저장/반영할 수 있는 양방향 통로가 필요합니다.

### 1.1 요구사항 요약
*   `DocSource` Trait에 능동적 쓰기 권한 여부(`supports_write`) 확인 기능 추가.
*   `DocSource` Trait에 새 문서 생성(`create_document`), 기존 문서 수정(`update_document`), 삭제(`delete_document`) 인터페이스 추가.
*   문서의 포맷팅(Frontmatter 주입, 템플릿 렌더링)은 호스트(Host)인 MCP Server가 오롯이 책임지며, 플러그인은 I/O만 수행하도록 역할 분리.
*   `McpServer` 쓰기 도구 실행 전 외부 연동 지원 여부를 우선 점검하는 Guard 로직 배치.
*   **DB 즉시 동기화(Immediate Sync)**: 파일 시스템(또는 외부 API) 쓰기/삭제 완료 직후, 결과를 즉시 SQLite 데이터베이스에 강제 반영하여 동기화 지연(리드타임) 문제 차단.

---

## 2. 모듈별 구현 설계

### 2.1 `crates/plugin-sdk/src/lib.rs` (`DocSource` Trait 확장)

최상위 SDK 명세에 쓰기 능력(Capability)을 판별하고 문서를 생성/수정/삭제하는 인터페이스를 추가합니다.

```rust
#[async_trait]
pub trait DocSource: Send + Sync {
    // ... (기존 메서드 생략) ...

    /// 외부 데이터 소스가 문서를 능동적으로 생성/수정/삭제할 수 있는지 여부를 반환합니다.
    fn supports_write(&self) -> bool {
        false
    }

    /// 대상 시스템에 새로운 문서를 발행(Create)합니다.
    async fn create_document(&self, title: &str, content: &str) -> Result<SourceDocId, PluginError> {
        Err(PluginError::Internal("create_document not supported".into()))
    }

    /// 대상 시스템의 기존 문서를 수정(Update)합니다.
    /// [제약 사항]: 본 페이즈에서는 파일 내용은 덮어쓰지만(In-place), 파일명(Rename) 자체를 바꾸는 기능은 지원하지 않습니다.
    async fn update_document(&self, id: &SourceDocId, content: &str) -> Result<(), PluginError> {
        Err(PluginError::Internal("update_document not supported".into()))
    }

    /// 대상 시스템의 기존 문서를 삭제(Delete)합니다.
    async fn delete_document(&self, id: &SourceDocId) -> Result<(), PluginError> {
        Err(PluginError::Internal("delete_document not supported".into()))
    }
}
```

### 2.2 `crates/plugins/obsidian/src/lib.rs` (Obsidian 구현체 수정)

Obsidian은 로컬 파일시스템에 직접 접근하므로, 쓰기 권한을 `true`로 오픈합니다.

*   `supports_write(&self)`: 무조건 `true` 반환.
*   **Create**:
    *   플러그인 초기화 시 전달된 `vault_path`를 참조합니다.
    *   `title`에 기반하여 운영체제에서 허용되는 안전한 파일명(예: `title.md`)을 산출합니다. (특수문자 제거, 중복 시 `-1` 등 넘버링 부여)
    *   `tokio::fs::write`를 사용해 물리적 파일을 저장하고, 생성된 파일의 상대 경로를 `SourceDocId`로 매핑하여 반환합니다.
*   **Update**:
    *   전달된 `id` (상대 경로)를 통해 볼트 내 기존 파일을 찾아 내용을 덮어씁니다(`In-place Update`). 파일명은 변하지 않습니다.
*   **Delete**:
    *   해당 경로의 파일을 `tokio::fs::remove_file` 등으로 영구 삭제합니다.

### 2.3 `crates/core/src/plugin/wasm_adapter.rs` (WASM 플러그인 어댑터)

Confluence, GitHub 등 WebAssembly 방식으로 로드되는 외장 플러그인들은 MVP 기준 쓰기를 지원하지 않습니다.
*   `supports_write() -> false`의 기본(Default) 구현을 타게 두어, 이 프로젝트를 대상으로 쓰기를 시도할 경우 Host 단에서 안전하게 예외처리 되도록 둡니다.

### 2.4 `crates/mcp-server/src/dispatch.rs` 및 `tools/workspace.rs`

기존 단순 SQLite 워크스페이스 전용 쓰기 도구 체계를 범용적으로 확장 적용합니다.

1.  **MCP 도구 스키마 (`dispatch.rs`) 확장**:
    *   `doxus_create_document`, `doxus_update_document`, `doxus_delete_document`, `doxus_apply_template`에 대상 위치를 콕 집을 수 있는 **`project` (Optional) 파라미터**를 추가합니다.
2.  **프로젝트 권한 가드 (`tools/workspace.rs`)**:
    *   `project`가 입력된 경우, DB에서 매칭되는 플러그인 구현체를 가져와 `supports_write()` 결과를 묻습니다. 결과가 `false`라면 곧바로 JSON-RPC 오류를 띄웁니다.
3.  **데이터 정합성 및 DB 즉시 동기화 로직 (Critical)**:
    *   **Create / Update 성공 시**: 백그라운드 동기화 스케줄러(`SyncRunner`)를 기다리지 않습니다. MCP 도구 내에서 **즉시 `plugin.fetch_document(id)`를 호출**하여 파싱된 최종 해시(Hash), 메타데이터, 태그를 획득하고, 이를 Doxus SQLite `documents` 테이블에 강제 삽입(Upsert)하여 데이터 정합성을 즉결 보장합니다.
    *   **Delete 성공 시**: 즉시 SQLite `documents` 테이블에서 해당 `project_id`와 `source_doc_id`에 매칭되는 레코드를 삭제(DELETE) 쿼리합니다.

---

## 3. 테스트 및 검증 방안

*   **단위 테스트(Unit Tests)**:
    *   `ObsidianPlugin::create_document`가 넘겨받은 문자열을 변형 없이 저장하며, 특수문자가 포함된 Title에서도 유효한 File I/O 처리가 일어나는지 Mock Test 로 검증.
*   **통합 정합성 테스트(Integration Tests)**:
    *   `doxus_create_document` 호출(Obsidian 타겟) 직후, `doxus_search`나 DB 검색 쿼리를 날렸을 때 **즉시(Immediately)** 생성한 문서가 조회 및 랭킹되는지(DB 강제 동기화 여부) End-to-End 검증 시행.
