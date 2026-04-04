---
title: "doxus Phase 0~3 TDD 구현 세션"
aliases:
  - doxus-phase0-3
  - doxus-phase0-3-implementation
  - doxus 구현 세션
  - doxus Phase 0~3
tags:
  - devlog
  - implementation
  - rust
created: "2026-04-04"
updated: "2026-04-04"
---

<!-- docsmith: auto-generated 2026-04-04 -->

# doxus Phase 0~3 TDD 구현 세션

## 배경

doxus는 obsidian-nexus의 차세대 진화판으로, WASM 플러그인 기반 다중 소스 통합 문서 검색 허브다. 로컬 퍼스트 + 에이전트 친화적 설계를 핵심으로 하며, 이 세션에서는 autopilot + team agents를 활용해 Phase 0부터 Phase 3까지 TDD 방식으로 전체 기반 구현을 완료했다.

기존 obsidian-nexus의 FTS5 + 벡터 하이브리드 검색 엔진을 계승하면서, ONNX 내장 임베딩과 Extism WASM 플러그인 시스템을 새롭게 도입하는 것이 이번 구현의 핵심 과제였다.

## 변경 내용

### 주요 변경사항

**Phase 0-A: ONNX 임베딩 PoC**
- `EmbeddingProvider` trait 정의 (async, Send+Sync, thiserror 기반 EmbeddingError)
- `OnnxEmbedder`: all-MiniLM-L6-v2.onnx (86MB) + tokenizer.json 번들, 384차원 벡터 출력
- 배치 인퍼런스: TensorRef 기반 mean pooling + L2 normalize
- 코사인 유사도 검증, 13개 테스트 (unit 11개 + ONNX inference 2개 `#[ignore]`)
- `ort = "2.0.0-rc.12"` with `download-binaries` feature 사용

**Phase 0-B: Extism WASM PoC**
- `Plugin`: Send=YES, Sync=NO 확정
- 최종 아키텍처 패턴: `Arc<Mutex<Plugin>> + tokio::spawn_blocking`
- extism 1.21.0 (번들 Wasmtime 41.0.4) 통합 검증

**Phase 1: Cargo Workspace + Core 포팅**
- workspace members: core, plugin-sdk, plugins/obsidian, cli, mcp-server, agent, extism-poc
- DB 마이그레이션 V1~V8 (V4 vec0는 extension 로드 후 별도 적용)
- `SearchEngine`: FTS5 + BM25 검색, `SearchQuery` 빌더 패턴
- doxus-cli: `project add/list/remove/enable/disable`, `index`, `search`, `status` 커맨드
- MCP 서버: 37개 `docnx_*` 도구 (3개 구현, 나머지 stub)

**Phase 2a: plugin-sdk + Obsidian 플러그인**
- `DocSource` trait: async_trait, Send+Sync, optional OAuth 메서드
- 공유 타입: PluginError, RawDocument, FetchAllOpts, DocumentStream
- `ObsidianPlugin`: walkdir로 `.md` 파일 읽기, 숨김 경로 필터링 (상대 경로 기준)
- 페이지네이션: cursor = offset 문자열 (opaque)
- 버그 수정: 절대 경로 컴포넌트 기준 hidden dir 필터 → 상대 경로 기준으로 수정

**Phase 2b: WasmDocSourceAdapter**
- `Arc<Mutex<Plugin>> + tokio::spawn_blocking` 패턴 구현
- `call_wasm<I: Serialize, O: DeserializeOwned>()` 제네릭 헬퍼
- `DocSource` trait 구현 (WASM 함수 없으면 빈 결과 반환)

**Phase 2c: Host Function 인프라**
- `PluginManifest`: ABI 버전 검증, `http_domains` 화이트리스트
- `KvStore`: `Arc<RwLock<HashMap>>` 플러그인 전용 KV 저장소
- `WasmDocSourceAdapter`에 `kv_get` / `kv_set` / `is_http_allowed` 메서드 추가

