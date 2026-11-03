//! In-process DRF throttle wiring for the assistant handlers (D-06).
//!
//! [`crate::assistant::throttles`] pins the six throttle specs (scope,
//! quota, window) and the sliding-window algorithm; the cache itself and
//! the wiring stay with the handler layer (throttles.rs docs). Django
//! backs the cache with Redis (`django_redis`), which the Rust side has
//! no handle for — `AppState` carries Postgres pools only, and the
//! foundation crates are read-only for port issues. [`Governor`]
//! therefore runs the exact DRF loop over a process-local history map:
//!
//! * cache key `throttle_<scope>_<ident>` (ident = user pk, else IP),
//! * trim history entries `<= now - duration`, allow iff fewer than
//!   `num_requests` remain, else deny,
//! * success records `now`.
//!
//! Single-process deployments (and the proxy contract gate) observe
//! exactly Django-with-local-memory-cache behaviour. Multi-worker
//! deployments need a shared-cache follow-up: per-process histories
//! admit up to quota *per worker*. That follow-up is recorded in the
//! PIDASHCONV-257 PR, not worked around here.
//!
//! Denial renders [`crate::assistant::throttles::RATE_LIMIT_BODY`]
//! (429), the `auth_exception_handler` rewrite of DRF's `Throttled`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::throttles::{cache_key, ThrottleSpec};

/// One throttle history: the DRF cache-loop state for a single
/// `throttle_<scope>_<ident>` key (timestamps of allowed requests).
#[derive(Debug, Default)]
pub struct History {
    entries: Vec<Instant>,
}

impl History {
    /// DRF `allow_request` (`SimpleRateThrottle`, verbatim): drop entries
    /// `<= now - duration`, allow iff fewer than `num_requests` remain,
    /// recording `now` on success.
    pub fn allow(&mut self, spec: ThrottleSpec, now: Instant) -> bool {
        let horizon = now
            .checked_sub(Duration::from_secs(spec.window_secs))
            .unwrap_or(now);
        self.entries.retain(|at| *at > horizon);
        if (self.entries.len() as u32) < spec.requests {
            self.entries.push(now);
            true
        } else {
            false
        }
    }
}

/// Process-local throttle store: one [`History`] per cache key.
#[derive(Debug, Default)]
pub struct Governor {
    histories: Mutex<HashMap<String, History>>,
}

impl Governor {
    /// Empty store (tests build their own; handlers share [`shared`]).
    pub fn new() -> Self {
        Self::default()
    }

    /// Check `scope` for `ident` at `now`: allow records, deny does not.
    pub fn check(&self, spec: ThrottleSpec, ident: &str, now: Instant) -> bool {
        let key = cache_key(spec.scope, ident);
        let mut histories = self.histories.lock().expect("throttle lock");
        histories.entry(key).or_default().allow(spec, now)
    }
}

/// The handler-shared store (one process, like Django's local-memory
/// cache fallback).
pub fn shared() -> &'static Governor {
    static SHARED: OnceLock<Governor> = OnceLock::new();
    SHARED.get_or_init(Governor::new)
}

/// Check `spec` for `ident` against the shared store at this instant.
pub fn throttle_check(spec: ThrottleSpec, ident: &str) -> bool {
    shared().check(spec, ident, Instant::now())
}

#[cfg(test)]
mod tests {
    use super::super::throttles::{AGENT_TOKEN_THROTTLE, TRANSCRIBE_THROTTLE};
    use super::*;
    use std::time::Duration;

    fn scope_at(governor: &Governor, spec: ThrottleSpec, ident: &str, at: Instant) -> bool {
        governor.check(spec, ident, at)
    }

    #[test]
    fn burst_up_to_quota_then_denies() {
        let governor = Governor::new();
        let start = Instant::now();
        for _ in 0..20 {
            assert!(scope_at(&governor, TRANSCRIBE_THROTTLE, "u1", start));
        }
        assert!(!scope_at(&governor, TRANSCRIBE_THROTTLE, "u1", start));
    }

    #[test]
    fn window_expiry_readmits() {
        let governor = Governor::new();
        let start = Instant::now();
        for _ in 0..12 {
            assert!(scope_at(&governor, AGENT_TOKEN_THROTTLE, "u9", start));
        }
        assert!(!scope_at(&governor, AGENT_TOKEN_THROTTLE, "u9", start));
        // Entries `<= now - duration` trim (DRF pops the boundary too):
        // just inside the window the burst still counts; exactly at the
        // horizon it is gone.
        let horizon = start + Duration::from_secs(60);
        assert!(!scope_at(
            &governor,
            AGENT_TOKEN_THROTTLE,
            "u9",
            horizon - Duration::from_millis(1)
        ));
        assert!(scope_at(&governor, AGENT_TOKEN_THROTTLE, "u9", horizon));
    }

    #[test]
    fn histories_are_per_scope_and_ident() {
        let governor = Governor::new();
        let start = Instant::now();
        assert!(scope_at(&governor, TRANSCRIBE_THROTTLE, "alice", start));
        assert!(scope_at(&governor, TRANSCRIBE_THROTTLE, "bob", start));
        assert!(scope_at(&governor, AGENT_TOKEN_THROTTLE, "alice", start));
        // Denial does not record: a denied caller stays denied, but the
        // count never inflates past quota + 0.
        for _ in 0..19 {
            scope_at(&governor, TRANSCRIBE_THROTTLE, "alice", start);
        }
        assert!(!scope_at(&governor, TRANSCRIBE_THROTTLE, "alice", start));
        assert!(scope_at(&governor, TRANSCRIBE_THROTTLE, "bob", start));
    }
}
