//! Pure output shapes for the ops `instance` command group (D-37,
//! PIDASHCONV-809): pod scan/create lines, the test-email template, and
//! the scheduler dry-run verdict rows plus their human/JSON renders.
//!
//! Verdict *computation* (cron conversion, RRULE next-fires) calls
//! `pidash_jobs::tasks_ticker::rrule`, which this crate cannot depend
//! on, so it lives in the binary next to the I/O; everything here is
//! pure formatting over caller-supplied rows.

use std::fmt::Write as _;

// ---------------------------------------------------------------------------
// ensure_project_pods
// ---------------------------------------------------------------------------

/// `f"{project.identifier}_pod_1"` (`ensure_project_pods.py:67`).
pub fn pod_name(identifier: &str) -> String {
    format!("{identifier}_pod_1")
}

/// `project.project_lead or project.default_assignee` (`:72-73`):
/// Django's `or` falls through on `None`.
pub fn choose_creator(
    project_lead_id: Option<uuid::Uuid>,
    default_assignee_id: Option<uuid::Uuid>,
) -> Option<uuid::Uuid> {
    project_lead_id.or(default_assignee_id)
}

/// No-missing branch (`:47-51`).
pub const ALL_HAVE_PODS: &str = "All projects already have a pod. Nothing to do.";

/// Dry-run tail (`:57-59`).
pub const DRY_RUN_TAIL: &str = "--dry-run: no changes written.";

/// `f"Found {n} project(s) without a pod:"` (`:53`).
pub fn found_line(count: usize) -> String {
    format!("Found {count} project(s) without a pod:")
}

/// `f"  - {project.id} ({project.identifier})"` (`:54-55`).
pub fn missing_line(id: &uuid::Uuid, identifier: &str) -> String {
    format!("  - {id} ({identifier})")
}

/// `f"Created {created} default pod(s)."` (`:83`).
pub fn created_line(created: usize) -> String {
    format!("Created {created} default pod(s).")
}

// ---------------------------------------------------------------------------
// test_email
// ---------------------------------------------------------------------------

/// `subject = "Test email from Pi Dash"` (`test_email.py:47`).
pub const TEST_SUBJECT: &str = "Test email from Pi Dash";

/// `render_to_string("emails/test_email.html")` with no context: the
/// template carries no tags, so it renders byte-verbatim. Mirrors
/// `apps/api/templates/emails/test_email.html` (pinned by
/// `template_matches_django_file`).
pub const TEST_TEMPLATE_HTML: &str = concat!(
    "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Strict//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd\"> \n",
    "<html>\n",
    "    <p>This is a test email sent to verify if email configuration is working as expected in your Pi Dash instance.</p>\n",
    "\n",
    "<p>Regards,</br> Team Pi Dash </p>\n",
    "</html>",
);

/// Pre-send line (`:52`).
pub const TRYING_LINE: &str = "Trying to send test email...";

/// Success line (`:64`).
pub const SENT_LINE: &str = "Email successfully sent";

/// Delivery failure line, printed to stdout (`:66-67`).
pub fn delivery_error_line(error: &str) -> String {
    format!("Error: Email could not be delivered due to {error}")
}

// ---------------------------------------------------------------------------
// dry_run_scheduler_migration
// ---------------------------------------------------------------------------

/// Post-migration no-op notice (`dry_run_scheduler_migration.py:100-103`).
pub const CRON_GONE_NOTICE: &str =
    "scheduler_bindings.cron column no longer exists — migration 0140 has already run.";

/// MATCH tolerance in seconds (`:180-184`).
pub const MATCH_TOLERANCE_SECS: f64 = 60.0;

/// One dry-run verdict row: the `entry` dict keys in insertion order
/// (`:131-141`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunEntry {
    pub binding_id: String,
    pub workspace: Option<String>,
    pub enabled: bool,
    pub cron: String,
    pub cron_next_fire: Option<String>,
    pub rrule: Option<String>,
    pub rrule_next_fire: Option<String>,
    pub verdict: DryRunVerdict,
    pub reason: Option<String>,
}

