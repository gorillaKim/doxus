use async_trait::async_trait;
use doxus_core::plugin::manager::PluginManager;
use doxus_plugin_sdk::{
    Capabilities, ChangeSet, DocSource, DocumentStream, FetchAllOpts, FetchChangesOpts,
    HealthStatus, PluginConfig, PluginError, PluginKind, PluginMetadata, PluginSecrets, RawDocument,
    SourceDocId,
};
use tempfile::TempDir;

// ── Minimal mock plugin ───────────────────────────────────────────────────────

struct MockPlugin {
    meta: PluginMetadata,
}

impl MockPlugin {
    fn new(id: &str) -> Self {
        Self {
            meta: PluginMetadata {
                id: id.into(),
                name: id.into(),
                version: "0.1.0".into(),
                kind: PluginKind::External,
            },
        }
    }
}

#[async_trait]
impl DocSource for MockPlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.meta
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            incremental_sync: false,
            oauth: false,
            native_search: false,
        }
    }

    async fn validate_config(&self, _config: &PluginConfig) -> Result<(), PluginError> {
        Ok(())
    }

    async fn initialize(
        &mut self,
        _config: PluginConfig,
        _secrets: PluginSecrets,
    ) -> Result<(), PluginError> {
        Ok(())
    }

    async fn fetch_all(&self, _opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        Ok(DocumentStream {
            documents: vec![],
            next_cursor: None,
            estimated_total: None,
        })
    }

    async fn fetch_changes(&self, _opts: FetchChangesOpts) -> Result<ChangeSet, PluginError> {
        Ok(ChangeSet {
            updated: vec![],
            deleted_ids: vec![],
            next_cursor: None,
        })
    }

    async fn fetch_document(&self, _id: &SourceDocId) -> Result<RawDocument, PluginError> {
        Err(PluginError::NotFound("mock".into()))
    }

    async fn health_check(&self) -> HealthStatus {
        HealthStatus {
            healthy: true,
            message: None,
        }
    }
}

// ── WASM fixture helpers ──────────────────────────────────────────────────────

/// Minimal valid WebAssembly module (8 bytes: magic + version).
const MINIMAL_WASM: &[u8] = b"\x00asm\x01\x00\x00\x00";

fn write_wasm_fixture(dir: &std::path::Path, plugin_id: &str) {
    std::fs::write(dir.join(format!("{plugin_id}.wasm")), MINIMAL_WASM).unwrap();
}

fn write_manifest_fixture(dir: &std::path::Path, plugin_id: &str) {
    let toml = format!(
        r#"plugin_id = "{plugin_id}"
display_name = "Test Minimal"
version = "0.1.0"
abi_version = 1
http_domains = []
kv_namespaces = []
secrets = []
"#
    );
    std::fs::write(dir.join(format!("{plugin_id}.manifest.toml")), toml).unwrap();
}

// ── Tests ─────────────────────────────────────────────────────────────────────

fn make_manager() -> (PluginManager, TempDir) {
    let tmp = TempDir::new().unwrap();
    let mut mgr = PluginManager::new(tmp.path().to_path_buf());
    mgr.register_factory("com.doxus.confluence", || {
        Box::new(MockPlugin::new("com.doxus.confluence")) as Box<dyn DocSource + Send + Sync>
    });
    mgr.register_factory("com.doxus.github", || {
        Box::new(MockPlugin::new("com.doxus.github")) as Box<dyn DocSource + Send + Sync>
    });
    (mgr, tmp)
}

#[test]
fn test_get_source_returns_confluence_plugin() {
    let (mgr, _tmp) = make_manager();
    let source = mgr.get_source("com.doxus.confluence");
    assert!(source.is_some(), "expected Some for com.doxus.confluence");
}

#[test]
fn test_get_source_returns_github_plugin() {
    let (mgr, _tmp) = make_manager();
    let source = mgr.get_source("com.doxus.github");
    assert!(source.is_some(), "expected Some for com.doxus.github");
}

#[test]
fn test_get_source_unknown_returns_none() {
    let (mgr, _tmp) = make_manager();
    let source = mgr.get_source("com.unknown.plugin");
    assert!(source.is_none(), "expected None for unknown plugin");
}

#[test]
fn test_get_source_metadata_id_matches() {
    let (mgr, _tmp) = make_manager();
    let source = mgr.get_source("com.doxus.confluence").unwrap();
    assert_eq!(source.metadata().id, "com.doxus.confluence");

    let (mgr2, _tmp2) = make_manager();
    let source2 = mgr2.get_source("com.doxus.github").unwrap();
    assert_eq!(source2.metadata().id, "com.doxus.github");
}

#[test]
fn get_source_loads_wasm_from_disk() {
    let tmp = TempDir::new().unwrap();
    write_wasm_fixture(tmp.path(), "com.test.minimal");
    write_manifest_fixture(tmp.path(), "com.test.minimal");

    let mgr = PluginManager::new(tmp.path().to_path_buf());
    let source = mgr.get_source("com.test.minimal");
    assert!(source.is_some(), "expected Some when .wasm + .manifest.toml present");
    assert_eq!(source.unwrap().metadata().id, "com.test.minimal");
}

#[test]
fn get_source_returns_none_without_manifest() {
    let tmp = TempDir::new().unwrap();
    write_wasm_fixture(tmp.path(), "com.test.minimal");
    // no manifest file written

    let mgr = PluginManager::new(tmp.path().to_path_buf());
    let source = mgr.get_source("com.test.minimal");
    assert!(source.is_none(), "expected None when .manifest.toml is missing");
}

#[test]
fn get_source_factory_takes_precedence_over_wasm() {
    let tmp = TempDir::new().unwrap();
    write_wasm_fixture(tmp.path(), "com.doxus.confluence");
    write_manifest_fixture(tmp.path(), "com.doxus.confluence");

    let mut mgr = PluginManager::new(tmp.path().to_path_buf());
    mgr.register_factory("com.doxus.confluence", || {
        Box::new(MockPlugin::new("com.doxus.confluence")) as Box<dyn DocSource + Send + Sync>
    });

    let source = mgr.get_source("com.doxus.confluence").unwrap();
    // MockPlugin's kind is External; WasmDocSourceAdapter's kind is also External,
    // but we verify via the metadata name which MockPlugin sets to the id string.
    assert_eq!(source.metadata().name, "com.doxus.confluence",
        "factory-provided plugin should be returned, not WASM adapter");
}
