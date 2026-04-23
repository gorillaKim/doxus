-- V32: 날짜 필터 성능 향상을 위한 documents 인덱스 추가
CREATE INDEX IF NOT EXISTS idx_documents_created_at ON documents(created_at);
CREATE INDEX IF NOT EXISTS idx_documents_updated_at ON documents(updated_at);
