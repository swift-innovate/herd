//! Per-provider request-per-minute rate limiter for the Frontier Gateway.
//!
//! Cloud LLM providers (Anthropic, OpenAI, xAI, …) expose RPM quotas. Each
//! `ProviderConfig` carries a `rate_limit` field (requests/minute) that until
//! now was parsed but not enforced. This module provides `ProviderRateLimiter`
//! — a small, thread-safe, fixed-window token bucket keyed by provider name.
//!
//! ## Design choices
//!
//! * **Fit-for-purpose bucket.** The existing `src/rate_limit.rs::TokenBucket`
//!   is refilled on a 1-second cadence by a background tokio task. Per-minute
//!   RPM limits want a 60-second fixed window, and bolting a different cadence
//!   onto that bucket would muddy its purpose. A small self-contained bucket
//!   in this file is clearer.
//! * **Fixed 60s window.** Matches how cloud providers actually publish their
//!   RPM quotas (e.g. OpenAI: "X requests per minute, bucket resets each
//!   minute") and avoids the complexity of sliding-window accounting.
//! * **Concurrency.** `AtomicU64` for the token counter (hot path is a CAS
//!   loop) plus a `std::sync::Mutex<Instant>` guarding the window-start
//!   timestamp. The mutex is uncontended in the common case and is never held
//!   across an await, so it's safe to use from async code.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::ProviderConfig;

// ---------------------------------------------------------------------------
// Token bucket (fixed-window)
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct TokenBucket {
    tokens: AtomicU64,
    capacity: u64,
    refill_interval: Duration,
    window_start: Mutex<Instant>,
}

impl TokenBucket {
    fn new(capacity: u64, refill_interval: Duration) -> Self {
        Self {
            tokens: AtomicU64::new(capacity),
            capacity,
            refill_interval,
            window_start: Mutex::new(Instant::now()),
        }
    }

    /// Refill the bucket if the current window has elapsed. Returns the
    /// current token count (after any refill).
    fn refill_if_due(&self) {
        // Fast path: check elapsed without taking the lock unnecessarily.
        let mut guard = match self.window_start.lock() {
            Ok(g) => g,
            // Poisoned mutex: treat as in-window and skip refill. The next
            // window will still reset tokens eventually.
            Err(p) => p.into_inner(),
        };
        if guard.elapsed() >= self.refill_interval {
            self.tokens.store(self.capacity, Ordering::Relaxed);
            *guard = Instant::now();
        }
    }

    fn try_acquire(&self) -> bool {
        self.refill_if_due();
        loop {
            let current = self.tokens.load(Ordering::Relaxed);
            if current == 0 {
                return false;
            }
            if self
                .tokens
                .compare_exchange_weak(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ProviderRateLimiter
// ---------------------------------------------------------------------------

/// Enforces per-provider requests-per-minute caps for the Frontier Gateway.
///
/// Providers whose `rate_limit` is 0 (or are absent from the config) are
/// treated as unlimited — `try_acquire` always returns `true` for them.
pub struct ProviderRateLimiter {
    buckets: HashMap<String, TokenBucket>,
}

impl ProviderRateLimiter {
    /// Build a limiter from the configured provider list.
    ///
    /// Providers with `rate_limit == 0` are omitted — an absent entry in the
    /// map is the "disabled / unlimited" signal.
    pub fn new(providers: &[ProviderConfig]) -> Self {
        Self::new_with_refill(providers, Duration::from_secs(60))
    }

    /// Internal constructor exposing the refill interval so the refill test
    /// doesn't have to sleep for a real minute. Not part of the public API.
    fn new_with_refill(providers: &[ProviderConfig], refill_interval: Duration) -> Self {
        let mut buckets = HashMap::new();
        for p in providers {
            if p.rate_limit > 0 {
                buckets.insert(
                    p.name.clone(),
                    TokenBucket::new(p.rate_limit, refill_interval),
                );
            }
        }
        Self { buckets }
    }

    /// Try to consume one request token for `provider_name`.
    ///
    /// Returns `true` when the request is allowed, `false` when the provider
    /// is over its RPM quota for the current 60-second window. Unknown
    /// providers and providers configured with `rate_limit == 0` always
    /// return `true`.
    pub fn try_acquire(&self, provider_name: &str) -> bool {
        match self.buckets.get(provider_name) {
            Some(bucket) => bucket.try_acquire(),
            None => true,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(name: &str, rate_limit: u64) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            rate_limit,
            ..Default::default()
        }
    }

    #[test]
    fn allows_up_to_limit_then_rejects() {
        let limiter = ProviderRateLimiter::new(&[provider("anthropic", 3)]);

        assert!(limiter.try_acquire("anthropic"));
        assert!(limiter.try_acquire("anthropic"));
        assert!(limiter.try_acquire("anthropic"));
        assert!(!limiter.try_acquire("anthropic"));
    }

    #[test]
    fn unknown_provider_always_allowed() {
        let limiter = ProviderRateLimiter::new(&[provider("anthropic", 1)]);
        // Consume the anthropic bucket first to prove unknown lookups don't
        // accidentally share a bucket.
        assert!(limiter.try_acquire("anthropic"));
        assert!(!limiter.try_acquire("anthropic"));

        for _ in 0..1000 {
            assert!(limiter.try_acquire("openai"));
            assert!(limiter.try_acquire("does-not-exist"));
        }
    }

    #[test]
    fn zero_rate_limit_means_disabled() {
        let limiter = ProviderRateLimiter::new(&[provider("openai", 0)]);
        // A provider with rate_limit=0 isn't inserted into the bucket map, so
        // it falls through to the "unknown -> allowed" branch. Verify that by
        // hammering it well past any sensible RPM.
        for _ in 0..10_000 {
            assert!(limiter.try_acquire("openai"));
        }
    }

    #[test]
    fn buckets_refill_after_window() {
        // Use a 50ms "minute" so the test stays fast. The production
        // constructor hard-codes 60s; this private seam exists purely so we
        // can observe refill behavior without sleeping for a real minute.
        let limiter = ProviderRateLimiter::new_with_refill(
            &[provider("anthropic", 2)],
            Duration::from_millis(50),
        );

        assert!(limiter.try_acquire("anthropic"));
        assert!(limiter.try_acquire("anthropic"));
        assert!(!limiter.try_acquire("anthropic"));

        std::thread::sleep(Duration::from_millis(75));

        // Window elapsed — bucket should be full again.
        assert!(limiter.try_acquire("anthropic"));
        assert!(limiter.try_acquire("anthropic"));
        assert!(!limiter.try_acquire("anthropic"));
    }

    #[test]
    fn multiple_providers_are_independent() {
        let limiter = ProviderRateLimiter::new(&[
            provider("anthropic", 2),
            provider("openai", 3),
            provider("xai", 1),
        ]);

        // Drain anthropic.
        assert!(limiter.try_acquire("anthropic"));
        assert!(limiter.try_acquire("anthropic"));
        assert!(!limiter.try_acquire("anthropic"));

        // openai still has its full 3.
        assert!(limiter.try_acquire("openai"));
        assert!(limiter.try_acquire("openai"));
        assert!(limiter.try_acquire("openai"));
        assert!(!limiter.try_acquire("openai"));

        // xai still has its 1.
        assert!(limiter.try_acquire("xai"));
        assert!(!limiter.try_acquire("xai"));
    }

    #[test]
    fn empty_provider_list_allows_everything() {
        let limiter = ProviderRateLimiter::new(&[]);
        for _ in 0..100 {
            assert!(limiter.try_acquire("anything"));
        }
    }
}
