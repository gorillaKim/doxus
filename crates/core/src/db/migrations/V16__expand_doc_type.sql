-- V16: templates.doc_type CHECK 제약 확장 (devlog, weekly, study, library, article 추가)
-- SQLite는 CHECK 제약 변경 불가 → 테이블 재생성

CREATE TABLE IF NOT EXISTS templates_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT,
    doc_type    TEXT NOT NULL DEFAULT 'note'
                CHECK(doc_type IN (
                    'note','meeting','decision','journal','retrospective',
                    'todo','techspec','devlog','weekly','study','library','article','history','other'
                )),
    content     TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL
);

INSERT INTO templates_new SELECT * FROM templates;

DROP TABLE templates;

ALTER TABLE templates_new RENAME TO templates;

CREATE INDEX IF NOT EXISTS idx_templates_project ON templates(project_id);
