-- V18: 워크스페이스 및 템플릿 기능 삭제에 따른 데이터 정리
-- 1. 워크스페이스 타입의 프로젝트 삭제
DELETE FROM projects WHERE source_type = 'workspace';

-- 2. 템플릿 관련 데이터 정리 (V15에서 이미 테이블은 삭제되었을 수 있으나 데이터 무결성 체크)
-- (만약 테이블이 여전히 존재한다면 데이터 삭제)
DELETE FROM projects WHERE source_type = 'template';
