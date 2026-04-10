-- V10: plugin KV store (namespace-isolated, per-plugin persistent key-value)
CREATE TABLE IF NOT EXISTS plugin_kv (
    plugin_id   TEXT    NOT NULL,
    namespace   TEXT    NOT NULL,
    key         TEXT    NOT NULL,
    value       BLOB    NOT NULL,
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (plugin_id, namespace, key)
);
