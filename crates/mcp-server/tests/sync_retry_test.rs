//! TDD tests for retry + exponential backoff in sync_loop.
//!
//! These tests drive the `retry_with_backoff` function and `RetryPolicy` struct
//! that must be added to `crates/mcp-server/src/sync_loop.rs`.

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use std::time::Duration;

use doxus_mcp::sync_loop::{retry_with_backoff, RetryPolicy};

// ── helpers ──────────────────────────────────────────────────────────────────

fn default_policy() -> RetryPolicy {
    RetryPolicy {
        max_retries: 3,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(30),
    }
}

// ── 1. sync_retries_on_transient_failure ─────────────────────────────────────
//
// First call fails, second call succeeds.  The result must reflect the
// successful second call.

#[tokio::test]
async fn sync_retries_on_transient_failure() {
    tokio::time::pause();

    let call_count = Arc::new(AtomicU32::new(0));
    let cc = Arc::clone(&call_count);

    let result = retry_with_backoff(&default_policy(), || {
        let cc = Arc::clone(&cc);
        async move {
            let n = cc.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Err("transient".to_string())
            } else {
                Ok(42u32)
            }
        }
    })
    .await;

    assert_eq!(result, Ok(42u32));
    assert_eq!(call_count.load(Ordering::SeqCst), 2, "should call twice");
}

// ── 2. sync_gives_up_after_max_retries ───────────────────────────────────────
//
// All 3 retries fail → returns Err after max_retries+1 total attempts.

#[tokio::test]
async fn sync_gives_up_after_max_retries() {
    tokio::time::pause();

    let call_count = Arc::new(AtomicU32::new(0));
    let cc = Arc::clone(&call_count);

    let policy = RetryPolicy {
        max_retries: 3,
        base_delay: Duration::from_millis(10),
        max_delay: Duration::from_secs(30),
    };

    let result: Result<u32, String> = retry_with_backoff(&policy, || {
        let cc = Arc::clone(&cc);
        async move {
            cc.fetch_add(1, Ordering::SeqCst);
            Err("permanent error".to_string())
        }
    })
    .await;

    assert!(result.is_err(), "should ultimately fail");
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        4, // 1 initial + 3 retries
        "should attempt max_retries+1 times total"
    );
}

// ── 3. backoff_increases_exponentially ───────────────────────────────────────
//
// Measure the tokio::time::Instant before each attempt with `pause()`.
// Each inter-attempt gap must be at least base * 2^(attempt-1).
// (jitter is at most base*0.1, so we allow a 15% tolerance above the
//  theoretical minimum.)

#[tokio::test]
async fn backoff_increases_exponentially() {
    tokio::time::pause();

    let base = Duration::from_millis(100);
    let policy = RetryPolicy {
        max_retries: 3,
        base_delay: base,
        max_delay: Duration::from_secs(60), // high so max_delay doesn't clamp
    };

    let timestamps: Arc<std::sync::Mutex<Vec<tokio::time::Instant>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let ts = Arc::clone(&timestamps);

    let _: Result<(), String> = retry_with_backoff(&policy, || {
        let ts = Arc::clone(&ts);
        async move {
            ts.lock().unwrap().push(tokio::time::Instant::now());
            Err("fail".to_string())
        }
    })
    .await;

    let times = timestamps.lock().unwrap();
    assert_eq!(times.len(), 4, "1 initial + 3 retries");

    for attempt in 1..times.len() {
        let gap = times[attempt] - times[attempt - 1];
        // Expected minimum: base * 2^(attempt-1)
        let expected_min = base * 2u32.pow((attempt - 1) as u32);
        assert!(
            gap >= expected_min,
            "gap at attempt {attempt} ({gap:?}) should be >= {expected_min:?}"
        );
        // Must not exceed max_delay + jitter headroom (base * 0.1 + small epsilon)
        let expected_max = expected_min + base / 8 + Duration::from_millis(5);
        assert!(
            gap <= expected_max,
            "gap at attempt {attempt} ({gap:?}) should be <= {expected_max:?} (exponential, not more)"
        );
    }
}

// ── 4. backoff_respects_max_delay ────────────────────────────────────────────
//
// With a very short max_delay, even later retries must be capped.

