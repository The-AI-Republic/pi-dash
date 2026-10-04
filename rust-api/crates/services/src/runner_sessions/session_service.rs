#![forbid(unsafe_code)]

//! Runner session service (D-14, stage 5).
//!
//! Port of `apps/api/pi_dash/runner/services/session_service.py:33-508`:
//!
//! * `apply_hello` + `_merge_dev_metadata` + `_agent_capabilities`
//!   (`:58-147`) → [`merge_dev_metadata`], [`agent_capabilities`],
//!   [`plan_hello_update`], [`HELLO_UPDATE_SQL`].
//! * `reap_stale_busy_runs` (`:150-290`) → [`parse_heartbeat_ts`],
//!   [`parse_in_flight_id`], [`effective_cutoff`],
//!   [`reapable_statuses`], the `STALE_*` builders,
//!   [`reap_error_detail`], [`cancel_retry_message`],
//!   [`plan_drain_after_commit`], [`SessionEffect`].
//! * `upsert_runner_live_state` + `parse_optional_uuid` (`:324-397`)
//!   → [`parse_optional_uuid`], [`plan_live_state_upsert`],
//!   [`LIVE_STATE_SELECT_SQL`], [`LIVE_STATE_INSERT_SQL`],
//!   [`live_state_update_sql`]. (`normalize_usage` is
//!   [`super::usage::normalize_usage`], re-exported from D-15 L1.)
//! * `mark_runner_online` / `mark_runner_offline`
//!   (`:454-459`) → [`MARK_RUNNER_ONLINE_SQL`],
//!   [`MARK_RUNNER_OFFLINE_SQL`].
//! * `resolve_runner_project_slug` (`:462-470`)
//!   → [`resolve_runner_slug_sql`], [`resolve_slug`].
//! * `build_session_open_redeliver` (`:400-451`)
//!   → [`parse_skip_id`], [`redeliver_cancel_sql`],
//!   [`redeliver_assign_sql`], [`plan_redeliver`] (frames via
//!   `envelopes::{redeliver_cancel_frame, build_assign_msg}`).
//! * `build_resume_ack` (`:473-508`) → [`resume_ack_lookup_sql`],
//!   [`RESUME_ACK_LAST_SEQ_SQL`], [`plan_resume_ack`].
//!
//! # Layering: plans, not queries
//!
//! This crate has no database handle, so every entry point is pure:
//! SQL text in Django shape (quoted identifiers, `%s` params rendered
//! as Postgres `$N`), update plans as ordered
//! [`SetClause`] lists pinning the
//! `SET` order, branch predicates over caller-fetched facts, and the
//! ordered [`SessionEffect`]s the executing layer fires after commit
//! via the foundation post-commit wrapper (`pidash_db::tx`). The
//! executing layers are the session handlers (PIDASHCONV-557/558).
//! Cross-domain work is carried as effect descriptors or provider
//! planner calls — never executed here:
//!
//! * matcher drains ([`SessionEffect::DrainRunner`] /
//!   [`SessionEffect::DrainPod`]) execute through [`super::drain`]
//!   (`drain_for_runner_by_id` / `drain_pod_by_id`);
//! * the cancel retry ([`SessionEffect::RetryCancelDelivery`])
//!   executes through [`super::pubsub::send_to_runner`];
//! * each reaped run finalizes through D-15
//!   (`crate::runner_runs::finalization`), planned by
//!   [`plan_reap_finalize`] with the exact `FAILED` /
//!   `heartbeat_reaped` call the source makes;
//! * each stopped-cancel run hands off through D-12
//!   ([`SessionEffect::CompleteProjectMoveHandoff`]).
//!
//! Reused, never redefined: [`super::guards::BUSY_STATUSES`], the
//! assign/cancel/resume frames
//! (`pidash_types::runner_sessions::envelopes`), `AgentRunStatus`
//! (`pidash_types::runner_runs`), the `agent_run` / `pod` column
//! lists (`pidash_db::runner_runs`), and the `SetClause` / `SetValue`
//! plan types (`crate::runner_runs`, the [`super::drain`] precedent).
//!
//! Fixture: `rust-api/fixtures/runner_sessions/fx-rses-06-session-service.json`
//! (FX-RSES-06). Every section is replayed by the `#[cfg(test)]`
//! suite below; every SQL builder is pinned byte-for-byte against
//! the captured Django 4.2.30 statements (`%s` → `$N`
//! positionally, interpolated literals → params).
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * The grace windows are racy by design: a run assigned within
//!   [`ASSIGN_DELIVERY_GRACE_SECS`] of the poll, or a heartbeat
//!   older than [`OFFLINE_GRACE_SECS`], is spared by timestamp
//!   arithmetic, not by a lock (source comments `:35-55`).
//! * The heartbeat timestamp is clamped into
//!   `[now - OFFLINE_GRACE_SECS, now]` — a future `ts` silently
//!   becomes `now` (`:178-179`).
//! * `ts` parses via `datetime.fromisoformat(ts.replace("Z",
//!   "+00:00"))`: *every* `Z` is replaced, not just a suffix, and a
//!   naive timestamp parses fine but then raises `TypeError` in the
//!   clamp comparison. [`parse_heartbeat_ts`] models the raise as
//!   [`HeartbeatError::NaiveTimestamp`] (the executing layer maps it
//!   to the same 500); unparseable values fall back to `now`.
//! * The stopped-cancel barrier queries carry the redundant
//!   `status IN (reapable…) AND status = 'cancel_requested'`
//!   conjunction verbatim (`:246-247`).
//! * `save(update_fields=…)` orders `SET` by model field-definition
//!   order, *not* by the `update_fields` list: hello writes
//!   `capabilities` first (`:130-141`), and the live-state
//!   `sorted(set(update_fields))` (`:397`) is cosmetic —
//!   `observed_run_id` still leads. `.update()` instead preserves
//!   call order (barrier: `status, ended_at, queue_position`).
//! * `pod_ids` is a `set`, so multi-pod drain order is arbitrary in
//!   Python; Rust drains in first-seen order (reaped runs, then
//!   stopped cancels), which is deterministic for the same rows.
//! * An unparseable `in_flight_run` is silently ignored (no
//!   exclusion, no cancel retry), and `resume_ack` echoes the
//!   passed `run_id` verbatim, never normalized.
//! * `body.get("os", "") or runner.os` keeps the current value for
//!   *any* falsy body value (`0`, `False`, `[]`, …), not just a
//!   missing key — and a truthy non-string is stored via `str()`.

use chrono::{DateTime, Duration, Utc};
use pidash_types::runner_runs::AgentRunStatus;
use pidash_types::runner_sessions::envelopes;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::runner_runs::finalization::{plan_finalize_values, FinalizeValues};
use crate::runner_runs::{qualified_columns, SetClause, SetValue};
use crate::runner_sessions::guards::BUSY_STATUSES;

// ---------------------------------------------------------------------------
// Grace windows (`session_service.py:33-55`)
// ---------------------------------------------------------------------------

/// Heartbeat staleness floor (`:33`): a reported `ts` older than
/// `now - OFFLINE_GRACE_SECS` is clamped up to that bound.
pub const OFFLINE_GRACE_SECS: i64 = 60;

/// Fresh-assignment shield (`:55`): a run is only stale when
/// `assigned_at` is older than *both* the reported heartbeat and
/// `now - ASSIGN_DELIVERY_GRACE_SECS`.
pub const ASSIGN_DELIVERY_GRACE_SECS: i64 = 60;

// ---------------------------------------------------------------------------
// JSON frame helpers (per-module mirrors; the `runner_runs` twins are
// module-private and every services dir keeps its own copies)
// ---------------------------------------------------------------------------

/// Python truthiness for JSON frame values (`None`/`False`/`0`/`""`/
/// `[]`/`{}` are falsy; everything else is truthy).
fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                n.as_f64().map(|f| f != 0.0).unwrap_or(false)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Python `str.strip()`: Rust's `is_whitespace` set plus
/// U+001C..=U+001F (same formula as the sibling twins).
fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// First `max` Unicode code points of `text` (`text[:max]`).
/// Byte-slicing would panic on a UTF-8 boundary; `chars().take()`
/// cannot. A string that fits needs no walk.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

/// Python `str(value)` for JSON frame values: `None`/`True`/`False`
/// spellings, integers verbatim, floats in CPython repr form,
/// strings as-is, containers in single-quote repr form.
fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => py_num_str(n),
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_repr_str(k), py_repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// CPython `repr` of a number: integers verbatim, floats via
/// [`py_float_str`].
fn py_num_str(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    n.as_f64()
        .map(py_float_str)
        .unwrap_or_else(|| n.to_string())
}

/// CPython `repr(float)`: Rust's `Debug` shortest round-trip with
/// the exponent normalized to Python's `e±XX` form (`1e16` →
/// `1e+16`, `1e-5` → `1e-05`). Same formula as the sibling twin;
/// the two engines switch to exponent notation at different
/// magnitudes in rare cases — frame floats are already absurd that
/// far down.
fn py_float_str(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_owned();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    let rust = format!("{f:?}");
    let Some(pos) = rust.find('e') else {
        return rust;
    };
    let (mantissa, exp) = rust.split_at(pos);
    let exp = &exp[1..];
    let (sign, digits) = match exp.strip_prefix('-') {
        Some(digits) => ("-", digits),
        None => ("+", exp.strip_prefix('+').unwrap_or(exp)),
    };
    format!("{mantissa}e{sign}{digits:0>2}")
}

/// Python `repr(value)` for a value nested in a container: identical
/// to [`py_str`] except strings, which gain quotes.
fn py_repr(value: &Value) -> String {
    match value {
        Value::String(s) => py_repr_str(s),
        other => py_str(other),
    }
}

/// Python `repr(str)`: single quotes unless the string contains
/// `'` but not `"`, backslash escapes for quotes/backslash/
/// whitespace, `\xNN` / `\uNNNN` / `\U00NNNNNN` for the rest of
/// the non-printables. Same formula as the sibling twin.
fn py_repr_str(s: &str) -> String {
    let use_double = s.contains('\'') && !s.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        if c == quote {
            out.push('\\');
            out.push(c);
        } else {
            match c {
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if py_printable(c) => out.push(c),
                c if (c as u32) < 0x100 => {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                }
                c if (c as u32) < 0x1_0000 => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => {
                    out.push_str(&format!("\\U{:08x}", c as u32));
                }
            }
        }
    }
    out.push(quote);
    out
}

/// Approximation of `str.isprintable` for [`py_repr_str`]:
/// controls, DEL and U+2028/2029 are non-printable; everything
/// else (including the exotic `Cf` format characters) prints raw.
fn py_printable(c: char) -> bool {
    !(c.is_control() || c == '\u{7f}' || c == '\u{2028}' || c == '\u{2029}')
}

/// `UUID(str)` exactly as CPython's `uuid.UUID(hex)` parses it
/// (verified against CPython 3.12 source + probes): global
/// case-sensitive `urn:` / `uuid:` removal in that order, `{}`/`}`
/// stripped from both ends, *all* hyphens removed wherever they
/// sit, exactly 32 chars left, then `int(hex, 16)` — which strips
/// surrounding whitespace, takes one sign (`-` only survives for
/// zero), and allows single underscores between digits.
///
/// `Uuid::parse_str` accepts a strict subset (exact hyphen
/// positions, paired braces, lowercase `urn:uuid:` only), so the
/// callers below try it first and fall back here; anything this
/// rejects, CPython rejects too.
fn parse_uuid_cpython(text: &str) -> Option<Uuid> {
    let no_urn = text.replace("urn:", "").replace("uuid:", "");
    let stripped = no_urn.trim_matches(|c| c == '{' || c == '}');
    let compact: String = stripped.chars().filter(|c| *c != '-').collect();
    if compact.len() != 32 {
        return None;
    }
    // `int(hex, 16)`: surrounding whitespace (Unicode plus
    // U+001C..=U+001F, the same delta as [`py_strip`]), one sign,
    // hex digits with single underscores between digits only.
    let trimmed =
        compact.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c));
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    let negative = trimmed.starts_with('-');
    if digits.is_empty() {
        return None;
    }
    let mut value: u128 = 0;
    let mut prev_underscore = true; // leading '_' rejected
    let mut any_digit = false;
    for c in digits.chars() {
        if c == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        let digit = c.to_digit(16)?;
        any_digit = true;
        prev_underscore = false;
        value = value * 16 + digit as u128;
    }
    if !any_digit || prev_underscore {
        return None;
    }
    if negative && value != 0 {
        return None;
    }
    Some(Uuid::from_u128(value))
}

/// `str(UUID(str))` for the `Option`-returning call sites: the
/// fast strict parse first, the exact CPython fallback second.
fn parse_uuid_arg(text: &str) -> Option<Uuid> {
    Uuid::parse_str(text)
        .ok()
        .or_else(|| parse_uuid_cpython(text))
}

/// Same, keeping the strict parse's error for the
/// `Result`-returning call site.
fn parse_uuid_result(text: &str) -> Result<Option<Uuid>, uuid::Error> {
    match Uuid::parse_str(text) {
        Ok(id) => Ok(Some(id)),
        Err(error) => match parse_uuid_cpython(text) {
            Some(id) => Ok(Some(id)),
            None => Err(error),
        },
    }
}

// ---------------------------------------------------------------------------
// Hello (`session_service.py:58-147`)
// ---------------------------------------------------------------------------

/// Merge whitelisted session-open metadata (`_merge_dev_metadata`,
/// `:58-89`): `engine_version` sets/clears `codex_version` (string
/// only, `[:64]`); a *missing* `working_dir` keeps the current
/// value while an explicit empty string clears it (`[:1024]`).
/// Non-string `working_dir` values are ignored; a non-dict current
/// value starts from `{}`.
pub fn merge_dev_metadata(current: &Value, body: &Map<String, Value>) -> Map<String, Value> {
    let mut metadata = current.as_object().cloned().unwrap_or_default();
    if let Some(Value::String(version)) = body.get("engine_version") {
        if version.is_empty() {
            metadata.remove("codex_version");
        } else {
            metadata.insert(
                "codex_version".to_string(),
                Value::String(truncate_chars(version, 64)),
            );
        }
    }
    let Some(working_dir) = body.get("working_dir") else {
        return metadata;
    };
    let Value::String(working_dir) = working_dir else {
        return metadata;
    };
    if working_dir.is_empty() {
        metadata.remove("working_dir");
    } else {
        metadata.insert(
            "working_dir".to_string(),
            Value::String(truncate_chars(working_dir, 1024)),
        );
    }
    metadata
}

/// `agent_kind` charset (`_AGENT_KIND_RE`, `:92`):
/// `^[a-z][a-z0-9_]{0,31}$` — a lowercase lead plus up to 31 tail
/// chars (32 total). Checked by hand; the shape is exactly the
/// regex (ASCII-only, anchored both ends), including `$` matching
/// before one trailing newline (`x\n` validates; the raw value,
/// newline included, is what persists).
fn is_valid_agent_kind(raw: &str) -> bool {
    let body = raw.strip_suffix('\n').unwrap_or(raw);
    let mut chars = body.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    let mut len = 1;
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return false;
        }
        len += 1;
        if len > 32 {
            return false;
        }
    }
    true
}

