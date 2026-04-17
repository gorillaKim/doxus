-- V26: Reset last_synced and content_hash again to ensure they are picked up.
-- This is a belt-and-suspenders approach because V25 might have been applied 
-- before the logic was fully expanded.

UPDATE documents SET content_hash = '';
UPDATE source_instances SET last_synced = 0;
