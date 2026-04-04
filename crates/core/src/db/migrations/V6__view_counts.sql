-- V6: 조회수 추적
CREATE TABLE IF NOT EXISTS view_counts (
    document_id  INTEGER PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    count        INTEGER NOT NULL DEFAULT 0,
    last_viewed  INTEGER
);

CREATE TABLE IF NOT EXISTS audit_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    event_type  TEXT NOT NULL,
    payload     TEXT,
    occurred_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_project ON audit_log(project_id);
CREATE INDEX IF NOT EXISTS idx_audit_event ON audit_log(event_type);