/// Derive the `capabilities` list from the Hello's `agent_kind`
/// (`_agent_capabilities`, `:95-114`): `(capabilities, changed)`. A
/// missing/invalid kind returns the current list verbatim (a copy;
/// `[]` for a non-list current) with `changed == false`, so the
/// caller never persists it; a valid kind returns `[agent:<kind>]`
/// with `changed` set only when it differs from current.
pub fn agent_capabilities(body: &Map<String, Value>, current: &Value) -> (Value, bool) {
    let valid = match body.get("agent_kind") {
        Some(Value::String(raw)) if is_valid_agent_kind(raw) => Some(raw),
        _ => None,
    };
    let Some(raw) = valid else {
        let keep = match current {
            Value::Array(_) => current.clone(),
            _ => Value::Array(Vec::new()),
        };
        return (keep, false);
    };
    let desired = Value::Array(vec![Value::String(format!("agent:{raw}"))]);
    let same = current == &desired;
    (desired, !same)
}

/// Caller-owned runner facts for [`plan_hello_update`]: the row's
/// current `os` / `arch` / `runner_version` / `dev_metadata` /
/// `capabilities` (empty-string defaults for the `CharField`s,
/// `[]` for capabilities).
pub struct HelloFacts {
    pub os: String,
    pub arch: String,
    pub runner_version: String,
    pub dev_metadata: Value,
    pub capabilities: Value,
}

/// One `apply_hello` row update (`:125-141`): the ordered `SET`
/// clauses plus whether `capabilities` changed. `SET` order is the
/// model field-definition order (`capabilities` first when
/// present), *not* the `update_fields` list order — Django filters
/// concrete fields by the update set. `last_heartbeat_at` is
/// [`SetValue::Now`] (`timezone.now()` at execution).
pub struct HelloPlan {
    pub set_clauses: Vec<SetClause>,
    pub capabilities_changed: bool,
}

/// Plan the hello update: `body.get(key, "") or current` per scalar
/// (any falsy body value keeps the row value; a truthy non-string
/// stores via `str()`), the metadata merge, the capability
/// derivation, and the heartbeat bump.
pub fn plan_hello_update(facts: &HelloFacts, body: &Map<String, Value>) -> HelloPlan {
    let pick = |key: &str, current: &str| -> String {
        match body.get(key) {
            Some(value) if py_truthy(value) => match value {
                Value::String(s) => s.clone(),
                other => py_str(other),
            },
            _ => current.to_owned(),
        }
    };
    let (capabilities, capabilities_changed) = agent_capabilities(body, &facts.capabilities);
    let mut set_clauses = Vec::with_capacity(6);
    if capabilities_changed {
        set_clauses.push(SetClause {
            column: "capabilities",
            value: SetValue::Json(capabilities),
        });
    }
    set_clauses.push(SetClause {
        column: "os",
        value: SetValue::Text(pick("os", &facts.os)),
    });
    set_clauses.push(SetClause {
        column: "arch",
        value: SetValue::Text(pick("arch", &facts.arch)),
    });
    set_clauses.push(SetClause {
        column: "runner_version",
        value: SetValue::Text(pick("version", &facts.runner_version)),
    });
    set_clauses.push(SetClause {
        column: "dev_metadata",
        value: SetValue::Json(Value::Object(merge_dev_metadata(&facts.dev_metadata, body))),
    });
    set_clauses.push(SetClause {
        column: "last_heartbeat_at",
        value: SetValue::Now,
    });
    HelloPlan {
        set_clauses,
        capabilities_changed,
    }
}

/// Hello `UPDATE` with a capabilities change (`:141`,
/// FX-RSES-06 `apply_hello.sql[0]`): `$1` capabilities (JSON),
/// `$2` os, `$3` arch, `$4` runner_version, `$5` dev_metadata
/// (JSON), `$6` heartbeat, `$7` runner id.
pub const HELLO_UPDATE_SQL: &str = "UPDATE \"runner\" SET \"capabilities\" = $1, \"os\" = $2, \"arch\" = $3, \"runner_version\" = $4, \"dev_metadata\" = $5, \"last_heartbeat_at\" = $6 WHERE \"runner\".\"id\" = $7";

/// Hello `UPDATE` without one (`apply_hello_empty_body.sql[0]`):
/// `$1` os, `$2` arch, `$3` runner_version, `$4` dev_metadata
/// (JSON), `$5` heartbeat, `$6` runner id.
pub const HELLO_UPDATE_NO_CAPABILITIES_SQL: &str = "UPDATE \"runner\" SET \"os\" = $1, \"arch\" = $2, \"runner_version\" = $3, \"dev_metadata\" = $4, \"last_heartbeat_at\" = $5 WHERE \"runner\".\"id\" = $6";

// ---------------------------------------------------------------------------
// Heartbeat reaper (`reap_stale_busy_runs`, `:150-290`)
// ---------------------------------------------------------------------------

/// Why [`parse_heartbeat_ts`] failed. Only the naive-timestamp arm
/// fails: Python raises `TypeError` there (uncaught → 500), so the
/// executing layer maps this to the same 500. Every other bad input
/// falls back to `now`, exactly as the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatError {
    /// The `ts` parsed but carries no offset (`:175` ok, `:178`
    /// raises `TypeError` comparing naive with aware).
    NaiveTimestamp,
}

/// Parse and clamp the reported heartbeat (`:172-179`):
/// `datetime.fromisoformat(ts.replace("Z", "+00:00"))` for string
/// values (`now` for anything else, `now` on `ValueError`), then
/// clamped into `[now - OFFLINE_GRACE_SECS, now]`.
///
/// Faithful quirks: *every* `Z` is replaced before parsing (not
/// just a suffix), and a successfully parsed *naive* timestamp
/// returns [`HeartbeatError::NaiveTimestamp`] — Python raises
/// `TypeError` in the `min()` comparison instead of falling back.
/// The grammar ports `fromisoformat`'s calendar shapes exactly
/// (verified against CPython 3.12 probes): extended and basic
/// dates/times freely mixed (never mixed *within* the time), any
/// single-char separator, `HH[:MM[:SS]]` / `HH[MM[SS]]` with a
/// `[.,]` fraction (digits required at end of string, empty only
/// before an offset) truncated to microseconds, years 1-9999, and
/// `±HH[:MM[:SS]]` / `±HHMM[SS]` / `±HH` offsets strictly under
/// 24h. Residual fallbacks to `now`: week dates (naive in Python)
/// and separator-less basic fractions (`T12305900`, with CPython's
/// length-7 hole — deliberately not replicated). No JSON clock
/// emits any of these; ordinal dates fail in CPython too.
pub fn parse_heartbeat_ts(
    ts: Option<&Value>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, HeartbeatError> {
    let Some(Value::String(raw)) = ts else {
        return Ok(now);
    };
    let normalized = raw.replace('Z', "+00:00");
    match parse_iso_timestamp(&normalized) {
        IsoTimestamp::Aware(heartbeat_ts) => {
            let clamped = heartbeat_ts.min(now);
            Ok(clamped.max(now - Duration::seconds(OFFLINE_GRACE_SECS)))
        }
        IsoTimestamp::Naive => Err(HeartbeatError::NaiveTimestamp),
        IsoTimestamp::Garbage => Ok(now),
    }
}

/// A `fromisoformat` outcome: aware (converted to UTC), naive
/// (parses, but Python raises comparing it), or garbage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IsoTimestamp {
    Aware(DateTime<Utc>),
    Naive,
    Garbage,
}

/// Strict `datetime.fromisoformat` for calendar shapes (see
/// [`parse_heartbeat_ts`]). Fractions truncate to microseconds, as
/// CPython does; offsets convert to UTC.
fn parse_iso_timestamp(text: &str) -> IsoTimestamp {
    use chrono::{NaiveDate, NaiveTime};
    let chars: Vec<char> = text.chars().collect();
    let digits_at = |pos: usize, len: usize| -> Option<u32> {
        let slice = chars.get(pos..pos + len)?;
        if !slice.iter().all(|c| c.is_ascii_digit()) {
            return None;
        }
        slice.iter().collect::<String>().parse().ok()
    };
    // Date: `\d{8}` basic or `\d{4}-\d{2}-\d{2}` extended.
    let (year, month, day, mut pos) = if let (Some(y), Some(m), Some(d)) =
        (digits_at(0, 4), digits_at(4, 2), digits_at(6, 2))
    {
        (y, m, d, 8)
    } else if let (Some(y), Some(m), Some(d)) = (digits_at(0, 4), digits_at(5, 2), digits_at(8, 2))
    {
        if chars.get(4) != Some(&'-') || chars.get(7) != Some(&'-') {
            return IsoTimestamp::Garbage;
        }
        (y, m, d, 10)
    } else {
        return IsoTimestamp::Garbage;
    };
    // `datetime` years start at 1; chrono would accept year 0.
    if year == 0 {
        return IsoTimestamp::Garbage;
    }
    if pos == chars.len() {
        // Bare date (extended or basic): parses, naive.
        return match NaiveDate::from_ymd_opt(year as i32, month, day) {
            Some(_) => IsoTimestamp::Naive,
            None => IsoTimestamp::Garbage,
        };
    }
    // Separator: any single char (positional, even a digit).
    pos += 1;
    // Time: `HH[:MM[:SS]]` extended or `HH[MM[SS]]` basic. The two
    // never mix within one time (`T12:3059` and `T1230:59` both
    // fail); a fraction or offset may follow HH, MM or SS alike.
    let Some(hour) = digits_at(pos, 2) else {
        return IsoTimestamp::Garbage;
    };
    pos += 2;
    let mut minute: u32 = 0;
    let mut second: u32 = 0;
    if chars.get(pos) == Some(&':') {
        // Extended: each `:` commits to the next component.
        pos += 1;
        let Some(m) = digits_at(pos, 2) else {
            return IsoTimestamp::Garbage;
        };
        minute = m;
        pos += 2;
        if chars.get(pos) == Some(&':') {
            pos += 1;
            let Some(s) = digits_at(pos, 2) else {
                return IsoTimestamp::Garbage;
            };
            second = s;
            pos += 2;
        }
    } else if let Some(m) = digits_at(pos, 2) {
        // Basic: digit pairs extend to MM, then SS.
        minute = m;
        pos += 2;
        if let Some(s) = digits_at(pos, 2) {
            second = s;
            pos += 2;
        }
    }
    // Fraction: `[.,]` + digits after HH, MM or SS alike. Empty
    // digits parse only before an offset (`T09.+00:00` is aware; a
    // trailing separator at end of string is garbage).
    let mut micros: u32 = 0;
    if chars.get(pos) == Some(&'.') || chars.get(pos) == Some(&',') {
        pos += 1;
        let mut count = 0;
        while chars.get(pos).is_some_and(|c| c.is_ascii_digit()) {
            if count < 6 {
                micros = micros * 10 + chars[pos].to_digit(10).unwrap_or(0);
                count += 1;
            }
            pos += 1;
        }
        if count == 0 && !matches!(chars.get(pos), Some('+') | Some('-')) {
            return IsoTimestamp::Garbage;
        }
        micros *= 10u32.pow(6 - count);
    }
    let date = match NaiveDate::from_ymd_opt(year as i32, month, day) {
        Some(date) => date,
        None => return IsoTimestamp::Garbage,
    };
    let time = match NaiveTime::from_hms_micro_opt(hour, minute, second, micros) {
        Some(time) => time,
        None => return IsoTimestamp::Garbage,
    };
    if pos == chars.len() {
        return IsoTimestamp::Naive;
    }
    // Offset: `[+-]` + `HH[:MM[:SS]]` / `HHMM[SS]` / `HH`,
    // strictly under 24h; MM/SS unrange-checked (timedelta math).
    let sign_secs: i64 = match chars.get(pos) {
        Some('+') => 1,
        Some('-') => -1,
        _ => return IsoTimestamp::Garbage,
    };
    pos += 1;
    let rest: String = chars[pos..].iter().collect();
    let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
    let fields = match rest.len() {
        2 if digits.len() == 2 => Some((&digits[0..2], "00", "00")),
        4 if digits.len() == 4 && !rest.contains(':') => Some((&digits[0..2], &digits[2..4], "00")),
        5 if rest.chars().nth(2) == Some(':') && digits.len() == 4 => {
            Some((&digits[0..2], &digits[2..4], "00"))
        }
        6 if digits.len() == 6 && !rest.contains(':') => {
            Some((&digits[0..2], &digits[2..4], &digits[4..6]))
        }
        8 if rest.chars().nth(2) == Some(':')
            && rest.chars().nth(5) == Some(':')
            && digits.len() == 6 =>
        {
            Some((&digits[0..2], &digits[2..4], &digits[4..6]))
        }
        _ => None,
    };
    let Some((hh, mm, ss)) = fields else {
        return IsoTimestamp::Garbage;
    };
    let offset_secs: i64 = hh.parse::<i64>().unwrap_or(99) * 3600
        + mm.parse::<i64>().unwrap_or(99) * 60
        + ss.parse::<i64>().unwrap_or(99);
    if offset_secs >= 86_400 {
        return IsoTimestamp::Garbage;
    }
    let naive = date.and_time(time);
    let base = DateTime::from_naive_utc_and_offset(naive, Utc);
    // Checked: Python attaches the offset without arithmetic (the
    // clamp only compares), so out-of-range instants must not
    // panic. An underflow is off-scale early, an overflow
    // off-scale late — but the outcome enum carries no instant,
    // so settle exactly what the caller needs: underflow always
    // clamps to the floor, overflow always to `now`. Both are
    // decided here via explicit bounds, never by panicking.
    match base.checked_sub_signed(Duration::seconds(sign_secs * offset_secs)) {
        Some(utc) => IsoTimestamp::Aware(utc),
        None if sign_secs * offset_secs >= 0 => IsoTimestamp::Aware(DateTime::<Utc>::MIN_UTC),
        None => IsoTimestamp::Aware(DateTime::<Utc>::MAX_UTC),
    }
}

/// The reaper assignment cutoff (`:194-195`): `min(heartbeat_ts,
/// now - ASSIGN_DELIVERY_GRACE_SECS)` — a run is stale only when
/// `assigned_at` predates *both* bounds. Note both grace constants
/// are 60s, so with the clamped heartbeat this is always `now -
/// 60s`: the `ts` value never moves the cutoff (only a naive `ts`
/// 500s); the `min()` is kept exactly as the source computes it.
pub fn effective_cutoff(now: DateTime<Utc>, heartbeat_ts: DateTime<Utc>) -> DateTime<Utc> {
    heartbeat_ts.min(now - Duration::seconds(ASSIGN_DELIVERY_GRACE_SECS))
}

/// Normalize the reported in-flight run id (`:181-187`):
/// `str(UUID(str(value)))` for truthy values (lowercase hyphenated),
/// `None` for falsy or unparseable values (silently ignored — no
/// exclusion, no cancel retry).
pub fn parse_in_flight_id(in_flight: Option<&Value>) -> Option<String> {
    let value = in_flight?;
    if !py_truthy(value) {
        return None;
    }
    let text = match value {
        Value::String(s) => s.clone(),
        other => py_str(other),
    };
    parse_uuid_arg(&text).map(|id| id.to_string())
}

