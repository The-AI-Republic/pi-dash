//! Export-task DTOs: wire names, constants and pure value builders.
//!
//! Ports `apps/api/pi_dash/bgtasks/export_task.py:28-124`,
//! `exporter_expired_task.py:23-53` and
//! `analytic_plot_export.py:29-88,410-435` (shapes only; row/CSV logic
//! lives in `pidash-services`).
//!
//! DRF Decimal/datetime rendering is byte-exact passthrough at this
//! layer: numeric and temporal CSV cells arrive already rendered by the
//! query/serializer edge (Python `str(value)`), and [`CsvCell::render`]
//! preserves that string verbatim — this module never reformats numbers
//! or datetimes.

use serde::{Deserialize, Serialize};

/// Celery wire name, exactly as `.delay()` and the beat schedule call it
/// (`export_task.py:127-128` `@shared_task`).
pub const ISSUE_EXPORT_TASK_NAME: &str = "pi_dash.bgtasks.export_task.issue_export_task";

/// `exporter_expired_task.py:22-23` `@shared_task`.
pub const DELETE_OLD_S3_LINK_TASK_NAME: &str =
    "pi_dash.bgtasks.exporter_expired_task.delete_old_s3_link";

/// `analytic_plot_export.py:349-350` `@shared_task`.
pub const ANALYTIC_EXPORT_TASK_NAME: &str =
    "pi_dash.bgtasks.analytic_plot_export.analytic_export_task";

/// `analytic_plot_export.py:409-410` `@shared_task`.
pub const EXPORT_ANALYTICS_CSV_TASK_NAME: &str =
    "pi_dash.bgtasks.analytic_plot_export.export_analytics_to_csv_email";

/// All four names in oracle order
/// (`contract-tests/tasks_cleanup/test_cleanup_tasks.py::CLEANUP_TASKS`).
pub const EXPORT_TASK_NAMES: [&str; 4] = [
    ISSUE_EXPORT_TASK_NAME,
    DELETE_OLD_S3_LINK_TASK_NAME,
    ANALYTIC_EXPORT_TASK_NAME,
    EXPORT_ANALYTICS_CSV_TASK_NAME,
];

/// `export_task.py:46-47`: presigned-URL lifetime, 7 days in seconds.
pub const EXPORT_URL_EXPIRES_IN_SECS: u64 = 7 * 24 * 60 * 60;

/// `exporter_expired_task.py:26`: rows expire 8 days after creation.
pub const EXPIRY_DAYS: i64 = 8;

/// `exporter.py:24-27` valid `format_type` values; anything else raises
/// `ValueError`, which `issue_export_task` turns into `failed` + reason
/// (`export_task.py:192-201`).
pub const EXPORT_FORMATS: [&str; 3] = ["csv", "json", "xlsx"];

/// `analytic_plot_export.py:29-43` `row_mapping`, in source order.
/// The CSV header for an axis is `row_mapping.get(axis, "X-Axis")`
/// (`:204-207`); the y header is `row_mapping.get(y, "Y-Axis")`.
pub const ROW_MAPPING: [(&str, &str); 13] = [
    ("state__name", "State"),
    ("state__group", "State Group"),
    ("labels__id", "Label"),
    ("assignees__id", "Assignee Name"),
    ("start_date", "Start Date"),
    ("target_date", "Due Date"),
    ("completed_at", "Completed At"),
    ("created_at", "Created At"),
    ("issue_count", "Issue Count"),
    ("priority", "Priority"),
    ("estimate", "Estimate"),
    ("issue_cycle__cycle_id", "Cycle"),
    ("issue_module__module_id", "Module"),
];

/// `analytic_plot_export.py:45-49` axis constants.
pub const ASSIGNEE_ID: &str = "assignees__id";
pub const LABEL_ID: &str = "labels__id";
pub const STATE_ID: &str = "state_id";
pub const CYCLE_ID: &str = "issue_cycle__cycle_id";
pub const MODULE_ID: &str = "issue_module__module_id";

/// Fallback headers when an axis is absent from [`ROW_MAPPING`]
/// (`generate_segmented_rows:205`, `generate_non_segmented_rows:345`).
/// `x_axis="state_id"` is not a mapping key, so state-segmented exports
/// head their first column `"X-Axis"`.
pub const FALLBACK_X_HEADER: &str = "X-Axis";
pub const FALLBACK_Y_HEADER: &str = "Y-Axis";

/// `send_export_email:54` subject, verbatim.
pub const EXPORT_EMAIL_SUBJECT: &str = "Your Export is ready";

/// `send_export_email:55` template, rendered with an empty context `{}`.
pub const EXPORT_EMAIL_TEMPLATE: &str = "emails/exports/analytics.html";

/// `send_export_email:86` attachment name pattern.
pub fn analytics_attachment_name(slug: &str) -> String {
    format!("{slug}-analytics.csv")
}

/// `export_task.py:46`: `{workspace_id}/export-{slug}-{token_id[:6]}-{today}.zip`
/// with `today = timezone.now().date()` (ISO). The `[:6]` slice counts
/// Python code points, so the prefix is taken over `char`s, not bytes.
pub fn export_s3_key(workspace_id: &str, slug: &str, token_id: &str, today_iso: &str) -> String {
    let prefix: String = token_id.chars().take(6).collect();
    format!("{workspace_id}/export-{slug}-{prefix}-{today_iso}.zip")
}

