-- V36: 프로젝트 신선도 설정

-- add freshness_policy_json to projects
ALTER TABLE projects ADD COLUMN freshness_policy_json TEXT;
