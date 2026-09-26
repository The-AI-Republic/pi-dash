#![forbid(unsafe_code)]

//! Beat-equivalent schedule: which task fires, and when.
//!
//! Transcribes `app.conf.beat_schedule` from `apps/api/pi_dash/celery.py`
//! — both the 22-entry literal dict and the 4 settings-backed entries
//! registered in `_register_settings_backed_beat_entries` — into data the
//! scheduler loop in [`crate::scheduler`] evaluates every tick.
//!
//! Two cadence kinds, exactly the two Celery uses here:
//!
//! - `timedelta(seconds=n)` → [`Cadence::IntervalSecs`]: due when at least
//!   `n` seconds passed since the last run.
//! - `crontab(...)` → [`Cadence::Crontab`]: due when the current minute
//!   matches every field (Celery's `crontab.is_due` requires all five of
//!   minute, hour, day-of-month, month and day-of-week to match) and the
//!   entry did not already fire in this minute — the once-per-minute
//!   guard that replaces beat's precise next-due bookkeeping.
//!
//! Last-run state persists in [`SCHEDULE_TABLE`] so a restart does not
//! refire entries (the `DatabaseScheduler` half of the Python setup).

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Datelike, Timelike, Utc};
use thiserror::Error as ThisError;

/// Table holding the last fire time per schedule entry.
pub const SCHEDULE_TABLE: &str = "rust_job_schedule";

/// One minute-field value of a crontab entry: an explicit set of matching
/// values, parsed from Celery's `crontab(...)` string syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronField {
    values: BTreeSet<u32>,
}

impl CronField {
    fn all(min: u32, max: u32) -> Self {
        Self {
            values: (min..=max).collect(),
        }
    }

    /// Parse one field: `*`, `*/n`, `a`, `a-b`, `a-b/n`, comma lists.
    /// Bounds follow Celery's crontab ranges (minute 0-59, hour 0-23,
    /// day-of-month 1-31, month 1-12, day-of-week 0-6 with 7 as Sunday).
    pub fn parse(spec: &str, min: u32, max: u32) -> Result<Self, ScheduleError> {
        let mut values = BTreeSet::new();
        for part in spec.split(',') {
            let part = part.trim();
            let (range, step) = match part.split_once('/') {
                Some((range, step)) => (
                    range,
                    step.parse::<u32>().map_err(|_| ScheduleError::BadCron {
                        spec: spec.to_owned(),
                    })?,
                ),
                None => (part, 1),
            };
            if step == 0 {
                return Err(ScheduleError::BadCron {
                    spec: spec.to_owned(),
                });
            }
            let (lo, hi) = if range == "*" || range.is_empty() {
                (min, max)
            } else if let Some((a, b)) = range.split_once('-') {
                (
                    a.parse::<u32>().map_err(|_| ScheduleError::BadCron {
                        spec: spec.to_owned(),
                    })?,
                    b.parse::<u32>().map_err(|_| ScheduleError::BadCron {
                        spec: spec.to_owned(),
                    })?,
                )
            } else {
                let v = range.parse::<u32>().map_err(|_| ScheduleError::BadCron {
                    spec: spec.to_owned(),
                })?;
                (v, v)
            };
            if lo < min || hi > max || lo > hi {
                return Err(ScheduleError::BadCron {
                    spec: spec.to_owned(),
                });
            }
            // `checked_add` below: `hi` is attacker-influenced only via
            // direct API use, but a wrap-around must never hang the loop.
            let mut v = lo;
            loop {
                values.insert(v);
                match v.checked_add(step) {
                    Some(next) if next <= hi => v = next,
                    _ => break,
                }
            }
        }
        if values.is_empty() {
            return Err(ScheduleError::BadCron {
                spec: spec.to_owned(),
            });
        }
        Ok(Self { values })
    }

    pub fn matches(&self, value: u32) -> bool {
        self.values.contains(&value)
    }
}

/// A Celery `crontab(...)` schedule: all five fields must match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crontab {
    pub minute: CronField,
    pub hour: CronField,
    pub day_of_month: CronField,
    pub month_of_year: CronField,
    pub day_of_week: CronField,
}

