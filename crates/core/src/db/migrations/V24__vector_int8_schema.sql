-- V24: Convert chunk_embeddings to int8 for storage efficiency.
-- This is a destructive change (Phase 2 Quantization).
-- Existing float32 vectors will be removed; full re-indexing is required.

DROP TABLE IF EXISTS chunk_embeddings;

-- Create the virtual table with int8 type.
-- Note: sqlite-vec uses 'int8[dimension]' for 1-byte integer vectors.
CREATE VIRTUAL TABLE chunk_embeddings USING vec0(
  chunk_id INTEGER PRIMARY KEY,
  vector int8[384]
);
