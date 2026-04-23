# 문서 신선도 & 스케줄러 매니저 — 상세 구현 계획

> **상태**: 설계 확정 대기  
> **작성일**: 2026-04-23  
> **대상 버전**: v0.5.0  

---

## 1. 개요

doxus에 두 가지 핵심 기능을 추가한다:

1. **문서 신선도(Freshness) 관리** — 문서가 낡았는지, 안정적인 문서인지를 자동으로 분류하고 관리
2. **범용 스케줄러 매니저(SchedulerManager)** — 신선도 체크, 동기화, 에이전트 작업 등 모든 반복 작업을 통합 관리

### 해결하는 문제

| 문제 | 현재 | 개선 후 |
|------|------|---------|
| 방치된 문서 감지 불가 | `content_hash` 변경만 추적 | 시간 기반 감쇠 점수로 자동 감지 |
| 안정 문서 vs 부패 문서 구분 불가 | 없음 | 보관 등급(Short/Mid/Long) + 변경 패턴 분석 |
| 반복 작업이 분산 | `SyncManager`, 캐시 cleanup이 각각 별도 loop | `SchedulerManager`로 통합 |
| 에이전트 정기 작업 불가 | 수동 트리거만 가능 | 스케줄 기반 자동 에이전트 위임 |

---

## 2. 신선도 모델

### 2.1 점수 산출 공식

```
Score = 100 × e^(-λt)

λ = ln(2) / effective_half_life
effective_half_life = base_half_life(tier) × sensitivity_multiplier(mode)
t = 마지막 콘텐츠 변경일로부터 경과 일수
```

### 2.2 보관 등급 (Retention Tier)

| 등급 | 비유 | 반감기 | 소스 기본 매핑 |
|------|------|--------|---------------|
| 🥛 Short-term | 우유 | 45일 | GitHub Issue, API 문서 |
| 🍞 Mid-term | 빵 | 90일 | Confluence Wiki, Obsidian, Discussion |
| 🥫 Long-term | 통조림 | 180일 | ADR, 정책 문서 (제목/태그 패턴 매칭) |

### 2.3 감도 모드 (Sensitivity Mode) — 프로젝트별

| 모드 | 배율 | Short 실제 | Mid 실제 | Long 실제 |
|------|------|-----------|---------|----------|
| 🔴 Strict | ×0.5 | 22일 | 45일 | 90일 |
| 🟡 Normal | ×1.0 | 45일 | 90일 | 180일 |
| 🟢 Relaxed | ×1.5 | 67일 | 135일 | 270일 |

### 2.4 상태 전이

| 상태 | 조건 | 의미 |
|------|------|------|
| 🟢 Fresh | Score ≥ 70 | 최신 문서 |
| 🟡 Aging | 40 ≤ Score < 70 | 주의 필요 |
| 🔴 Stale | Score < 40 | 최신화 필요 |
| 🗑️ Obsolete | 사용자/에이전트 마킹 | 인덱스 제외 대상 |

> **Archival 개념 폐기** — Long-term 등급 승격으로 대체. 영구 면제 없음.

---

## 3. 구현 Phase 0: 스케줄러 매니저

### 3.1 파일 구조

```
crates/core/src/scheduler/          ← 신규 모듈
├── mod.rs                          ← SchedulerManager 정의 + re-exports
├── db.rs                           ← scheduled_jobs / job_runs DB 접근
├── executor.rs                     ← SystemExecutor + AgentExecutor
└── schedule.rs                     ← Schedule enum + next_run_at 계산
```

### 3.2 DB 마이그레이션: `V31__scheduler.sql`

```sql
-- V31: 범용 스케줄러

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
```

**등록 위치**: `crates/core/src/db/mod.rs` — `MIGRATIONS` 배열에 추가
```rust
// db/mod.rs 의 MIGRATIONS 배열 끝에 추가
("V31__scheduler", include_str!("migrations/V31__scheduler.sql")),
```

### 3.3 핵심 타입: `schedule.rs`