impl Crontab {
    /// Parse the five crontab fields. `day_of_week` accepts 0-7 with 7
    /// folded to Sunday, as Celery does.
    pub fn parse(
        minute: &str,
        hour: &str,
        day_of_month: &str,
        month_of_year: &str,
        day_of_week: &str,
    ) -> Result<Self, ScheduleError> {
        let mut this = Self {
            minute: CronField::parse(minute, 0, 59)?,
            hour: CronField::parse(hour, 0, 23)?,
            day_of_month: CronField::parse(day_of_month, 1, 31)?,
            month_of_year: CronField::parse(month_of_year, 1, 12)?,
            day_of_week: CronField::parse(day_of_week, 0, 7)?,
        };
        if this.day_of_week.values.remove(&7) {
            this.day_of_week.values.insert(0);
        }
        Ok(this)
    }

    /// Every day at `hour:minute` UTC (the `crontab(hour=h, minute=m)` entries).
    pub fn daily(hour: u32, minute: u32) -> Self {
        Self {
            minute: CronField::parse(&minute.to_string(), 0, 59).expect("valid"),
            hour: CronField::parse(&hour.to_string(), 0, 23).expect("valid"),
            day_of_month: CronField::all(1, 31),
            month_of_year: CronField::all(1, 12),
            day_of_week: CronField::all(0, 6),
        }
    }

    /// True when all five fields match `now` (UTC, mirroring
    /// `CELERY_TIMEZONE = "UTC"`).
    pub fn matches(&self, now: &DateTime<Utc>) -> bool {
        // Chrono's weekday counts Monday = 0; cron counts Sunday = 0.
        let dow = now.weekday().num_days_from_sunday();
        self.minute.matches(now.minute())
            && self.hour.matches(now.hour())
            && self.day_of_month.matches(now.day())
            && self.month_of_year.matches(now.month())
            && self.day_of_week.matches(dow)
    }
}

/// How often a schedule entry fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cadence {
    /// Fire at least every `0` seconds (Celery `timedelta(seconds=n)`).
    IntervalSecs(u64),
    /// Fire when the minute matches (Celery `crontab(...)`).
    Crontab(Crontab),
}

/// One beat schedule entry: the `celery.py` name, task and cadence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeatEntry {
    pub name: &'static str,
    pub task: &'static str,
    pub cadence: Cadence,
}

/// Why a schedule spec is unusable. A bad entry never fires: the
/// scheduler loop logs and skips it (mirroring the loop scanner's
/// "bad RRULE" warning path) instead of panicking the loop.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum ScheduleError {
    #[error("bad crontab field: {spec}")]
    BadCron { spec: String },
}