/// `MATCH` / `MISMATCH` / `FAIL` (`:180-190`, default `FAIL` at `:138`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DryRunVerdict {
    Match,
    Mismatch,
    Fail,
}

impl DryRunVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Match => "MATCH",
            Self::Mismatch => "MISMATCH",
            Self::Fail => "FAIL",
        }
    }
}

/// Aggregate counts over the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DryRunCounts {
    pub matches: usize,
    pub mismatches: usize,
    pub fails: usize,
}

impl DryRunCounts {
    pub fn of(entries: &[DryRunEntry]) -> Self {
        let mut counts = Self::default();
        for entry in entries {
            match entry.verdict {
                DryRunVerdict::Match => counts.matches += 1,
                DryRunVerdict::Mismatch => counts.mismatches += 1,
                DryRunVerdict::Fail => counts.fails += 1,
            }
        }
        counts
    }

    pub fn needs_review(self) -> bool {
        self.mismatches > 0 || self.fails > 0
    }
}

/// Python `repr` of a string for the `{workspace!r}` / `{cron!r}`
/// fragments (`:219-221`): same pragmatic scope as the rrule port's
/// helper (ASCII controls, quote, backslash; the rest verbatim).
pub fn py_repr_str(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `repr` of an `Option<String>` (`None` renders `None`).
pub fn py_repr_opt(value: Option<&str>) -> String {
    match value {
        Some(text) => py_repr_str(text),
        None => "None".to_string(),
    }
}

/// Human-readable report (`:212-238`), one line per `self.stdout.write`
/// call (each gains its trailing newline at print time).
pub fn format_human(entries: &[DryRunEntry], counts: DryRunCounts) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Dry-run cron → RRULE for {} binding(s) — MATCH={} MISMATCH={} FAIL={}",
            entries.len(),
            counts.matches,
            counts.mismatches,
            counts.fails
        ),
        "=".repeat(80),
    ];
    for entry in entries {
        lines.push(format!(
            "\n[{}] binding={}  workspace={}  enabled={}",
            entry.verdict.as_str(),
            entry.binding_id,
            py_repr_opt(entry.workspace.as_deref()),
            if entry.enabled { "True" } else { "False" },
        ));
        lines.push(format!("  cron:   {}", py_repr_str(&entry.cron)));
        if let Some(rrule) = entry.rrule.as_deref() {
            lines.push(format!("  rrule:  {rrule}"));
        }
        if let Some(next) = entry.cron_next_fire.as_deref() {
            lines.push(format!("  cron next-fire:  {next}"));
        }
        if let Some(next) = entry.rrule_next_fire.as_deref() {
            lines.push(format!("  rrule next-fire: {next}"));
        }
        if let Some(reason) = entry.reason.as_deref() {
            lines.push(format!("  reason: {reason}"));
        }
    }
    lines.push(format!("\n{}", "=".repeat(80)));
    lines.push(format!(
        "Totals — MATCH={}  MISMATCH={}  FAIL={}",
        counts.matches, counts.mismatches, counts.fails
    ));
    if counts.needs_review() {
        lines.push(
            "\nNon-MATCH bindings need manual review before deploying migration 0140.".to_string(),
        );
    }
    lines
}