```rust
// crates/core/src/scheduler/schedule.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Schedule {
    Interval { seconds: u64 },
    Daily { hour: u8, minute: u8 },
    Weekly { day_of_week: u8, hour: u8, minute: u8 },
    Monthly { day_of_month: u8, hour: u8, minute: u8 },
}

impl Schedule {
    /// 현재 시각 기준으로 다음 실행 시점(unix timestamp)을 계산
    pub fn next_run_after(&self, now_epoch: i64) -> i64 {
        // chrono::NaiveDateTime 사용하여 계산
        // Interval: now + seconds
        // Daily/Weekly/Monthly: 다음 해당 시각
        todo!()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub id: i64,
    pub project_id: Option<i64>,
    pub job_name: String,
    pub executor: Executor,
    pub action: String,
    pub action_config: serde_json::Value,
    pub schedule: Schedule,
    pub enabled: bool,
    pub run_on_idle: bool,
    pub last_run_at: Option<i64>,
    pub next_run_at: i64,
    pub created_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Executor {
    System,
    Agent,
}
```

### 3.4 DB 접근 계층: `db.rs`

```rust
// crates/core/src/scheduler/db.rs

use rusqlite::Connection;
use super::schedule::{ScheduledJob, Schedule, Executor};

pub struct SchedulerDb<'a> {
    conn: &'a Connection,
}

impl<'a> SchedulerDb<'a> {
    pub fn new(conn: &'a Connection) -> Self { Self { conn } }

    /// enabled + next_run_at <= now 인 작업 목록 반환
    /// run_on_idle=1인 작업은 is_idle=true일 때만 포함
    pub fn due_jobs(&self, now: i64, is_idle: bool) -> Result<Vec<ScheduledJob>, rusqlite::Error> {
        // SELECT ... FROM scheduled_jobs
        // WHERE enabled = 1
        //   AND next_run_at <= ?1
        //   AND (run_on_idle = 0 OR ?2 = 1)
        todo!()
    }

    /// 작업 생성 → id 반환
    pub fn insert_job(&self, job: &ScheduledJob) -> Result<i64, rusqlite::Error> { todo!() }

    /// 실행 완료 후 next_run_at 갱신 + job_runs 기록
    pub fn mark_completed(&self, job_id: i64, result: &str) -> Result<(), rusqlite::Error> { todo!() }

    pub fn mark_failed(&self, job_id: i64, error: &str) -> Result<(), rusqlite::Error> { todo!() }

    pub fn list_jobs(&self, project_id: Option<i64>) -> Result<Vec<ScheduledJob>, rusqlite::Error> { todo!() }

    pub fn delete_job(&self, job_id: i64) -> Result<(), rusqlite::Error> { todo!() }

    pub fn disable_job(&self, job_id: i64) -> Result<(), rusqlite::Error> { todo!() }
}
```

### 3.5 실행자(Executor) 분기: `executor.rs`

