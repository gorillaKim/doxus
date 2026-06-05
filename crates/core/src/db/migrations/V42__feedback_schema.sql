CREATE TABLE IF NOT EXISTS document_feedbacks (
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    agent_id    TEXT NOT NULL,
    score       REAL NOT NULL CHECK(score >= -1.0 AND score <= 1.0),
    session_id  TEXT,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS idx_feedbacks_doc ON document_feedbacks(document_id);
CREATE INDEX IF NOT EXISTS idx_feedbacks_session ON document_feedbacks(session_id);
