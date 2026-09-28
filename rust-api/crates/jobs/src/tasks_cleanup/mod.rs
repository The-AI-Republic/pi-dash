//! Issue/analytics export + expiry tasks (D-09, jobs layer).
//!
//! Port of `apps/api/pi_dash/bgtasks/export_task.py:127-226`
//! (`issue_export_task`), `exporter_expired_task.py:22-53`
//! (`delete_old_s3_link`) and `analytic_plot_export.py:349-435`
//! (`analytic_export_task`, `export_analytics_to_csv_email`).
//!
//! This module owns the Celery wire surface: the four task names, the
//! `.delay()` payload constructors and the expiry-cutoff renderer. The
//! pure row/CSV/ZIP/S3/mail decisions live in `pidash-services`
//! (`tasks_cleanup::exports`); the DTOs in `pidash-types`.
//!
//! Ownership: these tasks stay Python-owned. No local handler is
//! registered here — registering one would steal live traffic from the
//! Python workers while S3, SMTP and the exporter serializers still
//! live there. The domain gate flips ownership after the oracle replay
//! passes on both backends. Until then [`is_export_task`] is the routing
//! predicate the worker consults, and every name routes to `PythonOwned`.
//!
//! * [`workspace_seed`] — `bgtasks/workspace_seed_task.py` (PIDASHCONV-190).
//!
//! Background-task handlers for the D-09 cleanup domain (stage 5).
//!
//! Ports the Celery registration half of
//! `apps/api/pi_dash/bgtasks/deletion_task.py` (PIDASHCONV-186). Task
//! bodies live in `pidash-services` (`tasks_cleanup::deletion`); this
//! module owns the Celery task names, the `(args, kwargs)` extraction,
//! and the [`Registry`][crate::worker::Registry] wiring.
//!
//! Ownership note: [`register_deletion_tasks`] only builds the handler
//! table. Flipping the live worker to these handlers (calling it from
//! the binary) is the domain gate's call (PIDASHCONV-192, after the
//! PIDASHCONV-21 proxy pass) — not this layer issue.

use chrono::{DateTime, Duration, Utc};
use serde_json::{Map, Value};

use crate::celery::CeleryTaskMessage;
use pidash_types::tasks_cleanup::exports_dto::{
    ANALYTIC_EXPORT_TASK_NAME, DELETE_OLD_S3_LINK_TASK_NAME, EXPIRY_DAYS,
    EXPORT_ANALYTICS_CSV_TASK_NAME, ISSUE_EXPORT_TASK_NAME,
};

pub use pidash_types::tasks_cleanup::exports_dto::EXPORT_TASK_NAMES;

// D-09 cleanup retention + mongo flush handlers (PIDASHCONV-185):
// port of the `@shared_task` entry points of cleanup_task.py (`:422-:479`).
pub mod assets;
pub mod cleanup;
pub mod deletion;

pub use cleanup::register_cleanup_handlers;
pub use deletion::{
    hard_delete_handler, register_deletion_tasks, soft_delete_handler, HARD_DELETE_TASK,
    RESTORE_TASK_NAME, SOFT_DELETE_TASK,
};

/// True for the four D-09 export task names. The worker forwards them to
/// the Python plane until the domain gate flips ownership.
pub fn is_export_task(task: &str) -> bool {
    EXPORT_TASK_NAMES.contains(&task)
}

/// `issue_export_task.delay(provider, workspace_id, project_ids,
/// token_id, multiple, slug)` (`export_task.py:128-135`): all six
/// parameters travel as positional Celery args.
pub fn issue_export_task_message(
    provider: &str,
    workspace_id: &str,
    project_ids: &[String],
    token_id: &str,
    multiple: bool,
    slug: &str,
) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        ISSUE_EXPORT_TASK_NAME,
        vec![
            Value::String(provider.to_owned()),
            Value::String(workspace_id.to_owned()),
            Value::Array(project_ids.iter().cloned().map(Value::String).collect()),
            Value::String(token_id.to_owned()),
            Value::Bool(multiple),
            Value::String(slug.to_owned()),
        ],
        Map::new(),
    )
}

/// `delete_old_s3_link.delay()` (`exporter_expired_task.py:22-23`): no
/// arguments.
pub fn delete_old_s3_link_message() -> CeleryTaskMessage {
    CeleryTaskMessage::new(DELETE_OLD_S3_LINK_TASK_NAME, Vec::new(), Map::new())
}

/// `analytic_export_task.delay(email, data, slug)`
/// (`analytic_plot_export.py:350`): `data` is the analytics request
/// dict verbatim.
pub fn analytic_export_task_message(
    email: &str,
    data: Map<String, Value>,
    slug: &str,
) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        ANALYTIC_EXPORT_TASK_NAME,
        vec![
            Value::String(email.to_owned()),
            Value::Object(data),
            Value::String(slug.to_owned()),
        ],
        Map::new(),
    )
}

