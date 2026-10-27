//! Issue / comment / state reads.
//!
//! Verbatim moves of the `pidash-api` handler helpers (OVL-05,
//! PIDASHCONV-1543); the handlers call these and map [`Error`](super::Error)
//! onto their denial type.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::{DbError, Error};
use crate::orchestration::blockers::{
    has_open_blockers_sql, relations_summary, summary_sql, BlockerRow, RelationsSummary,
};

/// The cached blocker summary (`relations_summary(obj)`): both direction
/// lists plus the open-blockers `Exists`, in that order. Moved from
/// `api::app_issues::handlers_core`.
pub async fn fetch_blocker_summary(
    pool: &PgPool,
    issue_id: &Uuid,
) -> Result<RelationsSummary, Error> {
    let fetch_list = async |blocking: bool| -> Result<Vec<BlockerRow>, Error> {
        let sql = summary_sql(blocking).replace(":issue_id", "$1");
        type BlockerTuple = (Uuid, i32, String, Option<String>, Option<String>);
        let rows: Vec<BlockerTuple> = sqlx::query_as(&sql)
            .bind(issue_id)
            .fetch_all(pool)
            .await
            .map_err(|error| DbError::new("fetch-blocker-summary", error))?;
        Ok(rows
            .into_iter()
            .map(
                |(issue_id, sequence_id, project_identifier, state_name, state_group)| BlockerRow {
                    issue_id,
                    sequence_id,
                    project_identifier,
                    state_name,
                    state_group,
                },
            )
            .collect())
    };
    let blocked_by = fetch_list(false).await?;
    let blocking = fetch_list(true).await?;
    let open_sql = has_open_blockers_sql().replace(":issue_id", "$1");
    let open: Option<(i32,)> = sqlx::query_as(&open_sql)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| DbError::new("fetch-blocker-summary-open", error))?;
    Ok(relations_summary(&blocked_by, &blocking, open.is_some()))
}

/// `IssueCommentSerializer.get_url` (`serializers/issue.py:994-999`):
/// `issue_web_url(workspace.slug, project.identifier, issue.sequence_id)`
/// over the ROW's FKs (fetched, never the path params). Moved from
/// `v1_work_items::handlers_social`.
pub async fn comment_url(
    pool: &PgPool,
    workspace_id: &Uuid,
    project_id: &Uuid,
    issue_id: &Uuid,
    web_base: Option<&str>,
) -> Result<Option<String>, Error> {
    let slug: Option<String> =
        sqlx::query_scalar(r#"SELECT "slug" FROM "workspaces" WHERE "id" = $1"#)
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| DbError::new("comment-url-slug", error))?
            .flatten();
    let identifier: Option<String> =
        sqlx::query_scalar(r#"SELECT "identifier" FROM "projects" WHERE "id" = $1"#)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| DbError::new("comment-url-ident", error))?
            .flatten();
    let sequence: Option<i32> =
        sqlx::query_scalar(r#"SELECT "sequence_id" FROM "issues" WHERE "id" = $1"#)
            .bind(issue_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| DbError::new("comment-url-seq", error))?
            .flatten();
    Ok(crate::v1_work_items::shape_issue::issue_url(
        web_base,
        slug.as_deref(),
        identifier.as_deref(),
        sequence.map(i64::from),
    ))
}

/// The NULL-state arm of `Issue.save` (`db/models/issue.py:288-308`):
/// default non-triage state first, else the first non-triage state, both
/// by `Meta.ordering` (`sequence`); with no states at all the issue keeps
/// its NULL state. `completed_at` is untouched in every NULL-state case —
/// the recompute runs only when the issue already had a state (the `else`
/// arm, `issue.py:300-308`). Moved from
/// `v1_work_items::handlers_relations`.
pub async fn resolve_workpad_state(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: &Uuid,
    state_id: Option<Uuid>,
    state_group: Option<&str>,
    completed_at: Option<DateTime<Utc>>,
) -> Result<(Option<Uuid>, Option<DateTime<Utc>>), Error> {
    if let Some(state_id) = state_id {
        let group = state_group.unwrap_or_default();
        if group == "completed" {
            return Ok((Some(state_id), Some(now_utc())));
        }
        return Ok((Some(state_id), None));
    }
    let found: Option<(Uuid, String)> = sqlx::query_as(
        r#"SELECT "id", "group" FROM "states"
           WHERE "deleted_at" IS NULL AND "group" != 'triage' AND NOT ("is_triage")
             AND "project_id" = $1 AND "default"
           ORDER BY "sequence" LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(&mut **txn)
    .await
    .map_err(|error| DbError::new("workpad-default-state", error))?;
    let found = match found {
        Some(found) => Some(found),
        None => sqlx::query_as(
            r#"SELECT "id", "group" FROM "states"
                   WHERE "deleted_at" IS NULL AND "group" != 'triage' AND NOT ("is_triage")
                     AND "project_id" = $1
                   ORDER BY "sequence" LIMIT 1"#,
        )
        .bind(project_id)
        .fetch_optional(&mut **txn)
        .await
        .map_err(|error| DbError::new("workpad-random-state", error))?,
    };
    // The `completed_at` recompute lives in the `else` arm of the
    // `self.state is None` branch (`issue.py:300-308`): a stateless issue
    // keeps its `completed_at` untouched whether or not a state is found.
    match found {
        Some((id, _group)) => Ok((Some(id), completed_at)),
        // No states at all: `self.state` stays `None`, `completed_at`
        // untouched (the `else` arm never runs).
        None => Ok((None, completed_at)),
    }
}

/// The workpad-PATCH `description_stripped` recompute
/// (`db/models/issue.py:346-350`): `None` for empty HTML, else
/// `html_processor.strip_tags` — the entity-decoding MLStripper
/// (`pidash_db::app_pages::strip::ml_strip_tags`), NOT Django's
/// verbatim-entity kernel. Moved from
/// `v1_work_items::handlers_relations`.
pub fn workpad_stripped(description_html: &str) -> Option<String> {
    if description_html.is_empty() {
        None
    } else {
        Some(pidash_db::app_pages::strip::ml_strip_tags(description_html))
    }
}

/// `timezone.now()` truncated to microseconds: Python datetimes carry no
/// nanos, and `timestamptz` stores micros — an untruncated `Utc::now()`
/// would render nanos in the response while the DB row reads back micros.
/// (Moved with [`resolve_workpad_state`]; the handler keeps its own copy
/// for its other callers.)
fn now_utc() -> DateTime<Utc> {
    trunc_micros(Utc::now())
}

/// Truncate an instant to microsecond precision (see [`now_utc`]).
fn trunc_micros(dt: DateTime<Utc>) -> DateTime<Utc> {
    let nanos = dt.timestamp_subsec_nanos();
    dt - chrono::Duration::nanoseconds(i64::from(nanos % 1000))
}
