-- V35: 문서 신선도

CREATE TABLE IF NOT EXISTS document_freshness (
    document_id     INTEGER PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    freshness_score REAL NOT NULL DEFAULT 100.0,
    status          TEXT NOT NULL DEFAULT 'fresh'
                    CHECK(status IN ('fresh', 'aging', 'stale', 'obsolete')),
    retention_tier  TEXT NOT NULL DEFAULT 'mid'
                    CHECK(retention_tier IN ('short', 'mid', 'long')),
    tier_source     TEXT NOT NULL DEFAULT 'auto'
                    CHECK(tier_source IN ('auto', 'user')),
    change_count    INTEGER NOT NULL DEFAULT 0,
    first_seen_at   INTEGER NOT NULL,
    last_content_change INTEGER,
    reviewed_at     INTEGER,
    reviewed_by     TEXT,
    review_note     TEXT,
    review_count    INTEGER NOT NULL DEFAULT 0,
    score_updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS document_change_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    old_hash    TEXT NOT NULL,
    new_hash    TEXT NOT NULL,
    changed_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_change_log_doc ON document_change_log(document_id);
CREATE INDEX IF NOT EXISTS idx_freshness_status ON document_freshness(status);
CREATE INDEX IF NOT EXISTS idx_freshness_score ON document_freshness(freshness_score);
CREATE INDEX IF NOT EXISTS idx_freshness_tier ON document_freshness(retention_tier);

-- content_hash 변경 시 자동 추적
CREATE TRIGGER IF NOT EXISTS track_content_change
AFTER UPDATE ON documents
WHEN old.content_hash != new.content_hash
BEGIN
    INSERT INTO document_change_log (document_id, old_hash, new_hash, changed_at)
    VALUES (new.id, old.content_hash, new.content_hash, unixepoch());

    UPDATE document_freshness
    SET change_count = change_count + 1,
        last_content_change = unixepoch(),
        freshness_score = 100.0,
        status = 'fresh',
        score_updated_at = unixepoch()
    WHERE document_id = new.id;
END;