```rust
// crates/core/src/scheduler/executor.rs

use crate::indexing::IndexingService;
use crate::sync::SyncRunner;

pub struct JobResult {
    pub success: bool,
    pub message: String,
}

/// doxus core가 직접 처리하는 작업
pub async fn execute_system(
    action: &str,
    config: &serde_json::Value,
    indexer: &IndexingService,
    // sync_runner: &SyncRunner,  ← 추후 SyncRunner 통합 시
) -> JobResult {
    match action {
        "full_index" => {
            let project = config["project"].as_str().unwrap_or("");
            match indexer.index_project(project).await {
                Ok(n) => JobResult { success: true, message: format!("{project}: {n}건 인덱싱") },
                Err(e) => JobResult { success: false, message: e },
            }
        }
        "incremental_sync" => {
            // SyncRunner::run_once_for() 호출 — Phase 0에서는 index_project로 대체
            let project = config["project"].as_str().unwrap_or("");
            match indexer.index_project(project).await {
                Ok(n) => JobResult { success: true, message: format!("{project}: {n}건 동기화") },
                Err(e) => JobResult { success: false, message: e },
            }
        }
        "freshness_batch" => {
            // Phase 1에서 구현, Phase 0에서는 no-op
            JobResult { success: true, message: "freshness_batch not yet implemented".into() }
        }
        _ => JobResult { success: false, message: format!("unknown system action: {action}") },
    }
}

/// Node.js sidecar에 위임하는 작업
/// 반환: 에이전트의 최종 응답 텍스트
pub async fn execute_agent(
    action: &str,
    config: &serde_json::Value,
    project_name: Option<&str>,
    // sidecar: &SyncSidecarManager,  ← 추후 연동
) -> JobResult {
    // Phase 0: 프롬프트 생성만 하고 sidecar 호출은 Phase 2에서
    let prompt = match action {
        "freshness_review" => format!(
            "프로젝트 '{}'의 신선도를 점검하세요.",
            project_name.unwrap_or("all")
        ),
        "custom_prompt" => config["prompt"].as_str().unwrap_or("").to_string(),
        _ => return JobResult { success: false, message: format!("unknown agent action: {action}") },
    };
    // TODO: sidecar.send_message(HostMessage::Start { ... })
    JobResult { success: true, message: format!("agent prompt queued: {}", &prompt[..prompt.len().min(100)]) }
}
```

### 3.6 매니저 본체: `mod.rs`

```rust
// crates/core/src/scheduler/mod.rs

pub mod db;
pub mod executor;
pub mod schedule;

pub use schedule::{Schedule, ScheduledJob, Executor};
pub use db::SchedulerDb;

use std::sync::{Arc, Mutex};
use crate::indexing::IndexingService;

pub struct SchedulerManager {
    conn: Arc<Mutex<rusqlite::Connection>>,
    indexer: Arc<IndexingService>,
}

impl SchedulerManager {
    pub fn new(
        conn: Arc<Mutex<rusqlite::Connection>>,
        indexer: Arc<IndexingService>,
    ) -> Self {
        Self { conn, indexer }
    }

    /// 매 tick마다 호출 — due 작업 실행
    pub async fn tick(&self, is_idle: bool) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let due_jobs = {
            let conn = self.conn.lock().unwrap();
            let sdb = SchedulerDb::new(&conn);
            sdb.due_jobs(now, is_idle).unwrap_or_default()
        };

        for job in due_jobs {
            let result = match job.executor {
                Executor::System => {
                    executor::execute_system(
                        &job.action,
                        &job.action_config,
                        &self.indexer,
                    ).await
                }
                Executor::Agent => {
                    let project_name = job.project_id.map(|_| job.job_name.as_str());
                    executor::execute_agent(
                        &job.action,
                        &job.action_config,
                        project_name,
                    ).await
                }
            };

            // DB에 결과 기록
            let conn = self.conn.lock().unwrap();
            let sdb = SchedulerDb::new(&conn);
            if result.success {
                let _ = sdb.mark_completed(job.id, &result.message);
            } else {
                let _ = sdb.mark_failed(job.id, &result.message);
            }
        }
    }

    /// 앱 최초 실행 시 기본 스케줄 등록
    pub fn ensure_defaults(&self) {
        let conn = self.conn.lock().unwrap();
        let sdb = SchedulerDb::new(&conn);
        let existing = sdb.list_jobs(None).unwrap_or_default();
        if existing.is_empty() {
            // freshness_batch — 매일 03:00
            // cleanup_obsolete — 매주 일 04:00
            // (구체적 INSERT 로직)
        }
    }
}
```

### 3.7 기존 코드 통합 포인트

#### `apps/desktop/src-tauri/src/state.rs`

```diff
 pub struct AppState {
     // ... 기존 필드 ...
     pub sync_manager: Arc<SyncManager>,
+    pub scheduler_manager: Arc<SchedulerManager>,
 }
```

#### `apps/desktop/src-tauri/src/main.rs` (setup 블록)

