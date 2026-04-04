# Phase 0-B: Extism Plugin Send+Sync 判定

**Date**: 2026-04-04
**Status**: ✅ Complete
**extism Version**: 1.21.0

## 実行結果

```
[2026-04-04T00:34:04.097167Z INFO extism_poc] === Extism WASM PoC — Phase 0-B ===
┌─ Extism Plugin Send+Sync Verdict ───────────────────────┐
│ Send: YES  Sync: NO                                   │
│ Adapter pattern: Arc<Mutex<Plugin>> + spawn_blocking
└─────────────────────────────────────────────────────────┘

[2026-04-04T00:34:04.097527Z INFO extism_poc] Loading minimal WASM module...
[2026-04-04T00:34:04.138545Z INFO extism_poc] ✅ WASM plugin loaded successfully
[2026-04-04T00:34:04.138963Z INFO extism_poc] Testing Arc<Mutex<Plugin>> + spawn_blocking pattern...
[2026-04-04T00:34:04.141188Z INFO extism_poc] ✅ spawn_blocking: OK
[2026-04-04T00:34:04.141432Z INFO extism_poc] ✅ HttpRequest/HttpResponse JSON serialization OK

=== Phase 0-B Summary ===
✅ extism crate compiles and links
✅ Plugin: Send  — can move across threads
⚠️  Plugin: !Sync — requires Mutex for shared access
✅ Pattern decided: Arc<Mutex<Plugin>> + tokio::spawn_blocking
✅ HttpRequest/HttpResponse are serde JSON-serializable
→  Phase 2b WasmDocSourceAdapter: use spawn_blocking pattern
```

## 判定結果

| 項目 | 結果 | 根拠 |
|------|------|------|
| **Plugin: Send** | ✅ YES | extism 1.21.0 Plugin は Send を実装 |
| **Plugin: Sync** | ❌ NO | extism 1.21.0 Plugin は Sync を実装しない |
| **Build Status** | ✅ Success | `cargo build -p extism-poc` 成功 |
| **Runtime Status** | ✅ Success | `cargo run -p extism-poc` 成功 |

## Phase 2b WasmDocSourceAdapter アーキテクチャ決定

extism 1.x の Plugin が `Send + !Sync` であることが確認されたため、以下のアーキテクチャを採用する:

### アダプターパターン

```rust
pub struct WasmDocSourceAdapter {
    /// Plugin インスタンスを Arc<Mutex<_>> で包装
    /// Sync 要件を満たすため必須
    plugin: Arc<Mutex<Plugin>>,
}

#[async_trait]
impl DocSource for WasmDocSourceAdapter {
    async fn fetch_all(&self, opts: FetchAllOpts) -> Result<DocumentStream, PluginError> {
        let plugin_clone = self.plugin.clone();
        // spawn_blocking で Plugin メソッドを同期コンテキストで実行
        tokio::task::spawn_blocking(move || {
            let guard = plugin_clone.lock().unwrap();
            // synchronous Plugin method call
            guard.call("fetch_all", ...)
        })
        .await
        .map_err(|e| PluginError::Internal(e.to_string()))?
    }
}
```

### 理由

1. **Plugin: Send**
   - Plugin インスタンスはスレッド間で移動可能
   - Arc でラップして複数の非同期タスクから共有可能

2. **Plugin: !Sync**
   - Plugin は共有参照からの並行アクセスを許可しない
   - Mutex で保護し、排他的アクセスを保証

3. **spawn_blocking パターン**
   - tokio 非同期ランタイムと Plugin の同期 API を統合
   - blocking operation として分類され、tokio の main thread をブロックしない
   - 複数の fetch_all 呼び出しは spawn_blocking task pool で順序付けされる

### 制約事項

- **並行アクセス**: 複数の fetch_all/fetch_changes は順序付けされる (Mutex により)
- **パフォーマンス**: WASM プラグインの複数同時実行は Phase 3+ で検討
  - マルチインスタンス: `source_instances` テーブルで異なる Plugin インスタンスを保有可能
  - 並行実行: Phase 2c Host Function 拡張後に独立した Plugin インスタンスの並行化が可能

## 実装チェックリスト (Phase 2b)

- [x] extism 1.x: Plugin Send+Sync 特性確認
- [x] Arc<Mutex<Plugin>> パターン実行可能性確認
- [x] spawn_blocking パターン実行可能性確認
- [x] HttpRequest/HttpResponse JSON シリアライゼーション確認
- [ ] WasmDocSourceAdapter 実装 (Phase 2b)
- [ ] Host Function (http_request, log) 実装 (Phase 2b)
- [ ] WASM プラグイン マニフェスト検証 (Phase 2c)
- [ ] OAuth フロー対応 (Phase 2d)

## 主要な確認事項

### ✅ 成功した項目

1. **extism crate コンパイル**: extism 1.21.0 は安定版、Wasmtime 41.0.4 ベース
2. **WASM プラグインロード**: 最小限の WASM モジュール (memory export) で成功
3. **Arc<Mutex<Plugin>> + spawn_blocking パターン**:
   - Plugin を Arc<Mutex<_>> にラップ可能
   - spawn_blocking task 内で lock/unlock 成功
   - エラーなし

4. **型安全なシリアライゼーション**:
   - HttpRequest/HttpResponse は serde で JSON シリアライゼーション可能
   - WASM 境界を越えた通信プロトコルは ready

### ⚠️ 注意事項

1. **Plugin !Sync**: マルチスレッド共有は Mutex 必須
2. **spawn_blocking オーバーヘッド**: 各 fetch_all は blocking task として実行
   - 大量並行要求の場合、Phase 3 で複数 Plugin インスタンス化を検討

## 次のステップ

### Phase 1 (優先度: 高)
- Cargo workspace 初期化
- crates/core, crates/plugin-sdk の実装開始
- CLI/Desktop/MCP スカフォルド

### Phase 2a (優先度: 高)
- DocSource trait の完全実装
- Obsidian プラグイン (in-process, Rust)

### Phase 2b (本トラック, 優先度: 高)
- WasmDocSourceAdapter の実装
- Host Function (http_request, log) の実装
- テスト WASM プラグイン (Rust → wasm32)

### Phase 2c-2d (優先度: 中)
- Host Function セキュリティ強化
- OAuth 認証フロー

---

**判定者**: Phase 0-B Extism PoC
**最終判定**: Arc<Mutex<Plugin>> + tokio::spawn_blocking パターンで Phase 2b 実装可能
