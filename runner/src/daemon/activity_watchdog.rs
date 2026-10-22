//! Passive-kill activity watchdog for one agent run.
//!
//! The runner cannot tell a frozen agent from a live one sitting silent inside
//! a long tool call, so it never kills a run just for being quiet. Provable
//! deaths (agent stdout closing, a terminal agent error, the turn ending) are
//! handled where they surface. This watchdog is only the backstop for a run
//! that never proves itself dead: after `unhealthy_after` without an agent
//! event the run is marked unhealthy (reported, not killed); after
//! `kill_after` without one it is failed. Any agent event resets both clocks.
//!
//! Pure state machine over `tokio::time::Instant` so the supervisor loop stays
//! thin and the thresholds are unit-testable without a live agent.

use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::time::Instant;

use crate::config::schema::AgentWatchdogConfig;

/// What the supervisor must do when the watchdog's deadline fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WatchdogAction {
    /// Report the run unhealthy and keep it running.
    MarkUnhealthy,
    /// Fail the run.
    Kill,
}

#[derive(Debug)]
pub(crate) struct ActivityWatchdog {
    unhealthy_after: Duration,
    kill_after: Duration,
    last_activity: Instant,
    last_activity_at: DateTime<Utc>,
    unhealthy: bool,
}

impl ActivityWatchdog {
    pub(crate) fn new(cfg: AgentWatchdogConfig, now: Instant, now_at: DateTime<Utc>) -> Self {
        Self::with_thresholds(cfg.unhealthy_after(), cfg.kill_after(), now, now_at)
    }

    pub(crate) fn with_thresholds(
        unhealthy_after: Duration,
        kill_after: Duration,
        now: Instant,
        now_at: DateTime<Utc>,
    ) -> Self {
        Self {
            unhealthy_after,
            kill_after: kill_after.max(unhealthy_after),
            last_activity: now,
            last_activity_at: now_at,
            unhealthy: false,
        }
    }

    /// Record agent activity. Returns `true` when this clears an unhealthy
    /// mark, so the caller can report the recovery.
    pub(crate) fn on_activity(&mut self, now: Instant, now_at: DateTime<Utc>) -> bool {
        self.last_activity = now;
        self.last_activity_at = now_at;
        std::mem::replace(&mut self.unhealthy, false)
    }

    /// The next instant the supervisor must wake for, and what to do then.
    pub(crate) fn next_deadline(&self) -> (Instant, WatchdogAction) {
        if self.unhealthy {
            (self.last_activity + self.kill_after, WatchdogAction::Kill)
        } else {
            (
                self.last_activity + self.unhealthy_after,
                WatchdogAction::MarkUnhealthy,
            )
        }
    }

    /// Apply the action due at the deadline from [`next_deadline`].
    pub(crate) fn on_deadline(&mut self) -> WatchdogAction {
        let (_, action) = self.next_deadline();
        if action == WatchdogAction::MarkUnhealthy {
            self.unhealthy = true;
        }
        action
    }

    pub(crate) fn is_unhealthy(&self) -> bool {
        self.unhealthy
    }

    pub(crate) fn last_activity_at(&self) -> DateTime<Utc> {
        self.last_activity_at
    }

    pub(crate) fn unhealthy_after(&self) -> Duration {
        self.unhealthy_after
    }

    pub(crate) fn kill_after(&self) -> Duration {
        self.kill_after
    }
}

/// Render a watchdog threshold for operator-facing messages: whole hours when
/// it is a whole number of hours, else minutes, else seconds.
pub(crate) fn describe_threshold(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 3600 && secs.is_multiple_of(3600) {
        let h = secs / 3600;
        format!("{h} hour{}", if h == 1 { "" } else { "s" })
    } else if secs >= 60 && secs.is_multiple_of(60) {
        let m = secs / 60;
        format!("{m} minute{}", if m == 1 { "" } else { "s" })
    } else {
        format!("{secs} second{}", if secs == 1 { "" } else { "s" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: Duration = Duration::from_secs(3600);

    fn watchdog(now: Instant) -> ActivityWatchdog {
        ActivityWatchdog::new(AgentWatchdogConfig::default(), now, Utc::now())
    }

    #[test]
    fn marks_unhealthy_at_24h_then_kills_at_48h_of_silence() {
        let t0 = Instant::now();
        let mut wd = watchdog(t0);

        assert_eq!(
            wd.next_deadline(),
            (t0 + 24 * H, WatchdogAction::MarkUnhealthy)
        );
        assert_eq!(wd.on_deadline(), WatchdogAction::MarkUnhealthy);
        assert!(wd.is_unhealthy());

        // The kill deadline counts from the last activity, not from the mark.
        assert_eq!(wd.next_deadline(), (t0 + 48 * H, WatchdogAction::Kill));
        assert_eq!(wd.on_deadline(), WatchdogAction::Kill);
    }

    #[test]
    fn activity_resets_both_clocks_and_clears_the_unhealthy_mark() {
        let t0 = Instant::now();
        let mut wd = watchdog(t0);
        assert!(
            !wd.on_activity(t0 + H, Utc::now()),
            "healthy run has nothing to clear"
        );
        assert_eq!(
            wd.next_deadline(),
            (t0 + 25 * H, WatchdogAction::MarkUnhealthy)
        );

        wd.on_deadline();
        let at = Utc::now();
        assert!(
            wd.on_activity(t0 + 30 * H, at),
            "activity reports the recovery"
        );
        assert!(!wd.is_unhealthy());
        assert_eq!(wd.last_activity_at(), at);
        assert_eq!(
            wd.next_deadline(),
            (t0 + 54 * H, WatchdogAction::MarkUnhealthy)
        );
    }

    #[test]
    fn kill_threshold_is_never_below_the_unhealthy_threshold() {
        let t0 = Instant::now();
        let mut wd = ActivityWatchdog::with_thresholds(2 * H, H, t0, Utc::now());
        assert_eq!(wd.kill_after(), 2 * H);
        wd.on_deadline();
        // Equal thresholds: the kill is due at the same instant as the mark.
        assert_eq!(wd.next_deadline(), (t0 + 2 * H, WatchdogAction::Kill));
    }

    #[test]
    fn describe_threshold_picks_the_largest_whole_unit() {
        assert_eq!(describe_threshold(48 * H), "48 hours");
        assert_eq!(describe_threshold(H), "1 hour");
        assert_eq!(
            describe_threshold(Duration::from_secs(90 * 60)),
            "90 minutes"
        );
        assert_eq!(describe_threshold(Duration::from_secs(45)), "45 seconds");
    }
}
