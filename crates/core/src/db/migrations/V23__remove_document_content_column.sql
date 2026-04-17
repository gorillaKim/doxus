-- V23: Remove the redundant 'content' column from 'documents' table.
-- Since Phase 1 implemented On-Demand Retrieval via DocumentService, 
-- we no longer need the full text stored in the primary table.

ALTER TABLE documents DROP COLUMN content;