/// The 22 literal entries of `app.conf.beat_schedule` in
/// `apps/api/pi_dash/celery.py`, in file order.
///
/// Note the two `delete_old_s3_link` entries (`01:30` and `03:45`): both
/// exist in the Python dict, so both fire here. Ported as-is; whether the
/// duplication is intentional is a follow-up for the exporter domain.
pub fn literal_schedule() -> Vec<BeatEntry> {
    vec![
        BeatEntry {
            name: "check-every-five-minutes-to-send-email-notifications",
            task: "pi_dash.bgtasks.email_notification_task.stack_email_notification",
            cadence: Cadence::Crontab(Crontab::parse("*/5", "*", "*", "*", "*").expect("valid")),
        },
        BeatEntry {
            name: "run-every-6-hours-for-instance-trace",
            task: "pi_dash.license.bgtasks.tracer.instance_traces",
            cadence: Cadence::Crontab(Crontab::parse("0", "*/6", "*", "*", "*").expect("valid")),
        },
        BeatEntry {
            name: "check-every-day-to-delete-hard-delete",
            task: "pi_dash.bgtasks.deletion_task.hard_delete",
            cadence: Cadence::Crontab(Crontab::daily(0, 0)),
        },
        BeatEntry {
            name: "check-every-day-to-archive-and-close",
            task: "pi_dash.bgtasks.issue_automation_task.archive_and_close_old_issues",
            cadence: Cadence::Crontab(Crontab::daily(1, 0)),
        },
        BeatEntry {
            name: "check-every-day-to-delete_exporter_history",
            task: "pi_dash.bgtasks.exporter_expired_task.delete_old_s3_link",
            cadence: Cadence::Crontab(Crontab::daily(1, 30)),
        },
        BeatEntry {
            name: "check-every-day-to-delete-file-asset",
            task: "pi_dash.bgtasks.file_asset_task.delete_unuploaded_file_asset",
            cadence: Cadence::Crontab(Crontab::daily(2, 0)),
        },
        BeatEntry {
            name: "check-every-day-to-delete-api-logs",
            task: "pi_dash.bgtasks.cleanup_task.delete_api_logs",
            cadence: Cadence::Crontab(Crontab::daily(2, 30)),
        },
        BeatEntry {
            name: "check-every-day-to-delete-email-notification-logs",
            task: "pi_dash.bgtasks.cleanup_task.delete_email_notification_logs",
            cadence: Cadence::Crontab(Crontab::daily(2, 45)),
        },
        BeatEntry {
            name: "check-every-day-to-delete-page-versions",
            task: "pi_dash.bgtasks.cleanup_task.delete_page_versions",
            cadence: Cadence::Crontab(Crontab::daily(3, 0)),
        },
        BeatEntry {
            name: "check-every-day-to-delete-issue-description-versions",
            task: "pi_dash.bgtasks.cleanup_task.delete_issue_description_versions",
            cadence: Cadence::Crontab(Crontab::daily(3, 15)),
        },
        BeatEntry {
            name: "check-every-day-to-delete-webhook-logs",
            task: "pi_dash.bgtasks.cleanup_task.delete_webhook_logs",
            cadence: Cadence::Crontab(Crontab::daily(3, 30)),
        },
        BeatEntry {
            name: "check-every-day-to-delete-exporter-history",
            task: "pi_dash.bgtasks.exporter_expired_task.delete_old_s3_link",
            cadence: Cadence::Crontab(Crontab::daily(3, 45)),
        },
        BeatEntry {
            name: "runner-expire-stale-approvals",
            task: "runner.expire_stale_approvals",
            cadence: Cadence::Crontab(Crontab::parse("*/1", "*", "*", "*", "*").expect("valid")),
        },
        BeatEntry {
            name: "runner-mark-offline-runners",
            task: "runner.mark_offline_runners",
            cadence: Cadence::Crontab(Crontab::parse("*/1", "*", "*", "*", "*").expect("valid")),
        },
        BeatEntry {
            name: "runner-reconcile-stalled-runs",
            task: "runner.reconcile_stalled_runs",
            cadence: Cadence::IntervalSecs(30),
        },
        BeatEntry {
            name: "runner-sweep-agent-chat-state",
            task: "runner.sweep_agent_chat_state",
            cadence: Cadence::IntervalSecs(30),
        },
        BeatEntry {
            name: "assistant-sweep-stale-turns",
            task: "assistant.sweep_stale_turns",
            cadence: Cadence::IntervalSecs(30),
        },
        BeatEntry {
            name: "runner-sweep-chat-message-dedupe",
            task: "runner.sweep_chat_message_dedupe",
            cadence: Cadence::Crontab(Crontab::daily(4, 0)),
        },
        BeatEntry {
            name: "scan-due-agent-tickers",
            task: "pi_dash.bgtasks.agent_ticker.scan_due_tickers",
            cadence: Cadence::Crontab(Crontab::parse("*", "*", "*", "*", "*").expect("valid")),
        },
        BeatEntry {
            name: "github-issue-sync-every-4h",
            task: "pi_dash.bgtasks.git_sync_task.sync_all_bindings",
            cadence: Cadence::Crontab(Crontab::parse("0", "*/4", "*", "*", "*").expect("valid")),
        },
        BeatEntry {
            name: "scan-due-scheduler-bindings",
            task: "pi_dash.bgtasks.scheduler.scan_due_bindings",
            cadence: Cadence::Crontab(Crontab::parse("*", "*", "*", "*", "*").expect("valid")),
        },
        BeatEntry {
            name: "scan-due-loop-targets",
            task: "pi_dash.bgtasks.loop.scan_due_targets",
            cadence: Cadence::Crontab(Crontab::parse("*", "*", "*", "*", "*").expect("valid")),
        },
    ]
}

