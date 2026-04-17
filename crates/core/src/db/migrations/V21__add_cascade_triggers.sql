-- V21: Delete triggers for vector index and content cache extension.

-- 1. Cascade delete for vector embeddings when a chunk is deleted.
CREATE TRIGGER IF NOT EXISTS chunks_vector_delete BEFORE DELETE ON chunks BEGIN
    DELETE FROM chunk_embeddings WHERE chunk_id = old.id;
END;

-- 2. Cascade delete for content cache when a document is deleted.
CREATE TRIGGER IF NOT EXISTS document_cache_delete BEFORE DELETE ON documents BEGIN
    DELETE FROM content_cache WHERE doc_id = old.source_doc_id;
END;

-- 3. Extend content_cache to support Conditional GET (etag, last_modified).
ALTER TABLE content_cache ADD COLUMN etag TEXT;
ALTER TABLE content_cache ADD COLUMN last_modified TEXT;
