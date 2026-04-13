-- V14: 워크스페이스 통합 — workspaces/workspace_documents/workspace_templates → projects/documents/templates

-- 1. projects에 is_default 컬럼 추가 (디폴트 워크스페이스 마커, 앱이 관리)
ALTER TABLE projects ADD COLUMN is_default INTEGER NOT NULL DEFAULT 0;

-- is_default=1은 반드시 하나만 존재 (부분 유니크 인덱스)
CREATE UNIQUE INDEX IF NOT EXISTS idx_single_default_workspace
    ON projects(is_default) WHERE is_default = 1;

-- 2. 전역 템플릿 테이블 신설
--    project_id NULL = 전역 템플릿 / NOT NULL = 특정 프로젝트 전용 템플릿
CREATE TABLE IF NOT EXISTS templates (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT,
    doc_type    TEXT NOT NULL DEFAULT 'note'
                CHECK(doc_type IN ('note','meeting','decision','journal','retrospective','todo','techspec','other')),
    content     TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_templates_project ON templates(project_id);

-- 3. workspace_templates → templates 마이그레이션
--    workspace_templates 컬럼: id, name, description, config_json, created_at
--    doc_type은 'note'로 기본값, content는 빈 문자열 (config_json은 Handlebars 설정이므로 변환 불가)
INSERT INTO templates (name, description, doc_type, content, created_at)
SELECT
    wt.name,
    wt.description,
    'note',
    '',
    wt.created_at
FROM workspace_templates wt
WHERE NOT EXISTS (
    SELECT 1 FROM templates t WHERE t.name = wt.name AND t.project_id IS NULL
);

-- 4. workspaces → projects 마이그레이션 (source_type='workspace')
--    path는 Rust seed 함수에서 실제 경로로 업데이트됨 (~ 미확장 문제 회피)
INSERT INTO projects (name, display_name, description, path, status, source_type, config_json, created_at, updated_at)
SELECT
    'ws-' || w.id,
    w.name,
    w.description,
    'pending-path',
    'active',
    'workspace',
    '{}',
    w.created_at,
    w.updated_at
FROM workspaces w
WHERE NOT EXISTS (
    SELECT 1 FROM projects p WHERE p.name = 'ws-' || w.id
);

-- 5. workspace_documents → documents 마이그레이션
--    workspace_documents에 workspace_id FK 없음 → 첫 번째 workspace-project에 귀속 (known limitation)
--    doc_type/status/priority/tags → metadata_json
INSERT INTO documents (
    project_id, source_doc_id, file_path, title, content, content_hash,
    indexing_status, metadata_json, created_at, updated_at
)
SELECT
    (SELECT id FROM projects WHERE source_type = 'workspace' ORDER BY created_at LIMIT 1),
    'ws-doc-' || wd.id,
    wd.file_path,
    wd.title,
    wd.content,
    wd.content_hash,
    'pending',
    json_object(
        'doc_type', wd.doc_type,
        'status', wd.status,
        'priority', wd.priority,
        'tags', wd.tags
    ),
    wd.created_at,
    wd.updated_at
FROM workspace_documents wd
WHERE EXISTS (
    SELECT 1 FROM projects WHERE source_type = 'workspace'
)
AND NOT EXISTS (
    SELECT 1 FROM documents d WHERE d.source_doc_id = 'ws-doc-' || wd.id
);