/// The statuses the reaper may reap (`:197-204`): [`BUSY_STATUSES`]
/// in source-tuple order, minus `ASSIGNED` / `WAITING_FOR_WORKTREE`
/// / `CANCEL_REQUESTED` on the session-open path
/// (`exclude_redeliverable`), so `build_session_open_redeliver` can
/// push those runs back instead of failing them.
pub fn reapable_statuses(exclude_redeliverable: bool) -> Vec<AgentRunStatus> {
    BUSY_STATUSES
        .iter()
        .copied()
        .filter(|status| {
            !exclude_redeliverable
                || !matches!(
                    status,
                    AgentRunStatus::Assigned
                        | AgentRunStatus::WaitingForWorktree
                        | AgentRunStatus::CancelRequested
                )
        })
        .collect()
}

/// Render a status `IN` list in slice order:
/// `('running', 'awaiting_approval', …)`.
fn status_in_list(statuses: &[AgentRunStatus]) -> String {
    let inner: Vec<&str> = statuses.iter().map(AgentRunStatus::value).collect();
    format!("('{}')", inner.join("', '"))
}

/// Shared stale-run `WHERE` core (`:206-212`): `assigned_at < $1`
/// (the [`effective_cutoff`]) `AND runner_id = $2 AND status IN
/// (…)` plus the `NOT (id = $3)` in-flight exclusion when the poll
/// reported one. Django sorts the `filter()` kwargs (`assigned_at`,
/// `runner_id`, `status`) and appends the `exclude()` term after.
fn stale_where(statuses: &[AgentRunStatus], exclude_in_flight: bool) -> String {
    let mut where_ = format!(
        "(\"agent_run\".\"assigned_at\" < $1 AND \"agent_run\".\"runner_id\" = $2 AND \"agent_run\".\"status\" IN {}",
        status_in_list(statuses)
    );
    if exclude_in_flight {
        where_.push_str(" AND NOT (\"agent_run\".\"id\" = $3)");
    }
    where_
}

/// Stopped-cancel barrier id scan (`:246`,
/// `reap_poll_path.sql[1]`): the stale core plus the redundant
/// `AND status = 'cancel_requested'` (kept verbatim), `ORDER BY
/// created_at DESC` (the `AgentRun` default ordering).
pub fn stale_cancel_ids_sql(statuses: &[AgentRunStatus], exclude_in_flight: bool) -> String {
    format!(
        "SELECT \"agent_run\".\"id\" FROM \"agent_run\" WHERE {} AND \"agent_run\".\"status\" = 'cancel_requested') ORDER BY \"agent_run\".\"created_at\" DESC",
        stale_where(statuses, exclude_in_flight)
    )
}

/// Stopped-cancel barrier pod-id scan (`:247`,
/// `reap_poll_path.sql[2]`): same `WHERE`, `pod_id` projection.
pub fn stale_cancel_pod_ids_sql(statuses: &[AgentRunStatus], exclude_in_flight: bool) -> String {
    format!(
        "SELECT \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE {} AND \"agent_run\".\"status\" = 'cancel_requested') ORDER BY \"agent_run\".\"created_at\" DESC",
        stale_where(statuses, exclude_in_flight)
    )
}

/// Reap candidate scan (`:256`): `SELECT id, pod_id` over the stale
/// core, plus `AND NOT (id IN (…))` once the barrier has moved
/// stopped cancels out (`:254`). `excluded_ids` is the barrier id
/// count (0 renders no term); the `IN` params follow the in-flight
/// param when one is bound (`$4…`) and the runner id otherwise
/// (`$3…`).
pub fn stale_pairs_sql(
    statuses: &[AgentRunStatus],
    exclude_in_flight: bool,
    excluded_ids: usize,
) -> String {
    let mut where_ = stale_where(statuses, exclude_in_flight);
    if excluded_ids > 0 {
        let first = if exclude_in_flight { 4 } else { 3 };
        let params: Vec<String> = (0..excluded_ids)
            .map(|i| format!("${}", first + i))
            .collect();
        where_.push_str(&format!(
            " AND NOT (\"agent_run\".\"id\" IN ({}))",
            params.join(", ")
        ));
    }
    format!(
        "SELECT \"agent_run\".\"id\", \"agent_run\".\"pod_id\" FROM \"agent_run\" WHERE {where_}) ORDER BY \"agent_run\".\"created_at\" DESC"
    )
}

/// In-flight cancel-requested existence probe (`:213-217`,
/// `reap_poll_path.sql[0]`): `$1` in-flight id, `$2` runner id.
pub const CANCEL_REQUESTED_EXISTS_SQL: &str = "SELECT 1 AS \"a\" FROM \"agent_run\" WHERE (\"agent_run\".\"id\" = $1 AND \"agent_run\".\"runner_id\" = $2 AND \"agent_run\".\"status\" = 'cancel_requested') LIMIT 1";

/// Whether the poll schedules the cancel redelivery
/// (`:211-241`): only when an in-flight id parsed, the run is
/// still `CANCEL_REQUESTED`, and the path is the poll path (the
/// session-open path passes `exclude_redeliverable`, which skips
/// the whole `if in_flight_id:` arm's retry — the `exclude` still
/// applies to the stale scan).
pub fn should_schedule_cancel_retry(
    in_flight_present: bool,
    cancel_requested_exists: bool,
    exclude_redeliverable: bool,
) -> bool {
    in_flight_present && cancel_requested_exists && !exclude_redeliverable
}

/// The cancel frame the retry delivers (`:224-233`): `v, type,
/// run_id, reason` in order. Note the reason —
/// `cancellation_pending` — differs from the reconnect reason the
/// redeliver path uses.
pub fn cancel_retry_message(run_id: &str) -> Map<String, Value> {
    let mut msg = Map::with_capacity(4);
    msg.insert("v".to_string(), Value::from(1));
    msg.insert("type".to_string(), Value::String("cancel".to_string()));
    msg.insert("run_id".to_string(), Value::String(run_id.to_string()));
    msg.insert(
        "reason".to_string(),
        Value::String("cancellation_pending".to_string()),
    );
    msg
}

/// `logger.exception` line the retry emits when the redelivery
/// fails (`:234-239`): `session_service: failed to redeliver
/// cancellation for run <id>: <error>`. The executing layer logs
/// one such line per failed retry (this crate has no `tracing`
/// dependency), mirroring the [`super::pubsub`] warnings style.
pub fn cancel_retry_error_line(run_id: &str, error: &str) -> String {
    format!("session_service: failed to redeliver cancellation for run {run_id}: {error}")
}

/// Stopped-cancel barrier write (`:249-253`,
/// `reap_cancel_barrier.sql[2]`): `.update()` preserves call order
/// (`status, ended_at, queue_position`). `$1` is `now` (`ended_at`),
/// `$2…` the stopped-cancel ids. Call only with a non-empty id
/// list — the source guards the write with `if stopped_cancel_ids:`.
pub fn cancel_barrier_update_sql(id_count: usize) -> String {
    debug_assert_ne!(id_count, 0, "barrier write needs stopped-cancel ids");
    let params: Vec<String> = (0..id_count).map(|i| format!("${}", i + 2)).collect();
    format!(
        "UPDATE \"agent_run\" SET \"status\" = 'cancelled', \"ended_at\" = $1, \"queue_position\" = NULL WHERE \"agent_run\".\"id\" IN ({})",
        params.join(", ")
    )
}

/// The `error` detail every reaped run finalizes with (`:262-265`).
pub fn reap_error_detail(in_flight_id: Option<&str>) -> String {
    format!(
        "reaped by heartbeat: runner reported in_flight_run={} but cloud had this run marked busy",
        in_flight_id.unwrap_or("(none)")
    )
}

/// `error_code` every reaped run finalizes with (`:270`).
pub const REAP_ERROR_CODE: &str = "heartbeat_reaped";

/// Plan one reaped run's `finalize_agent_run` call (`:266-272`):
/// `FAILED` with `updates = {error, error_code}` and
/// `expected_runner_id`. D-15 owns the lock, the conditional
/// `UPDATE`, the cloud-only terminal event and the publish effects
/// — the executing layer runs those through
/// `crate::runner_runs::finalization` (lock
/// [`lock_run_for_finalize_sql`](crate::runner_runs::finalization::lock_run_for_finalize_sql)
/// with runner, no status; then
/// [`finalize_update_sql`](crate::runner_runs::finalization::finalize_update_sql)).
/// Each run finalizes in its own transaction, exactly as the
/// source's per-call `transaction.atomic()`.
pub fn plan_reap_finalize(detail: &str) -> FinalizeValues {
    plan_finalize_values(
        AgentRunStatus::Failed,
        &[
            ("error", SetValue::Text(detail.to_owned())),
            ("error_code", SetValue::Text(REAP_ERROR_CODE.to_owned())),
        ],
    )
    .expect("FAILED is terminal")
}

/// Whether the reaper schedules `_drain_after_commit` (`:257-258`,
/// `:290`): only when at least one run was reaped or barriered.
pub fn should_schedule_drain(reaped_count: usize, stopped_cancel_count: usize) -> bool {
    reaped_count > 0 || stopped_cancel_count > 0
}

/// One deferred session-service side effect. Each variant names the
/// exact Python call it replaces and the boundary it fires on; the
/// executing layer runs them through `pidash_db::tx` post-commit
/// actions with the same per-effect isolation as the source.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEffect {
    /// Cancel redelivery retry (`_retry_cancel`, `:223-241`):
    /// `send_to_runner(runner_id, cancel_retry_message(run_id))`,
    /// isolated — failures log [`cancel_retry_error_line`].
    RetryCancelDelivery {
        runner_id: Uuid,
        message: Map<String, Value>,
    },
    /// `drain_for_runner_by_id(runner_id)` (D-14 [`super::drain`]).
    DrainRunner { runner_id: Uuid },
    /// `drain_pod_by_id(pod_id)` (D-14 [`super::drain`]).
    DrainPod { pod_id: Uuid },
    /// `complete_project_move_handoff(run_id)` (D-12
    /// `orchestration.service`).
    CompleteProjectMoveHandoff { run_id: Uuid },
}

/// Plan `_drain_after_commit` (`:277-290`): one handoff effect per
/// stopped-cancel id in barrier order, then the runner drain, then
/// one pod drain per pod id. `pod_ids` arrives deduplicated in
/// first-seen order (reaped runs, then stopped cancels) — Python
/// iterates a `set`, which is arbitrary; see the ported-bugs note.
pub fn plan_drain_after_commit(
    runner_id: Uuid,
    pod_ids: &[Uuid],
    handoff_ids: &[Uuid],
) -> Vec<SessionEffect> {
    let mut effects = Vec::with_capacity(handoff_ids.len() + 1 + pod_ids.len());
    effects.extend(
        handoff_ids
            .iter()
            .map(|run_id| SessionEffect::CompleteProjectMoveHandoff { run_id: *run_id }),
    );
    effects.push(SessionEffect::DrainRunner { runner_id });
    effects.extend(
        pod_ids
            .iter()
            .map(|pod_id| SessionEffect::DrainPod { pod_id: *pod_id }),
    );
    effects
}

/// Collect drain pod ids in first-seen order: reaped `(run, pod)`
/// pairs first, then the stopped-cancel pod ids — skipping nulls.
/// Python unions two id sets (`:273-274`); this keeps the same
/// membership with a deterministic order.
pub fn drain_pod_ids(
    reaped_pod_ids: &[Option<Uuid>],
    stopped_pod_ids: &[Option<Uuid>],
) -> Vec<Uuid> {
    let mut ids = Vec::new();
    for maybe in reaped_pod_ids
        .iter()
        .chain(stopped_pod_ids.iter())
        .flatten()
    {
        if !ids.contains(maybe) {
            ids.push(*maybe);
        }
    }
    ids
}

// ---------------------------------------------------------------------------
// Live-state upsert (`upsert_runner_live_state`, `:306-397`)
// ---------------------------------------------------------------------------

/// Snapshot fields stored on `RunnerLiveState` (`SNAPSHOT_FIELDS`,
/// `:312-321`), in source order. `observed_run_id` drives the wipe
/// and is not a wipe target; `tokens` and `model` are unpacked by
/// the upsert rather than copied field-for-field.
pub const SNAPSHOT_FIELDS: [&str; 8] = [
    "last_event_at",
    "last_event_kind",
    "last_event_summary",
    "agent_pid",
    "agent_subprocess_alive",
    "approvals_pending",
    "llm_model",
    "turn_count",
];

/// `runner_live_state` columns in model field-definition order
/// (`models.py:1471-1501`): the `SELECT` / `INSERT` projection and
/// the `UPDATE` `SET` order (Django's `save(update_fields=…)` —
/// the upsert's `sorted(set(…))` is cosmetic).
pub const LIVE_STATE_COLUMNS: [&str; 12] = [
    "runner_id",
    "observed_run_id",
    "last_event_at",
    "last_event_kind",
    "last_event_summary",
    "agent_pid",
    "agent_subprocess_alive",
    "approvals_pending",
    "usage",
    "llm_model",
    "turn_count",
    "updated_at",
];

/// `llm_model` bound (`models.py:1498`, `max_length=128`).
pub const LLM_MODEL_MAX_CHARS: usize = 128;

/// Parse an `observed_run_id` value (`parse_optional_uuid`,
/// `:324-333`): `None` for missing/explicit-null, the UUID for
/// present-and-valid, `Err` for malformed values (the caller logs
/// and skips the whole poll's ingestion). Non-string JSON values go
/// through `str()` exactly as the source does (`UUID(str(raw))`);
/// no `str()` of a number/bool/container is a UUID spelling, so
/// those always fail.
pub fn parse_optional_uuid(raw: Option<&Value>) -> Result<Option<Uuid>, uuid::Error> {
    match raw {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => parse_uuid_result(s),
        Some(other) => parse_uuid_result(&py_str(other)),
    }
}

/// Caller-owned live-state facts for [`plan_live_state_upsert`]:
/// the row's current `observed_run_id` (`None` when the row is new
/// — `get_or_create` leaves it `NULL` — or idle).
pub struct LiveStateFacts {
    pub observed_run_id: Option<Uuid>,
}

/// One live-state upsert decision: whether the `get_or_create`
/// runs, the ordered `SET` clauses when it does (model
/// field-definition order; `updated_at` appended by
/// [`live_state_update_sql`]), or a skip with the exact warning
/// line.
pub enum LiveStatePlan {
    /// Run the `get_or_create`, then persist these `SET` clauses
    /// via [`live_state_update_sql`] (never empty).
    Update { set_clauses: Vec<SetClause> },
    /// Run the `get_or_create`, then persist nothing: the only
    /// observability key was an unchanged `observed_run_id`
    /// (`update_fields` empty → no `save()`, `:396-397`).
    NoChange,
    /// Run the `get_or_create`, log the warning, skip the poll:
    /// malformed `observed_run_id`. No `UPDATE` follows.
    SkippedInvalidRunId { warning: String },
    /// No observability fields (or an empty/non-dict body): return
    /// before any SQL — not even the `get_or_create` select.
    Noop,
}

/// `logger.warning` line for a malformed `observed_run_id`
/// (`:364-368`): `ignoring runner live-state update for <id>:
/// invalid observed_run_id <repr>`. `%r` of a string is
/// single-quoted; other JSON values render via `py_repr`.
pub fn invalid_run_id_warning(runner_id: &Uuid, raw: &Value) -> String {
    format!(
        "ignoring runner live-state update for {runner_id}: invalid observed_run_id {}",
        py_repr(raw)
    )
}

