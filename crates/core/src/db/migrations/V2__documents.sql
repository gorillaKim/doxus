-- V2: documents 테이블
CREATE TABLE IF NOT EXISTS documents (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id     INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source_doc_id  TEXT NOT NULL,
    file_path      TEXT,
    title          TEXT,
    content        TEXT NOT NULL,
    content_hash   TEXT NOT NULL,
    plugin_id      TEXT,
    indexing_status TEXT NOT NULL DEFAULT 'pending'
                    CHECK(indexing_status IN ('pending', 'indexed', 'failed')),
    last_indexed   INTEGER,
    UNIQUE(project_id, source_doc_id)
);

CREATE INDEX IF NOT EXISTS idx_documents_project ON documents(project_id);
CREATE INDEX IF NOT EXISTS idx_documents_hash ON documents(content_hash);