/// `exporter_expired_task.py:26`: `timezone.now() - timedelta(days=8)`,
/// in epoch seconds.
pub fn expiry_cutoff_secs(now_secs: i64) -> i64 {
    now_secs - EXPIRY_DAYS * 86_400
}

/// `send_export_email:75-76`: `use_tls/use_ssl = (value == "1")`, a plain
/// string compare against the stored email configuration.
pub fn email_flag_is_one(value: &str) -> bool {
    value == "1"
}

/// `export_task.py:205-215` export filename: one file per project
/// (`multiple`) or a single workspace file.
pub fn export_filename(slug: &str, multiple: bool, project_id: &str, workspace_id: &str) -> String {
    if multiple {
        format!("{slug}-{project_id}")
    } else {
        format!("{slug}-{workspace_id}")
    }
}

/// One CSV cell. Variants mirror what Python's `csv.writer` actually
/// receives from the row builders: already-rendered strings (DRF Decimal
/// / datetime output preserved verbatim), integers, floats, booleans, or
/// `None` (missing). [`CsvCell::render`] is Python `str(value)` with
/// `None` rendering as the empty string — the writer then quotes every
/// field (`QUOTE_ALL`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CsvCell {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Empty,
}

impl CsvCell {
    /// Python `str(value)`; `None` → `""`.
    pub fn render(&self) -> String {
        match self {
            CsvCell::Text(text) => text.clone(),
            CsvCell::Int(value) => value.to_string(),
            CsvCell::Float(value) => {
                // Python `str(float)` shortest-repr rendering; Rust's
                // `{}` Display matches it for finite values (`5.0`,
                // `0.5`). Non-finite floats never occur in export rows.
                if value.fract() == 0.0 && value.is_finite() {
                    format!("{value:.1}")
                } else {
                    format!("{value}")
                }
            }
            CsvCell::Bool(value) => {
                // Python `str(True)` / `str(False)`.
                if *value {
                    "True".to_owned()
                } else {
                    "False".to_owned()
                }
            }
            CsvCell::Empty => String::new(),
        }
    }
}

/// `send_export_email:52-88` mail payload shape: recipient, subject, the
/// rendered plain-text body, the attachment name and the CSV bytes. The
/// SMTP connection flags resolve via [`email_flag_is_one`].
#[derive(Debug, Clone, PartialEq)]
pub struct ExportEmail {
    pub to: String,
    pub subject: String,
    pub text_body: String,
    pub attachment_name: String,
    pub attachment_bytes: Vec<u8>,
    pub use_tls: bool,
    pub use_ssl: bool,
}

impl ExportEmail {
    pub fn new(
        to: &str,
        slug: &str,
        text_body: &str,
        csv_bytes: &[u8],
        tls_raw: &str,
        ssl_raw: &str,
    ) -> Self {
        Self {
            to: to.to_owned(),
            subject: EXPORT_EMAIL_SUBJECT.to_owned(),
            text_body: text_body.to_owned(),
            attachment_name: analytics_attachment_name(slug),
            attachment_bytes: csv_bytes.to_vec(),
            use_tls: email_flag_is_one(tls_raw),
            use_ssl: email_flag_is_one(ssl_raw),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_names_match_celery_wire() {
        assert_eq!(
            ISSUE_EXPORT_TASK_NAME,
            "pi_dash.bgtasks.export_task.issue_export_task"
        );
        assert_eq!(
            DELETE_OLD_S3_LINK_TASK_NAME,
            "pi_dash.bgtasks.exporter_expired_task.delete_old_s3_link"
        );
        assert_eq!(
            ANALYTIC_EXPORT_TASK_NAME,
            "pi_dash.bgtasks.analytic_plot_export.analytic_export_task"
        );
        assert_eq!(
            EXPORT_ANALYTICS_CSV_TASK_NAME,
            "pi_dash.bgtasks.analytic_plot_export.export_analytics_to_csv_email"
        );
    }

    #[test]
    fn s3_key_layout_matches_python() {
        assert_eq!(
            export_s3_key("ws-1", "my-slug", "abcdef123456", "2026-09-28"),
            "ws-1/export-my-slug-abcdef-2026-09-28.zip"
        );
        // `token_id[:6]` counts code points; short tokens pass through.
        assert_eq!(
            export_s3_key("ws-1", "s", "ab", "2026-09-28"),
            "ws-1/export-s-ab-2026-09-28.zip"
        );
    }

    #[test]
    fn expiry_cutoff_is_eight_days() {
        // Frozen fixture instant 2026-09-28T06:00:00Z.
        assert_eq!(expiry_cutoff_secs(1_758_946_800) - 1_758_946_800, -691_200);
    }

    #[test]
    fn email_flags_compare_against_one() {
        assert!(email_flag_is_one("1"));
        assert!(!email_flag_is_one("True"));
        assert!(!email_flag_is_one(""));
    }

    #[test]
    fn cell_render_matches_python_str() {
        assert_eq!(CsvCell::Text("5.0".into()).render(), "5.0");
        assert_eq!(CsvCell::Int(5).render(), "5");
        assert_eq!(CsvCell::Bool(true).render(), "True");
        assert_eq!(CsvCell::Empty.render(), "");
    }
}
