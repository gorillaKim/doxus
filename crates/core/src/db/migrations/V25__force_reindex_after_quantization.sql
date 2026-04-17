-- V25: Force re-indexing of all documents.
-- This is necessary after the Phase 2 Vector Quantization (V24) because the vector table was dropped
-- and recreated with a different data type (int8), but the documents' content_hashes were preserved.
-- Resetting the hashes ensures the indexer treats all documents as "new" and generates fresh int8 vectors.

UPDATE documents SET content_hash = '';
UPDATE source_instances SET last_synced = 0;