/// Defaults for the 4 settings-backed entries registered in
/// `_register_settings_backed_beat_entries` (from `settings/common.py`).
pub const DEFAULT_CLOUD_AGENT_DISPATCH_SECS: u64 = 10;
pub const DEFAULT_CLOUD_AGENT_SWEEP_SECS: u64 = 30;
pub const DEFAULT_MANAGED_RUNNER_SWEEP_SECS: u64 = 300;
pub const DEFAULT_AGENT_RUN_TERMINAL_RECONCILE_SECS: u64 = 30;

/// The 4 settings-backed entries with Django defaults. The scheduler loop
/// applies environment overrides on top via [`beat_schedule`].
pub fn settings_backed_schedule() -> Vec<BeatEntry> {
    vec![
        BeatEntry {
            name: "cloud-agent-scan-queued-runs",
            task: "cloud_agent.scan_queued_runs",
            cadence: Cadence::IntervalSecs(DEFAULT_CLOUD_AGENT_DISPATCH_SECS),
        },
        BeatEntry {
            name: "cloud-agent-sweep-stale-runs",
            task: "cloud_agent.sweep_stale_runs",
            cadence: Cadence::IntervalSecs(DEFAULT_CLOUD_AGENT_SWEEP_SECS),
        },
        BeatEntry {
            name: "managed-runner-expire-waiting-runs",
            task: "managed_runner.expire_waiting_runs",
            cadence: Cadence::IntervalSecs(DEFAULT_MANAGED_RUNNER_SWEEP_SECS),
        },
        BeatEntry {
            name: "agent-run-reconcile-terminal-effects",
            task: "runner.reconcile_agent_run_terminal_effects",
            cadence: Cadence::IntervalSecs(DEFAULT_AGENT_RUN_TERMINAL_RECONCILE_SECS),
        },
    ]
}

fn env_secs(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

/// The full schedule: the 22 literal entries plus the 4 settings-backed
/// entries with their Django-setting environment overrides applied
/// (`CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS`,
/// `CLOUD_AGENT_SWEEP_INTERVAL_SECONDS`,
/// `MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS`,
/// `AGENT_RUN_TERMINAL_RECONCILE_INTERVAL_SECONDS` — same names as
/// `settings/common.py`).
pub fn beat_schedule() -> Vec<BeatEntry> {
    let mut entries = literal_schedule();
    let mut backed = settings_backed_schedule();
    for entry in &mut backed {
        let Cadence::IntervalSecs(default) = entry.cadence else {
            continue;
        };
        let env_name = match entry.name {
            "cloud-agent-scan-queued-runs" => "CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS",
            "cloud-agent-sweep-stale-runs" => "CLOUD_AGENT_SWEEP_INTERVAL_SECONDS",
            "managed-runner-expire-waiting-runs" => "MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS",
            "agent-run-reconcile-terminal-effects" => {
                "AGENT_RUN_TERMINAL_RECONCILE_INTERVAL_SECONDS"
            }
            _ => continue,
        };
        entry.cadence = Cadence::IntervalSecs(env_secs(env_name, default));
    }
    entries.extend(backed);
    entries
}

/// True when `entry` is due at `now` given its last fire time.
///
/// - Intervals fire when never run or when `n` seconds elapsed.
/// - Crontabs fire when the minute matches and the entry did not already
///   fire in this minute (the singleton-tick guard).
pub fn is_due(entry: &BeatEntry, now: &DateTime<Utc>, last_run: Option<&DateTime<Utc>>) -> bool {
    match &entry.cadence {
        Cadence::IntervalSecs(secs) => match last_run {
            None => true,
            Some(last) => now.signed_duration_since(*last).num_seconds() >= *secs as i64,
        },
        Cadence::Crontab(cron) => {
            if !cron.matches(now) {
                return false;
            }
            match last_run {
                None => true,
                Some(last) => {
                    last.date_naive() != now.date_naive()
                        || last.hour() != now.hour()
                        || last.minute() != now.minute()
                }
            }
        }
    }
}

/// The entries due at `now`, given each entry's last fire time.
pub fn due_entries<'a>(
    schedule: &'a [BeatEntry],
    now: &DateTime<Utc>,
    last_runs: &HashMap<String, DateTime<Utc>>,
) -> Vec<&'a BeatEntry> {
    schedule
        .iter()
        .filter(|entry| is_due(entry, now, last_runs.get(entry.name)))
        .collect()
}

