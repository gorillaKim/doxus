---
title: Doxus Phase 1 Tech Spec - Backend Core & Architecture Refactoring
date: 2026-04-14
status: draft
---

# Doxus Phase 1 기술 스펙 (Tech Spec)

본 문서는 Doxus 잔여 개발 계획 중 **Phase 1: 백엔드 코어 버그 수정 및 아키텍처 정리** 단계에 대한 구체적인 기술 사양과 구현 방법을 정의합니다.

> **Note:** 본 스펙은 2026-04-14 진행된 아키텍처 관점의 기술 리뷰(Self-Review) 피드백이 반영된 업데이트 버전입니다.

## 1. Confluence 플러그인 토큰 갱신 파이프라인 완성

### 1.1 개요
현재 Confluence 플러그인의 OAuth 1시간 만료 토큰을 갱신하는 `refresh_token()` 로직이 존재하지만, 플러그인이 데이터를 패치(`fetch_changes`, `fetch_all`) 하기 전 유효성을 능동적으로 검증하는 파이프라인이 누락되어 있습니다. 이로 인해 토큰 만료 시 문서 동기화가 실패할 수 있습니다.

### 1.2 구현 방안 및 제약 사항
*   **WASM 샌드박스의 한계 극복:** Confluence 플러그인은 `wasm32-unknown-unknown` 기반의 단일 스레드(Single-threaded) 환경에서 동작합니다. 따라서 `tokio::sync::RwLock` 등 비동기 멀티스레딩 Lock을 사용하면 런타임 패닉이나 컴파일 에러가 발생합니다.
*   **Host Function을 이용한 상태 기반 갱신:**
    *   WASM 플러그인 내부 메모리에 락을 거는 대신 **Host Function(`kv_get`, `kv_set`)을 활용**하여 토큰 만료 시간을 체크합니다.
    *   API 요청을 수행하는 래퍼(Wrapper) 함수에 **인터셉터 구조**를 추가하여, `if 갱신시간_경과 { refresh_token_로직_전개 후 kv_set 갱신 }` 형태로 순차 검증(Sequential validation)을 수행하도록 구현합니다.
*   **영향 범위:** `crates/plugins/confluence/src/lib.rs`, HTTP 통신 관련 모듈.

## 2. `mcp-server` 유지보수성 확보 (모듈 분리 리팩토링)

### 2.1 개요
기존 구조에서 비대했던 `main.rs`의 내용을 `crates/mcp-server/src/lib.rs`로 단순 이관하면서, 현재 `lib.rs`가 3,300줄(약 140KB)이 넘는 Monolithic 파일 형태가 되었습니다. 도구(Tool) 추가 확장과 단위 테스트(Testability)를 위해 도메인별 모듈 분리가 시급합니다.

### 2.2 구현 방안
*   `lib.rs` 구조를 하위 디렉토리 체계로 전면 리팩토링하되, SQLite 연결 컨텍스트 무결성을 보장하기 위해 다음과 같은 아키텍처를 채택합니다.
    *   `src/server.rs`: `McpServer` 구조체 정의 및 DB 구조, 플러그인 초기화 관리.
    *   `src/dispatch.rs`: `dispatch()` 및 `dispatch_tool()` 라우팅 분기 로직 배정.
    *   `src/types.rs`: `McpRequest`, `McpResponse`, `McpError` 등 통신 프로토콜 객체 선별.
    *   `src/tools/`: 40여 개의 도구군을 도메인별 파일로 명확히 분리하되, **모든 도구 핸들러 함수는 `&McpServer` 참조자나 구체적인 파라미터(`&Connection`, `&EmbeddingProvider`)를 전달받는 순수 함수(Pure/Stateless Function) 형태 혹은 `McpServer`의 확장 모듈(`impl McpServer`) 형태로 작성**하여 런타임 상태 무결성을 유지합니다.
        *   `tools/project.rs` (`add_project`, `remove_project`, `index_project` 등)
        *   `tools/search.rs` (`search`, `get_document`, `resolve_alias` 등)
        *   `tools/plugin.rs`
        *   `tools/graph.rs`
        *   `tools/workspace.rs`
*   **영향 범위:** `crates/mcp-server/src/*` 디렉토리 전반.

## 3. 문서 쓰기(Write) 권한 제어 연동 확립

### 3.1 개요
워크스페이스 시스템이나 템플릿 적용을 통해 새로운 문서를 생성하거나 타 프로젝트로 내용을 반영(Create/Update) 하려면, 해당 프로젝트의 데이터 소스가 수정(Write) 가능한 상태인지 런타임에 인지할 수 있어야 합니다.

### 3.2 구현 방안
*   **`DocSource` Trait 확장 (`plugin-sdk/src/lib.rs`):**
    ```rust
    #[async_trait]
    pub trait DocSource: Send + Sync {
        // ... (기존 읽기 관련 메서드 선언) ...

        /// 해당 소스가 문서 생성을 지원하는지 여부를 반환합니다. 
        /// 기본값은 false(Read-only)로, 오버라이드하지 않은 기존 플러그인을 보호합니다.
        fn supports_write(&self) -> bool { false }

        /// 문서를 해당 소스에 직접 생성/추가합니다.
        async fn create_document(&self, title: &str, content: &str, meta: &Value) -> Result<SourceDocId, PluginError> {
            Err(PluginError::NotSupported("create_document".into()))
        }
    }
    ```
*   **플러그인별 확장성 고려 (디자인 반영):**
    *   단순 하드코딩된 상수(`true/false`)가 아닌, 플러그인 초기화 시 전달받는 `PluginConfig` 인자나 사용자 권한(토큰 Scopes)을 통해 **런타임에 쓰기 가능 여부를 동적으로 판별하여 `bool`로 반환**하도록 고도화 여지를 둡니다.
    *   **ObsidianPlugin:** 본인 소유의 로컬 파일시스템에 접근하므로 우선적으로 쓰기가 허용되게 구현합니다.
    *   **Confluence / GitHub:** 현재는 MVP 수준에 맞춰 디폴트 `false` 상태로 설정합니다.
*   **MCP 타겟 예외 제어 연동:** `mcp-server`의 워크스페이스 문서 생성 도구(`doxus_apply_template`, `doxus_create_document` 등)가 실행될 때, 대상 프로젝트 인스턴스의 `supports_write()` 메서드를 우선 체크합니다. `false`로 확인되면 프로세스를 차단하고 명확한 예외 처리(`Not Supported`)를 반환합니다.
*   **영향 범위:** `crates/plugin-sdk/src/lib.rs`, `crates/plugins/obsidian/src/lib.rs`, `crates/mcp-server/src/tools/workspace.rs`

---

## 📅 작업 진행 (Dependencies)
- 도구 간 코드 충돌을 예방하고 전체 시스템의 관리 안정성을 확보하기 위해, 제일 광범위하고 분산 파급력이 큰 **`mcp-server` 리팩토링(모듈 분할) 작업을 0순위로 시작**합니다. 이후 Confluence 토큰과 Trait 변경을 각각의 분리된 브랜치/도메인에서 병렬 또는 순차적으로 작업할 수 있습니다.
