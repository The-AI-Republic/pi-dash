//! `core::relations::{relate, unrelate}` (NEWPIDASH-190 §4.3): the
//! shared relation check (`check_targets`: self, cross-workspace
//! refusal, dedupe), per-target pair fetch, verdict loop (create /
//! unchanged / conflicts for relate; removed / not-related for
//! unrelate) and the guarded single-statement inserts
//! (`relate_row_or_nothing`) plus soft deletes.
//!
//! The resolution stays with the wrappers: the operations path
//! resolves targets from the actor's visible pool while the agent
//! resolves source + targets from its run scope — both already
//! resolve to `ResolvedIssue`, so the check and the loop below are
//! external-input free. The `pair_exact`/`raw_targets`/`newest_only`
//! shapes stay in `RelationBatch` for the wrappers' resolve and
//! check legs (the verdict loop is verdict-equivalent across them).
//!
//! The relation activity emission is a post-batch step and the two
//! wrappers build different encodings, so each emits its own after
//! the outcome (unchanged payloads). `grouped_relations` stays with
//! the wrappers too (Phase 3 moves it into the shared reads).
use chrono::Utc;
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, FromQueryResult, QueryFilter, QueryOrder, QuerySelect,
};
use uuid::Uuid;

use crate::assistant::tools_issues::actual_relation;
use crate::domain::relations as relation_domain;
use crate::orchestration::relations::{
    relate_insert_row, relate_verdict, type_from, PairRow, RelateConflict, RelateVerdict,
    ResolvedIssue,
};
use pidash_db::entity::issue_relations;

use super::CoreError;

/// The resolved source of a batch: both wrappers resolve to the
/// same five fields (`id`, `project_id`, `workspace_id`,
/// `project_identifier`, `sequence_id`) from their respective
/// resolution predicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSource {
    pub id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub project_identifier: String,
    pub sequence_id: i32,
}

impl CoreSource {
    pub fn identifier(&self) -> String {
        format!("{}-{}", self.project_identifier, self.sequence_id)
    }
}

/// A resolved target of a batch — the back-bridge across the two
/// wrappers' resolution shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTarget {
    pub id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub project_identifier: String,
    pub sequence_id: i32,
}

impl CoreTarget {
    pub fn identifier(&self) -> String {
        format!("{}-{}", self.project_identifier, self.sequence_id)
    }
}

impl From<&ResolvedIssue> for CoreTarget {
    fn from(value: &ResolvedIssue) -> Self {
        Self {
            id: value.id,
            project_id: value.project_id,
            workspace_id: value.workspace_id,
            project_identifier: value.project_identifier.clone(),
            sequence_id: value.sequence_id,
        }
    }
}

impl From<&crate::operations::relations::ResolvedIssue> for CoreTarget {
    fn from(value: &crate::operations::relations::ResolvedIssue) -> Self {
        Self {
            id: value.id,
            project_id: value.project_id,
            workspace_id: value.workspace_id,
            project_identifier: value.project_identifier.clone(),
            sequence_id: value.sequence_id,
        }
    }
}

pub fn check_targets(
    source: &CoreSource,
    targets: &[CoreTarget],
    raw_targets: bool,
) -> Result<Vec<CoreTarget>, CoreError> {
    let mut checked = Vec::with_capacity(targets.len());
    for target in targets {
        // `relations.py:166-170` — self-relations are refused.
        // With `raw_targets`, self ids and duplicate ids are
        // processed per-iteration instead (the MCP loop over the raw
        // `related_issue_ids`); the cross-workspace refusal still
        // runs.
        if !raw_targets && target.id == source.id {
            return Err(CoreError::Request(format!(
                "{} cannot be related to itself",
                source.identifier()
            )));
        }
        // `relations.py:171-173` — only targets in the source's
        // workspace are allowed.
        if target.workspace_id != source.workspace_id {
            return Err(CoreError::Request(format!(
                "Issue {} is in a different workspace",
                target.identifier()
            )));
        }
        if raw_targets || !checked.iter().any(|seen: &CoreTarget| seen.id == target.id) {
            checked.push(target.clone());
        }
    }
    Ok(checked)
}

impl From<&ResolvedIssue> for CoreSource {
    fn from(value: &ResolvedIssue) -> Self {
        Self {
            id: value.id,
            project_id: value.project_id,
            workspace_id: value.workspace_id,
            project_identifier: value.project_identifier.clone(),
            sequence_id: value.sequence_id,
        }
    }
}

impl From<&crate::operations::relations::SourceIssue> for CoreSource {
    fn from(value: &crate::operations::relations::SourceIssue) -> Self {
        Self {
            id: value.id,
            project_id: value.project_id,
            workspace_id: value.workspace_id,
            project_identifier: value.project_identifier.clone(),
            sequence_id: value.sequence_id,
        }
    }
}

