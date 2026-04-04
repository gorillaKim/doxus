-- V5: 문서 그래프 (aliases, links, tags, metadata)
CREATE TABLE IF NOT EXISTS document_aliases (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    alias       TEXT NOT NULL,
    UNIQUE(alias)
);

CREATE TABLE IF NOT EXISTS document_links (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id   INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    target_id   INTEGER REFERENCES documents(id) ON DELETE SET NULL,
    target_raw  TEXT NOT NULL,
    link_type   TEXT NOT NULL DEFAULT 'wikilink'
);

CREATE INDEX IF NOT EXISTS idx_links_source ON document_links(source_id);
CREATE INDEX IF NOT EXISTS idx_links_target ON document_links(target_id);

CREATE TABLE IF NOT EXISTS document_tags (
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    tag         TEXT NOT NULL,
    PRIMARY KEY (document_id, tag)
);

CREATE INDEX IF NOT EXISTS idx_tags_tag ON document_tags(tag);

CREATE TABLE IF NOT EXISTS document_metadata (
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (document_id, key)
);
