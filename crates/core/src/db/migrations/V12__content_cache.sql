-- V12: content_cache 테이블 + source_instances에 cache_ttl_minutes 추가

-- 문서 내용 캐시 (TTL 기반, 스케줄러가 만료분 정리)
CREATE TABLE IF NOT EXISTS content_cache (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin_id   TEXT NOT NULL,        -- 'com.doxus.confluence'
    doc_id      TEXT NOT NULL,        -- source_doc_id (플러그인이 발급한 원본 ID)
    content     TEXT NOT NULL,
    cached_at   INTEGER NOT NULL,     -- Unix timestamp (seconds)
    expires_at  INTEGER NOT NULL,     -- Unix timestamp (seconds), cached_at + ttl_minutes*60
    UNIQUE(plugin_id, doc_id)
);

CREATE INDEX IF NOT EXISTS idx_content_cache_expires ON content_cache(expires_at);
CREATE INDEX IF NOT EXISTS idx_content_cache_lookup ON content_cache(plugin_id, doc_id);

-- source_instances에 캐시 TTL 옵션 추가 (NULL = 캐시 비활성화, 최소 10분)
ALTER TABLE source_instances ADD COLUMN cache_ttl_minutes INTEGER;
