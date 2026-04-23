-- V33: 재인덱싱 이력 테이블 추가
CREATE TABLE IF NOT EXISTS reindex_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    scope TEXT NOT NULL,
    filter_json TEXT DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'pending',
    total_docs INTEGER DEFAULT 0,
    processed_docs INTEGER DEFAULT 0,
    error_message TEXT,
    started_at INTEGER NOT NULL,
    completed_at INTEGER
);
