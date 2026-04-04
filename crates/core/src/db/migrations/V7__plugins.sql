-- V7: 플러그인 시스템 테이블
CREATE TABLE IF NOT EXISTS plugins (
    id            TEXT PRIMARY KEY,   -- 'com.doxus.confluence'
    name          TEXT NOT NULL,
    version       TEXT NOT NULL,
    kind          TEXT NOT NULL DEFAULT 'external'
                  CHECK(kind IN ('builtin', 'external')),
    trust_level   TEXT NOT NULL DEFAULT 'unverified'
                  CHECK(trust_level IN ('official', 'verified', 'unverified')),
    manifest_json TEXT NOT NULL DEFAULT '{}',
    wasm_sha256   TEXT,
    auto_update   INTEGER NOT NULL DEFAULT 0,
    enabled       INTEGER NOT NULL DEFAULT 1,
    installed_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS source_instances (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin_id   TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}',
    sync_status TEXT NOT NULL DEFAULT 'idle'
                CHECK(sync_status IN ('idle', 'syncing', 'error')),
    last_synced INTEGER,
    sync_cursor TEXT,
    created_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_source_instances_plugin ON source_instances(plugin_id);
CREATE INDEX IF NOT EXISTS idx_source_instances_project ON source_instances(project_id);

CREATE TABLE IF NOT EXISTS registry_cache (
    id           TEXT PRIMARY KEY,
    data_json    TEXT NOT NULL,
    cached_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS plugin_audit_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin_id   TEXT NOT NULL,
    event_type  TEXT NOT NULL,  -- 'install', 'uninstall', 'update', 'enable', 'disable'
    version     TEXT,
    occurred_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS plugin_logs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin_id   TEXT NOT NULL,
    level       TEXT NOT NULL CHECK(level IN ('error', 'warn', 'info', 'debug', 'trace')),
    message     TEXT NOT NULL,
    fields_json TEXT,
    occurred_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_plugin_logs_plugin ON plugin_logs(plugin_id);
CREATE INDEX IF NOT EXISTS idx_plugin_logs_time ON plugin_logs(occurred_at);