/// One relate/unrelate batch.
#[derive(Debug, Clone)]
pub struct RelationBatch<'a> {
    /// The source issue, pre-resolved by the wrapper.
    pub source: &'a CoreSource,
    /// The resolved targets (same order as the request).
    pub targets: &'a [CoreTarget],
    /// The validated relation type (`validate_relation_type` first
    /// strips/lowercases server-side).
    pub relation_type: &'a str,
    /// The requesting actor (created_by stamps).
    pub actor_id: Uuid,
    /// Whether the wrapper narrowed source resolution to its own
    /// project. Slices in the resolution is caller state; the batch
    /// carries it so a reader can see the intent, and the batch loop
    /// never narrows on its own.
    pub own_project: bool,
    /// Whether the write keeps pair exactness (ops relate shape).
    pub pair_exact: bool,
    /// Whether raw (unnested) targets are allowed (ops relate shape).
    pub raw_targets: bool,
    /// Whether only the first matching row is unrelated (ops unrelate
    /// shape truncates per target).
    pub newest_only: bool,
}

/// `RelateOutcome { created, unchanged, conflicts, rows_affected }`.
#[derive(Debug, Clone)]
pub struct RelateOutcome {
    /// Created target issues, in request order (the ordered rows).
    pub created: Vec<CoreTarget>,
    /// Already-carrying target identifiers, in request order.
    pub unchanged: Vec<String>,
    /// Conflicting pairs with their `existing_relation` from source side.
    pub conflicts: Vec<RelateConflict>,
    /// The ordered relation row ids written (`rows_affected` for the
    /// per-target atomicity fence).
    pub rows_affected: Vec<Uuid>,
}

/// `UnrelateOutcome { removed, not_related }`.
#[derive(Debug, Clone)]
pub struct UnrelateOutcome {
    /// Soft-deleted target identifiers, in request order.
    pub removed: Vec<String>,
    /// The corresponding removed target ids (parallel to `removed`).
    pub removed_ids: Vec<Uuid>,
    /// Requested targets with no matching row, in request order.
    pub not_related: Vec<String>,
    /// The ordered relation row ids deleted (`rows_affected`).
    pub rows_affected: Vec<Uuid>,
}

/// `core::relations::relate(db, batch)` (§4.3).
pub async fn relate<C: sea_orm::ConnectionTrait>(
    db: &C,
    batch: &RelationBatch<'_>,
) -> Result<RelateOutcome, CoreError> {
    // `_check_targets` (rejects self in fact to `raw_targets`).
    let checked = check_targets(batch.source, batch.targets, batch.raw_targets)?;
    let mut created = Vec::new();
    let mut unchanged = Vec::new();
    let mut conflicts = Vec::new();
    let mut rows_affected = Vec::new();
    for target in &checked {
        let existing = related_fetch(db, batch.source.id, target.id).await?;
        if batch.pair_exact {
            // MCP `get_or_create(issue, related_issue)`
            // (`pi_dash_cloud/mcp/tools.py:664-675`): the lookup names
            // the exact oriented pair and no type, so a live row on
            // this orientation — any stored type — means "skipped"
            // (reported under `unchanged`), and cross-orientation rows
            // are invisible. The stored edge is the same triple the
            // default path writes (MCP's swap set and canonical type
            // match [`stored_edge`]).
            let (issue_id, related_id, stored) =
                stored_edge(batch.source.id, batch.relation_type, target.id);
            let oriented_live = existing
                .iter()
                .any(|row| row.issue_id == issue_id && row.related_issue_id == related_id);
            if oriented_live {
                unchanged.push(target.identifier());
                continue;
            }
            if insert_for(db, batch, issue_id, related_id, &stored).await? {
                created.push(target.clone());
                continue;
            }
            unchanged.push(target.identifier());
            continue;
        }
        match relate_verdict(&existing, &batch.source.id, batch.relation_type) {
            RelateVerdict::Unchanged => {
                unchanged.push(target.identifier());
            }
            RelateVerdict::Conflict { existing_relation } => {
                conflicts.push(RelateConflict {
                    identifier: target.identifier(),
                    existing_relation,
                });
            }
            RelateVerdict::Create => {
                let row = relate_insert_row(
                    Uuid::new_v4(),
                    &ResolvedIssue {
                        id: batch.source.id,
                        workspace_id: batch.source.workspace_id,
                        project_id: batch.source.project_id,
                        project_identifier: batch.source.project_identifier.clone(),
                        sequence_id: batch.source.sequence_id,
                    },
                    batch.relation_type,
                    &ResolvedIssue {
                        id: target.id,
                        workspace_id: target.workspace_id,
                        project_id: target.project_id,
                        project_identifier: target.project_identifier.clone(),
                        sequence_id: target.sequence_id,
                    },
                    batch.actor_id,
                );
                let value = relation_domain::RelationRow {
                    id: row.id,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    created_by: Some(row.created_by_id),
                    updated_by: row.updated_by_id,
                    project_id: row.project_id,
                    workspace_id: row.workspace_id,
                    issue_id: row.issue_id,
                    related_issue_id: row.related_issue_id,
                    relation_type: row.relation_type.as_str(),
                };
                let inserted = relation_domain::relate_row_or_nothing(db, &value).await?;
                if inserted {
                    rows_affected.push(row.id);
                    created.push(target.clone());
                }
            }
        }
    }
    Ok(RelateOutcome {
        created,
        unchanged,
        conflicts,
        rows_affected,
    })
}

