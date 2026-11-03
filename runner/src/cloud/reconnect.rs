//! Reconnect policy for the per-runner HTTP transport (PDASHOSS01-306).
//!
//! Two independent pieces, both pure so they can be unit-tested without a
//! server or a clock:
//!
//! - [`Backoff`]: exponential backoff with ±20% jitter. Bootstrap starts at
//!   30 s and doubles up to a 6-hour cap, so a long cloud outage costs a
//!   handful of requests instead of two per minute per runner. Any success
//!   or reconnect kick (wake, network change, `pidash reconnect`) resets it.
//! - [`GoneConfirmation`]: counts consecutive `runner_gone` answers. The
//!   runner is purged locally only after [`GONE_CONFIRMATIONS`] answers
//!   spanning at least [`GONE_MIN_SPAN`]; any other answer resets the count,
//!   so a server outage can never delete a runner.
//!
//! [`ReconnectPolicy`] combines them and turns each failed attempt into a
//! [`RetryDecision`].

use std::time::{Duration, Instant};

use rand::Rng;

use crate::cloud::http::TransportError;

/// First wait after a failed bootstrap.
pub const BOOTSTRAP_INITIAL_BACKOFF: Duration = Duration::from_secs(30);
/// First wait after a failed poll on an established session. Kept short:
/// a single dropped long-poll should not cost the runner 30 s of latency.
pub const POLL_INITIAL_BACKOFF: Duration = Duration::from_secs(1);
/// Longest wait between attempts while the cloud is unreachable.
pub const MAX_BACKOFF: Duration = Duration::from_secs(6 * 60 * 60);
/// Every wait is randomised by up to this fraction in either direction so
/// runners don't all retry at the same moment when the server recovers.
pub const JITTER_FRACTION: f64 = 0.20;
/// Consecutive `runner_gone` answers needed before the local purge.
pub const GONE_CONFIRMATIONS: u32 = 10;
/// Minimum time between the first and the last confirming answer.
pub const GONE_MIN_SPAN: Duration = Duration::from_secs(5 * 60);
/// Wait between confirmation attempts (before jitter).
pub const GONE_RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// Exponential backoff: `initial`, doubling each step, capped at `cap`.
#[derive(Debug, Clone)]
pub struct Backoff {
    initial: Duration,
    cap: Duration,
    next: Duration,
}

impl Backoff {
    pub fn new(initial: Duration, cap: Duration) -> Self {
        Self {
            initial,
            cap,
            next: initial.min(cap),
        }
    }

    /// 30 s → 1 m → 2 m → … → 6 h, then 6 h forever.
    pub fn bootstrap() -> Self {
        Self::new(BOOTSTRAP_INITIAL_BACKOFF, MAX_BACKOFF)
    }

    /// 1 s → 2 s → … → 6 h, then 6 h forever.
    pub fn poll() -> Self {
        Self::new(POLL_INITIAL_BACKOFF, MAX_BACKOFF)
    }

    /// Return the current (un-jittered) step and advance to the next.
    pub fn next_base(&mut self) -> Duration {
        let current = self.next;
        self.next = current.saturating_mul(2).min(self.cap);
        current
    }

    /// Start over from the initial step.
    pub fn reset(&mut self) {
        self.next = self.initial.min(self.cap);
    }
}

/// Apply ±[`JITTER_FRACTION`] of random jitter to `base`.
pub fn jittered(base: Duration) -> Duration {
    let factor = rand::thread_rng().gen_range(-1.0..=1.0);
    jitter_by(base, factor)
}

/// Deterministic core of [`jittered`]: `factor` in `[-1, 1]` scales the
/// ±20% band (`-1` → 80% of `base`, `1` → 120%).
pub fn jitter_by(base: Duration, factor: f64) -> Duration {
    let factor = factor.clamp(-1.0, 1.0);
    base.mul_f64(1.0 + factor * JITTER_FRACTION)
}

/// Outcome of [`GoneConfirmation::record`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoneVerdict {
    /// Still confirming: `n` consecutive answers so far.
    Confirming(u32),
    /// Enough consecutive answers over a long enough span: purge.
    Purge(u32),
}

/// How many `runner_gone` answers, over how long, confirm a deletion, and
/// how often to ask while confirming. Production uses [`Default`]; tests
/// shrink the durations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoneTiming {
    pub confirmations: u32,
    pub min_span: Duration,
    pub retry_interval: Duration,
}

impl Default for GoneTiming {
    fn default() -> Self {
        Self {
            confirmations: GONE_CONFIRMATIONS,
            min_span: GONE_MIN_SPAN,
            retry_interval: GONE_RETRY_INTERVAL,
        }
    }
}

