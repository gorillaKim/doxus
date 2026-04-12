-- V13: documents 테이블에 메타정보 컬럼 추가
ALTER TABLE documents ADD COLUMN created_at INTEGER;
ALTER TABLE documents ADD COLUMN updated_at INTEGER;
ALTER TABLE documents ADD COLUMN metadata_json TEXT NOT NULL DEFAULT '{}';
