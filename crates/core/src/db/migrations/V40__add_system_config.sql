-- V40: 앱 버전 추적 및 post-update 마이그레이션을 위한 system_config 테이블
CREATE TABLE IF NOT EXISTS system_config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- 부트스트랩: 기존 사용자 첫 적용 시 '0.0.0'으로 초기화.
-- 신규 설치와 동일하게 full migration path를 밟도록 함.
INSERT OR IGNORE INTO system_config (key, value) VALUES ('last_run_version', '0.0.0');
