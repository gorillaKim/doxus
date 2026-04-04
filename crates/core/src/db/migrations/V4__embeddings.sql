-- V4: 벡터 임베딩 (sqlite-vec vec0 가상 테이블)
-- sqlite-vec 익스텐션이 로드된 후 실행
CREATE VIRTUAL TABLE IF NOT EXISTS chunk_embeddings USING vec0(
    chunk_id INTEGER PRIMARY KEY,
    embedding FLOAT[384]
);