/// Counts consecutive `runner_gone` answers.
#[derive(Debug, Clone, Default)]
pub struct GoneConfirmation {
    timing: GoneTiming,
    count: u32,
    first_at: Option<Instant>,
}

impl GoneConfirmation {
    pub fn new(timing: GoneTiming) -> Self {
        Self {
            timing,
            count: 0,
            first_at: None,
        }
    }

    /// Record one `runner_gone` answer received at `now`.
    pub fn record(&mut self, now: Instant) -> GoneVerdict {
        let first = *self.first_at.get_or_insert(now);
        self.count = self.count.saturating_add(1);
        if self.count >= self.timing.confirmations
            && now.saturating_duration_since(first) >= self.timing.min_span
        {
            GoneVerdict::Purge(self.count())
        } else {
            GoneVerdict::Confirming(self.count())
        }
    }

    /// Any answer other than `runner_gone` breaks the streak.
    pub fn reset(&mut self) {
        self.count = 0;
        self.first_at = None;
    }

    /// Answers counted so far, capped at the target: past it the daemon is
    /// only waiting out the minimum span, so status shows `10/10`, not
    /// `12/10`.
    pub fn count(&self) -> u32 {
        self.count.min(self.timing.confirmations)
    }
}

/// What to do after a failed attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryDecision {
    /// Wait `delay`, then try again. `reason` is a short human label for
    /// `pidash status`.
    Retry { delay: Duration, reason: String },
    /// The cloud confirmed the runner is gone: purge it locally.
    Purge { confirmations: u32 },
}

/// Backoff plus `runner_gone` confirmation, driven by attempt outcomes.
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    backoff: Backoff,
    gone: GoneConfirmation,
}

impl ReconnectPolicy {
    pub fn new(backoff: Backoff) -> Self {
        Self {
            backoff,
            gone: GoneConfirmation::default(),
        }
    }

    pub fn bootstrap() -> Self {
        Self::new(Backoff::bootstrap())
    }

    /// Replace the confirmation timing (tests).
    pub fn with_gone_timing(mut self, timing: GoneTiming) -> Self {
        self.gone = GoneConfirmation::new(timing);
        self
    }

    /// The attempt succeeded: start both counters over.
    pub fn on_success(&mut self) {
        self.backoff.reset();
        self.gone.reset();
    }

    /// Wake, network change or `pidash reconnect`: retry from the start of
    /// the backoff schedule. Deliberately leaves the gone count alone — a
    /// kick is not an answer from the server.
    pub fn on_kick(&mut self) {
        self.backoff.reset();
    }

    /// Consecutive `runner_gone` answers so far (0 when not confirming).
    pub fn gone_count(&self) -> u32 {
        self.gone.count()
    }

    /// Decide what follows a failed attempt that ended at `now`.
    pub fn on_failure(&mut self, err: &TransportError, now: Instant) -> RetryDecision {
        self.on_failure_with(err, now, jittered)
    }

    /// [`Self::on_failure`] with an injectable jitter function for tests.
    pub fn on_failure_with(
        &mut self,
        err: &TransportError,
        now: Instant,
        jitter: impl Fn(Duration) -> Duration,
    ) -> RetryDecision {
        if matches!(err, TransportError::RunnerGone) {
            // The server answered, so the outage backoff starts over.
            self.backoff.reset();
            return match self.gone.record(now) {
                GoneVerdict::Purge(confirmations) => RetryDecision::Purge { confirmations },
                GoneVerdict::Confirming(n) => RetryDecision::Retry {
                    delay: jitter(self.gone.timing.retry_interval),
                    reason: format!(
                        "deleted in cloud; confirming {n}/{}",
                        self.gone.timing.confirmations
                    ),
                },
            };
        }
        self.gone.reset();
        let base = self.backoff.next_base();
        let delay = match err {
            TransportError::RateLimited {
                retry_after: Some(after),
            } => (*after).min(MAX_BACKOFF).max(jitter(base)),
            _ => jitter(base),
        };
        RetryDecision::Retry {
            delay,
            reason: failure_reason(err),
        }
    }
}

