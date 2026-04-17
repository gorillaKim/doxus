# doxus — Comprehensive Unimplemented Items Analysis

**Generated:** 2026-04-10  
**Last Updated:** 2026-04-10 (re-verified against actual code)  
**Scope:** Full monorepo analysis (Rust crates + React/TypeScript frontend + Database)  
**Architecture Reference:** `.claude/rules/architecture.md` Phase 0-8 roadmap

---

## Executive Summary

doxus is substantially **Phase 1 complete** with extensive infrastructure in place. Most **Phase 2-8 features are architecture-planned but code-unstarted**. Below is a granular breakdown organized by category and severity.

---

## 1. MCP Server Tools — 39/39 Tools Declared, ~20 Partially Stubbed

**File:** `crates/mcp-server/src/lib.rs` (2,454 lines)

### 1.1 Fully Implemented ✓
- `doxus_status` — operational (line 146)
- `doxus_list_projects` — fully implemented (line 163)
- `doxus_add_project` — fully implemented (line 199)
- `doxus_remove_project` — fully implemented (index-only delete, line 225)
- `doxus_search` — hybrid FTS5 + vector (line 277)
- `doxus_get_document` — full content fetch (line 337)
- `doxus_get_section` — heading extraction (line 371)
- `doxus_get_metadata` — frontmatter + hashes (line 415)
- `doxus_get_toc` — table of contents
- `doxus_get_ranking` — popularity sorting
- `doxus_get_backlinks` / `doxus_get_links` — graph traversal
- `doxus_find_related` — RRF ranking
- `doxus_find_path` — shortest path
- `doxus_get_cluster` — multi-hop clustering
- `doxus_create_workspace_document` — full CRUD (line 1339)
- `doxus_update_workspace_document` — hash-based updates (line 1381)
- `doxus_delete_workspace_document` — deletion (line 1404)
- `doxus_list_workspace_documents` — filtering by type/status (line 1418)
- `doxus_apply_template` — workspace templates (line 1462)
- `doxus_diagnose` — server diagnostics
- `doxus_system_report` — full system health

### 1.2 Partial/Stubbed Implementations
| Tool | Status | Issue | Line |
|------|--------|-------|------|
| `doxus_index_project` | **Stub** | Always says "use CLI: `doxus index`" — never actually indexes | 254 |
| `doxus_sync_project` | **Stub** | Returns "use CLI" message — no incremental sync via MCP | ~1200 |
| `doxus_resolve_alias` | **Unimplemented** | No alias resolution logic | ~540 |
| `doxus_inspect_document` | **Stub** | Missing indexing state details | ~650 |
| `doxus_plugin_install` | **Stub** | Inserts to DB but doesn't download/verify WASM | 1134 |
| `doxus_plugin_remove` | **Stub** | Deletes from DB but doesn't clean up files | 1153 |
| `doxus_plugin_update` | **Stub** | Only updates version string | 1167 |
| `doxus_plugin_search` | **Partial** | Searches local DB only; no marketplace API | 1185 |
| `doxus_plugin_logs` | **Partial** | Fetches logs but missing detailed health | 1254 |
| `doxus_plugin_info` | **Unimplemented** | Function declared but body missing | ~1310 |

### 1.3 Known TODOs
```rust
// crates/mcp-server/src/main.rs:33
// TODO: Pass embedder to McpServer once it accepts an EmbeddingProvider.
```
**Impact:** Vector similarity search falls back to FTS5 only until embedder is passed.

---

## 2. Core Embedding Engine — ✅ Largely Implemented

**File:** `crates/core/src/embedding.rs`

### What's Done ✓
- `EmbeddingProvider` trait defined
- `OnnxEmbedder::embed()` **fully implemented** (lines 98–211): batch tokenization, tensor construction, ONNX inference, mean pooling with attention mask, L2 normalization
- `OllamaEmbedder` **fully implemented** (lines 252–307) — not just declared
- Cosine similarity function
- Graceful degradation: `NoOpEmbedder` fallback when no model is configured

### What's Missing ✗
| Item | Reason |
|------|--------|
| ONNX model file on disk | `all-MiniLM-L6-v2.onnx` not bundled — run `scripts/download-model.sh`. App starts without it (FTS-only mode). |
| Model caching | No persistent embedding cache |
| `McpServer` embedder wiring | `McpServer::new()` doesn't accept an `EmbeddingProvider` yet — see [main.rs:33](crates/mcp-server/src/main.rs) TODO. **This is the real Phase 1 blocker for vector search.** |

> **Note:** App starts and all tools work without the ONNX model (FTS-only mode). Vector search activates once McpServer accepts an embedder.

