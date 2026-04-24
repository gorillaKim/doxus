-- V34: 범용 스케줄러

CREATE TABLE IF NOT EXISTS scheduled_jobs (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id   INTEGER REFERENCES projects(id) ON DELETE CASCADE,

    -- 작업 정의
    job_name     TEXT NOT NULL,
    executor     TEXT NOT NULL CHECK(executor IN ('system', 'agent')),
    action       TEXT NOT NULL,
    action_config TEXT NOT NULL DEFAULT '{}',

    -- 스케줄
    schedule_json TEXT NOT NULL,

    -- 상태
    enabled      INTEGER NOT NULL DEFAULT 1,
    run_on_idle  INTEGER NOT NULL DEFAULT 1,
    last_run_at  INTEGER,
    next_run_at  INTEGER NOT NULL,

    -- 메타
    created_at   INTEGER NOT NULL,
    created_by   TEXT NOT NULL DEFAULT 'user'
                 CHECK(created_by IN ('user', 'system', 'agent'))
);

CREATE TABLE IF NOT EXISTS job_runs (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id       INTEGER NOT NULL REFERENCES scheduled_jobs(id) ON DELETE CASCADE,
    started_at   INTEGER NOT NULL,
    finished_at  INTEGER,
    status       TEXT NOT NULL DEFAULT 'running'
                 CHECK(status IN ('running', 'success', 'failed', 'cancelled')),
    result_text  TEXT,
    error_text   TEXT
);

CREATE INDEX IF NOT EXISTS idx_jobs_next_run ON scheduled_jobs(next_run_at)
    WHERE enabled = 1;
CREATE INDEX IF NOT EXISTS idx_job_runs_job ON job_runs(job_id);