/// Escape a string exactly like `json.dump` with `ensure_ascii=True`:
/// `"`, `\`, the C0 short forms, every other char outside `0x20..=0x7e`
/// as lowercase `\uXXXX` (astral chars as surrogate pairs).
pub fn py_json_escape(value: &str, out: &mut String) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let n = c as u32;
                if n > 0xffff {
                    let v = n - 0x1_0000;
                    let _ = write!(
                        out,
                        "\\u{:04x}\\u{:04x}",
                        0xd800 + (v >> 10),
                        0xdc00 + (v & 0x3ff)
                    );
                } else {
                    let _ = write!(out, "\\u{n:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn push_json_str(out: &mut String, value: &str) {
    py_json_escape(value, out);
}

fn push_json_opt(out: &mut String, value: Option<&str>) {
    match value {
        Some(text) => push_json_str(out, text),
        None => out.push_str("null"),
    }
}

/// Machine-readable envelope: `json.dump({"summary": ..., "rows":
/// [...]}, sys.stdout, indent=2, default=str)` plus the trailing
/// newline (`:194-211`). `default=str` never fires (every value is
/// str/bool/None/int).
pub fn format_json(entries: &[DryRunEntry], counts: DryRunCounts) -> String {
    let mut out = String::from("{\n  \"summary\": {\n");
    let _ = writeln!(out, "    \"total\": {},", entries.len());
    let _ = writeln!(out, "    \"match\": {},", counts.matches);
    let _ = writeln!(out, "    \"mismatch\": {},", counts.mismatches);
    let _ = writeln!(out, "    \"fail\": {}", counts.fails);
    if entries.is_empty() {
        out.push_str("  },\n  \"rows\": []\n}\n");
        return out;
    }
    out.push_str("  },\n  \"rows\": [\n");
    for (index, entry) in entries.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str("      \"binding_id\": ");
        push_json_str(&mut out, &entry.binding_id);
        out.push_str(",\n      \"workspace\": ");
        push_json_opt(&mut out, entry.workspace.as_deref());
        let _ = writeln!(
            out,
            ",\n      \"enabled\": {},",
            if entry.enabled { "true" } else { "false" }
        );
        out.push_str("      \"cron\": ");
        push_json_str(&mut out, &entry.cron);
        out.push_str(",\n      \"cron_next_fire\": ");
        push_json_opt(&mut out, entry.cron_next_fire.as_deref());
        out.push_str(",\n      \"rrule\": ");
        push_json_opt(&mut out, entry.rrule.as_deref());
        out.push_str(",\n      \"rrule_next_fire\": ");
        push_json_opt(&mut out, entry.rrule_next_fire.as_deref());
        out.push_str(",\n      \"verdict\": ");
        push_json_str(&mut out, entry.verdict.as_str());
        out.push_str(",\n      \"reason\": ");
        push_json_opt(&mut out, entry.reason.as_deref());
        out.push('\n');
        if index + 1 == entries.len() {
            out.push_str("    }\n");
        } else {
            out.push_str("    },\n");
        }
    }
    out.push_str("  ]\n}\n");
    out
}