**Phase Status:** Phase 0 Track A — `embed()` complete; McpServer wiring pending (Phase 1).

---

## 3. Plugin System — Extism Integration Incomplete

**File:** `crates/core/src/plugin/`

### Plugin Manager (manager.rs)
- ✓ Plugin installation (database side)
- ✓ Signature verification framework
- ✗ **Actual WASM loading from disk** — only lists filenames

### WASM Adapter (wasm_adapter.rs)
| Feature | Status | Notes |
|---------|--------|-------|
| Extism initialization | ✓ | `new()` creates adapter |
| manifest.toml parsing | ✓ | Validates permissions |
| `http_request` Host Function | ✓ **Fully Implemented** | reqwest 기반 실제 HTTP 실행, 도메인 allowlist, 메서드 라우팅 (lines 195–247) |
| `secrets_get` Host Function | **TODO** | Env vars only, no keychain (line 149) |
| `kv_get` / `kv_set` | **Unimplemented** | Declared but no code |
| `progress` reporting | **Unimplemented** | For long-running indexing |
| `content_transform` | ✓ | Basic HTML stripping only |

**Phase Status:** Phase 2b (WASM MVP) — framework exists; Host Functions incomplete.

---

## 4. Synchronization Engine — ⚠️ Framework Implemented, Logic Incomplete

**File:** `crates/core/src/sync/`

### What Exists ✓
- `SyncJob` struct with reschedule logic
- `SyncScheduler` — **구현됨** (interval 기반, `due_instances()` 포함, 테스트 커버리지 있음)
- `SyncRunner::run_once()` — **구현됨** (async, fetch_changes 호출, cursor DB 저장)

### Critical Gaps ✗
| Component | Status |
|-----------|--------|
| Incremental sync detection | **Stub** — 플러그인 `fetch_changes()` 가 항상 empty ChangeSet 반환 (Obsidian, Confluence, GitHub 모두) |
| Delta tracking | **Missing** — 플러그인 레벨에서 timestamp 기반 변경 감지 없음 |
| Retry logic on errors | **Missing** — no exponential backoff |
| Background task runner loop | **Missing** — scheduler가 spawn되어 루프로 실행되지 않음 |
| Rate limit handling | **Missing** — doesn't respect retry_after |

> **Note:** SyncScheduler와 SyncRunner 코드 자체는 존재함. 핵심 문제는 각 플러그인의 `fetch_changes()` 구현이 stub이라는 점.

**Impact:** Projects only index once; no incremental updates. All documents re-indexed on each trigger.

**Phase Status:** Phase 6 (sync scheduler) — framework exists; plugin-level delta detection not started.

---

## 5. Agent Sidecar — Framework Exists, JSONL Incomplete

**File:** `crates/agent/src/`

### Implemented ✓
- `AgentManager` — spawns Node.js sidecar, lifecycle management
- `cli_detector.rs` — detects Claude Code / Gemini CLI
- `PromptLoader` — loads prompts from `~/.doxus/agents/librarian/`

### Unimplemented ✗
| Item | Issue |
|------|-------|
| ~~JSONL streaming I/O~~ | ~~Protocol types exist but no stdin/stdout pump~~ → **구현됨** (`sidecar.rs:54-87`, `send()`/`recv()`) |
| Tool execution bridge | Sidecar calls doxus_* tools; bridge missing |
| Session state machine | No start→message→result→close tracking |
| Tool result injection | MCP results not sent back to sidecar |

**Phase Status:** Phase 3 (agent sidecar) — JSONL I/O 완료; 도구 실행 브릿지 미구현.

---

## 6. Authentication & Secrets — Keychain Missing

**File:** `crates/core/src/auth.rs`

| Feature | Status |
|---------|--------|
| OAuth flow definition | ✓ `OAuthFlow` struct exists |
| `SecretStore` trait | ✓ Defined |
| Memory backend (tests) | ✓ `MemorySecretStore` works |
| **Keychain backend (prod)** | **Stub** — no `security` / `keyring` crate |
| OAuth callback handler | **Not implemented** |
| Token refresh logic | **Missing** |
| Session persistence | **Missing** |

**Impact:** Plugin authentication via env vars only; no secure credential storage.

---

## 7. Marketplace — Skeleton Only

**File:** `crates/core/src/marketplace/`

| Module | Status | Notes |
|--------|--------|-------|
| `registry.rs` | **Stub** | No registry client |
| `signing.rs` | **Partial** | Signature verification works; registry lookup missing |
| `installer.rs` | **Stub** | Doesn't download .wasm files |