#[tokio::test]
async fn backoff_respects_max_delay() {
    tokio::time::pause();

    let base = Duration::from_millis(100);
    let max_delay = Duration::from_millis(150); // cap at 150ms

    let policy = RetryPolicy {
        max_retries: 5,
        base_delay: base,
        max_delay,
    };

    let timestamps: Arc<std::sync::Mutex<Vec<tokio::time::Instant>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let ts = Arc::clone(&timestamps);

    let _: Result<(), String> = retry_with_backoff(&policy, || {
        let ts = Arc::clone(&ts);
        async move {
            ts.lock().unwrap().push(tokio::time::Instant::now());
            Err("fail".to_string())
        }
    })
    .await;

    let times = timestamps.lock().unwrap();
    // From attempt 2 onward (where uncapped delay > max_delay), each gap
    // must be <= max_delay + jitter headroom.
    let jitter_headroom = base / 8 + Duration::from_millis(5);
    for attempt in 2..times.len() {
        let gap = times[attempt] - times[attempt - 1];
        assert!(
            gap <= max_delay + jitter_headroom,
            "gap at attempt {attempt} ({gap:?}) exceeds max_delay {max_delay:?}"
        );
    }
}

// ── 5. retry_with_jitter_does_not_thunderherd ────────────────────────────────
//
// Run N concurrent retry sequences with the same base policy.  After the first
// failure, record each sequence's sleep duration.  If jitter is applied, the
// durations must NOT all be identical.

#[tokio::test]
async fn retry_with_jitter_does_not_thunderherd() {
    tokio::time::pause();

    let policy = RetryPolicy {
        max_retries: 1,
        base_delay: Duration::from_millis(200),
        max_delay: Duration::from_secs(30),
    };

    const N: usize = 20;

    // We'll collect the sleep duration each sequence used by measuring the
    // timestamp delta between call #1 and call #2.
    let all_gaps: Arc<std::sync::Mutex<Vec<Duration>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    let mut handles = Vec::new();
    for _ in 0..N {
        let p = RetryPolicy {
            max_retries: policy.max_retries,
            base_delay: policy.base_delay,
            max_delay: policy.max_delay,
        };
        let gaps = Arc::clone(&all_gaps);
        handles.push(tokio::spawn(async move {
            let timestamps: Arc<std::sync::Mutex<Vec<tokio::time::Instant>>> =
                Arc::new(std::sync::Mutex::new(Vec::new()));
            let ts = Arc::clone(&timestamps);

            let _: Result<(), String> = retry_with_backoff(&p, || {
                let ts = Arc::clone(&ts);
                async move {
                    ts.lock().unwrap().push(tokio::time::Instant::now());
                    Err("fail".to_string())
                }
            })
            .await;

            let times = timestamps.lock().unwrap();
            if times.len() >= 2 {
                let gap = times[1] - times[0];
                gaps.lock().unwrap().push(gap);
            }
        }));
    }

    // Advance time so all retries complete.
    tokio::time::advance(Duration::from_secs(5)).await;
    for h in handles {
        let _ = h.await;
    }

    let gaps = all_gaps.lock().unwrap();
    assert!(
        gaps.len() >= N / 2,
        "expected at least {} gap measurements, got {}",
        N / 2,
        gaps.len()
    );

    // At least 2 distinct values → jitter is working.
    let first = gaps[0];
    let all_same = gaps.iter().all(|&g| g == first);
    assert!(
        !all_same,
        "all {N} retry gaps are identical ({first:?}), jitter is not applied"
    );
}

// ── Task 3: Rate Limit handling ───────────────────────────────────────────────

use doxus_mcp::sync_loop::handle_rate_limited;
use doxus_plugin_sdk::PluginError;
use tokio::sync::watch;

// ── 6. sync_waits_on_rate_limited ────────────────────────────────────────────
//
// When `handle_rate_limited` is called with retry_after_secs=2, it must sleep
// for that duration (verified via tokio::time::pause + advance) before
// returning RateLimitAction::Retry.