```diff
 // Start SyncManager background loop
 let manager_inner = manager.clone();
 tauri::async_runtime::spawn(async move {
     manager_inner.init_watchers().await;
     manager_inner.start_loop(rx).await;
 });

+// Start SchedulerManager tick loop
+let scheduler = state_arc.scheduler_manager.clone();
+tauri::async_runtime::spawn(async move {
+    scheduler.ensure_defaults();
+    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
+    loop {
+        interval.tick().await;
+        let is_idle = /* OS idle 감지 */ false; // TODO: Tauri idle detection
+        scheduler.tick(is_idle).await;
+    }
+});
```

#### `crates/core/src/lib.rs`

```diff
+pub mod scheduler;
```

### 3.8 기존 SyncManager와의 관계

```
현재 SyncManager (main.rs:240-244)
├── init_watchers()  ← 파일 시스템 감시 + Catch-up scan
└── start_loop(rx)   ← Focus/Periodic/FileEvent 트리거 처리

SchedulerManager (새로 추가)
├── tick loop (60초 간격)
├── SystemExecutor → IndexingService.index_project() 호출
│                  → (= SyncManager.run_task()와 동일한 함수 호출)
└── AgentExecutor → SyncSidecarManager를 통해 프롬프트 전달

※ SyncManager는 그대로 유지 — 실시간 이벤트(Focus, FileEvent) 처리 담당
※ SchedulerManager는 시간 기반 배치 작업 담당 — 역할 분리
```

---

## 4. 구현 Phase 1: 신선도 인프라

### 4.1 DB 마이그레이션: `V32__document_freshness.sql`

```sql
-- V32: 문서 신선도

CREATE TABLE IF NOT EXISTS document_freshness (
    document_id     INTEGER PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    freshness_score REAL NOT NULL DEFAULT 100.0,
    status          TEXT NOT NULL DEFAULT 'fresh'
                    CHECK(status IN ('fresh', 'aging', 'stale', 'obsolete')),
    retention_tier  TEXT NOT NULL DEFAULT 'mid'
                    CHECK(retention_tier IN ('short', 'mid', 'long')),
    tier_source     TEXT NOT NULL DEFAULT 'auto'
                    CHECK(tier_source IN ('auto', 'user')),
    change_count    INTEGER NOT NULL DEFAULT 0,
    first_seen_at   INTEGER NOT NULL,
    last_content_change INTEGER,
    reviewed_at     INTEGER,
    reviewed_by     TEXT,
    review_note     TEXT,
    review_count    INTEGER NOT NULL DEFAULT 0,
    score_updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS document_change_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    old_hash    TEXT NOT NULL,
    new_hash    TEXT NOT NULL,
    changed_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_change_log_doc ON document_change_log(document_id);
CREATE INDEX IF NOT EXISTS idx_freshness_status ON document_freshness(status);
CREATE INDEX IF NOT EXISTS idx_freshness_score ON document_freshness(freshness_score);
CREATE INDEX IF NOT EXISTS idx_freshness_tier ON document_freshness(retention_tier);

-- content_hash 변경 시 자동 추적
CREATE TRIGGER IF NOT EXISTS track_content_change
AFTER UPDATE ON documents
WHEN old.content_hash != new.content_hash
BEGIN
    INSERT INTO document_change_log (document_id, old_hash, new_hash, changed_at)
    VALUES (new.id, old.content_hash, new.content_hash, unixepoch());

    UPDATE document_freshness
    SET change_count = change_count + 1,
        last_content_change = unixepoch(),
        freshness_score = 100.0,
        status = 'fresh',
        score_updated_at = unixepoch()
    WHERE document_id = new.id;
END;
```

### 4.2 `V33__freshness_config.sql`

```sql
ALTER TABLE projects ADD COLUMN freshness_policy_json TEXT;
-- 기본값: {"sensitivity_mode":"normal","default_tier":"mid","thresholds":{"fresh":70,"aging":40}}
```

> ⚠️ **마이그레이션 번호 정리**: scheduler가 V31, freshness가 V32-V33. `db/mod.rs`의 `MIGRATIONS` 배열에 순서대로 추가.

### 4.3 파일 구조