**Missing Completely:**
- Official Registry Server (Documentation spec created, implementation pending)
- Plugin marketplace UI (searches local DB only)
- Plugin code signing automation
- Plugin version resolution (semver range)
- Dependency management

**Phase Status:** Phase 4 (marketplace) — not started.

---

## 8. Desktop App — Partial UI, Backend IPC Incomplete

**File:** `apps/desktop/src/`

### Pages Status
| Page | Status | Notes |
|------|--------|-------|
| DashboardPage.tsx | **Partial** | Shows stats but no real-time updates |
| SearchPage.tsx | **Working** | Uses `doxus_search` MCP tool |
| ProjectsPage.tsx | **Partial** | List works; enable/disable stubs |
| SettingsPage.tsx | **Stub** | No settings persistence |
| WorkspacePage.tsx | **TODO template** | Line 23: "TODO 목록" declared but unimplemented |
| MarketPage.tsx | **Mock** | Hardcoded `MOCK_PLUGINS` array; no real marketplace |

### ChatDrawer (Agent Chat)
| Feature | Status |
|---------|--------|
| Agent session startup | **Stub** — `invoke('agent_session_start')` not wired |
| Message streaming | **Not implemented** — no WebSocket/chunked handling |
| Tool use UI | **Missing** — doesn't display tool calls |
| Session history | **Stub** — uses store but no persistence |

### Tauri IPC Commands
| Command | Status |
|---------|--------|
| `search_documents` | ✓ Works |
| `add_project` | ✓ Works |
| `plugin_get_auth_status` | **Stub** — always returns `false` |
| `plugin_set_auth_*` | **Not implemented** |
| `agent_session_start` | **Not implemented** |
| `workspace_apply_template` | **Stub** — doesn't hydrate template |

**Phase Status:** Phase 8 (Desktop UI) — waiting on backend completion.

---

## 9. Database Schema — Mostly Complete, Some Tables Unused

**File:** `crates/core/src/db/migrations/`

### Tables Declared but Unused
| Table | Purpose | Status |
|-------|---------|--------|
| `plugins` | Plugin metadata | Partially used |
| `source_instances` | Plugin per-project config | Unused by sync runner |
| `workspace_documents` | Notes in workspace | MCP tools exist; desktop integration missing |
| `workspace_templates` | Reusable doc templates | Application stub |
| `plugin_logs` | Plugin runtime logs | Table exists; no log sink |
| `session_tokens` | OAuth tokens | Not created — no keychain |

### Indexing Gaps
- No FTS5 trigger to auto-update search index
- sqlite-vec extension load fails silently if missing
- No schema validation on startup

---

## 10. CLI Commands — Mostly Working, Some Missing

**File:** `crates/cli/src/main.rs`

### Implemented ✓
- `doxus project add/list/remove/enable/disable`
- `doxus index` — indexing pipeline
- `doxus search <query>` — uses SearchEngine
- `doxus status`
- `doxus plugin list/status`
- `doxus workspace list/create`

### Unimplemented ✗
| Command | Issue |
|---------|-------|
| `doxus plugin install <url>` | Action missing from enum |
| `doxus plugin remove/update` | Actions missing |
| `doxus workspace delete` | Action missing |
| `doxus sync` | Not in Commands enum |
| `doxus agent start` | Not in Commands enum |

### Error Handling Issues
Lines 563, 579, 581의 `panic!()` 는 **테스트 코드(`#[test]`) 내부**에 있음 — 프로덕션 코드 아님.

```rust
_ => panic!("expected Search command"),  // Line 563 — test only
_ => panic!("expected Add action"),      // Line 579 — test only
_ => panic!("expected Project command"), // Line 581 — test only
```

---

## 11. Plugin SDK — Trait Defined, Test Plugin Stub

**File:** `crates/plugin-sdk/src/lib.rs`

| Item | Status |
|------|--------|
| `DocSource` trait | ✓ Defined with all methods |
| `PluginMetadata` | ✓ Complete |
| `RawDocument` | ✓ Complete |
| `PluginError` enum | ✓ Complete |
| Test plugin fixture | **Stub** — returns empty vecs (line 21: `documents: vec![]`) |

---

## 12. Obsidian Plugin — In-Process, Incomplete

**File:** `crates/plugins/obsidian/src/lib.rs`

| Feature | Status |
|---------|--------|
| Vault scanning | ✓ Implemented |
| Document parsing | ✓ Implemented |
| **Frontmatter extraction** | **Stub** — no tag parsing (line 192: `tags: vec![]`) |
| **`fetch_changes()`** | **Stub** — always returns empty (line 214) |
| **Link extraction** | **Missing** — no backlink/forward link extraction |

---

