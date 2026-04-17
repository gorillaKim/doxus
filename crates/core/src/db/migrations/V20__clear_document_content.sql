-- Clear all existing document content to save space.
-- The content is already stored in fragments in the 'chunks' table,
-- and the full original text can be re-fetched or read from local files if needed.

UPDATE documents SET content = '';

-- Optional: Re-calculate hash to ensure integrity, but we'll keep the existing hashes as they represent the original text.