**Phase 2d: Auth 추상화**
- `SecretStore` trait: get / set / delete
- `MemorySecretStore`: 테스트 / CI용 인메모리 구현
- `OAuthFlow` 타입: auth_url, state, redirect_uri

**Phase 3: Confluence 플러그인 + Agent Sidecar**
- `ConfluencePlugin`: reqwest 기반, wiremock TDD 테스트
- Confluence REST API: content 목록 페이지네이션, health_check
- `ContentType::Html` plugin-sdk에 추가
- Agent sidecar: `CliKind` (ClaudeCode / GeminiCli / None), `detect_cli()`
- JSONL 프로토콜: `HostMessage` + `AgentMessage` (serde tagged enums)
- 환경변수 테스트 race condition 방지: `static ENV_LOCK: Mutex<()>` 도입

**보안 QA 수정 (code-reviewer + security-reviewer)**
- **CRITICAL — SSRF**: `url.contains(suffix)` 방식의 도메인 화이트리스트 우회 가능 → `url::Url::parse()`로 host만 비교, wildcard는 `host.ends_with(".suffix")`로 수정, 테스트 6개 추가
- **HIGH — 경로 주입**: `fetch_document`에서 `SourceDocId`를 URL에 직접 삽입 → ID 문자 검증 추가 (alphanumeric + hyphen + underscore만 허용)
- **MEDIUM — 락 패닉**: `MemorySecretStore` `RwLock::unwrap()` → `map_err(|_| AuthError::Keychain("lock poisoned"))`으로 교체

### 영향 범위

- `crates/core`: SearchEngine, DB 마이그레이션, EmbeddingProvider
- `crates/plugin-sdk`: DocSource trait, 공유 타입 전체
- `crates/plugins/obsidian`: ObsidianPlugin 구현
- `crates/plugins/confluence` (신규): ConfluencePlugin
- `crates/cli`: 전체 CLI 커맨드
- `crates/mcp-server`: 37개 도구 스캐폴드
- `crates/agent`: CliKind 감지, JSONL 프로토콜

## 결과

- 총 **68개 테스트**, 0 실패, 2 ignored (ONNX 모델 파일 필요)
- 보안 수정 완료: SSRF 우회 / 경로 주입 / 락 패닉 3건
- 커밋: `e2154c8` (Phase 2b~3 구현), `14f22ff` (보안 수정)
- Phase 0~3 기반 구현 완료, Phase 4 (플러그인 마켓) 준비 상태

## 교훈

- **ONNX 버전 이슈**: `ort = "2"` 지정 시 crates.io에서 찾지 못함. rc 버전은 반드시 전체 버전 문자열(`"2.0.0-rc.12"`)로 고정해야 한다.
- **WASM Send+Sync**: Extism `Plugin`은 Send=YES, Sync=NO. 멀티스레드 환경에서는 `Arc<Mutex<Plugin>> + tokio::spawn_blocking` 패턴이 유일한 안전한 해법이다. Phase 0-B PoC에서 이를 먼저 확정한 것이 Phase 2b 설계에 결정적이었다.
- **도메인 화이트리스트 보안**: 단순 문자열 `contains` 검사는 쿼리 파라미터나 경로에도 매칭돼 SSRF 우회가 가능하다. URL을 파싱한 뒤 host 컴포넌트만 비교해야 한다. 화이트리스트 구현은 반드시 negative 테스트(우회 시도)를 함께 작성할 것.
- **환경변수 테스트 격리**: `std::env::set_var`는 프로세스 전역 상태를 변경하므로 병렬 테스트에서 race condition이 발생한다. `static Mutex`로 직렬화하거나, 환경변수 대신 의존성 주입(DI) 패턴으로 설계하는 것이 근본적인 해결책이다.
- **ndarray 버전 충돌**: ort 내부에서 ndarray 0.17을 사용하는데 Cargo.toml에 0.16을 명시하면 충돌한다. ort가 re-export하는 버전을 직접 사용하거나 standalone 의존성을 제거해야 한다.

## 관련 문서

- [[doxus 아키텍처 설계]]
- [[obsidian-nexus]]