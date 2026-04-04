-- V1: projects 테이블
CREATE TABLE IF NOT EXISTS projects (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    description  TEXT,
    path         TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'active'
                 CHECK(status IN ('active', 'disabled')),
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);