```
crates/core/src/freshness/      ← 신규 모듈
├── mod.rs                      ← FreshnessService 정의 + re-exports
├── score.rs                    ← 점수 계산 + 상태 전이 로직
└── db.rs                       ← document_freshness 테이블 접근
```

### 4.4 점수 계산: `score.rs`

```rust
// crates/core/src/freshness/score.rs

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RetentionTier { Short, Mid, Long }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SensitivityMode { Strict, Normal, Relaxed }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FreshnessStatus { Fresh, Aging, Stale, Obsolete }

pub struct Thresholds {
    pub fresh: f64,  // 기본 70.0
    pub aging: f64,  // 기본 40.0
}

impl Default for Thresholds {
    fn default() -> Self { Self { fresh: 70.0, aging: 40.0 } }
}

pub fn base_half_life(tier: RetentionTier) -> f64 {
    match tier {
        RetentionTier::Short => 45.0,
        RetentionTier::Mid   => 90.0,
        RetentionTier::Long  => 180.0,
    }
}

pub fn sensitivity_multiplier(mode: SensitivityMode) -> f64 {
    match mode {
        SensitivityMode::Strict  => 0.5,
        SensitivityMode::Normal  => 1.0,
        SensitivityMode::Relaxed => 1.5,
    }
}

pub fn calculate_freshness(days_since_update: f64, tier: RetentionTier, mode: SensitivityMode) -> f64 {
    let half_life = base_half_life(tier) * sensitivity_multiplier(mode);
    let lambda = (2.0_f64).ln() / half_life;
    (100.0 * (-lambda * days_since_update).exp()).clamp(0.0, 100.0)
}

pub fn score_to_status(score: f64, thresholds: &Thresholds) -> FreshnessStatus {
    if score >= thresholds.fresh { FreshnessStatus::Fresh }
    else if score >= thresholds.aging { FreshnessStatus::Aging }
    else { FreshnessStatus::Stale }
}

/// 소스 타입 → 기본 등급 매핑
pub fn default_tier_for_source(plugin_id: &str) -> RetentionTier {
    if plugin_id.contains("github") { RetentionTier::Short }
    else { RetentionTier::Mid }
}
```

### 4.5 인덱싱 훅: `crates/core/src/indexing.rs` 수정

```rust
// indexing.rs의 문서 upsert 로직에 추가 (index_project 내 각 doc 처리 후)

// 신규 문서 → document_freshness 초기 레코드 삽입
// INSERT OR IGNORE INTO document_freshness (document_id, first_seen_at, score_updated_at, retention_tier)
// VALUES (?1, unixepoch(), unixepoch(), ?2)
// tier는 default_tier_for_source(plugin_id)로 결정
```

### 4.6 FreshnessService 주요 메서드

| 메서드 | 기능 | 호출 시점 |
|--------|------|----------|
| `recalculate_all()` | 전체 문서 점수 재계산 | freshness_batch 스케줄 |
| `get_score(doc_id)` | 단일 문서 lazy 계산 | 검색 결과 표시 시 |
| `get_stale_docs(project, status, limit)` | Stale/Aging 목록 | MCP doxus_get_stale_docs |
| `get_report(project)` | 프로젝트 요약 | MCP doxus_get_freshness_report |
| `mark_freshness(doc_id, action)` | 상태/등급 변경 | MCP doxus_mark_freshness |
| `suggest_long_term(project)` | Long-term 후보 감지 | 에이전트 워크플로우 |

---

## 5. 구현 Phase 2: MCP + 에이전트

### 5.1 MCP 도구 — `crates/mcp-server/src/tools/freshness.rs` (신규)

| 도구명 | 기능 | 파라미터 |
|--------|------|----------|
| `doxus_get_stale_docs` | 관리 필요 문서 조회 | project?, status, limit, sort_by |
| `doxus_get_freshness_report` | 프로젝트 신선도 요약 | project (필수) |
| `doxus_mark_freshness` | 등급/상태 변경 | project, id, action, note? |
| `doxus_set_sensitivity` | 감도 모드 변경 | project, mode |

