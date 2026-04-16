-- V18: 기본 워크스페이스(is_default=1) 및 기존 워크스페이스 데이터 영구 제거
-- 프로젝트 삭제 시 documents와 templates는 CASCADE DELETE됨

DELETE FROM projects 
WHERE is_default = 1 
   OR source_type = 'workspace' 
   OR name LIKE 'ws-%';

-- is_default 컬럼은 유지하되 (스키마 호환성), 더 이상 유니크 제약 조건이 필요 없으므로 인덱스 제거 시도
-- (SQLite는 DROP INDEX if exists 지원)
DROP INDEX IF EXISTS idx_single_default_workspace;