/// Short human label for why a runner is disconnected.
pub fn failure_reason(err: &TransportError) -> String {
    match err {
        TransportError::Connect(_) | TransportError::Timeout(_) | TransportError::Network(_) => {
            "server unreachable".to_string()
        }
        TransportError::Server { status, .. } if *status >= 500 => {
            format!("server unreachable: HTTP {status}")
        }
        TransportError::RateLimited { .. } => "rate limited".to_string(),
        TransportError::RunnerGone => "deleted in cloud".to_string(),
        TransportError::Server { status, body } => {
            let detail: String = body.chars().take(80).collect();
            format!("rejected: HTTP {status} {detail}")
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_jitter(d: Duration) -> Duration {
        d
    }

    fn mins(m: u64) -> Duration {
        Duration::from_secs(m * 60)
    }

    #[test]
    fn bootstrap_backoff_reaches_and_holds_six_hours() {
        let mut b = Backoff::bootstrap();
        let steps: Vec<u64> = (0..14).map(|_| b.next_base().as_secs()).collect();
        assert_eq!(
            steps,
            vec![
                30, 60, 120, 240, 480, 960, 1920, 3840, 7680, 15360, 21600, 21600, 21600, 21600
            ]
        );
    }

    #[test]
    fn backoff_resets_to_initial() {
        let mut b = Backoff::bootstrap();
        for _ in 0..20 {
            b.next_base();
        }
        b.reset();
        assert_eq!(b.next_base(), Duration::from_secs(30));
    }

    #[test]
    fn jitter_stays_within_twenty_percent() {
        let base = MAX_BACKOFF;
        assert_eq!(jitter_by(base, -1.0), base.mul_f64(0.8));
        assert_eq!(jitter_by(base, 1.0), base.mul_f64(1.2));
        assert_eq!(jitter_by(base, 0.0), base);
        for _ in 0..1000 {
            let d = jittered(base);
            assert!(d >= base.mul_f64(0.8) && d <= base.mul_f64(1.2), "{d:?}");
        }
    }

    #[test]
    fn jittered_waits_hold_at_the_cap_within_bounds() {
        let mut p = ReconnectPolicy::bootstrap();
        let t0 = Instant::now();
        let err = TransportError::Connect("refused".into());
        let mut last = Duration::ZERO;
        for _ in 0..30 {
            if let RetryDecision::Retry { delay, .. } = p.on_failure(&err, t0) {
                last = delay;
            } else {
                panic!("transient failure must never purge");
            }
        }
        assert!(last >= MAX_BACKOFF.mul_f64(0.8) && last <= MAX_BACKOFF.mul_f64(1.2));
    }

    #[test]
    fn success_and_kick_reset_the_backoff() {
        let mut p = ReconnectPolicy::bootstrap();
        let t0 = Instant::now();
        let err = TransportError::Timeout("t".into());
        for _ in 0..12 {
            p.on_failure_with(&err, t0, no_jitter);
        }
        p.on_success();
        assert_eq!(
            p.on_failure_with(&err, t0, no_jitter),
            RetryDecision::Retry {
                delay: Duration::from_secs(30),
                reason: "server unreachable".into()
            }
        );
        for _ in 0..12 {
            p.on_failure_with(&err, t0, no_jitter);
        }
        // Wake / network change / `pidash reconnect` all call on_kick.
        p.on_kick();
        assert!(matches!(
            p.on_failure_with(&err, t0, no_jitter),
            RetryDecision::Retry { delay, .. } if delay == Duration::from_secs(30)
        ));
    }

    #[test]
    fn retry_after_is_honored_on_429() {
        let mut p = ReconnectPolicy::bootstrap();
        let err = TransportError::RateLimited {
            retry_after: Some(mins(10)),
        };
        assert!(matches!(
            p.on_failure_with(&err, Instant::now(), no_jitter),
            RetryDecision::Retry { delay, .. } if delay == mins(10)
        ));
    }

    #[test]
    fn purges_after_ten_answers_spanning_five_minutes() {
        let mut p = ReconnectPolicy::bootstrap();
        let t0 = Instant::now();
        // Ten answers 30 s apart span only 4.5 minutes: not yet.
        for i in 0..10u64 {
            let now = t0 + Duration::from_secs(30 * i);
            match p.on_failure_with(&TransportError::RunnerGone, now, no_jitter) {
                RetryDecision::Retry { delay, reason } => {
                    assert_eq!(delay, GONE_RETRY_INTERVAL);
                    assert_eq!(reason, format!("deleted in cloud; confirming {}/10", i + 1));
                }
                RetryDecision::Purge { .. } => panic!("purged early at answer {}", i + 1),
            }
        }
        assert_eq!(
            p.on_failure_with(&TransportError::RunnerGone, t0 + mins(5), no_jitter),
            RetryDecision::Purge { confirmations: 10 }
        );
    }

    #[test]
    fn ten_quick_answers_never_purge_before_five_minutes() {
        let mut g = GoneConfirmation::default();
        let t0 = Instant::now();
        for i in 0..100u64 {
            let verdict = g.record(t0 + Duration::from_secs(i));
            let shown = (i as u32 + 1).min(GONE_CONFIRMATIONS);
            assert_eq!(verdict, GoneVerdict::Confirming(shown));
        }
        assert_eq!(g.count(), GONE_CONFIRMATIONS, "count is capped for display");
        assert_eq!(g.record(t0 + mins(5)), GoneVerdict::Purge(GONE_CONFIRMATIONS));
    }

    #[test]
    fn fewer_than_ten_answers_never_purge_however_long() {
        let mut g = GoneConfirmation::default();
        let t0 = Instant::now();
        for i in 0..9u64 {
            assert_eq!(
                g.record(t0 + mins(60 * i)),
                GoneVerdict::Confirming(i as u32 + 1)
            );
        }
    }

    #[test]
    fn any_other_answer_resets_the_confirmation_count() {
        let interruptions = [
            TransportError::Connect("refused".into()),
            TransportError::Timeout("t".into()),
            TransportError::Server {
                status: 503,
                body: String::new(),
            },
            TransportError::Server {
                status: 401,
                body: "{\"detail\":\"runner_not_found\"}".into(),
            },
            TransportError::RateLimited { retry_after: None },
        ];
        for interruption in interruptions {
            let mut p = ReconnectPolicy::bootstrap();
            let t0 = Instant::now();
            for i in 0..9u64 {
                p.on_failure_with(
                    &TransportError::RunnerGone,
                    t0 + Duration::from_secs(40 * i),
                    no_jitter,
                );
            }
            assert_eq!(p.gone_count(), 9);
            p.on_failure_with(&interruption, t0 + mins(6), no_jitter);
            assert_eq!(p.gone_count(), 0, "{interruption} must reset the count");
            // The streak restarts: one more gone answer is 1/10, not a purge.
            assert!(matches!(
                p.on_failure_with(&TransportError::RunnerGone, t0 + mins(7), no_jitter),
                RetryDecision::Retry { .. }
            ));
            assert_eq!(p.gone_count(), 1);
        }
        let mut p = ReconnectPolicy::bootstrap();
        for _ in 0..9 {
            p.on_failure_with(&TransportError::RunnerGone, Instant::now(), no_jitter);
        }
        p.on_success();
        assert_eq!(p.gone_count(), 0, "a success must reset the count");
    }

    #[test]
    fn fifty_transient_failures_of_each_kind_never_purge() {
        let kinds = [
            TransportError::Connect("connection refused".into()),
            TransportError::Connect("dns error: failed to lookup address".into()),
            TransportError::Timeout("operation timed out".into()),
            TransportError::Server {
                status: 503,
                body: "{\"detail\":\"unavailable\"}".into(),
            },
            TransportError::Server {
                status: 502,
                body: "<html><body>502 Bad Gateway</body></html>".into(),
            },
            TransportError::RateLimited { retry_after: None },
        ];
        for kind in kinds {
            let mut p = ReconnectPolicy::bootstrap();
            let t0 = Instant::now();
            for i in 0..50u64 {
                let decision = p.on_failure_with(&kind, t0 + mins(60 * i), no_jitter);
                assert!(
                    matches!(decision, RetryDecision::Retry { .. }),
                    "{kind} purged after {} failures",
                    i + 1
                );
            }
            assert_eq!(p.gone_count(), 0);
        }
    }

    #[test]
    fn gone_answer_resets_the_outage_backoff() {
        let mut p = ReconnectPolicy::bootstrap();
        let t0 = Instant::now();
        let err = TransportError::Connect("refused".into());
        for _ in 0..8 {
            p.on_failure_with(&err, t0, no_jitter);
        }
        p.on_failure_with(&TransportError::RunnerGone, t0, no_jitter);
        assert!(matches!(
            p.on_failure_with(&err, t0, no_jitter),
            RetryDecision::Retry { delay, .. } if delay == Duration::from_secs(30)
        ));
    }

    #[test]
    fn failure_reasons_read_well_in_status() {
        assert_eq!(
            failure_reason(&TransportError::Connect("x".into())),
            "server unreachable"
        );
        assert_eq!(
            failure_reason(&TransportError::Server {
                status: 502,
                body: "<html>".into()
            }),
            "server unreachable: HTTP 502"
        );
        assert_eq!(
            failure_reason(&TransportError::RateLimited { retry_after: None }),
            "rate limited"
        );
    }
}