### 5.2 MCP 도구 — `crates/mcp-server/src/tools/scheduler.rs` (신규)

| 도구명 | 기능 | 파라미터 |
|--------|------|----------|
| `doxus_create_schedule` | 스케줄 생성 | project?, job_name, executor, action, schedule |
| `doxus_list_schedules` | 스케줄 목록 | project?, executor?, include_runs? |
| `doxus_delete_schedule` | 스케줄 삭제/비활성화 | job_id, disable_only? |

### 5.3 도구 등록: `dispatch.rs` 추가

```rust
// dispatch.rs의 dispatch_tool 함수에 추가

// ── Freshness ──
"doxus_get_stale_docs"       => tools::freshness::get_stale_docs(server, id, args),
"doxus_get_freshness_report" => tools::freshness::get_freshness_report(server, id, args),
"doxus_mark_freshness"       => tools::freshness::mark_freshness(server, id, args),
"doxus_set_sensitivity"      => tools::freshness::set_sensitivity(server, id, args),

// ── Scheduler ──
"doxus_create_schedule"  => tools::scheduler::create_schedule(server, id, args),
"doxus_list_schedules"   => tools::scheduler::list_schedules(server, id, args),
"doxus_delete_schedule"  => tools::scheduler::delete_schedule(server, id, args),
```

### 5.4 도구 목록: `tool_list()` 추가

```rust
// dispatch.rs의 tool_list 함수에 추가 (tools 배열)

// Freshness
tool("doxus_get_stale_docs", "Get stale or aging documents that need attention", &[
    param_opt("project", "string", "Project name (omit for all)"),
    param_opt("status", "string", "Filter: stale|aging|all (default: stale)"),
    param_opt("limit", "number", "Max results (default 20)"),
    param_opt("sort_by", "string", "Sort: score_asc|last_change_asc"),
]),
tool("doxus_get_freshness_report", "Get project freshness summary report", &[
    param("project", "string", "Project name"),
]),
tool("doxus_mark_freshness", "Change document retention tier or freshness status", &[
    param("project", "string", "Project name"),
    param("id", "string", "Document ID"),
    param("action", "string", "Action: promote_long|promote_mid|demote_short|mark_reviewed|mark_obsolete|reset_score"),
    param_opt("note", "string", "Review note"),
]),
tool("doxus_set_sensitivity", "Change project freshness sensitivity mode", &[
    param("project", "string", "Project name"),
    param("mode", "string", "Mode: strict|normal|relaxed"),
]),

// Scheduler
tool("doxus_create_schedule", "Create a recurring scheduled job", &[
    param("job_name", "string", "Job display name"),
    param("executor", "string", "Executor: system|agent"),
    param("action", "string", "Action type"),
    param("schedule", "string", "Schedule JSON"),
    param_opt("project", "string", "Project name (omit for global)"),
]),
tool("doxus_list_schedules", "List all scheduled jobs", &[
    param_opt("project", "string", "Filter by project"),
    param_opt("include_runs", "boolean", "Include recent run history"),
]),
tool("doxus_delete_schedule", "Delete or disable a scheduled job", &[
    param("job_id", "number", "Job ID to delete"),
    param_opt("disable_only", "boolean", "Disable instead of delete"),
]),
```

### 5.5 에이전트 ToolBridge 허용 목록 추가

```rust
// crates/agent/src/tool_bridge.rs — ALLOWED_TOOLS에 추가
"doxus_get_stale_docs",
"doxus_get_freshness_report",
"doxus_mark_freshness",
"doxus_list_schedules",
```

> `doxus_set_sensitivity`, `doxus_create_schedule`, `doxus_delete_schedule`는 **에이전트 허용 제외** — 관리자 권한 수준 작업

### 5.6 에이전트 프롬프트 업데이트

**위치**: `~/.doxus/agents/prompts/librarian.md` (PromptLoader가 로드)

기존 사서 프롬프트에 신선도 점검 + 등급 승격 지침 추가.

---

## 6. 구현 Phase 3: Desktop UI

