//! Per-instance retry state tracking with exponential backoff.
//!
//! Used by the sync loop to skip instances that have recently failed,
//! applying a capped exponential backoff between attempts.

pub const MAX_RETRIES: u32 = 3;
pub const MAX_BACKOFF_SECS: u64 = 300;

pub(crate) struct InstanceRetryState {
    pub(crate) retry_count: u32,
    pub(crate) next_retry_at: Option<std::time::Instant>,
}

/// Tracks per-instance retry state with exponential backoff.
/// Used by the sync loop to skip instances that have recently failed.
pub struct RetryTracker {
    pub(crate) states: std::collections::HashMap<i64, InstanceRetryState>,
}

/// Compute exponential backoff duration.
///
/// Formula: `min(base_secs * 2^attempt, max_secs)` seconds.
pub fn compute_backoff(attempt: u32, base_secs: u64, max_secs: u64) -> std::time::Duration {
    let secs = (base_secs * 2u64.saturating_pow(attempt)).min(max_secs);
    std::time::Duration::from_secs(secs)
}

impl RetryTracker {
    pub fn new() -> Self {
        Self {
            states: std::collections::HashMap::new(),
        }
    }

    /// Returns `true` if the instance should be skipped this tick
    /// (either because it has exceeded max retries or is still waiting for
    /// its backoff window to expire).
    ///
    /// `now` is passed explicitly for deterministic testing.
    pub fn should_skip(&self, instance_id: i64, now: std::time::Instant) -> bool {
        match self.states.get(&instance_id) {
            None => false,
            Some(state) => {
                if state.retry_count >= MAX_RETRIES {
                    return true;
                }
                match state.next_retry_at {
                    Some(t) => now < t,
                    None => false,
                }
            }
        }
    }

    /// Record a failure for an instance.
    /// `retry_after_secs`: if `Some`, use that as the backoff duration (rate-limit case).
    /// Otherwise use exponential backoff: `min(2^retry_count * 10, MAX_BACKOFF_SECS)` seconds.
    ///
    /// `now` is passed explicitly for deterministic testing.
    pub fn record_failure(
        &mut self,
        instance_id: i64,
        retry_after_secs: Option<u64>,
        now: std::time::Instant,
    ) {
        let state = self.states.entry(instance_id).or_insert(InstanceRetryState {
            retry_count: 0,
            next_retry_at: None,
        });
        let backoff = match retry_after_secs {
            Some(s) => std::time::Duration::from_secs(s.min(MAX_BACKOFF_SECS)),
            None => compute_backoff(state.retry_count, 10, MAX_BACKOFF_SECS),
        };
        state.retry_count += 1;
        state.next_retry_at = Some(now + backoff);
    }

    /// Record a successful sync — clears retry state.
    pub fn record_success(&mut self, instance_id: i64) {
        self.states.remove(&instance_id);
    }
}