/// `export_analytics_to_csv_email.delay(data, headers, keys, email,
/// slug)` (`:410`): rows plus their header/key lists travel verbatim.
pub fn export_analytics_csv_message(
    data: Vec<Map<String, Value>>,
    headers: &[String],
    keys: &[String],
    email: &str,
    slug: &str,
) -> CeleryTaskMessage {
    CeleryTaskMessage::new(
        EXPORT_ANALYTICS_CSV_TASK_NAME,
        vec![
            Value::Array(data.into_iter().map(Value::Object).collect()),
            Value::Array(headers.iter().cloned().map(Value::String).collect()),
            Value::Array(keys.iter().cloned().map(Value::String).collect()),
            Value::String(email.to_owned()),
            Value::String(slug.to_owned()),
        ],
        Map::new(),
    )
}

/// Render `timezone.now() - timedelta(days=8)` the way the Django ORM
/// renders it into the expiry scan (`2026-09-20 06:00:00+00:00` for the
/// frozen fixture instant). `str(datetime)` keeps the `.%f` fraction
/// only when microseconds are nonzero, so the fraction is emitted
/// conditionally. Feed the result to the services-layer
/// `tasks_cleanup::exports::delete_old_s3_link_sql`.
pub fn format_expiry_cutoff(now: DateTime<Utc>) -> String {
    let cutoff = now - Duration::days(EXPIRY_DAYS);
    let micros = cutoff.timestamp_subsec_micros();
    if micros == 0 {
        cutoff.format("%Y-%m-%d %H:%M:%S+00:00").to_string()
    } else {
        format!("{}.{:06}+00:00", cutoff.format("%Y-%m-%d %H:%M:%S"), micros)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    #[test]
    fn export_tasks_route_python_owned() {
        for name in EXPORT_TASK_NAMES {
            assert!(is_export_task(name), "{name} must be recognised");
        }
        assert!(!is_export_task(
            "pi_dash.bgtasks.cleanup_task.delete_api_logs"
        ));
        assert!(!is_export_task(""));
    }

    #[test]
    fn issue_export_task_wire_shape() {
        let message = issue_export_task_message(
            "csv",
            "ws-1",
            &["p-1".to_owned(), "p-2".to_owned()],
            "tok-9",
            true,
            "my-slug",
        );
        assert_eq!(message.task, ISSUE_EXPORT_TASK_NAME);
        assert_eq!(
            message.args,
            vec![
                Value::String("csv".into()),
                Value::String("ws-1".into()),
                Value::Array(vec![
                    Value::String("p-1".into()),
                    Value::String("p-2".into())
                ]),
                Value::String("tok-9".into()),
                Value::Bool(true),
                Value::String("my-slug".into()),
            ]
        );
        assert!(message.kwargs.is_empty());
    }

    #[test]
    fn delete_old_s3_link_wire_shape() {
        let message = delete_old_s3_link_message();
        assert_eq!(message.task, DELETE_OLD_S3_LINK_TASK_NAME);
        assert!(message.args.is_empty() && message.kwargs.is_empty());
    }

    #[test]
    fn analytic_tasks_wire_shapes() {
        let mut data = Map::new();
        data.insert("x_axis".to_owned(), Value::String("state_id".into()));
        let message = analytic_export_task_message("a@x.io", data, "ws");
        assert_eq!(message.task, ANALYTIC_EXPORT_TASK_NAME);
        assert_eq!(message.args.len(), 3);

        let message = export_analytics_csv_message(
            Vec::new(),
            &["H".to_owned()],
            &["k".to_owned()],
            "a@x.io",
            "ws",
        );
        assert_eq!(message.task, EXPORT_ANALYTICS_CSV_TASK_NAME);
        assert_eq!(message.args.len(), 5);
    }

    #[test]
    fn expiry_cutoff_matches_fixture_instant() {
        // Frozen fixture instant 2026-09-28T06:00:00Z.
        let now = Utc.with_ymd_and_hms(2026, 9, 28, 6, 0, 0).unwrap();
        assert_eq!(format_expiry_cutoff(now), "2026-09-20 06:00:00+00:00");
    }

    #[test]
    fn expiry_cutoff_keeps_nonzero_microseconds() {
        // `str(timezone.now() - timedelta(days=8))` keeps `.%f` iff
        // nonzero; production `now()` carries microseconds.
        let now = Utc
            .with_ymd_and_hms(2026, 9, 28, 6, 0, 0)
            .unwrap()
            .with_nanosecond(123_456_000)
            .unwrap();
        assert_eq!(
            format_expiry_cutoff(now),
            "2026-09-20 06:00:00.123456+00:00"
        );
    }
}

pub mod workspace_seed;

pub use workspace_seed::{register_workspace_seed, TASK_NAME as WORKSPACE_SEED_TASK_NAME};
// D-09 version tasks: issue/description/page versions (PIDASHCONV-188).
// Worker-plane port of `issue_version_sync.py`,
// `issue_description_version_sync.py`, `issue_description_version_task.py`
// and `page_version_task.py`: names, parsing, message constructors and
// the seven task flows live in `versions`.
pub mod versions;

pub use versions::register_versions;
//! Sibling submodule: [`dummy_data`] ports
//! `apps/api/pi_dash/bgtasks/dummy_data_task.py` (execution + [`Registry`]
//! wiring); see its module docs.
//!
//! [`Registry`]: crate::worker::Registry

pub mod dummy_data;
