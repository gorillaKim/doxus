-- V43: 에이전트 공동 참조 관계를 추적하기 위한 document_co_refs 테이블 추가
CREATE TABLE IF NOT EXISTS document_co_refs (
    doc_a_id            INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    doc_b_id            INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    co_occurrence_count INTEGER NOT NULL DEFAULT 1,
    last_accessed       INTEGER NOT NULL,
    PRIMARY KEY (doc_a_id, doc_b_id),
    CHECK (doc_a_id < doc_b_id)
);
CREATE INDEX IF NOT EXISTS idx_co_refs_b ON document_co_refs(doc_b_id);
