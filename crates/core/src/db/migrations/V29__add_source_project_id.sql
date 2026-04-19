-- V29: projects 테이블에 source_project_id 컬럼 추가
ALTER TABLE projects ADD COLUMN source_project_id TEXT;

-- 기존 데이터 정합성을 위해 프로젝트 이름을 우선적으로 ID로 등록 (안정적이지 않을 수 있으나 초기값 제공)
-- 실제 안정적인 ID는 다음 인덱싱 사이클에서 플러그인이 업데이트함
UPDATE projects SET source_project_id = name WHERE source_project_id IS NULL;
