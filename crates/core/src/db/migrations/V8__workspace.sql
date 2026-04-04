-- V8: 워크스페이스 + 템플릿 (Phase 7에서 실제 구현)
CREATE TABLE IF NOT EXISTS workspace_templates (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    kind        TEXT NOT NULL DEFAULT 'user'
                CHECK(kind IN ('builtin', 'user')),
    template    TEXT NOT NULL,  -- Handlebars 마크다운
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workspace_documents (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path   TEXT NOT NULL UNIQUE,
    title       TEXT,
    doc_type    TEXT NOT NULL DEFAULT 'note'
                CHECK(doc_type IN ('note', 'meeting', 'decision', 'project', 'journal', 'template', 'other')),
    status      TEXT NOT NULL DEFAULT 'draft'
                CHECK(status IN ('draft', 'active', 'archived', 'done')),
    priority    TEXT NOT NULL DEFAULT 'medium'
                CHECK(priority IN ('low', 'medium', 'high', 'critical')),
    tags        TEXT,
    content_hash TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);
