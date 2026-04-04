-- V3: chunks 테이블 + FTS5 가상 테이블
CREATE TABLE IF NOT EXISTS chunks (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id  INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    heading_path TEXT,
    content      TEXT NOT NULL,
    chunk_index  INTEGER NOT NULL DEFAULT 0,
    UNIQUE(document_id, chunk_index)
);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    content,
    heading_path,
    content='chunks',
    content_rowid='id',
    tokenize='unicode61'
);

-- FTS5 동기화 트리거
CREATE TRIGGER IF NOT EXISTS chunks_fts_insert AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts(rowid, content, heading_path)
    VALUES (new.id, new.content, new.heading_path);
END;

CREATE TRIGGER IF NOT EXISTS chunks_fts_delete AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content, heading_path)
    VALUES ('delete', old.id, old.content, old.heading_path);
END;

CREATE TRIGGER IF NOT EXISTS chunks_fts_update AFTER UPDATE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content, heading_path)
    VALUES ('delete', old.id, old.content, old.heading_path);
    INSERT INTO chunks_fts(rowid, content, heading_path)
    VALUES (new.id, new.content, new.heading_path);
END;
