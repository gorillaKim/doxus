pub mod db;
pub mod runner;
pub mod scheduler;
pub use db::{DueInstance, SyncDb};
pub use runner::{SyncResult, SyncRunner};
pub use scheduler::{SyncError, SyncScheduler};

use std::time::{Duration, Instant};

/// In-memory job handle for tracking reschedule state per source instance.
/// Used alongside `SyncScheduler` when you need fine-grained per-instance control.
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
}