/// DDL for the schedule-state table. Idempotent.
pub fn ensure_schedule_schema_sql() -> &'static str {
    "CREATE TABLE IF NOT EXISTS rust_job_schedule (\
        name TEXT PRIMARY KEY,\
        last_run_at TIMESTAMPTZ NOT NULL DEFAULT now()\
    )"
}

fn fetch_last_runs_sql_inner() -> &'static str {
    "SELECT name, last_run_at FROM rust_job_schedule"
}

/// Load every entry's last fire time.
pub fn fetch_last_runs_sql() -> &'static str {
    fetch_last_runs_sql_inner()
}

fn mark_run_sql_inner() -> &'static str {
    "INSERT INTO rust_job_schedule (name, last_run_at) VALUES ($1, $2) \
     ON CONFLICT (name) DO UPDATE SET last_run_at = EXCLUDED.last_run_at"
}

/// Record an entry's fire time (upsert, so first fires insert).
pub fn mark_run_sql() -> &'static str {
    mark_run_sql_inner()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    #[test]
    fn literal_schedule_has_all_22_entries() {
        let schedule = literal_schedule();
        assert_eq!(schedule.len(), 22);
        let names: Vec<_> = schedule.iter().map(|e| e.name).collect();
        for expected in [
            "check-every-five-minutes-to-send-email-notifications",
            "run-every-6-hours-for-instance-trace",
            "check-every-day-to-delete-hard-delete",
            "check-every-day-to-archive-and-close",
            "check-every-day-to-delete_exporter_history",
            "check-every-day-to-delete-file-asset",
            "check-every-day-to-delete-api-logs",
            "check-every-day-to-delete-email-notification-logs",
            "check-every-day-to-delete-page-versions",
            "check-every-day-to-delete-issue-description-versions",
            "check-every-day-to-delete-webhook-logs",
            "check-every-day-to-delete-exporter-history",
            "runner-expire-stale-approvals",
            "runner-mark-offline-runners",
            "runner-reconcile-stalled-runs",
            "runner-sweep-agent-chat-state",
            "assistant-sweep-stale-turns",
            "runner-sweep-chat-message-dedupe",
            "scan-due-agent-tickers",
            "github-issue-sync-every-4h",
            "scan-due-scheduler-bindings",
            "scan-due-loop-targets",
        ] {
            assert!(names.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn full_schedule_adds_four_settings_backed_entries() {
        // Defaults only: a developer shell exporting the Django interval
        // names must not change what this asserts (overrides are covered
        // by `env_secs` fallback behavior, not here).
        for var in [
            "CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS",
            "CLOUD_AGENT_SWEEP_INTERVAL_SECONDS",
            "MANAGED_RUNNER_SWEEP_INTERVAL_SECONDS",
            "AGENT_RUN_TERMINAL_RECONCILE_INTERVAL_SECONDS",
        ] {
            std::env::remove_var(var);
        }
        let schedule = beat_schedule();
        assert_eq!(schedule.len(), 26);
        let find = |name: &str| schedule.iter().find(|e| e.name == name).expect(name);
        assert_eq!(
            find("cloud-agent-scan-queued-runs").cadence,
            Cadence::IntervalSecs(10)
        );
        assert_eq!(
            find("cloud-agent-sweep-stale-runs").cadence,
            Cadence::IntervalSecs(30)
        );
        assert_eq!(
            find("managed-runner-expire-waiting-runs").cadence,
            Cadence::IntervalSecs(300)
        );
        assert_eq!(
            find("agent-run-reconcile-terminal-effects").cadence,
            Cadence::IntervalSecs(30)
        );
    }

    #[test]
    fn env_override_wins_over_django_default() {
        // Uses a unique var name so parallel tests cannot race it.
        std::env::set_var("PIDASH_JOBS_TEST_INTERVAL_VAR", "42");
        assert_eq!(env_secs("PIDASH_JOBS_TEST_INTERVAL_VAR", 7), 42);
        std::env::set_var("PIDASH_JOBS_TEST_INTERVAL_VAR", "bogus");
        assert_eq!(env_secs("PIDASH_JOBS_TEST_INTERVAL_VAR", 7), 7);
        std::env::set_var("PIDASH_JOBS_TEST_INTERVAL_VAR", "0");
        assert_eq!(env_secs("PIDASH_JOBS_TEST_INTERVAL_VAR", 7), 7);
        std::env::remove_var("PIDASH_JOBS_TEST_INTERVAL_VAR");
    }

    #[test]
    fn crontab_fields_match_celery_semantics() {
        let every_five = Crontab::parse("*/5", "*", "*", "*", "*").unwrap();
        assert!(every_five.matches(&utc(2026, 9, 26, 12, 0, 0)));
        assert!(every_five.matches(&utc(2026, 9, 26, 12, 5, 30)));
        assert!(!every_five.matches(&utc(2026, 9, 26, 12, 6, 0)));

        let daily = Crontab::daily(2, 30);
        assert!(daily.matches(&utc(2026, 9, 26, 2, 30, 0)));
        assert!(!daily.matches(&utc(2026, 9, 26, 2, 31, 0)));
        assert!(!daily.matches(&utc(2026, 9, 26, 3, 30, 0)));

        let every_4h = Crontab::parse("0", "*/4", "*", "*", "*").unwrap();
        assert!(every_4h.matches(&utc(2026, 9, 26, 4, 0, 0)));
        assert!(!every_4h.matches(&utc(2026, 9, 26, 5, 0, 0)));

        // Comma lists and ranges.
        let mixed = Crontab::parse("0,30", "1-3", "*", "*", "*").unwrap();
        assert!(mixed.matches(&utc(2026, 9, 26, 2, 30, 0)));
        assert!(!mixed.matches(&utc(2026, 9, 26, 4, 30, 0)));
        assert!(!mixed.matches(&utc(2026, 9, 26, 2, 15, 0)));
    }

    #[test]
    fn bad_crontab_specs_are_rejected() {
        assert!(CronField::parse("*/0", 0, 59).is_err());
        assert!(CronField::parse("61", 0, 59).is_err());
        assert!(CronField::parse("nope", 0, 59).is_err());
        assert!(CronField::parse("5-2", 0, 59).is_err());
    }

    #[test]
    fn crontab_due_fires_once_per_minute() {
        let entry = BeatEntry {
            name: "x",
            task: "t",
            cadence: Cadence::Crontab(Crontab::parse("*", "*", "*", "*", "*").unwrap()),
        };
        let now = utc(2026, 9, 26, 12, 0, 10);
        assert!(is_due(&entry, &now, None));
        assert!(!is_due(&entry, &now, Some(&utc(2026, 9, 26, 12, 0, 0))));
        assert!(is_due(&entry, &now, Some(&utc(2026, 9, 26, 11, 59, 0))));
        assert!(!is_due(
            &BeatEntry {
                name: "y",
                task: "t",
                cadence: Cadence::Crontab(Crontab::daily(3, 0)),
            },
            &now,
            None
        ));
    }

    #[test]
    fn interval_due_tracks_elapsed_time() {
        let entry = BeatEntry {
            name: "x",
            task: "t",
            cadence: Cadence::IntervalSecs(30),
        };
        let now = utc(2026, 9, 26, 12, 0, 0);
        assert!(is_due(&entry, &now, None));
        assert!(!is_due(&entry, &now, Some(&utc(2026, 9, 26, 11, 59, 45))));
        assert!(is_due(&entry, &now, Some(&utc(2026, 9, 26, 11, 59, 30))));
    }

    #[test]
    fn due_entries_selects_only_due() {
        let schedule = vec![
            BeatEntry {
                name: "a",
                task: "t",
                cadence: Cadence::IntervalSecs(30),
            },
            BeatEntry {
                name: "b",
                task: "t",
                cadence: Cadence::Crontab(Crontab::daily(3, 0)),
            },
        ];
        let now = utc(2026, 9, 26, 12, 0, 0);
        let due = due_entries(&schedule, &now, &HashMap::new());
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].name, "a");
    }
}