/// Plan the observability-snapshot upsert (`:336-397`).
///
/// * Non-dict/empty bodies and bodies without `observed_run_id`,
///   a snapshot field, `tokens` or `model` are [`LiveStatePlan::Noop`]
///   (pre-flag runners; no SQL at all).
/// * A malformed `observed_run_id` is
///   [`LiveStatePlan::SkippedInvalidRunId`] (the `get_or_create`
///   already selected/inserted; nothing further runs).
/// * An unchanged `observed_run_id` with no other observability
///   keys is [`LiveStatePlan::NoChange`] (the `get_or_create` ran;
///   `update_fields` is empty so no `UPDATE` follows).
/// * An `observed_run_id` *change* (key present and different —
///   including new-row `NULL` vs incoming, and explicit-null after
///   a completed run) wipes every snapshot field plus `usage = {}`
///   before applying incoming values.
/// * Missing scalars never overwrite (same-run partial polls); an
///   explicit JSON null writes `NULL`.
/// * `tokens` normalizes through [`super::usage::normalize_usage`]
///   into `usage`; top-level `model` strips, truncates to 128 chars
///   (`[:128]`), and stores `None` when blank.
///
/// `SET` order is the model field-definition order, matching what
/// Django persists for the computed `update_fields` set.
pub fn plan_live_state_upsert(
    facts: &LiveStateFacts,
    status_entry: Option<&Map<String, Value>>,
    runner_id: &Uuid,
) -> LiveStatePlan {
    let Some(entry) = status_entry else {
        return LiveStatePlan::Noop;
    };
    if entry.is_empty() {
        return LiveStatePlan::Noop;
    }
    let has_snapshot = entry.contains_key("observed_run_id")
        || SNAPSHOT_FIELDS.iter().any(|f| entry.contains_key(*f))
        || entry.contains_key("tokens")
        || entry.contains_key("model");
    if !has_snapshot {
        return LiveStatePlan::Noop;
    }
    let incoming_run_id = match parse_optional_uuid(entry.get("observed_run_id")) {
        Ok(id) => id,
        Err(_) => {
            let raw = entry.get("observed_run_id").cloned().unwrap_or(Value::Null);
            return LiveStatePlan::SkippedInvalidRunId {
                warning: invalid_run_id_warning(runner_id, &raw),
            };
        }
    };
    // Wipe placeholders first; the per-field, `tokens` and
    // `model` arms overwrite them in place below.
    let mut sets: Vec<SetClause> = Vec::new();
    if entry.contains_key("observed_run_id") && facts.observed_run_id != incoming_run_id {
        for field in SNAPSHOT_FIELDS {
            sets.push(SetClause {
                column: field_to_column(field),
                value: SetValue::Null,
            });
        }
        sets.push(SetClause {
            column: "usage",
            value: SetValue::Json(Value::Object(Map::new())),
        });
        sets.push(SetClause {
            column: "observed_run_id",
            value: match incoming_run_id {
                Some(id) => SetValue::Text(id.to_string()),
                None => SetValue::Null,
            },
        });
    }
    for field in SNAPSHOT_FIELDS {
        if let Some(value) = entry.get(field) {
            let clause = SetClause {
                column: field_to_column(field),
                value: scalar_set_value(value),
            };
            upsert_clause(&mut sets, clause);
        }
    }
    if let Some(tokens) = entry.get("tokens") {
        upsert_clause(
            &mut sets,
            SetClause {
                column: "usage",
                value: SetValue::Json(super::usage::normalize_usage(tokens)),
            },
        );
    }
    if entry.contains_key("model") {
        let model = match entry.get("model") {
            Some(Value::String(s)) => s.clone(),
            Some(other) if py_truthy(other) => py_str(other),
            _ => String::new(),
        };
        // `str(value or "").strip()[:128] or None`.
        let model = truncate_chars(py_strip(&model), LLM_MODEL_MAX_CHARS);
        upsert_clause(
            &mut sets,
            SetClause {
                column: "llm_model",
                value: if model.is_empty() {
                    SetValue::Null
                } else {
                    SetValue::Text(model)
                },
            },
        );
    }
    if sets.is_empty() {
        return LiveStatePlan::NoChange;
    }
    // Order by model field-definition position (Django's
    // `save(update_fields=…)` — the source's `sorted(set(…))` only
    // feeds that filter).
    sets.sort_by_key(|clause| {
        LIVE_STATE_COLUMNS
            .iter()
            .position(|c| *c == clause.column)
            .unwrap_or(usize::MAX)
    });
    LiveStatePlan::Update { set_clauses: sets }
}

/// Snapshot field names are the column names verbatim.
fn field_to_column(field: &str) -> &'static str {
    match field {
        "last_event_at" => "last_event_at",
        "last_event_kind" => "last_event_kind",
        "last_event_summary" => "last_event_summary",
        "agent_pid" => "agent_pid",
        "agent_subprocess_alive" => "agent_subprocess_alive",
        "approvals_pending" => "approvals_pending",
        "llm_model" => "llm_model",
        "turn_count" => "turn_count",
        _ => unreachable!("SNAPSHOT_FIELDS is closed"),
    }
}

/// A snapshot scalar as a `SET` value: explicit JSON null writes
/// `NULL`; anything else binds as-is (the executing layer binds
/// the JSON scalar to the column, as Django's field adaptation
/// does — `last_event_at` strings parse as datetimes there).
fn scalar_set_value(value: &Value) -> SetValue {
    match value {
        Value::Null => SetValue::Null,
        Value::String(s) => SetValue::Text(s.clone()),
        other => SetValue::Json(other.clone()),
    }
}

/// Insert-or-replace a `SET` clause by column (later arms —
/// per-field, `tokens`, `model` — overwrite the wipe's placeholders
/// in place, as repeated `setattr` does before the single `save()`).
fn upsert_clause(sets: &mut Vec<SetClause>, clause: SetClause) {
    match sets
        .iter_mut()
        .find(|existing| existing.column == clause.column)
    {
        Some(existing) => existing.value = clause.value,
        None => sets.push(clause),
    }
}

/// Live-state `get_or_create` select (`:359`,
/// `upsert_first_snapshot.sql[0]`): `$1` runner id.
pub const LIVE_STATE_SELECT_SQL: &str = "SELECT \"runner_live_state\".\"runner_id\", \"runner_live_state\".\"observed_run_id\", \"runner_live_state\".\"last_event_at\", \"runner_live_state\".\"last_event_kind\", \"runner_live_state\".\"last_event_summary\", \"runner_live_state\".\"agent_pid\", \"runner_live_state\".\"agent_subprocess_alive\", \"runner_live_state\".\"approvals_pending\", \"runner_live_state\".\"usage\", \"runner_live_state\".\"llm_model\", \"runner_live_state\".\"turn_count\", \"runner_live_state\".\"updated_at\" FROM \"runner_live_state\" WHERE \"runner_live_state\".\"runner_id\" = $1 LIMIT 21";

/// Live-state `get_or_create` insert (`:359`,
/// `upsert_first_snapshot.sql[2]`): `$1` runner id, `$2`
/// `updated_at` (`auto_now`); every snapshot field `NULL`, `usage`
/// `'{}'::jsonb` (the `default=dict`).
pub const LIVE_STATE_INSERT_SQL: &str = "INSERT INTO \"runner_live_state\" (\"runner_id\", \"observed_run_id\", \"last_event_at\", \"last_event_kind\", \"last_event_summary\", \"agent_pid\", \"agent_subprocess_alive\", \"approvals_pending\", \"usage\", \"llm_model\", \"turn_count\", \"updated_at\") VALUES ($1, NULL, NULL, NULL, NULL, NULL, NULL, NULL, '{}'::jsonb, NULL, NULL, $2)";

/// Render the live-state `UPDATE` (`:397`) from planned `SET`
/// clauses (model field-definition order): `Null` renders literal
/// `NULL` (Django inlines `None`), anything else binds `$N` in
/// order; `updated_at` binds next (`auto_now`), `WHERE runner_id`
/// last.
pub fn live_state_update_sql(sets: &[SetClause]) -> String {
    let mut fragments = Vec::with_capacity(sets.len());
    let mut param = 0;
    for clause in sets {
        match &clause.value {
            SetValue::Null => fragments.push(format!("\"{}\" = NULL", clause.column)),
            _ => {
                param += 1;
                fragments.push(format!("\"{}\" = ${param}", clause.column));
            }
        }
    }
    param += 1;
    let updated_param = param;
    param += 1;
    let runner_param = param;
    format!(
        "UPDATE \"runner_live_state\" SET {}, \"updated_at\" = ${updated_param} WHERE \"runner_live_state\".\"runner_id\" = ${runner_param}",
        fragments.join(", ")
    )
}

// ---------------------------------------------------------------------------
// Presence + project slug (`:454-470`)
// ---------------------------------------------------------------------------

/// Mark a runner online (`mark_runner_online`, `:454-455`,
/// `mark_online_offline.online_sql[0]`): `.update()` preserves call
/// order (`status, last_heartbeat_at`). `$1` is `now`, `$2` the
/// runner id. No `REVOKED` exclusion — an online mark revives any row.
pub const MARK_RUNNER_ONLINE_SQL: &str = "UPDATE \"runner\" SET \"status\" = 'online', \"last_heartbeat_at\" = $1 WHERE \"runner\".\"id\" = $2";

/// Mark a runner offline (`mark_runner_offline`, `:458-459`,
/// `mark_online_offline.offline_sql[0]`): `.exclude(status=REVOKED)`
/// appends `AND NOT (status = 'revoked')`, so a revoked row stays
/// revoked. `$1` the runner id.
pub const MARK_RUNNER_OFFLINE_SQL: &str = "UPDATE \"runner\" SET \"status\" = 'offline' WHERE (\"runner\".\"id\" = $1 AND NOT (\"runner\".\"status\" = 'revoked'))";

/// Resolve `runner.pod.project.identifier`
/// (`resolve_runner_project_slug`, `:462-470`,
/// `resolve_runner_project_slug.sql[0]`): `select_related("pod",
/// "pod__project")` inner-joins both tables, so a podless runner
/// (or a pod without a project) returns no row; the `Runner`
/// default ordering applies; `$1` the runner id. The projection is
/// the three tables' columns in `_meta` order, built from the same
/// L2 lists the merged ports pin.
pub fn resolve_runner_slug_sql() -> String {
    format!(
        "SELECT {}, {}, {} FROM \"runner\" INNER JOIN \"pod\" ON (\"runner\".\"pod_id\" = \"pod\".\"id\") INNER JOIN \"projects\" ON (\"pod\".\"project_id\" = \"projects\".\"id\") WHERE \"runner\".\"id\" = $1 ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC LIMIT 1",
        qualified_columns("runner", &crate::runner_sessions::guards::RUNNER_COLUMNS),
        qualified_columns("pod", pidash_db::runner_runs::pod::COLUMNS),
        qualified_columns("projects", crate::runner_runs::PROJECT_COLUMNS),
    )
}

/// Caller-fetched facts for [`resolve_slug`]: whether the
/// [`resolve_runner_slug_sql`] row exists, the row's `pod_id`, and
/// the joined project's `identifier` (absent when the join partner
/// is missing).
pub struct SlugFacts {
    pub row_found: bool,
    pub pod_id: Option<Uuid>,
    pub project_identifier: Option<String>,
}

/// Pick the slug from the resolved row (`:465-470`): `None` when
/// the row is missing, the runner has no pod, or the project join
/// partner is missing — else the identifier. (With inner joins the
/// middle arms are defensive: a podless runner returns no row.)
pub fn resolve_slug(facts: &SlugFacts) -> Option<String> {
    if !facts.row_found || facts.pod_id.is_none() {
        return None;
    }
    facts.project_identifier.clone()
}

// ---------------------------------------------------------------------------
// Session-open redeliver + resume ack (`:400-451`, `:473-508`)
// ---------------------------------------------------------------------------

/// Full `agent_run` row projection in `_meta` order (the L2 column
/// list the merged ports pin), for the redeliver / resume-ack
/// lookups below.
fn agent_run_projection() -> String {
    qualified_columns("agent_run", pidash_db::runner_runs::agent_run::COLUMNS)
}

/// Normalize the redeliver skip id (`:417-422`):
/// `str(UUID(str(in_flight_run_id)))` for truthy values, `None`
/// for missing/empty/malformed ones (a bad skip silently disables
/// the exclusion — `redeliver_bad_skip`). The Python block is
/// identical to the in-flight one, so this delegates to
/// [`parse_in_flight_id`] — non-string JSON values go through
/// `str()` there, exactly as the source does.
pub fn parse_skip_id(in_flight_run_id: Option<&Value>) -> Option<String> {
    parse_in_flight_id(in_flight_run_id)
}

/// Cancel-first redeliver scan (`:424-430`): the oldest
/// `CANCEL_REQUESTED` run on the runner
/// (`ORDER BY assigned_at ASC, created_at ASC LIMIT 1`), skipping
/// the reconnect-reported run when one parsed. `$1` runner id,
/// `$2` the skip id when `skip` is set.
pub fn redeliver_cancel_sql(skip: bool) -> String {
    let mut where_ =
        "(\"agent_run\".\"runner_id\" = $1 AND \"agent_run\".\"status\" = 'cancel_requested'"
            .to_owned();
    if skip {
        where_.push_str(" AND NOT (\"agent_run\".\"id\" = $2)");
    }
    format!(
        "SELECT {} FROM \"agent_run\" WHERE {where_}) ORDER BY \"agent_run\".\"assigned_at\" ASC, \"agent_run\".\"created_at\" ASC LIMIT 1",
        agent_run_projection()
    )
}

/// Assign redeliver scan (`:439-448`): the oldest `ASSIGNED` /
/// `WAITING_FOR_WORKTREE` run on the runner (same ordering, same
/// skip). `RUNNING` runs are deliberately excluded — a lost
/// `RUNNING` run is the heartbeat reaper's job, not redelivery's.
pub fn redeliver_assign_sql(skip: bool) -> String {
    let mut where_ = "(\"agent_run\".\"runner_id\" = $1 AND \"agent_run\".\"status\" IN ('assigned', 'waiting_for_worktree')".to_owned();
    if skip {
        where_.push_str(" AND NOT (\"agent_run\".\"id\" = $2)");
    }
    format!(
        "SELECT {} FROM \"agent_run\" WHERE {where_}) ORDER BY \"agent_run\".\"assigned_at\" ASC, \"agent_run\".\"created_at\" ASC LIMIT 1",
        agent_run_projection()
    )
}

/// Caller-fetched facts for [`plan_redeliver`]: the cancel-first
/// lookup's run id (if any) and the assign lookup's payload fields
/// (if any). The executing layer runs [`redeliver_cancel_sql`]
/// first and [`redeliver_assign_sql`] only when it finds nothing.
pub struct RedeliverFacts {
    pub cancel_run_id: Option<Uuid>,
    pub assign_run: Option<RedeliverAssignFacts>,
}

/// The assign lookup's payload fields for the redelivered frame.
pub struct RedeliverAssignFacts {
    pub run_id: Uuid,
    pub work_item_id: Option<Uuid>,
    pub prompt: String,
    pub run_config: Map<String, Value>,
}

