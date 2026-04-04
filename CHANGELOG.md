# Changelog

## [Unreleased]

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
