#![forbid(unsafe_code)]

//! Shared `project_archived` helper (NEWPIDASH-236).
//!
//! Every project-scoped issue read/write on an archived project answers
//! `409` with code `project_archived` instead of the old system's silent
//! `200` empty list / `404`. Workspace-wide issue lists and search stay
//! silent (they are not scoped to one project) and never call this.
//! NEWPIDASH-240 (pages) and NEWPIDASH-239 (scheduler run-now) reuse
//! this module; keep the envelope in one place.
//!
//! The check runs after authentication (401) and the permission gate
//! (403) and before any issue query, so a non-member still gets 403.
//! Archive state is read from `projects.archived_at` — no schema change.
//! A soft-deleted project (`deleted_at IS NULL` fails) is never archived
//! here: the resolve paths 404 for it, so the check skips it too. Both
//! probes are SeaORM (`select_only` + `into_model`): two-column narrow
//! reads by name, so a new column cannot stale this gate.

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use pidash_db::entity::{projects, workspaces};
use sea_orm::{
    ColumnTrait, EntityTrait, FromQueryResult, JoinType, QueryFilter, QuerySelect, RelationTrait,
};
use sqlx::PgPool;
use uuid::Uuid;

/// A project that is archived: its identifier and the archive instant.
#[derive(Debug, Clone)]
pub struct ArchivedProject {
    pub identifier: String,
    pub archived_at: DateTime<Utc>,
}

/// Render the `archived_at` instant as `2026-11-13T18:57:37Z`.
pub fn format_archived_at(archived_at: &DateTime<Utc>) -> String {
    archived_at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// The exact 409 JSON body. `error` carries the human message with the
/// identifier and timestamp plus the unarchive recovery hint; `code`,
/// `project`/`project_identifier` and `archived_at` are machine-readable.
pub fn archived_body(info: &ArchivedProject) -> String {
    let ts = format_archived_at(&info.archived_at);
    let message = format!(
        "Project {} is archived (since {}). Unarchive it to read or change its work items.",
        info.identifier, ts
    );
    let identifier = serde_json::to_string(&info.identifier).expect("json string");
    let message = serde_json::to_string(&message).expect("json string");
    let ts = serde_json::to_string(&ts).expect("json string");
    format!(
        "{{\"error\":{message},\"code\":\"project_archived\",\"project\":{identifier},\"project_identifier\":{identifier},\"archived_at\":{ts}}}"
    )
}

/// One row of the gate probe: the project's identifier and archive instant.
#[derive(Clone, Debug, PartialEq, FromQueryResult)]
struct ArchivedRow {
    identifier: String,
    archived_at: Option<DateTime<FixedOffset>>,
}

fn from_row(row: ArchivedRow) -> Option<ArchivedProject> {
    row.archived_at.map(|archived_at| ArchivedProject {
        identifier: row.identifier,
        archived_at: archived_at.with_timezone(&Utc),
    })
}

/// Fetch the archived project by UUID, if it is archived.
pub async fn archived_by_id(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Option<ArchivedProject>, sqlx::Error> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = projects::Entity::find()
        .select_only()
        .columns([projects::Column::Identifier, projects::Column::ArchivedAt])
        .filter(projects::Column::Id.eq(project_id))
        .filter(projects::Column::DeletedAt.is_null())
        .filter(projects::Column::ArchivedAt.is_not_null())
        .into_model::<ArchivedRow>()
        .one(&db)
        .await
        .map_err(pidash_db::orm_tx::into_sqlx)?;
    Ok(row.and_then(from_row))
}

/// Fetch the archived project addressed by identifier inside a workspace,
/// if it is archived. The identifier match follows `Project.resolve`:
/// `strip().upper()` equality scoped to `workspace.slug` with
/// `deleted_at IS NULL`. Calls it with the already-resolved UUID should
/// instead use [`archived_by_id`]; this entry exists for the
/// by-identifier `PROJ-123` path where only the identifier (plus the
/// issue part) is known before the project fetch.
pub async fn archived_by_identifier(
    pool: &PgPool,
    workspace_slug: &str,
    project_identifier: &str,
) -> Result<Option<ArchivedProject>, sqlx::Error> {
    let upper = project_identifier.trim().to_uppercase();
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = projects::Entity::find()
        .select_only()
        .columns([projects::Column::Identifier, projects::Column::ArchivedAt])
        .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
        .filter(workspaces::Column::Slug.eq(workspace_slug))
        .filter(projects::Column::Identifier.eq(upper))
        .filter(projects::Column::DeletedAt.is_null())
        .filter(projects::Column::ArchivedAt.is_not_null())
        .into_model::<ArchivedRow>()
        .one(&db)
        .await
        .map_err(pidash_db::orm_tx::into_sqlx)?;
    Ok(row.and_then(from_row))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_matches_issue_example() {
        let info = ArchivedProject {
            identifier: "NEWFRONT".to_owned(),
            archived_at: DateTime::parse_from_rfc3339("2026-11-13T18:57:37+00:00")
                .expect("parse")
                .with_timezone(&Utc),
        };
        let body = archived_body(&info);
        assert_eq!(
            body,
            r#"{"error":"Project NEWFRONT is archived (since 2026-11-13T18:57:37Z). Unarchive it to read or change its work items.","code":"project_archived","project":"NEWFRONT","project_identifier":"NEWFRONT","archived_at":"2026-11-13T18:57:37Z"}"#
        );
    }
}
