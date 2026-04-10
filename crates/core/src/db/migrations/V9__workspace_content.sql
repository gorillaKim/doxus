-- V9: workspace_documents에 content 컬럼 추가
ALTER TABLE workspace_documents ADD COLUMN content TEXT NOT NULL DEFAULT '';
