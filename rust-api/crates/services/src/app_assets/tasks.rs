//! App-asset background tasks: sweep query + metadata publisher payloads (D-31, stage 5).
//!
//! Port of `apps/api/pi_dash/bgtasks/file_asset_task.py` (26 lines) and the
//! three in-scope `get_asset_object_metadata.delay(...)` publisher call sites
//! in `apps/api/pi_dash/app/views/asset/v2.py:177,386,587`.
//!
//! Everything here is pure: environment parsing, the sweep cutoff, the sweep
//! SQL text and the Celery kwargs the publishers enqueue. Database access, S3
//! calls and queue writes live behind the jobs layer, so tests replay the
//! fixture golden (`rust-api/fixtures/app_assets/tasks/file_asset.golden.json`)
//! with no database.
//!
//! # Scope (translate, don't redesign)
//!
//! * The sweep query is shared with D-09: [`SWEEP_SQL`] and
//!   [`parse_delete_days`] are re-exported from
//!   [`crate::tasks_cleanup::assets`] so the two domains can never fork.
//!   What is D-31-owned here is the string-default resolver
//!   ([`sweep_lookback_days`]), the cutoff ([`sweep_cutoff`]) and the
//!   publisher payloads below.
//! * `get_asset_object_metadata` task-body ownership stays with whichever
//!   domain ports it first — D-09 already did
//!   (`pidash-jobs` `tasks_cleanup::assets::register_metadata`). This module
//!   records the task name and the publisher kwargs only, never the body.
//! * No new queue tables: publishers enqueue through the existing
//!   Postgres-backed queue in Celery protocol v2.
//!
//! # Publisher call sites (all `if not asset.storage_metadata` guarded)
//!
//! | Call site | Python | Kwargs |
//! | --- | --- | --- |
//! | `UserAssetsV2Endpoint.patch` | `v2.py:177` `get_asset_object_metadata.delay(asset_id=str(asset_id))` | `{"asset_id": str}` |
//! | `WorkspaceFileAssetEndpoint.patch` | `v2.py:386` `get_asset_object_metadata.delay(asset_id=str(asset_id))` | `{"asset_id": str}` |
//! | `ProjectAssetEndpoint.patch` | `v2.py:587` `get_asset_object_metadata.delay(asset_id=str(pk))` | `{"asset_id": str}` |
//!
//! The project site passes the URL kwarg `pk` (which equals `asset.id`),
//! not a re-read — kept as-is. `.delay(kwarg=...)` enqueues with empty args
//! and these kwargs under the task name [`TASK_GET_ASSET_OBJECT_METADATA`].
//!
//! # Ported semantics (translate, don't redesign)
//!
//! * The sweep never hard-deletes: `.delete()` on the default manager is
//!   `SoftDeletionQuerySet.delete` → `UPDATE file_assets SET deleted_at`
//!   scoped to `deleted_at IS NULL` (`db/mixins.py:49-53`), even though the
//!   task docstring says "deletes" (`file_asset_task.py:22`). Port the
//!   manager behaviour, not the docstring.
//! * BUG (port as-is, `file_asset_task.py:24`):
//!   `os.environ.get("UNUPLOADED_ASSET_DELETE_DAYS", "7")` defaults to the
//!   STRING `"7"`, then `int()` parses it. An UNSET var and a var set to
//!   `"7"` behave identically; a non-numeric value raises `ValueError` and
//!   the task errors (no swallow). [`sweep_lookback_days`] resolves through
//!   the string default verbatim.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

/// Sweep predicate and Python `int()` parsing, shared with D-09.
///
/// Re-exported (not copied) so the two domains pin the same SQL text and the
/// same cutoff parsing; see [`crate::tasks_cleanup::assets`].
pub use crate::tasks_cleanup::assets::{parse_delete_days, SWEEP_SQL};

/// `os.environ` key for the sweep cutoff (`file_asset_task.py:24`).
pub const UNUPLOADED_ASSET_DELETE_DAYS_ENV: &str = "UNUPLOADED_ASSET_DELETE_DAYS";

/// Default when the env var is absent, as the STRING `"7"`
/// (`file_asset_task.py:24` — BUG, ported as-is).
pub const UNUPLOADED_ASSET_DELETE_DAYS_DEFAULT_STR: &str = "7";

/// Celery task name for the sweep (`file_asset_task.py:20-21`).
pub const TASK_DELETE_UNUPLOADED_FILE_ASSET: &str =
    "pi_dash.bgtasks.file_asset_task.delete_unuploaded_file_asset";

/// Celery task name for the metadata publishers (`storage_metadata_task.py:14`).
///
/// The body lives in D-09 (`tasks_cleanup::assets::TASK_GET_METADATA`, same
/// string); this const names what the three v2 publishers enqueue.
pub const TASK_GET_ASSET_OBJECT_METADATA: &str =
    "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata";

/// Port of `int(os.environ.get("UNUPLOADED_ASSET_DELETE_DAYS", "7"))`
/// (`file_asset_task.py:24`).
///
/// `None` (var absent) resolves through the string default `"7"`, parsed by
/// [`parse_delete_days`] exactly like any explicitly set value — so an UNSET
/// var and `"7"` behave identically, and a non-numeric value errors.
pub fn sweep_lookback_days(env: Option<&str>) -> Result<i64, String> {
    parse_delete_days(env.unwrap_or(UNUPLOADED_ASSET_DELETE_DAYS_DEFAULT_STR))
}

