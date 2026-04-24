use rusqlite::{Connection, Error};
use super::schedule::{ScheduledJob, Schedule, Executor};

pub struct SchedulerDb<'a> {
    conn: &'a Connection,
}

impl<'a> SchedulerDb<'a> {
    pub fn new(conn: &'a Connection) -> Self { 
        Self { conn } 
    }

    /// enabled + next_run_at <= now 인 작업 목록 반환
    /// run_on_idle=1인 작업은 is_idle=true일 때만 포함
    pub fn due_jobs(&self, now: i64, is_idle: bool) -> Result<Vec<ScheduledJob>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, job_name, executor, action, action_config, 
                    schedule_json, enabled, run_on_idle, last_run_at, next_run_at, created_by,
                    description, is_immutable 
             FROM scheduled_jobs 
             WHERE enabled = 1 
               AND next_run_at <= ?1 
               AND (run_on_idle = 0 OR ?2 = 1)"
        )?;

        let idle_flag = if is_idle { 1 } else { 0 };
        let iter = stmt.query_map(rusqlite::params![now, idle_flag], |row| {
            let config_str: String = row.get(5)?;
            let schedule_str: String = row.get(6)?;
            
            let executor_str: String = row.get(3)?;
            let executor = if executor_str == "system" { Executor::System } else { Executor::Agent };
            
            Ok(ScheduledJob {
                id: row.get(0)?,
                project_id: row.get(1)?,
                job_name: row.get(2)?,
                executor,
                action: row.get(4)?,
                action_config: serde_json::from_str(&config_str).unwrap_or(serde_json::Value::Null),
                schedule: serde_json::from_str(&schedule_str).unwrap_or_default(),
                enabled: row.get::<_, i64>(7)? == 1,
                run_on_idle: row.get::<_, i64>(8)? == 1,
                last_run_at: row.get(9)?,
                next_run_at: row.get(10)?,
                created_by: row.get(11)?,
                description: row.get(12)?,
                is_immutable: row.get::<_, i64>(13)? == 1,
            })
        })?;

        let mut jobs = Vec::new();
        for job in iter {
            jobs.push(job?);
        }
        Ok(jobs)
    }

    /// 작업 생성 → id 반환
    pub fn insert_job(&self, job: &ScheduledJob) -> Result<i64, Error> {
        let exec_str = match job.executor {
            Executor::System => "system",
            Executor::Agent => "agent",
        };
        let config_str = serde_json::to_string(&job.action_config).unwrap_or_else(|_| "{}".to_string());
        let schedule_str = serde_json::to_string(&job.schedule).unwrap_or_else(|_| "{}".to_string());
        
        self.conn.execute(
            "INSERT INTO scheduled_jobs (project_id, job_name, executor, action, action_config, 
                                       schedule_json, enabled, run_on_idle, next_run_at, created_at, created_by,
                                       description, is_immutable)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, unixepoch(), ?10, ?11, ?12)",
            rusqlite::params![
                job.project_id,
                job.job_name,
                exec_str,
                job.action,
                config_str,
                schedule_str,
                if job.enabled { 1 } else { 0 },
                if job.run_on_idle { 1 } else { 0 },
                job.next_run_at,
                job.created_by,
                job.description,
                if job.is_immutable { 1 } else { 0 },
            ]
        )?;
        
        Ok(self.conn.last_insert_rowid())
    }

    /// 실행 완료 후 next_run_at 갱신 + job_runs 기록
    pub fn mark_completed(&self, job_id: i64, result: &str) -> Result<(), Error> {
        let now = chrono::Utc::now().timestamp();
        
        // 1. Get schedule and target logic
        let schedule_str: String = self.conn.query_row(
            "SELECT schedule_json FROM scheduled_jobs WHERE id = ?1",
            [job_id],
            |r| r.get(0),
        )?;
        let schedule: Schedule = serde_json::from_str(&schedule_str).unwrap_or_default();
        let next_run = schedule.next_run_after(now);
        
        self.conn.execute(
            "UPDATE scheduled_jobs 
             SET last_run_at = ?1, next_run_at = ?2
             WHERE id = ?3",
            rusqlite::params![now, next_run, job_id]
        )?;
        
        self.conn.execute(
            "INSERT INTO job_runs (job_id, started_at, finished_at, status, result_text)
             VALUES (?1, ?2, ?3, 'success', ?4)",
            rusqlite::params![job_id, now, now, result]
        )?;
        
        Ok(())
    }

    pub fn mark_failed(&self, job_id: i64, error: &str) -> Result<(), Error> {
        let now = chrono::Utc::now().timestamp();
        
        // 1. Calculate next run even if failed
        let schedule_str: String = self.conn.query_row(
            "SELECT schedule_json FROM scheduled_jobs WHERE id = ?1",
            [job_id],
            |r| r.get(0),
        )?;
        let schedule: Schedule = serde_json::from_str(&schedule_str).unwrap_or_default();
        let next_run = schedule.next_run_after(now);
        
        self.conn.execute(
            "UPDATE scheduled_jobs 
             SET last_run_at = ?1, next_run_at = ?2
             WHERE id = ?3",
            rusqlite::params![now, next_run, job_id]
        )?;
        
        self.conn.execute(
            "INSERT INTO job_runs (job_id, started_at, finished_at, status, error_text)
             VALUES (?1, ?2, ?3, 'failed', ?4)",
            rusqlite::params![job_id, now, now, error]
        )?;
        
        Ok(())
    }

    pub fn list_jobs(&self, project_id: Option<i64>) -> Result<Vec<ScheduledJob>, Error> {
        let mut sql = "SELECT id, project_id, job_name, executor, action, action_config, 
                               schedule_json, enabled, run_on_idle, last_run_at, next_run_at, created_by,
                               description, is_immutable 
                        FROM scheduled_jobs".to_string();
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        
        if let Some(pid) = project_id {
            sql.push_str(" WHERE project_id = ?1");
            args.push(rusqlite::types::Value::Integer(pid));
        }

        let mut stmt = self.conn.prepare(&sql)?;
        let iter = stmt.query_map(rusqlite::params_from_iter(args), |row| {
            let config_str: String = row.get(5)?;
            let schedule_str: String = row.get(6)?;
            let executor_str: String = row.get(3)?;
            let executor = if executor_str == "system" { Executor::System } else { Executor::Agent };
            
            Ok(ScheduledJob {
                id: row.get(0)?,
                project_id: row.get(1)?,
                job_name: row.get(2)?,
                executor,
                action: row.get(4)?,
                action_config: serde_json::from_str(&config_str).unwrap_or(serde_json::Value::Null),
                schedule: serde_json::from_str(&schedule_str).unwrap_or_default(),
                enabled: row.get::<_, i64>(7)? == 1,
                run_on_idle: row.get::<_, i64>(8)? == 1,
                last_run_at: row.get(9)?,
                next_run_at: row.get(10)?,
                created_by: row.get(11)?,
                description: row.get(12)?,
                is_immutable: row.get::<_, i64>(13)? == 1,
            })
        })?;

        let mut jobs = Vec::new();
        for job in iter {
            jobs.push(job?);
        }
        Ok(jobs)
    }

    pub fn delete_job(&self, job_id: i64) -> Result<(), Error> {
        self.conn.execute("DELETE FROM scheduled_jobs WHERE id = ?1", [job_id])?;
        Ok(())
    }

    pub fn disable_job(&self, job_id: i64) -> Result<(), Error> {
        self.conn.execute("UPDATE scheduled_jobs SET enabled = 0 WHERE id = ?1", [job_id])?;
        Ok(())
    }

    pub fn update_job(&self, job_id: i64, job: &ScheduledJob) -> Result<(), Error> {
        let exec_str = match job.executor {
            Executor::System => "system",
            Executor::Agent => "agent",
        };
        let config_str = serde_json::to_string(&job.action_config).unwrap_or_else(|_| "{}".to_string());
        let schedule_str = serde_json::to_string(&job.schedule).unwrap_or_else(|_| "{}".to_string());

        self.conn.execute(
            "UPDATE scheduled_jobs 
             SET job_name = ?1, executor = ?2, action = ?3, action_config = ?4, 
                 schedule_json = ?5, run_on_idle = ?6, description = ?7
             WHERE id = ?8 AND is_immutable = 0",
            rusqlite::params![
                job.job_name,
                exec_str,
                job.action,
                config_str,
                schedule_str,
                if job.run_on_idle { 1 } else { 0 },
                job.description,
                job_id,
            ]
        )?;

        // If schedule changed, we should probably update next_run_at as well
        // But for simplicity, we'll let the next tick handle it or just keep current next_run_at
        // Re-calculating next_run_at
        let now = chrono::Utc::now().timestamp();
        let next_run = job.schedule.next_run_after(now);
        self.conn.execute(
            "UPDATE scheduled_jobs SET next_run_at = ?1 WHERE id = ?2",
            rusqlite::params![next_run, job_id]
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TestDb;
    use serde_json::json;

    #[test]
    fn test_insert_and_list() {
        let db = TestDb::new();
        let sdb = SchedulerDb::new(&db.conn);
        
        let job = ScheduledJob {
            id: 0,
            project_id: None,
            job_name: "test job".to_string(),
            description: None,
            executor: Executor::System,
            action: "echo".to_string(),
            action_config: json!({ "msg": "hello" }),
            schedule: Schedule::Interval { seconds: 120 },
            enabled: true,
            run_on_idle: false,
            is_immutable: false,
            last_run_at: None,
            next_run_at: 1000,
            created_by: "user".to_string(),
        };

        let id = sdb.insert_job(&job).unwrap();
        assert!(id > 0);

        let jobs = sdb.list_jobs(None).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].job_name, "test job");
        assert_eq!(jobs[0].schedule, Schedule::Interval { seconds: 120 });
    }

    #[test]
    fn test_due_jobs_filtering() {
        let db = TestDb::new();
        let sdb = SchedulerDb::new(&db.conn);
        
        let job1 = ScheduledJob {
            id: 0,
            project_id: None,
            job_name: "due now".to_string(),
            description: None,
            executor: Executor::System,
            action: "echo".to_string(),
            action_config: json!({}),
            schedule: Schedule::Interval { seconds: 60 },
            enabled: true,
            run_on_idle: false,
            is_immutable: false,
            last_run_at: None,
            next_run_at: 1000,
            created_by: "user".to_string(),
        };
        sdb.insert_job(&job1).unwrap();

        let job2 = ScheduledJob {
            id: 0,
            project_id: None,
            job_name: "future".to_string(),
            description: None,
            executor: Executor::System,
            action: "echo".to_string(),
            action_config: json!({}),
            schedule: Schedule::Interval { seconds: 60 },
            enabled: true,
            run_on_idle: false,
            is_immutable: false,
            last_run_at: None,
            next_run_at: 2000,
            created_by: "user".to_string(),
        };
        sdb.insert_job(&job2).unwrap();

        let due = sdb.due_jobs(1500, false).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].job_name, "due now");
    }
}