### 6.1 IPC 커맨드 추가

**위치**: `apps/desktop/src-tauri/src/commands/` (신규 또는 기존 파일 확장)

| 커맨드 | 기능 |
|--------|------|
| `get_freshness_dashboard` | 신선도 대시보드 데이터 |
| `update_freshness_mark` | UI에서 등급/상태 변경 |
| `update_sensitivity_mode` | 감도 모드 변경 |
| `list_scheduled_jobs` | 스케줄 목록 |
| `create_scheduled_job` | 스케줄 생성 |
| `delete_scheduled_job` | 스케줄 삭제 |
| `get_job_history` | 실행 이력 |

### 6.2 프론트엔드 페이지

| 페이지 | 위치 | 설명 |
|--------|------|------|
| `FreshnessPage.tsx` | `apps/desktop/src/pages/` | 신선도 대시보드 |
| `SchedulerPage.tsx` | `apps/desktop/src/pages/` | 스케줄 관리 |
| `SearchPage.tsx` 수정 | 기존 파일 | 검색 결과에 🥛🍞🥫 배지 추가 |
| `SettingsPage.tsx` 수정 | 기존 파일 | 감도 모드 설정 |

---

## 7. 검증 계획

### 7.1 단위 테스트

| 대상 | 테스트 내용 | 위치 |
|------|------------|------|
| `calculate_freshness()` | 경과일 × 등급 × 모드별 점수 검증 | `freshness/score.rs` |
| `score_to_status()` | 임계값 경계 테스트 | `freshness/score.rs` |
| `Schedule::next_run_after()` | 각 스케줄 타입별 다음 실행 시점 | `scheduler/schedule.rs` |
| `SchedulerDb::due_jobs()` | idle 필터, enabled 필터 | `scheduler/db.rs` |
| `track_content_change` 트리거 | content_hash 변경 시 자동 기록 | `db/mod.rs` tests |

### 7.2 통합 테스트

| 시나리오 | 방법 |
|----------|------|
| MCP `doxus_get_stale_docs` | MCP JSONL 프로토콜로 호출 후 결과 검증 |
| 스케줄 tick 실행 | TestDb에서 due job 생성 → tick() → job_runs 확인 |
| 에이전트 freshness_review | sidecar mock으로 프롬프트 전달 검증 |

### 7.3 수동 검증

- Desktop UI에서 신선도 대시보드 확인
- 검색 결과에 🥛🍞🥫 배지 노출 확인
- 감도 모드 변경 후 점수 재계산 확인
- 스케줄 CRUD 및 실행 이력 확인

---

## 8. 의존성 추가

| 크레이트 | 용도 | 적용 위치 |
|----------|------|----------|
| `chrono` | Schedule next_run 계산 | `doxus-core` |

> `chrono`는 이미 사용 가능할 수 있음 — `Cargo.toml` 확인 필요

---

## 9. 마이그레이션 순서 정리

현재 마지막: `V30__add_sync_config.sql`

| 번호 | 파일명 | 내용 |
|------|--------|------|
| V31 | `V31__scheduler.sql` | scheduled_jobs + job_runs |
| V32 | `V32__document_freshness.sql` | document_freshness + change_log + 트리거 |
| V33 | `V33__freshness_config.sql` | projects.freshness_policy_json |

---

## 10. 열린 결정사항

| # | 질문 | 권장안 |
|---|------|--------|
| 1 | 점수 갱신: 배치 + lazy 혼합? | ✅ 배치(일 1회) + 검색 시 lazy |
| 2 | Long-term 자동 승격: 사용자 확인 필수? | ✅ 에이전트가 제안 → 사용자 승인 |
| 3 | Obsolete 문서: 검색 제외? DB 보존? | ✅ 검색 제외 + DB 보존 |
| 4 | Agent 타임아웃 | 5분 권장 |
| 5 | job_runs 보존 기간 | 최근 100건 또는 30일 |
| 6 | idle 감지 방법 | Tauri window unfocused + 마지막 입력 5분 이상 |