/// Python `datetime.isoformat()` for a UTC instant: `+00:00` offset,
/// microseconds only when nonzero.
pub fn iso_utc(moment: &chrono::DateTime<chrono::Utc>) -> String {
    if moment.timestamp_subsec_nanos() == 0 {
        moment.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
    } else {
        moment.format("%Y-%m-%dT%H:%M:%S%.6f+00:00").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_matches_django_file() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../apps/api/templates/emails/test_email.html"
        );
        let file = std::fs::read_to_string(path).expect("django template readable");
        assert_eq!(TEST_TEMPLATE_HTML, file);
    }

    #[test]
    fn human_report_matches_python_layout() {
        let entries = vec![DryRunEntry {
            binding_id: "11111111-2222-3333-4444-555555555555".to_string(),
            workspace: Some("acme".to_string()),
            enabled: true,
            cron: "*/15 * * * *".to_string(),
            cron_next_fire: None,
            rrule: Some("FREQ=MINUTELY;INTERVAL=15".to_string()),
            rrule_next_fire: Some("2026-10-10T03:15:00+00:00".to_string()),
            verdict: DryRunVerdict::Fail,
            reason: Some("could not compute one or both next-fires".to_string()),
        }];
        let text = format_human(&entries, DryRunCounts::of(&entries)).join("\n") + "\n";
        let expected = "Dry-run cron → RRULE for 1 binding(s) — MATCH=0 MISMATCH=0 FAIL=1\n".to_string()
            + &"=".repeat(80)
            + "\n\n[FAIL] binding=11111111-2222-3333-4444-555555555555  workspace='acme'  enabled=True\n"
            + "  cron:   '*/15 * * * *'\n"
            + "  rrule:  FREQ=MINUTELY;INTERVAL=15\n"
            + "  rrule next-fire: 2026-10-10T03:15:00+00:00\n"
            + "  reason: could not compute one or both next-fires\n"
            + "\n"
            + &"=".repeat(80)
            + "\nTotals — MATCH=0  MISMATCH=0  FAIL=1\n"
            + "\nNon-MATCH bindings need manual review before deploying migration 0140.\n";
        assert_eq!(text, expected);
    }

    #[test]
    fn human_report_empty_bindings() {
        let text = format_human(&[], DryRunCounts::of(&[])).join("\n") + "\n";
        let expected = "Dry-run cron → RRULE for 0 binding(s) — MATCH=0 MISMATCH=0 FAIL=0\n"
            .to_string()
            + &"=".repeat(80)
            + "\n\n"
            + &"=".repeat(80)
            + "\nTotals — MATCH=0  MISMATCH=0  FAIL=0\n";
        assert_eq!(text, expected);
    }

    #[test]
    fn json_envelope_matches_dump_indent_2() {
        let entries = vec![DryRunEntry {
            binding_id: "b1".to_string(),
            workspace: None,
            enabled: false,
            cron: "".to_string(),
            cron_next_fire: None,
            rrule: None,
            rrule_next_fire: None,
            verdict: DryRunVerdict::Fail,
            reason: Some(
                "conversion error: cron must have exactly 5 fields, got 0: ''".to_string(),
            ),
        }];
        let text = format_json(&entries, DryRunCounts::of(&entries));
        let expected = "{\n  \"summary\": {\n    \"total\": 1,\n    \"match\": 0,\n    \"mismatch\": 0,\n    \"fail\": 1\n  },\n  \"rows\": [\n    {\n      \"binding_id\": \"b1\",\n      \"workspace\": null,\n      \"enabled\": false,\n      \"cron\": \"\",\n      \"cron_next_fire\": null,\n      \"rrule\": null,\n      \"rrule_next_fire\": null,\n      \"verdict\": \"FAIL\",\n      \"reason\": \"conversion error: cron must have exactly 5 fields, got 0: ''\"\n    }\n  ]\n}\n";
        assert_eq!(text, expected);
        assert!(format_json(&[], DryRunCounts::of(&[])).contains("\"rows\": []"));
    }

    #[test]
    fn json_escapes_ascii_like_python() {
        let mut out = String::new();
        py_json_escape("a\"b\\c\no\u{1}\u{7f}→", &mut out);
        assert_eq!(out, "\"a\\\"b\\\\c\\no\\u0001\\u007f\\u2192\"");
    }

    #[test]
    fn repr_quotes_like_python() {
        assert_eq!(py_repr_str("acme"), "'acme'");
        assert_eq!(py_repr_str("o'clock"), "\"o'clock\"");
        assert_eq!(py_repr_str("a\nb"), "'a\\nb'");
        assert_eq!(py_repr_opt(None), "None");
    }

    #[test]
    fn iso_renders_python_shape() {
        use chrono::TimeZone as _;
        let whole = chrono::Utc
            .with_ymd_and_hms(2026, 10, 10, 3, 15, 0)
            .unwrap();
        assert_eq!(iso_utc(&whole), "2026-10-10T03:15:00+00:00");
        let frac = whole + chrono::Duration::microseconds(123456);
        assert_eq!(iso_utc(&frac), "2026-10-10T03:15:00.123456+00:00");
    }
}
