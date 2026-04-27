-- V39: 마지막 증분 동기화 시점 저장 (fetch_changes since 기준점)
ALTER TABLE projects ADD COLUMN last_fetched_at INTEGER;
