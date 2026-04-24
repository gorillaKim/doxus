-- V37: 링크 해결 성능 최적화를 위한 인덱스 추가
-- 대소문자 구분 없는 검색을 위해 COLLATE NOCASE 인덱스 추가

CREATE INDEX IF NOT EXISTS idx_documents_title_nocase ON documents(title COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_documents_path_nocase ON documents(file_path COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_documents_source_id_nocase ON documents(source_doc_id COLLATE NOCASE);

-- 미결 링크 검색을 위한 조건부 인덱스
CREATE INDEX IF NOT EXISTS idx_links_unresolved ON document_links(target_raw) WHERE target_id IS NULL;