async fn insert_for<C: sea_orm::ConnectionTrait>(
    db: &C,
    batch: &RelationBatch<'_>,
    issue_id: Uuid,
    related_id: Uuid,
    stored: &str,
) -> Result<bool, CoreError> {
    let row = crate::domain::relations::RelationRow {
        id: Uuid::new_v4(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        created_by: Some(batch.actor_id),
        updated_by: None,
        project_id: batch.source.project_id,
        workspace_id: batch.source.workspace_id,
        issue_id,
        related_issue_id: related_id,
        relation_type: stored,
    };
    relation_domain::relate_row_or_nothing(db, &row)
        .await
        .map_err(CoreError::Sqlx)
}

/// `core::relations::unrelate(db, batch)` (§4.3).
pub async fn unrelate<C: sea_orm::ConnectionTrait>(
    db: &C,
    batch: &RelationBatch<'_>,
) -> Result<UnrelateOutcome, CoreError> {
    let checked = check_targets(batch.source, batch.targets, batch.raw_targets)?;
    let mut removed = Vec::new();
    let mut removed_ids = Vec::new();
    let mut not_related = Vec::new();
    let mut rows_affected = Vec::new();
    for target in &checked {
        let existing = related_fetch(db, batch.source.id, target.id).await?;
        // Self targets only arrive in raw mode (the default refuses
        // them in `check_targets`): the MCP queryset names the stored
        // endpoints and the canonical type
        // (`pi_dash_cloud/mcp/tools.py:726-732`), so a reverse-named
        // request (`blocking`) still matches the canonically stored
        // self row (`blocked_by`) — the viewpoint classifier would
        // only echo the stored name from the issue side.
        let self_target = target.id == batch.source.id;
        let mut matching: Vec<&PairRow> = existing
            .iter()
            .filter(|row| {
                if self_target {
                    row.relation_type == actual_relation(batch.relation_type)
                } else {
                    type_from(row, &batch.source.id) == batch.relation_type
                }
            })
            .collect();
        if matching.is_empty() {
            not_related.push(target.identifier());
            continue;
        }
        // `fetch_pair_rows` returns newest-first (the model's
        // `-created_at` ordering), so truncating to one keeps the MCP
        // `.first()` row.
        if batch.newest_only {
            matching.truncate(1);
        }
        for row in matching {
            let now = Utc::now();
            relation_domain::soft_delete_relation_row(db, row.id, batch.actor_id, now, now).await?;
            rows_affected.push(row.id);
        }
        removed.push(target.identifier());
        removed_ids.push(target.id);
    }
    Ok(UnrelateOutcome {
        removed,
        removed_ids,
        not_related,
        rows_affected,
    })
}

/// `_pair_rows` (`relations.py:117-120`) over the shared connection.
async fn related_fetch<C: sea_orm::ConnectionTrait>(
    db: &C,
    a_id: Uuid,
    b_id: Uuid,
) -> Result<Vec<PairRow>, CoreError> {
    #[derive(Debug, Clone, PartialEq, Eq, FromQueryResult)]
    struct StoredRow {
        id: Uuid,
        issue_id: Uuid,
        related_issue_id: Uuid,
        relation_type: String,
    }
    let rows = issue_relations::Entity::find()
        .select_only()
        .columns([
            issue_relations::Column::Id,
            issue_relations::Column::IssueId,
            issue_relations::Column::RelatedIssueId,
            issue_relations::Column::RelationType,
        ])
        .filter(issue_relations::Column::DeletedAt.is_null())
        .filter(
            Condition::any()
                .add(
                    Condition::all()
                        .add(issue_relations::Column::IssueId.eq(a_id))
                        .add(issue_relations::Column::RelatedIssueId.eq(b_id)),
                )
                .add(
                    Condition::all()
                        .add(issue_relations::Column::IssueId.eq(b_id))
                        .add(issue_relations::Column::RelatedIssueId.eq(a_id)),
                ),
        )
        .order_by_desc(issue_relations::Column::CreatedAt)
        .into_model::<StoredRow>()
        .all(db)
        .await
        .map_err(|error| CoreError::Db(crate::domain::error::DomainError::Db(error)))?;
    Ok(rows
        .into_iter()
        .map(|row| PairRow {
            id: row.id,
            issue_id: row.issue_id,
            related_issue_id: row.related_issue_id,
            relation_type: row.relation_type,
        })
        .collect())
}

pub use crate::orchestration::relations::stored_edge;