/// Port of `timezone.now() - timedelta(days=...)` (`file_asset_task.py:24`).
///
/// Returns the sweep cutoff: rows with `created_at` strictly before this are
/// in the sweep set. `$2` in [`SWEEP_SQL`]; `$1` is the sweep time stamped
/// into `deleted_at`.
pub fn sweep_cutoff(now: DateTime<Utc>, days: i64) -> DateTime<Utc> {
    now - chrono::Duration::days(days)
}

/// Port of the `.delay(asset_id=str(...))` kwargs at `v2.py:177,386,587`.
///
/// Every in-scope publisher enqueues empty args with exactly
/// `{"asset_id": "<str(uuid)>"}` under [`TASK_GET_ASSET_OBJECT_METADATA`].
/// The falsy-`storage_metadata` guard lives at the call sites (handlers),
/// not here.
pub fn metadata_delay_kwargs(asset_id: &str) -> Map<String, Value> {
    let mut kwargs = Map::with_capacity(1);
    kwargs.insert("asset_id".to_owned(), Value::String(asset_id.to_owned()));
    kwargs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Committed evidence replayed here without a database:
    /// `rust-api/fixtures/app_assets/tasks/file_asset.golden.json`.
    static FIXTURE: &str =
        include_str!("../../../../fixtures/app_assets/tasks/file_asset.golden.json");

    #[test]
    fn fixture_names_the_python_sources() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        let trace = fixture["_trace"].as_str().expect("trace line");
        assert!(trace.contains("bgtasks/file_asset_task.py"));
        assert!(trace.contains("bgtasks/storage_metadata_task.py"));
        assert!(trace.contains("app/views/asset/v2.py:177,386,587"));
    }

    #[test]
    fn env_default_resolves_through_the_string_7() {
        // BUG ported as-is (file_asset_task.py:24): UNSET behaves exactly
        // like an explicit "7" because the default is the string "7".
        assert_eq!(sweep_lookback_days(None).unwrap(), 7);
        assert_eq!(
            sweep_lookback_days(None).unwrap(),
            parse_delete_days(UNUPLOADED_ASSET_DELETE_DAYS_DEFAULT_STR).unwrap()
        );
        assert_eq!(sweep_lookback_days(Some("7")).unwrap(), 7);
        // Non-numeric values raise in Python (no swallow): they error here.
        for bad in ["", "  ", "3.0", "thirty"] {
            assert!(
                sweep_lookback_days(Some(bad)).is_err(),
                "{bad:?} must fail like int()"
            );
        }
    }

    #[test]
    fn sweep_sql_is_the_shared_soft_delete_statement() {
        // Never forked from D-09: identical text, soft UPDATE (deleted_at
        // stamp scoped to deleted_at IS NULL), never a physical DELETE.
        assert_eq!(SWEEP_SQL, crate::tasks_cleanup::assets::SWEEP_SQL);
        assert!(SWEEP_SQL.starts_with("UPDATE \"file_assets\" SET \"deleted_at\""));
        assert!(!SWEEP_SQL.contains("DELETE FROM"));
        assert!(SWEEP_SQL.contains("\"deleted_at\" IS NULL"));
        assert!(SWEEP_SQL.contains("\"created_at\" < $2"));
        assert!(SWEEP_SQL.contains("NOT \"file_assets\".\"is_uploaded\""));
    }

    #[test]
    fn sweep_cutoff_subtracts_whole_days() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-29T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            sweep_cutoff(now, 7),
            chrono::DateTime::parse_from_rfc3339("2026-09-22T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
        assert_eq!(sweep_cutoff(now, 0), now);
    }

    #[test]
    fn publisher_payloads_match_the_three_v2_call_sites() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture parses");
        let publishers = fixture["in_scope_publishers"]
            .as_array()
            .expect("publisher table");
        assert_eq!(publishers.len(), 3);
        // Task name pins the Celery name the v2 publishers enqueue under.
        // (D-09's worker registers the same string for the body; the
        // services crate cannot reference the jobs crate, so the literal
        // is pinned here instead of imported.)
        assert_eq!(
            TASK_GET_ASSET_OBJECT_METADATA,
            "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata"
        );
        // Every site enqueues exactly {"asset_id": "<str>"} — byte for byte.
        for (publisher, id) in publishers.iter().zip([
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
            "33333333-3333-3333-3333-333333333333",
        ]) {
            let call = publisher["call"].as_str().expect("call shape");
            assert!(
                call.starts_with("get_asset_object_metadata.delay(asset_id=str("),
                "unexpected call shape: {call}"
            );
            let kwargs = metadata_delay_kwargs(id);
            assert_eq!(kwargs.len(), 1);
            assert_eq!(Value::Object(kwargs), serde_json::json!({"asset_id": id}));
        }
    }

    #[test]
    fn sweep_task_name_matches_beat_and_delay() {
        assert_eq!(
            TASK_DELETE_UNUPLOADED_FILE_ASSET,
            "pi_dash.bgtasks.file_asset_task.delete_unuploaded_file_asset"
        );
    }
}