#[tokio::test]
async fn sync_waits_on_rate_limited() {
    tokio::time::pause();

    let (_tx, mut shutdown_rx) = watch::channel(false);

    let start = tokio::time::Instant::now();
    let action = handle_rate_limited(2, &mut shutdown_rx).await;
    let elapsed = start.elapsed();

    assert!(
        matches!(action, doxus_mcp::sync_loop::RateLimitAction::Retry),
        "expected Retry action"
    );
    assert!(
        elapsed >= Duration::from_secs(2),
        "expected at least 2s wait, got {elapsed:?}"
    );
}

// ── 7. rate_limited_does_not_count_as_failure ────────────────────────────────
//
// rate limit wait must NOT consume retry counter — the function returns
// RateLimitAction::Retry (not an error), so callers loop without incrementing
// their failure counter.

#[tokio::test]
async fn rate_limited_does_not_count_as_failure() {
    tokio::time::pause();

    let call_count = Arc::new(AtomicU32::new(0));
    let cc = Arc::clone(&call_count);

    // Simulate: first call → RateLimited(1s), second call → Ok
    // retry_with_backoff_rate_aware must not count the rate-limit as a retry.
    let (_tx, mut shutdown_rx) = watch::channel(false);

    let result: Result<u32, PluginError> = doxus_mcp::sync_loop::retry_with_backoff_rate_aware(
        &default_policy(),
        &mut shutdown_rx,
        || {
            let cc = Arc::clone(&cc);
            async move {
                let n = cc.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Err(PluginError::RateLimited { retry_after_secs: 1 })
                } else {
                    Ok(42u32)
                }
            }
        },
    )
    .await;

    assert!(result.is_ok(), "expected Ok after rate limit retry");
    // The rate-limited attempt + 1 successful = 2 calls, but 0 failure retries consumed.
    assert_eq!(call_count.load(Ordering::SeqCst), 2, "should call twice");
}

// ── 8. rate_limit_wait_cancelled_on_shutdown ─────────────────────────────────
//
// If a shutdown signal arrives during the rate-limit sleep, the wait must be
// cancelled immediately and RateLimitAction::Shutdown returned.

#[tokio::test]
async fn rate_limit_wait_cancelled_on_shutdown() {
    tokio::time::pause();

    let (tx, mut shutdown_rx) = watch::channel(false);

    // Send shutdown after 100ms, but rate limit is 60s
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = tx.send(true);
    });

    let start = tokio::time::Instant::now();
    // Advance time so the spawned task fires
    tokio::time::advance(Duration::from_millis(200)).await;
    let action = handle_rate_limited(60, &mut shutdown_rx).await;
    let elapsed = start.elapsed();

    assert!(
        matches!(action, doxus_mcp::sync_loop::RateLimitAction::Shutdown),
        "expected Shutdown action when shutdown fires during rate-limit wait"
    );
    assert!(
        elapsed < Duration::from_secs(60),
        "wait should have been cancelled, not waited full 60s"
    );
}

// ── 9. rate_limited_then_transient_error_retries_correctly ───────────────────
//
// Task 2+3 integration: RateLimited then transient error — transient error
// still uses normal retry counter (max_retries not yet exhausted).

#[tokio::test]
async fn rate_limited_then_transient_error_retries_correctly() {
    tokio::time::pause();

    let call_count = Arc::new(AtomicU32::new(0));
    let cc = Arc::clone(&call_count);

    let policy = RetryPolicy {
        max_retries: 3,
        base_delay: Duration::from_millis(10),
        max_delay: Duration::from_secs(30),
    };

    let (_tx, mut shutdown_rx) = watch::channel(false);

    // Call sequence: RateLimited → transient error → Ok
    let result: Result<u32, PluginError> = doxus_mcp::sync_loop::retry_with_backoff_rate_aware(
        &policy,
        &mut shutdown_rx,
        || {
            let cc = Arc::clone(&cc);
            async move {
                let n = cc.fetch_add(1, Ordering::SeqCst);
                match n {
                    0 => Err(PluginError::RateLimited { retry_after_secs: 1 }),
                    1 => Err(PluginError::Internal("transient".into())),
                    _ => Ok(99u32),
                }
            }
        },
    )
    .await;

    assert!(result.is_ok(), "expected Ok after rate limit + transient retry");
    assert_eq!(call_count.load(Ordering::SeqCst), 3, "3 calls total");
}