/// Pick the session-open redeliver payload (`:431-451`): the
/// cancel frame when a `CANCEL_REQUESTED` run waits, else the
/// assign frame for the oldest `ASSIGNED` / `WAITING_FOR_WORKTREE`
/// run, else `None`. Frames reuse the shapes port
/// ([`envelopes::redeliver_cancel_frame`], [`envelopes::build_assign_msg`]).
pub fn plan_redeliver(facts: &RedeliverFacts) -> Option<Map<String, Value>> {
    if let Some(cancel_id) = facts.cancel_run_id {
        return Some(envelopes::redeliver_cancel_frame(&cancel_id.to_string()));
    }
    let assign = facts.assign_run.as_ref()?;
    let work_item_id = assign.work_item_id.map(|id| id.to_string());
    Some(envelopes::build_assign_msg(
        &assign.run_id.to_string(),
        work_item_id.as_deref(),
        &assign.prompt,
        &assign.run_config,
    ))
}

/// Resume-ack run lookup (`:482`, `resume_ack_live.sql[0]`):
/// the full row by `(id, runner_id)`; `$1` run id, `$2` runner id.
/// Django sorts the `filter()` kwargs (`id`, `runner_id`).
pub fn resume_ack_lookup_sql() -> String {
    format!(
        "SELECT {} FROM \"agent_run\" WHERE (\"agent_run\".\"id\" = $1 AND \"agent_run\".\"runner_id\" = $2) ORDER BY \"agent_run\".\"created_at\" DESC LIMIT 1",
        agent_run_projection()
    )
}

/// Resume-ack `last_seq` lookup (`:501`, `resume_ack_live.sql[1]`):
/// newest event seq for the run (`ORDER BY seq DESC LIMIT 1`),
/// `NULL` when the run has no events. `$1` run id.
pub const RESUME_ACK_LAST_SEQ_SQL: &str = "SELECT \"agent_run_event\".\"seq\" FROM \"agent_run_event\" WHERE \"agent_run_event\".\"agent_run_id\" = $1 ORDER BY \"agent_run_event\".\"seq\" DESC LIMIT 1";

/// Caller-fetched facts for [`plan_resume_ack`]: the looked-up
/// run's status + `thread_id` (`None` when the lookup missed).
pub struct ResumeAckFacts {
    pub status: AgentRunStatus,
    pub thread_id: String,
}

