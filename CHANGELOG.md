# Changelog

## [Unreleased]

---

## [0.2.0] — 2026-06-11

### Phase 4~8 — Marketplace, Plugins, Sync, Workspace, Desktop

**Added**
- Plugin marketplace: ED25519 code signing, SHA-256 checksum, registry client
- `PluginManager::install_signed()` — enforces signature verification before WASM write
- GitHub Issues plugin (TDD, wiremock) with pagination and Bearer auth
- `SyncScheduler` — interval-based background sync job management
- `AuditEvent` structured logging via `tracing`
- Workspace + template management with SQLite V8 migration
- Desktop Tauri IPC stubs (`market_list_installed`, `get_workspaces`)
- React UI stubs: `MarketPage`, `WorkspacePage`, `useMarketStore`, `useWorkspaceStore`
- MCP: `doxus_plugin_install`, `doxus_plugin_remove`, `doxus_plugin_update` 실구현 (WASM 다운로드·검증·파일 정리 연동)
- MCP: `doxus_index_project`, `doxus_sync_project` core IndexingService 기반 실구현
- MCP: `doxus_plugin_info`, `doxus_resolve_alias` 실구현

**Changed**
- `SyncManager`: 8개 분산 Mutex → `SyncManagerState` 1개로 통합 (데드락 위험 구조 해소)
- `run_reindex()` (desktop): 독립 SQL → core `IndexingService` 경유로 통합
- `log_d!` 매크로: `eprintln!` → `tracing::debug!` 치환 (stdio 오염 방지)
- Plugin SDK: github/confluence/obsidian 공통 경로 조작·suffixing 로직을 `plugin-sdk`로 추출
- Migration: V18 중복 파일 제거, V22 placeholder 추가로 버전 연속성 확보

**Fixed**
- `lock().unwrap()` poison-unsafe 패턴을 `unwrap_or_else(|e| e.into_inner())` 로 일괄 치환
- CI: `cargo test --workspace` 전체 범위로 확대 + `cargo fmt --check` 게이트 추가

**Security**
- Fixed path traversal in plugin installer (`plugin_id` character validation)
- Fixed missing ED25519 signature enforcement in install path
- Fixed GitHub plugin missing `User-Agent` header (GitHub API requirement)
- Fixed owner/repo URL injection (`/` and `..` blocked in config validation)

---


## [0.1.0-phase3] — Phase 0~3

**Added**
- ONNX embedding engine (`EmbeddingProvider` trait, `OnnxEmbedder`, `OllamaEmbedder`)
- Extism WASM plugin runtime (`WasmDocSourceAdapter`, `Arc<Mutex<Plugin>>` pattern)
- Cargo workspace, SQLite DB (V1~V7 migrations), `SearchEngine` (FTS5 + sqlite-vec + RRF)
- CLI and MCP server scaffolds
- `DocSource` trait + Obsidian built-in plugin
- WASM host functions: `http_request`, `kv_get/set`, `progress`, `secrets_get`
- `PluginManifest` with HTTP domain allowlist (SSRF protection)
- `SecretStore` / `MemorySecretStore` + `OAuthFlow` abstraction
- Confluence plugin (TDD, wiremock)
- Agent sidecar JSONL protocol (`HostMessage`, `AgentMessage`) + CLI auto-detection

**Security**
- SSRF fix: domain allowlist uses URL parsing (not substring match)
- Path injection fix: `SourceDocId` validated before URL interpolation
- `RwLock` poison handling in `MemorySecretStore`
