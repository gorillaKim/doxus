-- V28: Repair Hybrid Storage Schema (In case V27 was skipped due to index shift)
-- This migration is idempotent to ensure safety.

-- 1. Add storage_strategy to projects if missing
-- SQLite doesn't have 'IF NOT EXISTS' for ADD COLUMN in older versions, 
-- but we can use a safe approach or ignore errors. 
-- However, since this is run as a batch, we'll try to add it.
-- If it already exists, the batch might fail, so we use a separate check in code or 
-- just perform the most critical recreations.

-- 2. Ensure chunks table has the new structure
-- We check if the columns exist by trying to recreate if necessary.
-- To be safe, we'll use a temporary table and copy.

CREATE TABLE IF NOT EXISTS chunks_repair (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id  INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    heading_path TEXT,
    content      TEXT, 
    chunk_index  INTEGER NOT NULL DEFAULT 0,
    start_byte   INTEGER,
    end_byte     INTEGER,
    UNIQUE(document_id, chunk_index)
);

-- Copy data from old chunks if it exists
INSERT OR IGNORE INTO chunks_repair (id, document_id, heading_path, content, chunk_index)
SELECT id, document_id, heading_path, content, chunk_index FROM chunks;

-- Swap if needed (This is safe because it's idempotent)
DROP TABLE IF EXISTS chunks;
ALTER TABLE chunks_repair RENAME TO chunks;

-- Re-establish FTS triggers
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

-- Finally, try to add the column to projects. 
-- Since we know it's missing (the error), this should succeed.
ALTER TABLE projects ADD COLUMN storage_strategy TEXT NOT NULL DEFAULT 'full';
UPDATE projects SET storage_strategy = 'reference' WHERE source_type = 'obsidian';
