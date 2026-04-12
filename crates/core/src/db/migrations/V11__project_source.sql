-- V11: projects 테이블에 source_type, config_json 컬럼 추가
ALTER TABLE projects ADD COLUMN source_type TEXT NOT NULL DEFAULT 'obsidian';
ALTER TABLE projects ADD COLUMN config_json TEXT NOT NULL DEFAULT '{}';