## 13. Confluence Plugin — WASM, Mostly Stub

**File:** `crates/plugins/confluence/src/lib.rs`

| Feature | Status |
|---------|--------|
| OAuth flow | **Not implemented** |
| REST API client | **Stub** — no actual HTTP calls |
| `fetch_all()` | **Stub** — returns `vec![]` |
| `fetch_changes()` | **Stub** — returns empty |

**Impact:** Cannot ingest Confluence; Phase 3 blocker.

---

## 14. GitHub Plugin — WASM, Mostly Stub

**File:** `crates/plugins/github/src/lib.rs`

| Feature | Status |
|---------|--------|
| REST API client | **Stub** — no GitHub API calls |
| Issues/Wiki/Discussions fetching | **Not implemented** |
| Authentication | **Not implemented** — no token handling |
| `fetch_changes()` | **Stub** — returns empty |

**Impact:** Cannot ingest GitHub; Phase 5 blocker.

---

## 15. Observability & Logging — Minimal

**File:** `crates/core/src/observability.rs`

| Feature | Status |
|---------|--------|
| Tracing subscriber init | ✓ |
| Audit event types | ✓ Defined (IndexStart, IndexComplete, PluginError, SyncStart, SyncComplete) |
| **Audit log query** | **Not implemented** |
| **Performance metrics** | **Missing** |
| **Error aggregation** | **Missing** |

---

## Summary Table — Priority Work (Updated)

### Phase 1 Blockers (Critical)
| Item | Est. LOC | Why Critical |
|------|----------|--------------|
| ~~Implement OnnxEmbedder::embed()~~ | ~~150~~ | ~~No vector search~~ → **완료** |
| Pass embedder to McpServer | 50 | Vector similarity disabled — 실제 Phase 1 블로커 |
| Wire IndexEngine to MCP API | 200 | MCP 경유 인덱싱 미작동 |
| Spawn background sync loop | 100 | SyncScheduler가 실제로 실행되지 않음 |

### Phase 2-4 Core Features (Important)
| Item | Est. LOC | Phase | Why Important |
|------|----------|-------|--------------|
| ~~Complete http_request Host Function~~ | ~~200~~ | ~~2b~~ | ~~Stub~~ → **완료** |
| `secrets_get` keychain 연동 | 80 | 2c | env var만 지원, 보안 취약 |
| `kv_get` / `kv_set` Host Function | 100 | 2b | WASM 플러그인 상태 저장 불가 |
| Implement OAuth flow | 250 | 2d | External plugins can't auth |
| Plugin registry server 구축 | 300 | 9 | No official hosting |
| Plugin registry client | 200 | 4 | No marketplace |
| Sync incremental logic (플러그인 레벨) | 300 | 6 | 플러그인 `fetch_changes()` 전부 stub |
| Confluence/GitHub plugins | 600 | 3-5 | Can't ingest data sources |

### Lower Priority (Polish)
| Item | Est. LOC | Phase |
|------|----------|-------|
| ~~Agent ChatDrawer JSONL I/O~~ | ~~200~~ | ~~3~~ | → **완료** (`sidecar.rs`) |
| Agent 도구 실행 브릿지 | 150 | 3 | JSONL 연결됐으나 doxus_* 호출 미연결 |
| Workspace templates | 150 | 7 | |
| Obsidian frontmatter 태그 파싱 | 50 | 2a | `tags: vec![]` stub |
| Obsidian `fetch_changes()` | 100 | 2a | 항상 empty ChangeSet 반환 |

---

## Quick Reference: Start Here For...

| Goal | File to Edit |
|------|--------------|
| Vector search 활성화 | `crates/mcp-server/src/lib.rs` → McpServer에 embedder 필드 추가 + `tool_search` 업그레이드 |
| Plugin install/update | `crates/mcp-server/src/lib.rs:1134+` → implement WASM download |
| Incremental sync | `crates/plugins/obsidian/src/lib.rs` → implement `fetch_changes()` |
| Background sync loop | `crates/core/src/sync/scheduler.rs` → spawn async loop |
| Agent chat | `apps/desktop/src/components/layout/ChatDrawer.tsx` + `crates/agent/src/sidecar.rs` (I/O 완료, 브릿지만 남음) |
| Confluence plugin | `crates/plugins/confluence/src/lib.rs` → implement Confluence REST API |
| OAuth | `crates/core/src/auth.rs` + plugin SDK oauth methods |
| Workspace templates | `crates/mcp-server/src/lib.rs:1462+` → template hydration |

---

**Total Estimated Work:** ~3-4 months focused development for full MVP  
**Generated by Explorer Agent — 2026-04-10**