/// Build the `resume_ack` payload (`:483-508`): `cancel` /
/// `unknown_run_on_reconnect` when the run is missing,
/// `cancel` / `cancellation_pending_on_reconnect` when it is
/// `CANCEL_REQUESTED`, `cancel` / `run_already_<status>` when it
/// is terminal, else `resume_ack` with the newest event seq (null
/// when there are no events). `run_id` echoes the passed value
/// verbatim (never normalized). Frames reuse the shapes port.
pub fn plan_resume_ack(
    run_id: &str,
    run: Option<&ResumeAckFacts>,
    last_seq: Option<i64>,
) -> Map<String, Value> {
    let Some(run) = run else {
        return envelopes::cancel_frame(run_id, envelopes::REASON_UNKNOWN_RUN_ON_RECONNECT);
    };
    if run.status == AgentRunStatus::CancelRequested {
        return envelopes::cancel_frame(
            run_id,
            envelopes::REASON_CANCELLATION_PENDING_ON_RECONNECT,
        );
    }
    if run.status.is_terminal() {
        return envelopes::cancel_frame(
            run_id,
            &envelopes::terminal_cancel_reason(run.status.value()),
        );
    }
    envelopes::resume_ack_frame(run_id, last_seq, run.status.value(), &run.thread_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const FIXTURE_RUNNER: &str = "1ad15e53f6da4da4a6319dd0796dec2f";

    /// FX-RSES-06 fixture document.
    fn fx() -> Value {
        let text =
            include_str!("../../../../fixtures/runner_sessions/fx-rses-06-session-service.json");
        serde_json::from_str(text).expect("fixture parses")
    }

    /// One captured statement from a fixture section.
    fn fx_sql(section: &str, index: usize) -> String {
        fx()[section]["sql"][index]
            .as_str()
            .unwrap_or_else(|| panic!("{section}.sql[{index}]"))
            .to_owned()
    }

    /// Pin `actual` SQL against a captured Django statement: apply
    /// the `(interpolated literal → $N)` substitutions, then require
    /// byte equality.
    fn pin(actual: &str, fixture_sql: &str, subs: &[(&str, &str)]) {
        let mut expected = fixture_sql.to_owned();
        for (old, new) in subs {
            assert!(
                expected.contains(old),
                "fixture literal missing for {new}: {old}"
            );
            expected = expected.replace(old, new);
        }
        assert_eq!(actual, expected);
    }

    /// A JSON-object body from pairs.
    fn body(pairs: Vec<(&str, Value)>) -> Map<String, Value> {
        pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
    }

    fn str_body(pairs: Vec<(&str, &str)>) -> Map<String, Value> {
        body(
            pairs
                .into_iter()
                .map(|(k, v)| (k, Value::String(v.to_owned())))
                .collect(),
        )
    }

    // -- hello ------------------------------------------------------

    #[test]
    fn merge_dev_metadata_replays_fixture_vectors() {
        let fx = fx()["_merge_dev_metadata"].clone();
        let current = serde_json::json!({"working_dir": "/old", "codex_version": "v0"});
        // Missing `working_dir` keeps; empty string clears.
        assert_eq!(
            Value::Object(merge_dev_metadata(&current, &Map::new())),
            fx["missing_working_dir_keeps"]
        );
        assert_eq!(
            Value::Object(merge_dev_metadata(
                &current,
                &str_body(vec![("working_dir", "")])
            )),
            fx["empty_string_clears"]
        );
        // Sets + truncates to 1024 chars.
        let long = "w".repeat(1500);
        let merged = merge_dev_metadata(
            &Value::Object(Map::new()),
            &str_body(vec![("working_dir", &long)]),
        );
        assert_eq!(Value::Object(merged.clone()), fx["sets_and_truncates"]);
        assert_eq!(
            merged["working_dir"].as_str().unwrap().chars().count(),
            fx["truncated_len"].as_u64().unwrap() as usize
        );
        // Non-string ignored; non-dict current starts from {}.
        assert_eq!(
            Value::Object(merge_dev_metadata(
                &current,
                &body(vec![("working_dir", Value::from(42))])
            ))["working_dir"],
            fx["non_string_ignored"]["working_dir"]
        );
        assert_eq!(
            Value::Object(merge_dev_metadata(
                &Value::String("nope".to_owned()),
                &str_body(vec![("working_dir", "/n")])
            )),
            fx["non_dict_current"]
        );
        // engine_version arms.
        assert_eq!(
            Value::Object(merge_dev_metadata(
                &Value::Object(Map::new()),
                &str_body(vec![("engine_version", "v2.0")])
            )),
            fx["engine_version_sets_codex"]
        );
        assert_eq!(
            Value::Object(merge_dev_metadata(
                &serde_json::json!({"codex_version": "v1"}),
                &str_body(vec![("engine_version", "")])
            )),
            fx["engine_version_empty_clears"]
        );
        assert_eq!(
            Value::Object(merge_dev_metadata(
                &serde_json::json!({"codex_version": "v1"}),
                &body(vec![("engine_version", Value::from(7))])
            )),
            fx["engine_version_non_string"]
        );
        let merged = merge_dev_metadata(
            &Value::Object(Map::new()),
            &str_body(vec![("engine_version", &"e".repeat(100))]),
        );
        assert_eq!(Value::Object(merged.clone()), fx["engine_version_trunc64"]);
        assert_eq!(
            merged["codex_version"].as_str().unwrap().chars().count(),
            fx["engine_truncated_len"].as_u64().unwrap() as usize
        );
    }

    #[test]
    fn agent_capabilities_replays_fixture_vectors() {
        let fx = fx()["_agent_capabilities"].clone();
        let check = |kind: Option<Value>, current: Value, case: &str| {
            let mut map = Map::new();
            if let Some(kind) = kind {
                map.insert("agent_kind".to_string(), kind);
            }
            let (caps, changed) = agent_capabilities(&map, &current);
            assert_eq!(caps, fx[case][0], "caps {case}");
            assert_eq!(changed, fx[case][1].as_bool().unwrap(), "changed {case}");
        };
        let old = serde_json::json!(["agent:old"]);
        let empty = Value::Array(Vec::new());
        check(None, old.clone(), "missing_kind_keeps_list");
        check(None, serde_json::json!({"n": 1}), "missing_kind_non_list");
        check(Some(Value::from("claude_code")), old.clone(), "valid_new");
        check(
            Some(Value::from("claude_code")),
            serde_json::json!(["agent:claude_code"]),
            "valid_same_no_change",
        );
        check(
            Some(Value::from("Claude_Code")),
            old.clone(),
            "invalid_upper",
        );
        check(
            Some(Value::from("claude-code")),
            empty.clone(),
            "invalid_dash",
        );
        check(Some(Value::from("")), empty.clone(), "invalid_empty");
        check(Some(Value::from(5)), empty.clone(), "invalid_non_string");
        check(
            Some(Value::from("a".repeat(33))),
            empty.clone(),
            "too_long_33",
        );
        check(
            Some(Value::from("a".repeat(32))),
            empty.clone(),
            "max_32_ok",
        );
        // Python `$` matches before one trailing newline: `x\n`
        // validates and persists verbatim (verified via stdlib); a
        // second newline cannot match.
        let (caps, changed) =
            agent_capabilities(&str_body(vec![("agent_kind", "muse_code\n")]), &empty);
        assert_eq!(caps, serde_json::json!(["agent:muse_code\n"]));
        assert!(changed);
        let (caps, changed) =
            agent_capabilities(&str_body(vec![("agent_kind", "muse_code\n\n")]), &empty);
        assert_eq!(caps, Value::Array(Vec::new()));
        assert!(!changed);
    }

    #[test]
    fn hello_plan_orders_set_by_field_definition() {
        let facts = HelloFacts {
            os: "old-os".to_string(),
            arch: String::new(),
            runner_version: String::new(),
            dev_metadata: Value::Object(Map::new()),
            capabilities: serde_json::json!(["agent:old"]),
        };
        let plan = plan_hello_update(
            &facts,
            &str_body(vec![
                ("os", "linux"),
                ("arch", "x86_64"),
                ("version", "3.1.4"),
                ("working_dir", "/home/u/proj"),
                ("engine_version", "codex-v9"),
                ("agent_kind", "muse_code"),
            ]),
        );
        assert!(plan.capabilities_changed);
        let columns: Vec<&str> = plan.set_clauses.iter().map(|c| c.column).collect();
        assert_eq!(
            columns,
            [
                "capabilities",
                "os",
                "arch",
                "runner_version",
                "dev_metadata",
                "last_heartbeat_at"
            ]
        );
        assert_eq!(
            plan.set_clauses[0].value,
            SetValue::Json(serde_json::json!(["agent:muse_code"]))
        );
        assert_eq!(
            plan.set_clauses[4].value,
            SetValue::Json(serde_json::json!({
                "codex_version": "codex-v9",
                "working_dir": "/home/u/proj",
            }))
        );
        assert_eq!(plan.set_clauses[5].value, SetValue::Now);
        // Runner-after values match the fixture row.
        let after = fx()["apply_hello"]["runner_after"].clone();
        assert_eq!(after["os"], "linux");
        assert_eq!(
            after["capabilities"],
            serde_json::json!(["agent:muse_code"])
        );
        assert_eq!(
            after["dev_metadata"],
            serde_json::json!({
                "working_dir": "/home/u/proj",
                "codex_version": "codex-v9",
            })
        );
    }

    #[test]
    fn hello_empty_body_keeps_falsy_and_skips_capabilities() {
        let facts = HelloFacts {
            os: "keepme".to_string(),
            arch: String::new(),
            runner_version: String::new(),
            dev_metadata: Value::Object(Map::new()),
            capabilities: serde_json::json!(["agent:x"]),
        };
        let plan = plan_hello_update(&facts, &Map::new());
        assert!(!plan.capabilities_changed);
        let columns: Vec<&str> = plan.set_clauses.iter().map(|c| c.column).collect();
        assert_eq!(
            columns,
            [
                "os",
                "arch",
                "runner_version",
                "dev_metadata",
                "last_heartbeat_at"
            ]
        );
        assert_eq!(
            plan.set_clauses[0].value,
            SetValue::Text("keepme".to_string())
        );
        // Falsy non-string body values also keep the row value.
        let plan = plan_hello_update(
            &facts,
            &body(vec![
                ("os", Value::from(0)),
                ("arch", Value::Bool(false)),
                ("version", Value::Array(Vec::new())),
            ]),
        );
        assert_eq!(
            plan.set_clauses[0].value,
            SetValue::Text("keepme".to_string())
        );
        assert_eq!(plan.set_clauses[1].value, SetValue::Text(String::new()));
        // …while a truthy non-string stores via str().
        let plan = plan_hello_update(&facts, &body(vec![("os", Value::from(7))]));
        assert_eq!(plan.set_clauses[0].value, SetValue::Text("7".to_string()));
        // Floats render in CPython repr form (exponent normalized).
        let plan = plan_hello_update(&facts, &body(vec![("os", serde_json::json!(1e16))]));
        assert_eq!(
            plan.set_clauses[0].value,
            SetValue::Text("1e+16".to_string())
        );
        let plan = plan_hello_update(&facts, &body(vec![("os", serde_json::json!(4.5))]));
        assert_eq!(plan.set_clauses[0].value, SetValue::Text("4.5".to_string()));
        // Fixture row echoes.
        let empty = fx()["apply_hello_empty_body"].clone();
        assert_eq!(empty["os"], "keepme");
        assert_eq!(empty["capabilities"], serde_json::json!(["agent:x"]));
    }

    #[test]
    fn hello_sql_pins_to_fixture() {
        pin(
            HELLO_UPDATE_SQL,
            &fx_sql("apply_hello", 0),
            &[
                ("'[\"agent:muse_code\"]'::jsonb", "$1"),
                ("\"os\" = 'linux'", "\"os\" = $2"),
                ("\"arch\" = 'x86_64'", "\"arch\" = $3"),
                ("\"runner_version\" = '3.1.4'", "\"runner_version\" = $4"),
                (
                    "'{\"codex_version\": \"codex-v9\", \"working_dir\": \"/home/u/proj\"}'::jsonb",
                    "$5",
                ),
                ("'2026-10-03 00:29:19.269669+00:00'::timestamptz", "$6"),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$7"),
            ],
        );
        pin(
            HELLO_UPDATE_NO_CAPABILITIES_SQL,
            &fx_sql("apply_hello_empty_body", 0),
            &[
                ("\"os\" = 'keepme'", "\"os\" = $1"),
                ("\"arch\" = ''", "\"arch\" = $2"),
                ("\"runner_version\" = ''", "\"runner_version\" = $3"),
                ("'{}'::jsonb", "$4"),
                ("'2026-10-03 00:29:19.328426+00:00'::timestamptz", "$5"),
                ("'4a790c25081944fdb9276c08dab10fd1'::uuid", "$6"),
            ],
        );
        // The hello path reaps with exclude_redeliverable (3-status IN).
        let reduced = reapable_statuses(true);
        pin(
            &stale_cancel_ids_sql(&reduced, false),
            &fx_sql("apply_hello", 1),
            &[
                ("'2026-10-03 00:28:19.282411+00:00'::timestamptz", "$1"),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$2"),
            ],
        );
        pin(
            &stale_cancel_pod_ids_sql(&reduced, false),
            &fx_sql("apply_hello", 2),
            &[
                ("'2026-10-03 00:28:19.282411+00:00'::timestamptz", "$1"),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$2"),
            ],
        );
        pin(
            &stale_pairs_sql(&reduced, false, 0),
            &fx_sql("apply_hello", 3),
            &[
                ("'2026-10-03 00:28:19.282411+00:00'::timestamptz", "$1"),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$2"),
            ],
        );
    }

    // -- heartbeat parsing --------------------------------------------

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 3, 0, 29, 19).unwrap()
    }

    #[test]
    fn heartbeat_ts_parses_and_clamps() {
        let now = fixed_now();
        // Missing / non-string → now.
        assert_eq!(parse_heartbeat_ts(None, now), Ok(now));
        assert_eq!(parse_heartbeat_ts(Some(&Value::from(7)), now), Ok(now));
        assert_eq!(parse_heartbeat_ts(Some(&Value::Null), now), Ok(now));
        // Garbage → now (ValueError arm).
        assert_eq!(
            parse_heartbeat_ts(Some(&Value::from("not-a-time")), now),
            Ok(now)
        );
        // Fresh Zulu parses exactly.
        let fresh = now - Duration::seconds(10);
        let ts = Value::String(fresh.format("%Y-%m-%dT%H:%M:%SZ").to_string());
        assert_eq!(parse_heartbeat_ts(Some(&ts), now), Ok(fresh));
        // Non-UTC offsets convert.
        let ts = Value::String("2026-10-03T02:29:09+02:00".to_string());
        assert_eq!(
            parse_heartbeat_ts(Some(&ts), now),
            Ok(now - Duration::seconds(10))
        );
        // Future clamps to now; old clamps to now - 60s.
        let ts = Value::String("2026-10-03T00:30:00Z".to_string());
        assert_eq!(parse_heartbeat_ts(Some(&ts), now), Ok(now));
        let ts = Value::String("2026-10-03T00:20:00Z".to_string());
        assert_eq!(
            parse_heartbeat_ts(Some(&ts), now),
            Ok(now - Duration::seconds(60))
        );
        // Naive timestamps raise (CPython TypeError, verified via stdlib).
        let ts = Value::String("2026-10-03T00:20:00".to_string());
        assert_eq!(
            parse_heartbeat_ts(Some(&ts), now),
            Err(HeartbeatError::NaiveTimestamp)
        );
        let ts = Value::String("2026-10-03".to_string());
        assert_eq!(
            parse_heartbeat_ts(Some(&ts), now),
            Err(HeartbeatError::NaiveTimestamp)
        );
        assert_eq!(OFFLINE_GRACE_SECS, 60);
        assert_eq!(ASSIGN_DELIVERY_GRACE_SECS, 60);
    }

    #[test]
    fn in_flight_id_normalizes_or_ignores() {
        assert_eq!(parse_in_flight_id(None), None);
        assert_eq!(parse_in_flight_id(Some(&Value::Null)), None);
        assert_eq!(parse_in_flight_id(Some(&Value::from(""))), None);
        assert_eq!(parse_in_flight_id(Some(&Value::from(0))), None);
        assert_eq!(
            parse_in_flight_id(Some(&Value::from("66EA9C4D-EF41-4D9A-9819-51E0E22D12C6"))),
            Some("66ea9c4d-ef41-4d9a-9819-51e0e22d12c6".to_string())
        );
        assert_eq!(parse_in_flight_id(Some(&Value::from("nope"))), None);
        assert_eq!(parse_in_flight_id(Some(&Value::from(5))), None);
        assert_eq!(parse_in_flight_id(Some(&Value::Bool(true))), None);
    }

    #[test]
    fn heartbeat_grammar_matches_fromisoformat_probes() {
        // Every vector below was cross-checked against CPython 3.12's
        // `datetime.fromisoformat(ts.replace("Z", "+00:00"))`: aware
        // values clamp into [now-60s, now], naive values raise,
        // garbage falls back to `now`.
        let now = fixed_now();
        let parse = |ts: &str| parse_heartbeat_ts(Some(&Value::from(ts)), now);
        let utc = |y, mo, d, h, mi, s| Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap();
        // Basic + mixed + any-separator shapes (fresh → exact).
        assert_eq!(
            parse("20261003T002909+00:00"),
            Ok(utc(2026, 10, 3, 0, 29, 9))
        );
        assert_eq!(
            parse("2026-10-03T002909+00:00"),
            Ok(utc(2026, 10, 3, 0, 29, 9))
        );
        assert_eq!(
            parse("2026-10-03X002909+00:00"),
            Ok(utc(2026, 10, 3, 0, 29, 9))
        );
        assert_eq!(
            parse("2026-10-03t00:29:09+00:00"),
            Ok(utc(2026, 10, 3, 0, 29, 9))
        );
        // Comma fractions and empty-dot fractions.
        assert_eq!(
            parse("2026-10-03T00:29:09,5+00:00"),
            Ok(utc(2026, 10, 3, 0, 29, 9) + Duration::milliseconds(500))
        );
        assert_eq!(
            parse("2026-10-03T00:29:09.+00:00"),
            Ok(utc(2026, 10, 3, 0, 29, 9))
        );
        // Empty fractions parse only before an offset.
        assert_eq!(
            parse("2026-10-03T00:29,+00:00"),
            Ok(utc(2026, 10, 3, 0, 29, 0))
        );
        // Short times (HH, HHMM, extended or basic) parse, with a
        // fraction or offset after any component.
        assert_eq!(
            parse("2026-10-03T00:29+00:00"),
            Ok(utc(2026, 10, 3, 0, 29, 0))
        );
        assert_eq!(
            parse("2026-10-03T0029+00:00"),
            Ok(utc(2026, 10, 3, 0, 29, 0))
        );
        assert_eq!(
            parse("2026-10-03T00:29.5+00:00"),
            Ok(utc(2026, 10, 3, 0, 29, 0) + Duration::milliseconds(500))
        );
        assert_eq!(
            parse("2026-10-03T0029,5+00:00"),
            Ok(utc(2026, 10, 3, 0, 29, 0) + Duration::milliseconds(500))
        );
        // Hour-only is aware this far back, so it clamps to the
        // floor (garbage would fall back to `now` instead).
        assert_eq!(
            parse("2026-10-03T00+00:00"),
            Ok(now - Duration::seconds(60))
        );
        // 9-digit fractions truncate to microseconds.
        let micros =
            Utc.with_ymd_and_hms(2026, 10, 3, 0, 29, 9).unwrap() + Duration::microseconds(123_456);
        assert_eq!(parse("2026-10-03T00:29:09.123456789+00:00"), Ok(micros));
        // Offset spellings convert.
        assert_eq!(
            parse("2026-10-03T00:29:09+0000"),
            Ok(utc(2026, 10, 3, 0, 29, 9))
        );
        assert_eq!(
            parse("2026-10-03T02:29:09+02"),
            Ok(utc(2026, 10, 3, 0, 29, 9))
        );
        assert_eq!(
            parse("2026-10-03T01:29:09+00:60"),
            Ok(utc(2026, 10, 3, 0, 29, 9))
        );
        // Naive shapes raise (bare dates included).
        for naive in [
            "20261003T002909",
            "20261003",
            "2026-10-03 00:29:09",
            "2026-10-03T00:29:09,5",
            "2026-10-03T00:29",
            "2026-10-03T0029",
            "2026-10-03T00",
            "2026-10-03T00:29.5",
            "20261003T0029",
        ] {
            assert_eq!(parse(naive), Err(HeartbeatError::NaiveTimestamp), "{naive}");
        }
        // Out-of-range instants clamp, never panic (Python only
        // compares, so it never overflows either).
        assert_eq!(
            parse("0001-01-01T00:00:00+23:59"),
            Ok(now - Duration::seconds(60))
        );
        assert_eq!(parse("9999-12-31T23:59:59-23:59"), Ok(now));
        // Garbage falls back to `now`.
        for garbage in [
            "2026-10-03T00:29:09z", // lowercase z rejected
            "2026-1-3T00:29:09+00:00",
            "2026-10-03T00:29:09+24:00",
            "2026-10-03T00:29:09+23:59:60",
            "2026-10-03T00:29:09,",
            "2026-10-03T00:29:09.",      // trailing separator, no offset
            "2026-W40-3T00:29:09+00:00", // week dates: residual
            "2026-10-03T00:29:09+00:00:0",
            " 2026-10-03T00:29:09+00:00",
            "2026-02-30T00:00:00+00:00",
            "2026-10-03T00:20:60+00:00",
            "0000-01-01T00:00:00+00:00", // year 0 rejected
            "0000-01-01",                // …bare too
            "2026-10-03T12:3059",        // time shapes never mix
            "2026-10-03T1230:59",
            "2026-10-03T123", // odd basic lengths
            "2026-10-03T12345",
            "2026-10-03T1230591",  // the length-7 hole
            "2026-10-03T12:",      // `:` commits to MM
            "2026-10-03T12:30:",   // …and to SS
            "2026-10-03T12305900", // separator-less fraction: residual
        ] {
            assert_eq!(parse(garbage), Ok(now), "{garbage}");
        }
    }

    #[test]
    fn uuid_parsing_matches_cpython_probes() {
        // Cross-checked against CPython 3.12 `uuid.UUID`: hyphens
        // strip anywhere, single braces strip from the ends, bare
        // `urn:`/`uuid:` strip (case-sensitive), and `int(hex, 16)`
        // takes whitespace/sign/single-underscores.
        let canon = "66ea9c4d-ef41-4d9a-9819-51e0e22d12c6";
        let plain = "66ea9c4def414d9a981951e0e22d12c6";
        for accepted in [
            "66ea9c4def41-4d9a-9819-51e0e22d12c6".to_string(),
            "6-6e-a9c4def414d9a981951e0e22d12c6".to_string(),
            "{".to_string() + plain,
            plain.to_string() + "}",
            "{{".to_string() + plain + "}}",
            "urn:".to_string() + plain,
            "uuid:".to_string() + plain,
            plain[..8].to_string() + "urn:" + &plain[8..],
        ] {
            assert_eq!(
                parse_in_flight_id(Some(&Value::from(accepted.as_str()))),
                Some(canon.to_string()),
                "{accepted}"
            );
        }
        // `int()` edges: surrounding whitespace and one sign shift
        // the value (leading zero); embedded underscores drop out.
        assert_eq!(
            parse_in_flight_id(Some(&Value::from(" ".to_string() + &plain[..31]))),
            Some("066ea9c4-def4-14d9-a981-951e0e22d12c".to_string())
        );
        assert_eq!(
            parse_in_flight_id(Some(&Value::from("+".to_string() + &plain[..31]))),
            Some("066ea9c4-def4-14d9-a981-951e0e22d12c".to_string())
        );
        assert_eq!(
            parse_in_flight_id(Some(&Value::from(
                plain[..16].to_string() + "_" + &plain[17..32] + &plain[32..]
            ))),
            Some("066ea9c4-def4-14d9-a819-51e0e22d12c6".to_string())
        );
        for rejected in [
            "URN:UUID:".to_string() + plain,
            plain[..8].to_string() + "{" + &plain[8..],
            plain[..31].to_string(),
            plain.to_string() + "1",
            plain[..15].to_string() + "__" + &plain[15..30],
            "-".to_string() + &plain[..31],
        ] {
            assert_eq!(
                parse_in_flight_id(Some(&Value::from(rejected.as_str()))),
                None,
                "{rejected}"
            );
        }
        // Same parser behind the skip id and the live-state uuid.
        assert_eq!(
            parse_skip_id(Some(&Value::from("66ea9c4def41-4d9a-9819-51e0e22d12c6"))),
            Some(canon.to_string())
        );
        assert!(parse_optional_uuid(Some(&Value::from("URN:UUID:".to_string() + plain))).is_err());
    }

    #[test]
    fn cutoff_takes_the_earlier_bound() {
        let now = fixed_now();
        let fresh = now - Duration::seconds(10);
        // Heartbeat newer than the grace → grace wins.
        assert_eq!(effective_cutoff(now, fresh), now - Duration::seconds(60));
        // Heartbeat older than the grace → heartbeat wins.
        let old = now - Duration::seconds(61);
        assert_eq!(effective_cutoff(now, old), old);
    }

    #[test]
    fn reapable_statuses_match_source_tuples() {
        use AgentRunStatus::*;
        assert_eq!(
            reapable_statuses(false),
            vec![
                Assigned,
                WaitingForWorktree,
                Running,
                CancelRequested,
                AwaitingApproval,
                AwaitingReauth
            ]
        );
        assert_eq!(
            reapable_statuses(true),
            vec![Running, AwaitingApproval, AwaitingReauth]
        );
    }

    // -- reaper SQL ---------------------------------------------------

    #[test]
    fn reap_poll_path_sql_pins_to_fixture() {
        let full = reapable_statuses(false);
        pin(
            CANCEL_REQUESTED_EXISTS_SQL,
            &fx_sql("reap_poll_path", 0),
            &[
                ("'66ea9c4def414d9a981951e0e22d12c6'::uuid", "$1"),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$2"),
            ],
        );
        for (index, build) in [
            (1, stale_cancel_ids_sql(&full, true)),
            (2, stale_cancel_pod_ids_sql(&full, true)),
            (3, stale_pairs_sql(&full, true, 0)),
        ] {
            pin(
                &build,
                &fx_sql("reap_poll_path", index),
                &[
                    ("'2026-10-03 00:28:19.483761+00:00'::timestamptz", "$1"),
                    ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$2"),
                    ("'66ea9c4def414d9a981951e0e22d12c6'::uuid", "$3"),
                ],
            );
        }
        // The reaped row carries the exact FAILED payload.
        let stale = fx()["reap_poll_path"]["stale_after"].clone();
        assert_eq!(stale["status"], "failed");
        assert_eq!(stale["error_code"], "heartbeat_reaped");
        assert_eq!(
            stale["error"],
            Value::String(reap_error_detail(Some(
                "66ea9c4d-ef41-4d9a-9819-51e0e22d12c6"
            )))
        );
        assert_eq!(fx()["reap_poll_path"]["fresh_after_status"], "running");
        assert_eq!(fx()["reap_poll_path"]["inflight_after_status"], "running");
        assert_eq!(REAP_ERROR_CODE, "heartbeat_reaped");
    }

    #[test]
    fn reap_cancel_barrier_sql_pins_to_fixture() {
        let full = reapable_statuses(false);
        pin(
            &stale_cancel_ids_sql(&full, false),
            &fx_sql("reap_cancel_barrier", 0),
            &[
                ("'2026-10-03 00:28:19.674320+00:00'::timestamptz", "$1"),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$2"),
            ],
        );
        pin(
            &stale_cancel_pod_ids_sql(&full, false),
            &fx_sql("reap_cancel_barrier", 1),
            &[
                ("'2026-10-03 00:28:19.674320+00:00'::timestamptz", "$1"),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$2"),
            ],
        );
        pin(
            &cancel_barrier_update_sql(1),
            &fx_sql("reap_cancel_barrier", 2),
            &[
                ("'2026-10-03 00:29:19.674320+00:00'::timestamptz", "$1"),
                ("'e7c85ddf8eda44e9a4cef965224eb682'::uuid", "$2"),
            ],
        );
        pin(
            &stale_pairs_sql(&full, false, 1),
            &fx_sql("reap_cancel_barrier", 3),
            &[
                ("'2026-10-03 00:28:19.674320+00:00'::timestamptz", "$1"),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$2"),
                ("'e7c85ddf8eda44e9a4cef965224eb682'::uuid", "$3"),
            ],
        );
        // With an in-flight exclusion bound, the IN list starts at $4.
        assert!(stale_pairs_sql(&full, true, 2).contains("IN ($4, $5)"));
        // The `(none)` detail is the barrier UPDATE's error literal.
        let update = fx_sql("reap_cancel_barrier", 6);
        assert!(
            update.contains(&reap_error_detail(None)),
            "barrier error literal"
        );
        let barrier = fx()["reap_cancel_barrier"].clone();
        assert_eq!(barrier["status_after"], "cancelled");
        assert_eq!(barrier["ended_at_set"], true);
        assert_eq!(barrier["queue_position"], Value::Null);
    }

    #[test]
    fn reap_cancel_retry_sql_and_frame_pin_to_fixture() {
        pin(
            CANCEL_REQUESTED_EXISTS_SQL,
            &fx_sql("reap_cancel_retry", 0),
            &[
                ("'d442d1559aa6497ab3f959e82f3e5369'::uuid", "$1"),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$2"),
            ],
        );
        // sql[1] is the outbox-owned session lookup — not this unit's.
        let full = reapable_statuses(false);
        for (index, build) in [
            (2, stale_cancel_ids_sql(&full, true)),
            (3, stale_cancel_pod_ids_sql(&full, true)),
            (4, stale_pairs_sql(&full, true, 0)),
        ] {
            pin(
                &build,
                &fx_sql("reap_cancel_retry", index),
                &[
                    ("'2026-10-03 00:28:19.741359+00:00'::timestamptz", "$1"),
                    ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$2"),
                    ("'d442d1559aa6497ab3f959e82f3e5369'::uuid", "$3"),
                ],
            );
        }
        // The redelivered frame equals the captured Redis payload minus mid.
        let redis = fx()["reap_cancel_retry"]["redis"].clone();
        let xadd = redis[1]["args"][1].as_str().unwrap();
        let payload_start = xadd.find("'payload': '").unwrap() + "'payload': '".len();
        let payload_end = xadd.rfind("'}").unwrap();
        let payload_text = &xadd[payload_start..payload_end];
        let payload_text = payload_text.replace("\\'", "'");
        let mut payload: Map<String, Value> =
            serde_json::from_str(&payload_text).expect("redis payload parses");
        payload.remove("mid");
        assert_eq!(
            Value::Object(cancel_retry_message("d442d155-9aa6-497a-b3f9-59e82f3e5369")),
            Value::Object(payload)
        );
        // Key order is the source dict order (v, type, run_id, reason).
        let retry = cancel_retry_message("d442d155-9aa6-497a-b3f9-59e82f3e5369");
        let keys: Vec<&str> = retry.keys().map(String::as_str).collect();
        assert_eq!(keys, ["v", "type", "run_id", "reason"]);
        assert_eq!(
            fx()["reap_cancel_retry"]["status_after"],
            "cancel_requested"
        );
    }

    #[test]
    fn reap_exclude_redeliverable_sql_pins_to_fixture() {
        let reduced = reapable_statuses(true);
        for (index, build) in [
            (0, stale_cancel_ids_sql(&reduced, false)),
            (1, stale_cancel_pod_ids_sql(&reduced, false)),
            (2, stale_pairs_sql(&reduced, false, 0)),
        ] {
            pin(
                &build,
                &fx_sql("reap_exclude_redeliverable", index),
                &[
                    ("'2026-10-03 00:28:19.758942+00:00'::timestamptz", "$1"),
                    ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$2"),
                ],
            );
        }
        let section = fx()["reap_exclude_redeliverable"].clone();
        assert_eq!(section["assigned_status_after"], "assigned");
        assert_eq!(section["poll_path_status_after"], "failed");
        // Bad poll inputs never raise (garbage ts → now, bad id → None).
        assert!(fx()["reap_bad_inputs_no_raise"].as_bool().unwrap());
        let now = fixed_now();
        assert_eq!(
            parse_heartbeat_ts(Some(&Value::from("bogus")), now),
            Ok(now)
        );
        assert_eq!(parse_in_flight_id(Some(&Value::from("bogus"))), None);
    }

    #[test]
    fn reap_finalize_call_matches_d15_boundary_and_fixture() {
        use crate::runner_runs::finalization::{
            finalize_lock_passes, finalize_update_sql, lock_run_for_finalize_sql,
        };
        let detail = reap_error_detail(Some("66ea9c4d-ef41-4d9a-9819-51e0e22d12c6"));
        let values = plan_reap_finalize(&detail);
        let columns: Vec<&str> = values.clauses.iter().map(|c| c.column).collect();
        assert_eq!(
            columns,
            [
                "status",
                "ended_at",
                "queue_position",
                "terminal_hooks_applied_at",
                "terminal_capacity_released_at",
                "error",
                "error_code"
            ]
        );
        assert_eq!(
            values.clauses[0].value,
            SetValue::Text("failed".to_string())
        );
        assert_eq!(values.clauses[5].value, SetValue::Text(detail.clone()));
        assert_eq!(
            values.clauses[6].value,
            SetValue::Text("heartbeat_reaped".to_string())
        );
        // Lock text pins (the terminal IN-list binds $2..$6; Django
        // inlines the same five values in set-hash order — order-free
        // per the D-15 contract).
        pin(
            &lock_run_for_finalize_sql(true, false),
            &fx_sql("reap_poll_path", 5),
            &[
                ("'83b95f70dd8b4233bfd11d1c616e091b'::uuid", "$1"),
                (
                    "('refused', 'failed', 'cancelled', 'completed', 'blocked')",
                    "($2, $3, $4, $5, $6)",
                ),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$7"),
            ],
        );
        // SET column order matches the captured UPDATE.
        let update = fx_sql("reap_poll_path", 6);
        let set_part = update
            .split(" SET ")
            .nth(1)
            .unwrap()
            .split(" WHERE ")
            .next()
            .unwrap();
        let set_columns: Vec<&str> = set_part
            .split(", ")
            .map(|assign| assign.split(" = ").next().unwrap().trim_matches('"'))
            .collect();
        assert_eq!(set_columns, columns);
        assert!(update.contains(&detail));
        assert!(update.contains("'heartbeat_reaped'"));
        // The rendered D-15 UPDATE binds one param per clause, id last.
        let rendered = finalize_update_sql(&values);
        assert!(rendered.ends_with("WHERE \"agent_run\".\"id\" = $8"));
        // Lock predicate for the reaper call: expected runner, no status.
        assert!(finalize_lock_passes(
            AgentRunStatus::Running,
            Some(Uuid::parse_str(FIXTURE_RUNNER).unwrap()),
            Some(Uuid::parse_str(FIXTURE_RUNNER).unwrap()),
            None
        ));
        assert!(!finalize_lock_passes(
            AgentRunStatus::Failed,
            Some(Uuid::parse_str(FIXTURE_RUNNER).unwrap()),
            Some(Uuid::parse_str(FIXTURE_RUNNER).unwrap()),
            None
        ));
    }

    #[test]
    fn reaper_scheduling_and_effects_follow_source_order() {
        // Cancel retry: in-flight + exists + poll path only.
        assert!(should_schedule_cancel_retry(true, true, false));
        assert!(!should_schedule_cancel_retry(false, true, false));
        assert!(!should_schedule_cancel_retry(true, false, false));
        assert!(!should_schedule_cancel_retry(true, true, true));
        // Drain: any reaped or barriered run.
        assert!(!should_schedule_drain(0, 0));
        assert!(should_schedule_drain(1, 0));
        assert!(should_schedule_drain(0, 1));
        // Effect order: handoffs (barrier order), runner, pods.
        let runner = Uuid::parse_str(FIXTURE_RUNNER).unwrap();
        let handoff = Uuid::parse_str("e7c85ddf-8eda-44e9-a4ce-f965224eb682").unwrap();
        let pod = Uuid::parse_str("35fbed9c-bbbc-4a45-bace-a0156b83ddc6").unwrap();
        assert_eq!(
            plan_drain_after_commit(runner, &[pod], &[handoff]),
            vec![
                SessionEffect::CompleteProjectMoveHandoff { run_id: handoff },
                SessionEffect::DrainRunner { runner_id: runner },
                SessionEffect::DrainPod { pod_id: pod },
            ]
        );
        // Pod-id collection: reaped first, then stopped, deduped, no nulls.
        assert_eq!(
            drain_pod_ids(&[Some(pod), None, Some(pod)], &[None, Some(pod)]),
            vec![pod]
        );
        // Retry error line matches the logger.exception text + Display.
        assert_eq!(
            cancel_retry_error_line("run-1", "boom"),
            "session_service: failed to redeliver cancellation for run run-1: boom"
        );
    }

    // -- live-state upsert ----------------------------------------------

    fn runner_id() -> Uuid {
        Uuid::parse_str(FIXTURE_RUNNER).unwrap()
    }

    fn live_facts(observed: Option<&str>) -> LiveStateFacts {
        LiveStateFacts {
            observed_run_id: observed.map(|s| Uuid::parse_str(s).unwrap()),
        }
    }

    fn update_clauses(plan: LiveStatePlan) -> Vec<SetClause> {
        match plan {
            LiveStatePlan::Update { set_clauses } => set_clauses,
            _ => panic!("expected Update plan"),
        }
    }

    #[test]
    fn live_state_noop_cases_run_no_sql() {
        // Empty / missing body and bodies without observability keys.
        assert!(matches!(
            plan_live_state_upsert(&live_facts(None), None, &runner_id()),
            LiveStatePlan::Noop
        ));
        assert!(matches!(
            plan_live_state_upsert(&live_facts(None), Some(&Map::new()), &runner_id()),
            LiveStatePlan::Noop
        ));
        assert!(matches!(
            plan_live_state_upsert(
                &live_facts(None),
                Some(&body(vec![("unrelated", Value::from(1))])),
                &runner_id()
            ),
            LiveStatePlan::Noop
        ));
        let noop = fx()["upsert_noop"].clone();
        assert_eq!(noop["empty_body_sql"], Value::Array(Vec::new()));
        assert_eq!(noop["no_snapshot_fields_sql"], Value::Array(Vec::new()));
        assert_eq!(noop["row_exists"], false);
        // An unchanged observed_run_id alone selects but never updates.
        assert!(matches!(
            plan_live_state_upsert(
                &live_facts(Some("bd707755-06a0-48bb-8350-9e30eac341f1")),
                Some(&str_body(vec![(
                    "observed_run_id",
                    "bd707755-06a0-48bb-8350-9e30eac341f1"
                )])),
                &runner_id()
            ),
            LiveStatePlan::NoChange
        ));
    }

    #[test]
    fn live_state_first_snapshot_pins_to_fixture() {
        let entry = body(vec![
            (
                "observed_run_id",
                Value::from("bd707755-06a0-48bb-8350-9e30eac341f1"),
            ),
            ("last_event_kind", Value::from("tool")),
            ("last_event_summary", Value::from("ran ls")),
            ("agent_pid", Value::from(4242)),
            ("agent_subprocess_alive", Value::Bool(true)),
            ("approvals_pending", Value::from(2)),
            (
                "tokens",
                serde_json::json!({"input_tokens": 100, "output_tokens": 50}),
            ),
            ("model", Value::from("claude-x")),
            ("turn_count", Value::from(9)),
        ]);
        let clauses = update_clauses(plan_live_state_upsert(
            &live_facts(None),
            Some(&entry),
            &runner_id(),
        ));
        let columns: Vec<&str> = clauses.iter().map(|c| c.column).collect();
        assert_eq!(
            columns,
            [
                "observed_run_id",
                "last_event_at",
                "last_event_kind",
                "last_event_summary",
                "agent_pid",
                "agent_subprocess_alive",
                "approvals_pending",
                "usage",
                "llm_model",
                "turn_count"
            ]
        );
        // Wipe placeholder survives only where the poll sent nothing.
        assert_eq!(clauses[1].value, SetValue::Null);
        assert_eq!(
            clauses[7].value,
            SetValue::Json(serde_json::json!({
                "input": 100,
                "output": 50,
                "total": 150,
                "raw": {"input_tokens": 100, "output_tokens": 50},
            }))
        );
        pin(
            &live_state_update_sql(&clauses),
            &fx_sql("upsert_first_snapshot", 4),
            &[
                ("'bd70775506a048bb83509e30eac341f1'::uuid", "$1"),
                ("\"last_event_kind\" = 'tool'", "\"last_event_kind\" = $2"),
                ("\"last_event_summary\" = 'ran ls'", "\"last_event_summary\" = $3"),
                ("\"agent_pid\" = 4242", "\"agent_pid\" = $4"),
                (
                    "\"agent_subprocess_alive\" = true",
                    "\"agent_subprocess_alive\" = $5",
                ),
                ("\"approvals_pending\" = 2", "\"approvals_pending\" = $6"),
                (
                    "'{\"input\": 100, \"output\": 50, \"total\": 150, \"raw\": {\"input_tokens\": 100, \"output_tokens\": 50}}'::jsonb",
                    "$7",
                ),
                ("\"llm_model\" = 'claude-x'", "\"llm_model\" = $8"),
                ("\"turn_count\" = 9", "\"turn_count\" = $9"),
                ("'2026-10-03 00:29:19.799707+00:00'::timestamptz", "$10"),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$11"),
            ],
        );
        // The persisted row matches the fixture row.
        let row = fx()["upsert_first_snapshot"]["row"].clone();
        assert_eq!(row["llm_model"], "claude-x");
        assert_eq!(row["turn_count"], 9);
        assert_eq!(row["usage"]["total"], 150);
    }

    #[test]
    fn live_state_partial_poll_never_overwrites_missing() {
        let entry = body(vec![("turn_count", Value::from(10))]);
        let clauses = update_clauses(plan_live_state_upsert(
            &live_facts(Some("bd707755-06a0-48bb-8350-9e30eac341f1")),
            Some(&entry),
            &runner_id(),
        ));
        assert_eq!(clauses.len(), 1);
        assert_eq!(clauses[0].column, "turn_count");
        pin(
            &live_state_update_sql(&clauses),
            &fx_sql("upsert_same_run_partial", 1),
            &[
                ("\"turn_count\" = 10", "\"turn_count\" = $1"),
                ("'2026-10-03 00:29:19.803287+00:00'::timestamptz", "$2"),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$3"),
            ],
        );
        assert_eq!(fx()["upsert_same_run_partial"]["row"]["turn_count"], 10);
        // …while an explicit null does overwrite.
        let entry = body(vec![("turn_count", Value::Null)]);
        let clauses = update_clauses(plan_live_state_upsert(
            &live_facts(Some("bd707755-06a0-48bb-8350-9e30eac341f1")),
            Some(&entry),
            &runner_id(),
        ));
        assert_eq!(clauses[0].value, SetValue::Null);
    }

    #[test]
    fn live_state_run_change_wipes_before_applying() {
        let entry = str_body(vec![(
            "observed_run_id",
            "a96a9859-f0af-4777-97e0-d6b69fee305c",
        )]);
        let clauses = update_clauses(plan_live_state_upsert(
            &live_facts(Some("bd707755-06a0-48bb-8350-9e30eac341f1")),
            Some(&entry),
            &runner_id(),
        ));
        assert_eq!(clauses.len(), 10);
        pin(
            &live_state_update_sql(&clauses),
            &fx_sql("upsert_run_change_wipe", 1),
            &[
                ("'a96a9859f0af477797e0d6b69fee305c'::uuid", "$1"),
                ("'{}'::jsonb", "$2"),
                ("'2026-10-03 00:29:19.807979+00:00'::timestamptz", "$3"),
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$4"),
            ],
        );
        let row = fx()["upsert_run_change_wipe"]["row"].clone();
        assert_eq!(row["usage"], serde_json::json!({}));
        assert_eq!(row["turn_count"], Value::Null);
        // A wipe followed by same-poll values overwrites in place.
        let entry = body(vec![
            (
                "observed_run_id",
                Value::from("a96a9859-f0af-4777-97e0-d6b69fee305c"),
            ),
            ("turn_count", Value::from(3)),
        ]);
        let clauses = update_clauses(plan_live_state_upsert(
            &live_facts(Some("bd707755-06a0-48bb-8350-9e30eac341f1")),
            Some(&entry),
            &runner_id(),
        ));
        let turn = clauses.iter().find(|c| c.column == "turn_count").unwrap();
        assert_eq!(turn.value, SetValue::Json(Value::from(3)));
    }

    #[test]
    fn live_state_model_truncates_and_blanks_null() {
        let trunc = fx()["upsert_model_truncation"].clone();
        let entry = str_body(vec![("model", &"m".repeat(200))]);
        let clauses = update_clauses(plan_live_state_upsert(
            &live_facts(None),
            Some(&entry),
            &runner_id(),
        ));
        assert_eq!(clauses.len(), 1);
        match &clauses[0].value {
            SetValue::Text(model) => assert_eq!(
                model.chars().count(),
                trunc["len_200_in"].as_u64().unwrap() as usize
            ),
            other => panic!("expected Text, got {other:?}"),
        }
        assert_eq!(LLM_MODEL_MAX_CHARS, 128);
        for blank in ["", "   "] {
            let entry = str_body(vec![("model", blank)]);
            let clauses = update_clauses(plan_live_state_upsert(
                &live_facts(None),
                Some(&entry),
                &runner_id(),
            ));
            assert_eq!(clauses[0].value, SetValue::Null);
        }
        assert_eq!(trunc["blank_becomes"], Value::Null);
    }

    #[test]
    fn live_state_bad_run_id_skips_with_warning() {
        let entry = body(vec![
            ("observed_run_id", Value::from("not-a-uuid")),
            ("turn_count", Value::from(5)),
        ]);
        match plan_live_state_upsert(&live_facts(None), Some(&entry), &runner_id()) {
            LiveStatePlan::SkippedInvalidRunId { warning } => assert_eq!(
                warning,
                format!(
                    "ignoring runner live-state update for {}: invalid observed_run_id 'not-a-uuid'",
                    runner_id()
                )
            ),
            _ => panic!("expected skip"),
        }
        // Only the get_or_create SELECT ran — no UPDATE.
        assert_eq!(
            fx()["upsert_bad_run_id"]["sql"].as_array().unwrap().len(),
            1
        );
        assert_eq!(fx()["upsert_bad_run_id"]["row_unchanged"], true);
        // parse_optional_uuid vectors.
        assert_eq!(parse_optional_uuid(None), Ok(None));
        assert_eq!(parse_optional_uuid(Some(&Value::Null)), Ok(None));
        let id = Uuid::parse_str("bd707755-06a0-48bb-8350-9e30eac341f1").unwrap();
        assert_eq!(
            parse_optional_uuid(Some(&Value::from("bd707755-06a0-48bb-8350-9e30eac341f1"))),
            Ok(Some(id))
        );
        assert!(parse_optional_uuid(Some(&Value::from("nope"))).is_err());
        assert!(parse_optional_uuid(Some(&Value::from(5))).is_err());
        assert!(parse_optional_uuid(Some(&Value::Bool(true))).is_err());
    }

    #[test]
    fn live_state_get_or_create_sql_pins_to_fixture() {
        pin(
            LIVE_STATE_SELECT_SQL,
            &fx_sql("upsert_first_snapshot", 0),
            &[("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$1")],
        );
        pin(
            LIVE_STATE_INSERT_SQL,
            &fx_sql("upsert_first_snapshot", 2),
            &[
                ("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$1"),
                ("'2026-10-03 00:29:19.798882+00:00'::timestamptz", "$2"),
            ],
        );
        // Column order matches the captured SELECT projection.
        let select = fx_sql("upsert_first_snapshot", 0);
        let projection = select
            .split(" FROM ")
            .next()
            .unwrap()
            .replace("SELECT ", "");
        let columns: Vec<&str> = projection
            .split(", ")
            .map(|q| q.split('.').nth(1).unwrap().trim_matches('"'))
            .collect();
        assert_eq!(columns, LIVE_STATE_COLUMNS);
        assert_eq!(
            SNAPSHOT_FIELDS,
            [
                "last_event_at",
                "last_event_kind",
                "last_event_summary",
                "agent_pid",
                "agent_subprocess_alive",
                "approvals_pending",
                "llm_model",
                "turn_count"
            ]
        );
    }

    // -- presence + slug -----------------------------------------------

    #[test]
    fn presence_sql_pins_to_fixture() {
        let marks = fx()["mark_online_offline"].clone();
        pin(
            MARK_RUNNER_ONLINE_SQL,
            marks["online_sql"][0].as_str().unwrap(),
            &[
                ("'2026-10-03 00:29:19.816829+00:00'::timestamptz", "$1"),
                ("'d6b04d1c09b246eebbf30b797d3ea9a9'::uuid", "$2"),
            ],
        );
        // Offline and revoked-excluded are the same statement.
        assert_eq!(marks["offline_sql"], marks["revoked_excluded_sql"]);
        pin(
            MARK_RUNNER_OFFLINE_SQL,
            marks["offline_sql"][0].as_str().unwrap(),
            &[("'d6b04d1c09b246eebbf30b797d3ea9a9'::uuid", "$1")],
        );
        assert_eq!(marks["online_after"]["status"], "online");
        assert_eq!(marks["online_after"]["heartbeat_set"], true);
        assert_eq!(marks["offline_after"], "offline");
        assert_eq!(marks["revoked_stays"], "revoked");
    }

    #[test]
    fn runner_slug_sql_and_predicate_pin_to_fixture() {
        pin(
            &resolve_runner_slug_sql(),
            &fx_sql("resolve_runner_project_slug", 0),
            &[("'1ad15e53f6da4da4a6319dd0796dec2f'::uuid", "$1")],
        );
        let section = fx()["resolve_runner_project_slug"].clone();
        assert_eq!(section["value"], "CT00003");
        assert_eq!(section["value"], section["expected"]);
        assert_eq!(section["podless"], Value::Null);
        let pod = Uuid::parse_str("35fbed9c-bbbc-4a45-bace-a0156b83ddc6").unwrap();
        assert_eq!(
            resolve_slug(&SlugFacts {
                row_found: true,
                pod_id: Some(pod),
                project_identifier: Some("CT00003".to_string()),
            }),
            Some("CT00003".to_string())
        );
        for facts in [
            SlugFacts {
                row_found: false,
                pod_id: Some(pod),
                project_identifier: Some("CT00003".to_string()),
            },
            SlugFacts {
                row_found: true,
                pod_id: None,
                project_identifier: Some("CT00003".to_string()),
            },
            SlugFacts {
                row_found: true,
                pod_id: Some(pod),
                project_identifier: None,
            },
        ] {
            assert_eq!(resolve_slug(&facts), None);
        }
    }

    // -- redeliver + resume ack -----------------------------------------

    #[test]
    fn skip_id_parses_or_disables() {
        assert_eq!(parse_skip_id(None), None);
        assert_eq!(parse_skip_id(Some(&Value::from(""))), None);
        assert_eq!(
            parse_skip_id(Some(&Value::from("5235E198-6AF1-40F2-A1E4-9E00A14EC2A0"))),
            Some("5235e198-6af1-40f2-a1e4-9e00a14ec2a0".to_string())
        );
        assert_eq!(parse_skip_id(Some(&Value::from("bogus"))), None);
        // Non-string JSON goes through `str()`, exactly like the
        // in-flight id (no JSON scalar's `str()` is a UUID spelling).
        assert_eq!(parse_skip_id(Some(&Value::from(7))), None);
        assert_eq!(parse_skip_id(Some(&Value::Bool(true))), None);
        assert_eq!(parse_skip_id(Some(&Value::Null)), None);
    }

    #[test]
    fn redeliver_sql_pins_to_fixture() {
        pin(
            &redeliver_cancel_sql(false),
            &fx_sql("redeliver_cancel_first", 0),
            &[("'7b1bf059c24d4a55ae1a20b8b05a160b'::uuid", "$1")],
        );
        pin(
            &redeliver_cancel_sql(true),
            &fx_sql("redeliver_skip_cancel", 0),
            &[
                ("'7b1bf059c24d4a55ae1a20b8b05a160b'::uuid", "$1"),
                ("'5235e1986af140f2a1e49e00a14ec2a0'::uuid", "$2"),
            ],
        );
        pin(
            &redeliver_assign_sql(true),
            &fx_sql("redeliver_skip_cancel", 1),
            &[
                ("'7b1bf059c24d4a55ae1a20b8b05a160b'::uuid", "$1"),
                ("'5235e1986af140f2a1e49e00a14ec2a0'::uuid", "$2"),
            ],
        );
        pin(
            &redeliver_cancel_sql(false),
            &fx_sql("redeliver_none", 0),
            &[("'eb112792fb4c44278c12b01ad18ca264'::uuid", "$1")],
        );
        pin(
            &redeliver_assign_sql(false),
            &fx_sql("redeliver_none", 1),
            &[("'eb112792fb4c44278c12b01ad18ca264'::uuid", "$1")],
        );
        assert_eq!(fx()["redeliver_none"]["msg"], Value::Null);
    }

    #[test]
    fn redeliver_messages_replay_fixture_branches() {
        // Cancel-first branch: exact message equality.
        let cancel_id = Uuid::parse_str("5235e198-6af1-40f2-a1e4-9e00a14ec2a0").unwrap();
        assert_eq!(
            plan_redeliver(&RedeliverFacts {
                cancel_run_id: Some(cancel_id),
                assign_run: None,
            })
            .map(Value::Object),
            Some(fx()["redeliver_cancel_first"]["msg"].clone())
        );
        // Assign branch: reconstructed run_config replays the frame.
        let msg = fx()["redeliver_skip_cancel"]["msg"].clone();
        let mut run_config = Map::new();
        run_config.insert("repo_url".to_string(), msg["repo_url"].clone());
        run_config.insert("repo_ref".to_string(), msg["repo_ref"].clone());
        run_config.insert(
            "git_work_branch".to_string(),
            msg["git_work_branch"].clone(),
        );
        run_config.insert("model".to_string(), msg["expected_codex_model"].clone());
        run_config.insert(
            "approval_policy_overrides".to_string(),
            msg["approval_policy_overrides"].clone(),
        );
        assert_eq!(
            plan_redeliver(&RedeliverFacts {
                cancel_run_id: None,
                assign_run: Some(RedeliverAssignFacts {
                    run_id: Uuid::parse_str("30a64411-fc3e-4217-956d-6f00ccbd30a9").unwrap(),
                    work_item_id: Some(
                        Uuid::parse_str("ee2bf318-7d95-41de-ab60-2462a29cb40a").unwrap()
                    ),
                    prompt: "do the thing".to_string(),
                    run_config,
                }),
            })
            .map(Value::Object),
            Some(msg)
        );
        // Nothing to redeliver.
        assert_eq!(
            plan_redeliver(&RedeliverFacts {
                cancel_run_id: None,
                assign_run: None,
            }),
            None
        );
        // A bad skip id still lands on the cancel branch.
        let bad = fx()["redeliver_bad_skip"].clone();
        assert_eq!(bad["msg_type"], "cancel");
        assert_eq!(bad["reason"], "cancellation_pending_on_reconnect");
    }

    #[test]
    fn resume_ack_sql_pins_and_projection_matches_l2() {
        pin(
            &resume_ack_lookup_sql(),
            &fx_sql("resume_ack_live", 0),
            &[
                ("'46970c5a6f3440f198975c7633d812ba'::uuid", "$1"),
                ("'7b1bf059c24d4a55ae1a20b8b05a160b'::uuid", "$2"),
            ],
        );
        pin(
            RESUME_ACK_LAST_SEQ_SQL,
            &fx_sql("resume_ack_live", 1),
            &[("'46970c5a6f3440f198975c7633d812ba'::uuid", "$1")],
        );
        pin(
            RESUME_ACK_LAST_SEQ_SQL,
            &fx_sql("resume_ack_live_no_events", 1),
            &[("'84b509cef7e047e88059c73ba1a5e97f'::uuid", "$1")],
        );
        // The lookup projection is the L2 agent_run column list.
        let projection = resume_ack_lookup_sql()
            .split(" FROM ")
            .next()
            .unwrap()
            .replace("SELECT ", "");
        let columns: Vec<&str> = projection
            .split(", ")
            .map(|q| q.split('.').nth(1).unwrap().trim_matches('"'))
            .collect();
        assert_eq!(columns, pidash_db::runner_runs::agent_run::COLUMNS);
    }

    #[test]
    fn resume_ack_branches_replay_fixture_messages() {
        let live_id = "46970c5a-6f34-40f1-9897-5c7633d812ba";
        // Unknown run echoes the passed id verbatim.
        assert_eq!(
            plan_resume_ack("3ef07f94-6573-45f8-a32f-3b947c58c236", None, None)
                .into_iter()
                .collect::<Map<_, _>>(),
            fx()["resume_ack_unknown_run"]["msg"]
                .as_object()
                .unwrap()
                .clone()
        );
        // Cancel-requested.
        assert_eq!(
            plan_resume_ack(
                "5235e198-6af1-40f2-a1e4-9e00a14ec2a0",
                Some(&ResumeAckFacts {
                    status: AgentRunStatus::CancelRequested,
                    thread_id: String::new(),
                }),
                None
            )
            .into_iter()
            .collect::<Map<_, _>>(),
            fx()["resume_ack_cancel_requested"]["msg"]
                .as_object()
                .unwrap()
                .clone()
        );
        // Terminal.
        assert_eq!(
            plan_resume_ack(
                "7041563b-e598-45df-90d5-4443c29329fa",
                Some(&ResumeAckFacts {
                    status: AgentRunStatus::Completed,
                    thread_id: String::new(),
                }),
                None
            )
            .into_iter()
            .collect::<Map<_, _>>(),
            fx()["resume_ack_terminal"]["msg"]
                .as_object()
                .unwrap()
                .clone()
        );
        // Live run with events.
        assert_eq!(
            plan_resume_ack(
                live_id,
                Some(&ResumeAckFacts {
                    status: AgentRunStatus::Running,
                    thread_id: "th-live".to_string(),
                }),
                Some(7)
            )
            .into_iter()
            .collect::<Map<_, _>>(),
            fx()["resume_ack_live"]["msg"].as_object().unwrap().clone()
        );
        // Live run without events: last_seq null.
        assert_eq!(
            plan_resume_ack(
                "84b509ce-f7e0-47e8-8059-c73ba1a5e97f",
                Some(&ResumeAckFacts {
                    status: AgentRunStatus::Running,
                    thread_id: String::new(),
                }),
                None
            )
            .into_iter()
            .collect::<Map<_, _>>(),
            fx()["resume_ack_live_no_events"]["msg"]
                .as_object()
                .unwrap()
                .clone()
        );
    }
}
