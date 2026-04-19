-- V27: Hybrid Storage Schema (Storage Strategy support and Table Recreation)

-- 1. Add storage_strategy to projects table
-- SQLite doesn't support DEFAULT value in a specific way for existing rows without recreation sometimes,
-- but we can add it and then update.
ALTER TABLE projects ADD COLUMN storage_strategy TEXT NOT NULL DEFAULT 'full';

-- Set 'reference' strategy for existing obsidian projects
UPDATE projects SET storage_strategy = 'reference' WHERE source_type = 'obsidian';

-- 2. Recreate chunks table to allow NULL content and add byte offsets
CREATE TABLE chunks_new (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id  INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    heading_path TEXT,
    content      TEXT, -- ALLOW NULL for hybrid storage
    chunk_index  INTEGER NOT NULL DEFAULT 0,
    start_byte   INTEGER,
    end_byte     INTEGER,
    UNIQUE(document_id, chunk_index)
);

-- 3. Copy existing data
INSERT INTO chunks_new (id, document_id, heading_path, content, chunk_index)
SELECT id, document_id, heading_path, content, chunk_index FROM chunks;

-- 4. Swap tables
DROP TABLE chunks;
ALTER TABLE chunks_new RENAME TO chunks;

-- 5. Restore and update FTS triggers (Essential after DROP TABLE)
DROP TRIGGER IF EXISTS chunks_fts_insert;
CREATE TRIGGER chunks_fts_insert AFTER INSERT ON chunks 
WHEN (new.content IS NOT NULL)
BEGIN
    INSERT INTO chunks_fts(rowid, content, heading_path)
    VALUES (new.id, new.content, new.heading_path);
END;

DROP TRIGGER IF EXISTS chunks_fts_delete;
CREATE TRIGGER chunks_fts_delete AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content, heading_path)
    VALUES ('delete', old.id, old.content, old.heading_path);
END;

DROP TRIGGER IF EXISTS chunks_fts_update;
CREATE TRIGGER chunks_fts_update AFTER UPDATE ON chunks 
WHEN (new.content IS NOT NULL)
BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content, heading_path)
    VALUES ('delete', old.id, old.content, old.heading_path);
    INSERT INTO chunks_fts(rowid, content, heading_path)
    VALUES (new.id, new.content, new.heading_path);
END;

-- 6. Restore Cascade triggers (Essential after DROP TABLE)
DROP TRIGGER IF EXISTS chunks_vector_delete;
CREATE TRIGGER chunks_vector_delete BEFORE DELETE ON chunks BEGIN
    DELETE FROM chunk_embeddings WHERE chunk_id = old.id;
END;
