-- V31: Add title column to chunks_fts for title-based search
-- chunks_fts previously only indexed content and heading_path, causing title-only searches to miss documents.

-- 1. Drop existing FTS table and triggers
DROP TRIGGER IF EXISTS chunks_fts_insert;
DROP TRIGGER IF EXISTS chunks_fts_delete;
DROP TRIGGER IF EXISTS chunks_fts_update;
DROP TABLE IF EXISTS chunks_fts;

-- 2. Recreate FTS5 without external content backing (simpler, self-contained)
CREATE VIRTUAL TABLE chunks_fts USING fts5(
    content,
    heading_path,
    title,
    tokenize='unicode61'
);

-- 3. Rebuild from existing chunks + documents
INSERT INTO chunks_fts(rowid, content, heading_path, title)
SELECT c.id, c.content, c.heading_path, d.title
FROM chunks c
JOIN documents d ON d.id = c.document_id
WHERE c.content IS NOT NULL;

-- 4. Recreate triggers to include title via subquery on documents
CREATE TRIGGER chunks_fts_insert AFTER INSERT ON chunks
WHEN (new.content IS NOT NULL)
BEGIN
    INSERT INTO chunks_fts(rowid, content, heading_path, title)
    SELECT new.id, new.content, new.heading_path, d.title
    FROM documents d WHERE d.id = new.document_id;
END;

CREATE TRIGGER chunks_fts_delete AFTER DELETE ON chunks BEGIN
    DELETE FROM chunks_fts WHERE rowid = old.id;
END;

CREATE TRIGGER chunks_fts_update AFTER UPDATE ON chunks
WHEN (new.content IS NOT NULL)
BEGIN
    DELETE FROM chunks_fts WHERE rowid = old.id;
    INSERT INTO chunks_fts(rowid, content, heading_path, title)
    SELECT new.id, new.content, new.heading_path, d.title
    FROM documents d WHERE d.id = new.document_id;
END;
