use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct SyncJob {
    pub source_instance_id: i64,
    pub interval: Duration,
    pub next_run_at: Instant,
}

impl SyncJob {
    pub fn new(source_instance_id: i64, interval: Duration) -> Self {
        Self {
            source_instance_id,
            interval,
            next_run_at: Instant::now(),
        }
    }

    pub fn reschedule(&mut self) {
        self.next_run_at = Instant::now() + self.interval;
    }

    pub fn is_due(&self) -> bool {
        Instant::now() >= self.next_run_at
    }
}

pub struct SyncScheduler {
    jobs: Vec<SyncJob>,
}

impl SyncScheduler {
    pub fn new() -> Self {
        Self { jobs: Vec::new() }
    }

    pub fn register(&mut self, job: SyncJob) {
        self.jobs.push(job);
    }

    pub fn cancel(&mut self, source_instance_id: i64) {
        self.jobs.retain(|j| j.source_instance_id != source_instance_id);
    }

    /// Returns source_instance_ids of due jobs and reschedules them
    pub fn tick(&mut self) -> Vec<i64> {
        let mut due = Vec::new();
        for job in &mut self.jobs {
            if job.is_due() {
                due.push(job.source_instance_id);
                job.reschedule();
            }
        }
        due
    }

    pub fn job_count(&self) -> usize {
        self.jobs.len()
    }
}

impl Default for SyncScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_job_is_immediately_due() {
        let job = SyncJob::new(1, Duration::from_secs(60));
        assert!(job.is_due());
    }

    #[test]
    fn rescheduled_job_is_not_immediately_due() {
        let mut job = SyncJob::new(1, Duration::from_secs(60));
        job.reschedule();
        assert!(!job.is_due());
    }

    #[test]
    fn tick_returns_due_jobs_and_reschedules() {
        let mut scheduler = SyncScheduler::new();
        scheduler.register(SyncJob::new(42, Duration::from_secs(3600)));
        let due = scheduler.tick();
        assert_eq!(due, vec![42]);
        let due2 = scheduler.tick();
        assert!(due2.is_empty());
    }

    #[test]
    fn cancel_removes_job() {
        let mut scheduler = SyncScheduler::new();
        scheduler.register(SyncJob::new(1, Duration::from_secs(60)));
        scheduler.register(SyncJob::new(2, Duration::from_secs(60)));
        scheduler.cancel(1);
        assert_eq!(scheduler.job_count(), 1);
    }

    #[test]
    fn tick_empty_scheduler_returns_empty() {
        let mut scheduler = SyncScheduler::new();
        assert!(scheduler.tick().is_empty());
    }

    #[test]
    fn register_multiple_jobs_tick_returns_all_due() {
        let mut scheduler = SyncScheduler::new();
        scheduler.register(SyncJob::new(10, Duration::from_secs(3600)));
        scheduler.register(SyncJob::new(20, Duration::from_secs(3600)));
        let due = scheduler.tick();
        assert_eq!(due.len(), 2);
        assert!(due.contains(&10));
        assert!(due.contains(&20));
    }
}
