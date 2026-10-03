#![forbid(unsafe_code)]

//! Agent-facing issue relations (D-12 L4, stage 5).
//!
//! Ports `orchestration/relations.py` whole — one implementation behind
//! every agent surface (`pidash issue relate` / `unrelate` / `relations`,
//! the assistant's `relate_issues` / `unrelate_issues` /
//! `list_issue_relations`) so the agent sees one vocabulary and one
//! output shape everywhere.
//!
//! Relation types are named from the *source* issue's point of view
//! ([`RELATION_TYPES`], display order, pairs adjacent). Writes are
//! idempotent (re-relating reports `unchanged`, re-removing reports
//! `not_related`), and a pair holds at most one live relation: asking
//! for a different one reports `conflicts` with the existing relation
//! and overwrites nothing.
//!
//! The services crate carries no `sqlx` dependency, so — per the
//! D-27/D-30/D-36 `queries.rs` precedent and the sibling
//! [`crate::orchestration::blockers`] module — SQL here is text plus
//! symbolic `:name` placeholders;
//! handlers translate each `:name` to a positional `$n` in the
//! statement's `*_PARAMS` order (first appearance) when binding via
//! `sqlx`. Pure shaping over fetched rows ([`relate_verdict`],
//! [`unrelate_matches`], [`grouped_relations`], the `*_result`
//! structs, [`RelationActivityEmit`]) mirrors the queryset-side logic
//! so it stays unit-testable with no database.
//!
//! Access control stays the caller's job, exactly as in Python: the
//! caller resolves the source and the targets through its own scoping
//! and hands the resolved rows here. [`classify_ref`] splits a raw
//! reference into its lookup shape, the `resolve_*_predicate`
//! fragments drop into the caller's scoped query, and the grouped
//! pool rows are fetched by the caller and narrowed by
//! [`grouped_relations`].
//!
//! The stored/inverse mapping reuses the merged assistant helpers
//! ([`actual_relation`] / [`inverse_relation`] from
//! `assistant::tools_issues`) — never re-ported. Everything else is
//! this module's own port: the vocabulary is owned here (as
//! `relations.py` owns it in Python), the SQL uses this module's
//! `:name` convention (the assistant inline port uses `$n`), results
//! are typed `Serialize` structs in Python dict order (not `json!`
//! values), and failures are [`RelationError`] (the assistant's
//! `ToolError` shape differs).
//!
//! Fixture: FX-ORCH-04
//! (`rust-api/fixtures/orchestration/fx04_relations/`:
//! `relation_types.golden.json`, `resolve_refs.golden.json`,
//! `relate.before_after.json`, `relate_race.golden.json`,
//! `unrelate.golden.json`, `grouped_relations.golden.json`). The
//! replay below pins the matrices, the resolve/relate/unrelate/grouped
//! goldens, the DB before/after row mapping, and the enqueue payload.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
//!
//! Existing quirks ported as-is (translation, don't redesign):
//!
//! 1. The relate `INSERT` carries `updated_by_id = NULL`: the explicit
//!    `updated_by=actor` argument is discarded by `BaseModel.save`
//!    (`db/models/base.py:36-39`), which forces `updated_by` to `None`
//!    on create. `created_by_id` is the crum user — the requesting
//!    actor on every agent path — so the handler binds `:actor_id`.
//! 2. The unrelate soft-delete stamps `updated_by_id` too: instance
//!    `delete()` goes through the full `save()`, and a non-adding
//!    save stamps `updated_by` from crum (`base.py:40-42`). (The
//!    `tasks_cleanup::soft_stamp_sql` precedent omits it — that is the
//!    worker-sweep flow with no crum user.)
//! 3. Instance `delete()` also fires a `soft_delete_related_objects`
//!    task (`mixins.py:73`); for relation rows that task is a no-op
//!    (no model references `issue_relations`) and FX-ORCH-04 does not
//!    pin its enqueue, so this module emits only the `issue_activity`
//!    call the fixture records.
//! 4. Non-ASCII digit tails (`X-²`) pass Python's `isdigit()` gate and
//!    then crash `int()`; here they are `Unresolved`, following the
//!    merged assistant interpretation (and `i64` overflow likewise
//!    resolves to `Unresolved` — Python finds no row either way).
//! 5. A post-conflict re-read that finds no live pair (a concurrent
//!    delete landing between the failed `INSERT` and the re-read)
//!    crashes Python (`sorted(set())[0]`); here it verdicts `Create` —
//!    no live pair means the pair is creatable.
//! 6. `created_at` / `updated_at` share one `:now` bind; Django calls
//!    `timezone.now()` twice, so the two stamps can differ by
//!    microseconds — unobservable to every consumer of this module.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::assistant::tools_issues::{actual_relation, inverse_relation};

// ---------------------------------------------------------------------------
// Vocabulary (`relations.py:55-74`)
// ---------------------------------------------------------------------------

/// Every relation type an agent may name, in display order
/// (`relations.py:55-66`). Pairs sit next to each other: the second of
/// each pair is the first seen from the other end.
pub const RELATION_TYPES: &[&str] = &[
    "blocked_by",
    "blocking",
    "relates_to",
    "duplicate",
    "start_before",
    "start_after",
    "finish_before",
    "finish_after",
    "implemented_by",
    "implements",
];

/// Types stored with the ends swapped under their forward name
/// (`relations.py:68-70`; `blocking` is written as `blocked_by` on
/// the other issue). Source order; membership is all that matters.
pub const REVERSE_TYPES: &[&str] = &["blocking", "start_after", "finish_after", "implements"];

/// Per-type cap on [`grouped_relations`] lists (`relations.py:72-74`);
/// matches `blockers.SUMMARY_LIMIT` so the issue read path stays light.
pub const GROUP_LIMIT: usize = 100;

/// `RelationError` (`relations.py:77-78`): invalid relation request
/// (unknown type, self-relation, cross-workspace target). One class
/// with verbatim messages, as in Python.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct RelationError {
    /// The exact message Python raises.
    pub message: String,
}

impl RelationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// `validate_relation_type` (`relations.py:81-85`): stripped,
/// lowercased; the message joins [`RELATION_TYPES`] in tuple order.
/// Byte-exact. Python also accepts `None` (same message); `None` is
/// unrepresentable at this `&str` boundary — a JSON `null` is rejected
/// by deserialization upstream.
pub fn validate_relation_type(relation_type: &str) -> Result<String, RelationError> {
    let value = relation_type.trim().to_lowercase();
    if RELATION_TYPES.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(RelationError::new(format!(
            "relation_type must be one of: {}",
            RELATION_TYPES.join(", ")
        )))
    }
}

/// `identifier` (`relations.py:88-89`): `PROJ-123` from the joined
/// project identifier and sequence id.
pub fn identifier(project_identifier: &str, sequence_id: i32) -> String {
    format!("{project_identifier}-{sequence_id}")
}

// ---------------------------------------------------------------------------
// Stored-edge math (`_stored_edge` / `_type_from`, `:92-114`)
// ---------------------------------------------------------------------------

/// One `_pair_rows` / grouped row: a live `issue_relations` row in
/// either direction. Column order matches [`pair_rows_sql`] and
/// [`grouped_relations_sql`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairRow {
    /// `issue_relations.id`.
    pub id: uuid::Uuid,
    /// `issue_relations.issue_id`.
    pub issue_id: uuid::Uuid,
    /// `issue_relations.related_issue_id`.
    pub related_issue_id: uuid::Uuid,
    /// `issue_relations.relation_type`, verbatim (legacy rows may hold
    /// a reverse name, which [`type_from`] normalises).
    pub relation_type: String,
}

/// `_stored_edge` (`relations.py:92-97`): the `(issue_id,
/// related_issue_id, stored_type)` triple for "source `<type>`
/// target". The stored type comes from the merged [`actual_relation`];
/// reverse types swap the ends.
pub fn stored_edge(
    source_id: uuid::Uuid,
    relation_type: &str,
    target_id: uuid::Uuid,
) -> (uuid::Uuid, uuid::Uuid, String) {
    let stored = actual_relation(relation_type).to_owned();
    if REVERSE_TYPES.contains(&relation_type) {
        (target_id, source_id, stored)
    } else {
        (source_id, target_id, stored)
    }
}

/// `_type_from` (`relations.py:100-114`): the relation `row`
/// expresses, named from `viewpoint_id`'s side. Rows stored under a
/// reverse name (which the UI never writes but older data may hold)
/// are normalised like `orchestration.blockers` does: the ends swap
/// and the type folds through the merged [`actual_relation`]; the far
/// side then folds through the merged [`inverse_relation`]. Unknown
/// junk passes through untouched, exactly as Python does (`.get(k,
/// k)` on both mappers).
pub fn type_from(row: &PairRow, viewpoint_id: &uuid::Uuid) -> String {
    let (stored, anchor) = if REVERSE_TYPES.contains(&row.relation_type.as_str()) {
        (
            actual_relation(row.relation_type.as_str()),
            &row.related_issue_id,
        )
    } else {
        (row.relation_type.as_str(), &row.issue_id)
    };
    if anchor == viewpoint_id {
        stored.to_owned()
    } else {
        inverse_relation(stored).to_owned()
    }
}

// ---------------------------------------------------------------------------
// Pair rows (`_pair_rows`, `:117-120`)
// ---------------------------------------------------------------------------

/// Bind order for [`pair_rows_sql`].
pub const PAIR_ROWS_PARAMS: &[&str] = &["a_id", "b_id"];

/// `_pair_rows` (`relations.py:117-120`): live rows for the pair in
/// either direction (`deleted_at IS NULL` from the default
/// soft-deletion manager). There is deliberately no self-pair
/// exclusion — Python has none here (unlike the grouped query) — and
/// no `ORDER BY`: `Meta.ordering` (`-created_at`) is unobservable to
/// the set-membership and delete-all consumers. Binds
/// [`PAIR_ROWS_PARAMS`].
pub fn pair_rows_sql() -> String {
    "SELECT r.\"id\", r.\"issue_id\", r.\"related_issue_id\", r.\"relation_type\" \
     FROM \"issue_relations\" r \
     WHERE ((r.\"issue_id\" = :a_id AND r.\"related_issue_id\" = :b_id) \
     OR (r.\"issue_id\" = :b_id AND r.\"related_issue_id\" = :a_id)) \
     AND r.\"deleted_at\" IS NULL"
        .to_owned()
}

// ---------------------------------------------------------------------------
// Reference resolution (`resolve_refs`, `:138-163`)
// ---------------------------------------------------------------------------

/// Lookup shape of one raw reference (`resolve_refs`,
/// `relations.py:138-163`): a UUID parses as an id lookup, otherwise
/// `PROJ-123` splits at the last `-` with an all-digit tail. Anything
/// else is unresolved, never an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefKind {
    /// UUID half (`issues.filter(id=...)`). The parsed value, so the
    /// handler binds the canonical hyphenated rendering — Postgres
    /// would reject the braced/`urn:` spellings Python accepts.
    Id(uuid::Uuid),
    /// Identifier half (`project__identifier__iexact` + `sequence_id`).
    Identifier {
        /// Project code, verbatim (the lookup is case-insensitive).
        project_code: String,
        /// `int(seq)`; Python's is unbounded, but anything past `i64`
        /// matches no row either way.
        sequence_id: i64,
    },
    /// Not a reference the pool could hold (`None`, empty, bad tail).
    Unresolved,
}

/// One classified reference: the stripped text plus its lookup shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedRef {
    /// `str(raw or "").strip()`.
    pub stripped: String,
    /// The lookup half, or [`RefKind::Unresolved`].
    pub kind: RefKind,
}

/// The per-reference half of `resolve_refs` (`relations.py:148-158`).
/// `Uuid::parse_str` accepts exactly the spellings CPython's
/// `uuid.UUID` does (simple, hyphenated, `{braced}`, lowercase
/// `urn:uuid:` — uppercase `URN:UUID:` fails on both sides, verified
/// against uuid 1.23 `parser.rs`). The identifier split mirrors
/// `rpartition("-")` plus the `sep and project_ident and
/// seq.isdigit()` gate, with ASCII-only digits (quirk 4).
pub fn classify_ref(raw: &str) -> ClassifiedRef {
    let stripped = raw.trim();
    if stripped.is_empty() {
        return ClassifiedRef {
            stripped: String::new(),
            kind: RefKind::Unresolved,
        };
    }
    if let Ok(id) = uuid::Uuid::parse_str(stripped) {
        return ClassifiedRef {
            stripped: stripped.to_owned(),
            kind: RefKind::Id(id),
        };
    }
    if let Some((code, tail)) = stripped.rsplit_once('-') {
        if !code.is_empty() && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(sequence_id) = tail.parse::<i64>() {
                return ClassifiedRef {
                    stripped: stripped.to_owned(),
                    kind: RefKind::Identifier {
                        project_code: code.to_owned(),
                        sequence_id,
                    },
                };
            }
        }
    }
    ClassifiedRef {
        stripped: stripped.to_owned(),
        kind: RefKind::Unresolved,
    }
}

/// The unresolved passthrough (`relations.py:160`): `ref or str(raw)`
/// — the stripped text, or the raw text when nothing is left after
/// stripping.
pub fn unresolved_passthrough(raw: &str) -> String {
    let stripped = raw.trim();
    if stripped.is_empty() {
        raw.to_owned()
    } else {
        stripped.to_owned()
    }
}

/// Bind order for [`resolve_id_predicate`].
pub const RESOLVE_ID_PARAMS: &[&str] = &["ref_id"];

/// Bind order for [`resolve_identifier_predicate`].
pub const RESOLVE_IDENTIFIER_PARAMS: &[&str] = &["project_code", "sequence_id"];

/// The UUID half of `resolve_refs` (`issues.filter(id=...)`) as a
/// predicate over the caller's scoped base query (`i`, the `issues`
/// alias). The caller's base carries the `IssueManager` liveness; this
/// adds only the ref match. Binds [`RESOLVE_ID_PARAMS`].
pub fn resolve_id_predicate(issue_alias: &str) -> String {
    format!("{issue_alias}.\"id\" = :ref_id")
}

/// The identifier half of `resolve_refs`
/// (`project__identifier__iexact` + `sequence_id`) as a predicate over
/// the caller's scoped base query. `iexact` renders as `UPPER()` on
/// both sides, mirroring the merged assistant `RESOLVE_IDENTIFIER_SQL`
/// (Django's Postgres rendering). Binds
/// [`RESOLVE_IDENTIFIER_PARAMS`].
pub fn resolve_identifier_predicate(issue_alias: &str, project_alias: &str) -> String {
    format!(
        "UPPER({project_alias}.\"identifier\") = UPPER(:project_code) \
         AND {issue_alias}.\"sequence_id\" = :sequence_id"
    )
}

// ---------------------------------------------------------------------------
// Targets (`_check_targets`, `:166-177`)
// ---------------------------------------------------------------------------

/// One issue resolved against the caller's scope: a `resolve_refs`
/// found row, and the relate/unrelate target shape. `project_id` /
/// `workspace_id` feed the relate write; the identifier parts feed the
/// verbatim error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIssue {
    /// `issues.id`.
    pub id: uuid::Uuid,
    /// `issues.workspace_id`.
    pub workspace_id: uuid::Uuid,
    /// `issues.project_id` (the relate write stores the *source's*).
    pub project_id: uuid::Uuid,
    /// `projects.identifier` (via `select_related("project")`).
    pub project_identifier: String,
    /// `issues.sequence_id`.
    pub sequence_id: i32,
}

impl ResolvedIssue {
    /// [`identifier`] for this issue.
    pub fn identifier(&self) -> String {
        identifier(&self.project_identifier, self.sequence_id)
    }
}

/// `_check_targets` (`relations.py:166-177`): self-relations and
/// cross-workspace targets raise with verbatim messages; duplicates
/// collapse, first-seen order kept.
pub fn check_targets(
    source: &ResolvedIssue,
    targets: &[ResolvedIssue],
) -> Result<Vec<ResolvedIssue>, RelationError> {
    let mut unique: Vec<ResolvedIssue> = Vec::new();
    let mut seen: HashSet<uuid::Uuid> = HashSet::new();
    for target in targets {
        if target.id == source.id {
            return Err(RelationError::new(format!(
                "{} cannot be related to itself",
                source.identifier()
            )));
        }
        if target.workspace_id != source.workspace_id {
            return Err(RelationError::new(format!(
                "{} is in a different workspace",
                target.identifier()
            )));
        }
        if seen.insert(target.id) {
            unique.push(target.clone());
        }
    }
    Ok(unique)
}

// ---------------------------------------------------------------------------
// Relate (`relate`, `:180-232`)
// ---------------------------------------------------------------------------

/// Per-target verdict of the relate loop (`relations.py:193-218`,
/// after validation and [`check_targets`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelateVerdict {
    /// No live pair row (`:196`): the handler attempts
    /// [`relate_insert_sql`] inside `transaction.atomic()`; on
    /// `IntegrityError` (code 23505) it re-reads the pair and verdicts
    /// again (`:208-210`).
    Create,
    /// `current == {requested}` (`:215-216`): report under `unchanged`.
    Unchanged,
    /// The pair already carries another type (`:217-218`): report
    /// under `conflicts` with `sorted(current)[0]` named from the
    /// source's side; nothing is overwritten.
    Conflict {
        /// The existing relation, source-side.
        existing_relation: String,
    },
}

/// `relate` lines 214-218 over the live pair rows: the viewpoint types
/// via [`type_from`], collected into a sorted set (`sorted(current)`
/// for the conflict pick). An empty set verdicts [`RelateVerdict::Create`]
/// (quirk 5).
pub fn relate_verdict(
    existing: &[PairRow],
    viewpoint_id: &uuid::Uuid,
    relation_type: &str,
) -> RelateVerdict {
    let current: BTreeSet<String> = existing
        .iter()
        .map(|row| type_from(row, viewpoint_id))
        .collect();
    if current.len() == 1 && current.contains(relation_type) {
        RelateVerdict::Unchanged
    } else if let Some(first) = current.iter().next() {
        RelateVerdict::Conflict {
            existing_relation: first.clone(),
        }
    } else {
        RelateVerdict::Create
    }
}

/// One `conflicts` entry (`relations.py:218`): `{identifier,
/// existing_relation}`. Field order is the Python dict order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelateConflict {
    /// Target identifier.
    pub identifier: String,
    /// The existing relation, named from the source's side.
    pub existing_relation: String,
}

/// The `relate` value (`relations.py:226-232`): `{issue,
/// relation_type, created, unchanged, conflicts}`. Field order is the
/// Python dict order, so `serde_json` renders byte-identical JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelateResult {
    /// Source identifier.
    pub issue: String,
    /// Validated relation type.
    pub relation_type: String,
    /// Created target identifiers, in request order.
    pub created: Vec<String>,
    /// Already-carrying target identifiers, in request order.
    pub unchanged: Vec<String>,
    /// Pairs carrying another type, in request order.
    pub conflicts: Vec<RelateConflict>,
}

/// The relate write, field by field (`relations.py:199-207` plus the
/// `BaseModel.save` audit stamps): which source fields land in which
/// columns. `project_id` / `workspace_id` are the *source's*;
/// `updated_by_id` is always `None` (quirk 1); `deleted_at` is always
/// `None`. Timestamps are the shared `:now` bind of
/// [`relate_insert_sql`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelateInsert {
    /// Fresh `uuid4` (`id` default), generated by the handler.
    pub id: uuid::Uuid,
    /// The requesting actor (crum user on agent paths).
    pub created_by_id: uuid::Uuid,
    /// Always `None` (quirk 1).
    pub updated_by_id: Option<uuid::Uuid>,
    /// Always `None` (a created row is live; the `NULL` literal in
    /// [`relate_insert_sql`]).
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Source's `project_id`.
    pub project_id: uuid::Uuid,
    /// Source's `workspace_id` (`ProjectBaseModel.save` re-derives it
    /// from the project — a no-op on consistent data).
    pub workspace_id: uuid::Uuid,
    /// Stored `issue_id` end (see [`stored_edge`]).
    pub issue_id: uuid::Uuid,
    /// Stored `related_issue_id` end (see [`stored_edge`]).
    pub related_issue_id: uuid::Uuid,
    /// Stored type (forward twin for reverse requests).
    pub relation_type: String,
}

/// Build the [`RelateInsert`] for one `Create` verdict: the
/// [`stored_edge`] ends plus the source-owned project/workspace and
/// the actor audit stamps.
pub fn relate_insert_row(
    id: uuid::Uuid,
    source: &ResolvedIssue,
    relation_type: &str,
    target: &ResolvedIssue,
    actor_id: uuid::Uuid,
) -> RelateInsert {
    let (issue_id, related_issue_id, stored) = stored_edge(source.id, relation_type, target.id);
    RelateInsert {
        id,
        created_by_id: actor_id,
        updated_by_id: None,
        deleted_at: None,
        project_id: source.project_id,
        workspace_id: source.workspace_id,
        issue_id,
        related_issue_id,
        relation_type: stored,
    }
}

/// Bind order for [`relate_insert_sql`] (first appearance; `:now`
/// binds once for both stamps).
pub const RELATE_INSERT_PARAMS: &[&str] = &[
    "id",
    "now",
    "actor_id",
    "project_id",
    "workspace_id",
    "issue_id",
    "related_issue_id",
    "stored_type",
];

/// The relate create (`relations.py:196-207`): one
/// `transaction.atomic()` `INSERT`, in the `issue_relation::COLUMNS`
/// order. `updated_by_id` / `deleted_at` are `NULL` literals (quirk
/// 1); the partial unique index on `(issue_id, related_issue_id)`
/// where live raises 23505 on a concurrent same-direction pair, which
/// the handler turns into a re-read. Binds [`RELATE_INSERT_PARAMS`].
pub fn relate_insert_sql() -> String {
    "INSERT INTO \"issue_relations\" (\"id\", \"created_at\", \"updated_at\", \"created_by_id\", \
     \"updated_by_id\", \"deleted_at\", \"project_id\", \"workspace_id\", \"issue_id\", \
     \"related_issue_id\", \"relation_type\") \
     VALUES (:id, :now, :now, :actor_id, NULL, NULL, :project_id, :workspace_id, :issue_id, \
     :related_issue_id, :stored_type)"
        .to_owned()
}

// ---------------------------------------------------------------------------
// Unrelate (`unrelate`, `:235-266`)
// ---------------------------------------------------------------------------

/// The `unrelate` value (`relations.py:261-266`): `{issue,
/// relation_type, removed, not_related}`. Field order is the Python
/// dict order, so `serde_json` renders byte-identical JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnrelateResult {
    /// Source identifier.
    pub issue: String,
    /// Validated relation type.
    pub relation_type: String,
    /// Removed target identifiers, in request order.
    pub removed: Vec<String>,
    /// Targets carrying no such relation, in request order.
    pub not_related: Vec<String>,
}

/// The exact-type match (`relations.py:247`): the live pair rows
/// expressing exactly `relation_type` from the source's side. A pair
/// carrying another type matches nothing — the row is left alone and
/// the target reports under `not_related`.
pub fn unrelate_matches<'a>(
    existing: &'a [PairRow],
    viewpoint_id: &uuid::Uuid,
    relation_type: &str,
) -> Vec<&'a PairRow> {
    existing
        .iter()
        .filter(|row| type_from(row, viewpoint_id) == relation_type)
        .collect()
}

/// Bind order for [`unrelate_delete_sql`] (first appearance).
pub const UNRELATE_DELETE_PARAMS: &[&str] = &["now", "actor_id", "row_id"];

/// One `row.delete()` (`relations.py:251-252`): instance soft-delete —
/// `deleted_at = now()` plus the full `save()` stamps (`updated_at`
/// via `auto_now`, `updated_by_id` via `BaseModel.save`, quirk 2).
/// Django rewrites every column; only these three change observably.
/// One statement per matched row, each followed by its own
/// [`RelationActivityEmit`] (`:253-260`). Binds
/// [`UNRELATE_DELETE_PARAMS`].
pub fn unrelate_delete_sql() -> String {
    "UPDATE \"issue_relations\" SET \"deleted_at\" = :now, \"updated_at\" = :now, \
     \"updated_by_id\" = :actor_id WHERE \"id\" = :row_id"
        .to_owned()
}

// ---------------------------------------------------------------------------
// Grouped list (`grouped_relations`, `:280-313`)
// ---------------------------------------------------------------------------

/// Bind order for [`grouped_relations_sql`] (first appearance;
/// `:issue_id` binds once across its three sites).
pub const GROUPED_RELATIONS_PARAMS: &[&str] = &["issue_id", "workspace_id"];

/// The grouped live-row fetch (`relations.py:287-290`): every live
/// row touching `:issue_id` in its workspace, self-pairs excluded
/// (`exclude(issue_id=..., related_issue_id=...)` renders as the
/// `NOT (...)` conjunct; the manager adds `deleted_at IS NULL`). No
/// `ORDER BY`: `Meta.ordering` is unobservable to the group/sort/cap
/// shaping. Binds [`GROUPED_RELATIONS_PARAMS`].
pub fn grouped_relations_sql() -> String {
    "SELECT r.\"id\", r.\"issue_id\", r.\"related_issue_id\", r.\"relation_type\" \
     FROM \"issue_relations\" r \
     WHERE (r.\"issue_id\" = :issue_id OR r.\"related_issue_id\" = :issue_id) \
     AND r.\"workspace_id\" = :workspace_id \
     AND NOT (r.\"issue_id\" = :issue_id AND r.\"related_issue_id\" = :issue_id) \
     AND r.\"deleted_at\" IS NULL"
        .to_owned()
}

/// One visibility-pool row for grouped shaping: the caller's pool
/// (`visible_issues`, defaulting to all live workspace issues)
/// narrowed to the wanted other ends, with project and state joined
/// (`select_related("state", "project")`, `:299-305`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupedIssue {
    /// `issues.id`.
    pub id: uuid::Uuid,
    /// `projects.identifier` (the sort major key).
    pub project_identifier: String,
    /// `issues.sequence_id` (the sort minor key).
    pub sequence_id: i32,
    /// `issues.name`.
    pub name: String,
    /// `states.name`, or `None` without a state.
    pub state_name: Option<String>,
    /// `states.group`, or `None` without a state.
    pub state_group: Option<String>,
}

/// One `_item` (`relations.py:269-277`): `{id, identifier, name,
/// state, state_group}`. Field order is the Python dict order, so
/// `serde_json` renders byte-identical JSON. A stateless row renders
/// explicit `null`s (the `None`-vs-absent-key trap).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupedItem {
    /// `str(issue.id)`.
    pub id: String,
    /// `PROJ-123`.
    pub identifier: String,
    /// Issue name.
    pub name: String,
    /// State name, or `None` without a state.
    pub state: Option<String>,
    /// State group, or `None` without a state.
    pub state_group: Option<String>,
}

/// `_item` (`relations.py:269-277`).
pub fn grouped_item(issue: &GroupedIssue) -> GroupedItem {
    GroupedItem {
        id: issue.id.to_string(),
        identifier: identifier(&issue.project_identifier, issue.sequence_id),
        name: issue.name.clone(),
        state: issue.state_name.clone(),
        state_group: issue.state_group.clone(),
    }
}

/// The `grouped_relations` value (`relations.py:310-313`): every
/// [`RELATION_TYPES`] key always present, in tuple order, each a
/// `(project.identifier, sequence_id)`-sorted list capped at
/// [`GROUP_LIMIT`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupedRelations {
    /// Issues this one is blocked by.
    pub blocked_by: Vec<GroupedItem>,
    /// Issues this one blocks.
    pub blocking: Vec<GroupedItem>,
    /// Related issues.
    pub relates_to: Vec<GroupedItem>,
    /// Duplicates.
    pub duplicate: Vec<GroupedItem>,
    /// Issues this one starts before.
    pub start_before: Vec<GroupedItem>,
    /// Issues this one starts after.
    pub start_after: Vec<GroupedItem>,
    /// Issues this one finishes before.
    pub finish_before: Vec<GroupedItem>,
    /// Issues this one finishes after.
    pub finish_after: Vec<GroupedItem>,
    /// Implementers of this issue.
    pub implemented_by: Vec<GroupedItem>,
    /// Issues this one implements.
    pub implements: Vec<GroupedItem>,
}

impl GroupedRelations {
    /// Empty groups for every key (the `:291` initializer shape).
    pub fn empty() -> Self {
        Self {
            blocked_by: Vec::new(),
            blocking: Vec::new(),
            relates_to: Vec::new(),
            duplicate: Vec::new(),
            start_before: Vec::new(),
            start_after: Vec::new(),
            finish_before: Vec::new(),
            finish_after: Vec::new(),
            implemented_by: Vec::new(),
            implements: Vec::new(),
        }
    }

    /// Push one item onto the named group. Only called with known
    /// [`RELATION_TYPES`] members (the `:295` guard lives in
    /// [`grouped_relations`]); the fallback arm is unreachable.
    fn push(&mut self, relation: &str, item: GroupedItem) {
        match relation {
            "blocked_by" => self.blocked_by.push(item),
            "blocking" => self.blocking.push(item),
            "relates_to" => self.relates_to.push(item),
            "duplicate" => self.duplicate.push(item),
            "start_before" => self.start_before.push(item),
            "start_after" => self.start_after.push(item),
            "finish_before" => self.finish_before.push(item),
            "finish_after" => self.finish_after.push(item),
            "implemented_by" => self.implemented_by.push(item),
            "implements" => self.implements.push(item),
            _ => {}
        }
    }
}

/// `grouped_relations` (`relations.py:280-313`) over fetched rows:
/// `rows` is the [`grouped_relations_sql`] set, `pool` the caller's
/// visibility pool narrowed to the wanted other ends (`:298-305`; the
/// caller applies its own scoping — `None` in Python means all live
/// workspace issues). Each group sorts by `(project.identifier,
/// sequence_id)` (`:307-309`; stable, like Python's `sorted`) and caps
/// at [`GROUP_LIMIT`] (`:311`).
pub fn grouped_relations(
    issue_id: &uuid::Uuid,
    rows: &[PairRow],
    pool: &[GroupedIssue],
) -> GroupedRelations {
    let mut other_by_type: HashMap<String, HashSet<uuid::Uuid>> = HashMap::new();
    for row in rows {
        let other = if row.issue_id == *issue_id {
            row.related_issue_id
        } else {
            row.issue_id
        };
        let relation = type_from(row, issue_id);
        if RELATION_TYPES.contains(&relation.as_str()) {
            other_by_type.entry(relation).or_default().insert(other);
        }
    }
    let pool_by_id: HashMap<uuid::Uuid, &GroupedIssue> =
        pool.iter().map(|issue| (issue.id, issue)).collect();
    let mut grouped = GroupedRelations::empty();
    for relation in RELATION_TYPES {
        let mut items: Vec<&GroupedIssue> = other_by_type
            .get(*relation)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| pool_by_id.get(id).copied())
                    .collect()
            })
            .unwrap_or_default();
        items.sort_by(|a, b| {
            (&a.project_identifier, a.sequence_id).cmp(&(&b.project_identifier, b.sequence_id))
        });
        for issue in items.into_iter().take(GROUP_LIMIT) {
            grouped.push(relation, grouped_item(issue));
        }
    }
    grouped
}

// ---------------------------------------------------------------------------
// Activity enqueue (`_log_activity`, `:123-135`)
// ---------------------------------------------------------------------------

/// Celery wire name for `issue_activity` (bare `@shared_task`) — the
/// same task the estimate sites publish to (see
/// `app_project::tasks::ISSUE_ACTIVITY_TASK`); the relations call site
/// additionally passes `notification=True`.
pub const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";

/// `type` passed by the relate-created call (`relations.py:221`).
pub const RELATION_CREATED_ACTIVITY: &str = "issue_relation.activity.created";
/// `type` passed by the unrelate-removed call (`relations.py:255`).
pub const RELATION_DELETED_ACTIVITY: &str = "issue_relation.activity.deleted";

/// Kwarg order of `_log_activity` (`relations.py:126-135`): call-site
/// order, which differs from the worker signature order
/// (`issue_activities_task.py:1504-1516`). `notification` is always
/// `True` here (`subscriber` keeps its worker default and is not
/// passed).
pub const ISSUE_ACTIVITY_KWARG_ORDER: &[&str] = &[
    "type",
    "requested_data",
    "actor_id",
    "issue_id",
    "project_id",
    "current_instance",
    "epoch",
    "notification",
];

/// Quote one JSON string value exactly as `json.dumps` /
/// `DjangoJSONEncoder` render it for these payloads. Inputs are
/// always ASCII here (validated relation types, UUID hex), so
/// `serde_json`'s UTF-8-raw rendering coincides with `ensure_ascii`.
fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("str serializes")
}

/// `json.dumps({"relation_type": ..., "issues": [...]})`
/// (`relations.py:222`): CPython *default* separators (`", "`, `":
/// "`), keys in insertion order, created ids as `str(...)` strings.
pub fn relate_created_requested_data(relation_type: &str, created_ids: &[uuid::Uuid]) -> String {
    let ids = created_ids
        .iter()
        .map(|id| json_string(&id.to_string()))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{\"relation_type\": {}, \"issues\": [{ids}]}}",
        json_string(relation_type)
    )
}

/// `json.dumps({"relation_type": ..., "related_issue": ...})`
/// (`relations.py:256`): same rendering rules as
/// [`relate_created_requested_data`].
pub fn unrelate_requested_data(relation_type: &str, target_id: &uuid::Uuid) -> String {
    format!(
        "{{\"relation_type\": {}, \"related_issue\": {}}}",
        json_string(relation_type),
        json_string(&target_id.to_string())
    )
}

/// `json.dumps({"relation_type": ...})` (`relations.py:259`): the
/// unrelate `current_instance` rendering.
pub fn relation_current_instance(relation_type: &str) -> String {
    format!("{{\"relation_type\": {}}}", json_string(relation_type))
}

/// One `_log_activity(...)` call (`relations.py:123-135`). The handler
/// publishes this transactionally (F-09 `enqueue_in`, same transaction
/// as the relate/unrelate write); until the relations task group is
/// Rust-owned, the Python worker consumes it from Celery protocol v2.
/// `epoch` is `int(timezone.now().timestamp())`, supplied by the
/// caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationActivityEmit {
    /// `type`: [`RELATION_CREATED_ACTIVITY`] or [`RELATION_DELETED_ACTIVITY`].
    pub activity_type: String,
    /// Pre-rendered `requested_data` text (see the `*_requested_data`
    /// renderers).
    pub requested_data: String,
    /// `str(actor.id)`.
    pub actor_id: String,
    /// `str(issue.id)` (the source).
    pub issue_id: String,
    /// `str(issue.project_id)`.
    pub project_id: String,
    /// Pre-rendered `current_instance` text, or `None` on relate-created.
    pub current_instance: Option<String>,
    /// `int(timezone.now().timestamp())`.
    pub epoch: i64,
}

impl RelationActivityEmit {
    /// Celery task name this emit publishes to.
    pub fn task_name(&self) -> &'static str {
        ISSUE_ACTIVITY_TASK
    }

    /// `.delay()` positional args: always empty.
    pub fn args(&self) -> Vec<Value> {
        Vec::new()
    }

    /// `.delay()` kwargs in call-site order ([`ISSUE_ACTIVITY_KWARG_ORDER`]).
    pub fn kwargs(&self) -> Map<String, Value> {
        let mut kwargs = Map::with_capacity(ISSUE_ACTIVITY_KWARG_ORDER.len());
        kwargs.insert("type".to_owned(), Value::String(self.activity_type.clone()));
        kwargs.insert(
            "requested_data".to_owned(),
            Value::String(self.requested_data.clone()),
        );
        kwargs.insert("actor_id".to_owned(), Value::String(self.actor_id.clone()));
        kwargs.insert("issue_id".to_owned(), Value::String(self.issue_id.clone()));
        kwargs.insert(
            "project_id".to_owned(),
            Value::String(self.project_id.clone()),
        );
        kwargs.insert(
            "current_instance".to_owned(),
            self.current_instance
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        kwargs.insert("epoch".to_owned(), Value::Number(self.epoch.into()));
        kwargs.insert("notification".to_owned(), Value::Bool(true));
        kwargs
    }
}

/// The relate-created call (`relations.py:219-225`): one emit for the
/// whole batch (`if created:`), `current_instance=None`.
pub fn relate_created_emit(
    relation_type: &str,
    created_ids: &[uuid::Uuid],
    actor_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    epoch: i64,
) -> RelationActivityEmit {
    RelationActivityEmit {
        activity_type: RELATION_CREATED_ACTIVITY.to_owned(),
        requested_data: relate_created_requested_data(relation_type, created_ids),
        actor_id: actor_id.to_string(),
        issue_id: issue_id.to_string(),
        project_id: project_id.to_string(),
        current_instance: None,
        epoch,
    }
}

/// The unrelate-removed call (`relations.py:253-260`): one emit per
/// removed target.
pub fn unrelate_deleted_emit(
    relation_type: &str,
    target_id: &uuid::Uuid,
    actor_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    epoch: i64,
) -> RelationActivityEmit {
    RelationActivityEmit {
        activity_type: RELATION_DELETED_ACTIVITY.to_owned(),
        requested_data: unrelate_requested_data(relation_type, target_id),
        actor_id: actor_id.to_string(),
        issue_id: issue_id.to_string(),
        project_id: project_id.to_string(),
        current_instance: Some(relation_current_instance(relation_type)),
        epoch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_db::app_issues::models_links::issue_relation;

    /// Load one FX-ORCH-04 fixture file.
    fn fixture(name: &str) -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/orchestration/fx04_relations/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).expect("fx04 fixture exists");
        serde_json::from_str(&text).expect("fx04 fixture parses")
    }

    fn uuid(text: &str) -> uuid::Uuid {
        text.parse().expect("fixture uuid parses")
    }

    /// Compact rendering (insertion order) for byte comparisons: struct
    /// field order on our side, document order on the fixture side
    /// (`preserve_order`).
    fn compact(value: &serde_json::Value) -> String {
        serde_json::to_string(value).expect("value serializes")
    }

    fn to_value<T: Serialize>(value: &T) -> serde_json::Value {
        serde_json::to_value(value).expect("struct serializes")
    }

    /// `:name` placeholders in first-appearance order.
    fn placeholders_in_order(sql: &str) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        let mut chars = sql.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == ':' {
                let mut name = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_alphanumeric() || next == '_' {
                        name.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !name.is_empty() && !found.contains(&name) {
                    found.push(name);
                }
            }
        }
        found
    }

    fn assert_params(sql: &str, params: &[&str]) {
        let order = placeholders_in_order(sql);
        let expected: Vec<String> = params.iter().map(|p| p.to_string()).collect();
        assert_eq!(order, expected, "bind order of {sql}");
    }

    fn pair_row(issue: &str, related: &str, stored: &str) -> PairRow {
        PairRow {
            id: uuid("99999999-aaaa-bbbb-cccc-ffffffffffff"),
            issue_id: uuid(issue),
            related_issue_id: uuid(related),
            relation_type: stored.to_owned(),
        }
    }

    // FX4 ids shared across the relate/unrelate/grouped goldens.
    const FX4A_1: &str = "99999999-aaaa-bbbb-cccc-000000004101";
    const FX4A_2: &str = "99999999-aaaa-bbbb-cccc-000000004102";
    const FX4A_3: &str = "99999999-aaaa-bbbb-cccc-000000004103";
    const FX4B_1: &str = "99999999-aaaa-bbbb-cccc-000000004104";
    const FX4A_4: &str = "99999999-aaaa-bbbb-cccc-000000004111";
    const FX4A_5: &str = "99999999-aaaa-bbbb-cccc-000000004112";
    const FX4A_6: &str = "99999999-aaaa-bbbb-cccc-000000004121";
    const FX4A_7: &str = "99999999-aaaa-bbbb-cccc-000000004122";
    const FX4A_8: &str = "99999999-aaaa-bbbb-cccc-000000004123";
    const FX4A_9: &str = "99999999-aaaa-bbbb-cccc-000000004124";
    const FX4A_11: &str = "99999999-aaaa-bbbb-cccc-000000004132";
    const FX4A_12: &str = "99999999-aaaa-bbbb-cccc-000000004133";
    const FX4_PROJECT_A: &str = "99999999-aaaa-bbbb-cccc-000000004051";
    const FX4_PROJECT_B: &str = "99999999-aaaa-bbbb-cccc-000000004052";
    const FX4_ACTOR: &str = "99999999-aaaa-bbbb-cccc-000000004002";
    const FX4_EPOCH: i64 = 1791028800;

    fn resolved(
        id: &str,
        workspace: &str,
        project: &str,
        code: &str,
        sequence_id: i32,
    ) -> ResolvedIssue {
        ResolvedIssue {
            id: uuid(id),
            workspace_id: uuid(workspace),
            project_id: uuid(project),
            project_identifier: code.to_owned(),
            sequence_id,
        }
    }

    /// `relation_types.golden.json`: vocabulary, validation, stored-edge
    /// and viewpoint matrices.
    #[test]
    fn vocabulary_matches_fixture() {
        let golden = fixture("relation_types.golden.json");
        let types: Vec<&str> = golden["RELATION_TYPES"]
            .as_array()
            .expect("types is array")
            .iter()
            .map(|v| v.as_str().expect("type is str"))
            .collect();
        assert_eq!(types, RELATION_TYPES);
        let mut reverse: Vec<&str> = REVERSE_TYPES.to_vec();
        reverse.sort_unstable();
        let mut expected: Vec<&str> = golden["REVERSE_TYPES"]
            .as_array()
            .expect("reverse is array")
            .iter()
            .map(|v| v.as_str().expect("type is str"))
            .collect();
        expected.sort_unstable();
        assert_eq!(reverse, expected);
        assert_eq!(golden["GROUP_LIMIT"].as_u64(), Some(GROUP_LIMIT as u64));
    }

    #[test]
    fn validate_matches_fixture() {
        let golden = fixture("relation_types.golden.json");
        for (input, expected) in golden["validate_ok"]
            .as_object()
            .expect("validate_ok is object")
        {
            assert_eq!(
                validate_relation_type(input).expect("valid type"),
                expected.as_str().expect("ok value is str"),
                "input {input:?}"
            );
        }
        // Fixture keys are `repr()`s (`'blocking_typo'`, `"''"`, `None`).
        let errors = &golden["validate_errors"];
        for (input, key) in [
            ("blocking_typo", "'blocking_typo'"),
            ("", "''"),
            ("depends_on", "'depends_on'"),
        ] {
            let err = validate_relation_type(input).expect_err("invalid type");
            assert_eq!(err.message, errors[key].as_str().expect("msg is str"));
            assert_eq!(err.to_string(), err.message);
        }
        // `None` is unrepresentable at the `&str` boundary; the message it
        // pins is the same one every other invalid input gets.
        assert_eq!(
            errors["None"].as_str(),
            errors["''"].as_str(),
            "None shares the invalid-type message"
        );
    }

    #[test]
    fn stored_edge_matrix_matches_fixture() {
        let golden = fixture("relation_types.golden.json");
        let source = uuid("00000000-0000-0000-0000-000000000001");
        let target = uuid("00000000-0000-0000-0000-000000000002");
        for relation in RELATION_TYPES {
            let entry = &golden["stored_edge_matrix"][relation];
            let (issue_id, related_id, stored) = stored_edge(source, relation, target);
            assert_eq!(
                issue_id == source,
                entry["issue_is_source"].as_bool().expect("bool"),
                "{relation}: source end"
            );
            assert_eq!(
                stored,
                entry["stored_type"].as_str().expect("stored is str"),
                "{relation}: stored type"
            );
            // The other end is whichever end the source is not on.
            assert_eq!(
                related_id,
                if issue_id == source { target } else { source },
                "{relation}: opposite end"
            );
        }
    }

    #[test]
    fn type_from_matrix_matches_fixture() {
        let golden = fixture("relation_types.golden.json");
        let a = "00000000-0000-0000-0000-000000000001";
        let b = "00000000-0000-0000-0000-000000000002";
        for entry in golden["type_from_matrix"]
            .as_array()
            .expect("matrix is array")
        {
            // Stored spellings are `(0001->0002, <type>)`, legacy rows
            // prefix `legacy `.
            let stored = entry["stored"].as_str().expect("stored is str");
            let stored_type = stored
                .rsplit_once(", ")
                .expect("comma")
                .1
                .strip_suffix(')')
                .expect("paren");
            let row = pair_row(a, b, stored_type);
            assert_eq!(
                type_from(&row, &uuid(a)),
                entry["from_source"].as_str().expect("str"),
                "stored {stored_type} from source"
            );
            assert_eq!(
                type_from(&row, &uuid(b)),
                entry["from_target"].as_str().expect("str"),
                "stored {stored_type} from target"
            );
        }
    }

    /// `resolve_refs.golden.json`: request order, case-insensitive
    /// codes, UUID-vs-split routing, unresolved passthrough.
    #[test]
    fn resolve_refs_matches_fixture() {
        let golden = fixture("resolve_refs.golden.json");
        // The live FX4 pool the fixture resolved against.
        let pool: Vec<(&str, i64, uuid::Uuid)> = vec![
            ("FX4A", 1, uuid(FX4A_1)),
            ("FX4A", 2, uuid(FX4A_2)),
            ("FX4A", 3, uuid(FX4A_3)),
            ("FX4B", 1, uuid(FX4B_1)),
        ];
        let mut found: Vec<String> = Vec::new();
        let mut found_ids: Vec<String> = Vec::new();
        let mut unresolved: Vec<String> = Vec::new();
        for raw in golden["input_refs"].as_array().expect("refs is array") {
            let raw = raw.as_str().expect("ref is str");
            let classified = classify_ref(raw);
            let hit = match &classified.kind {
                RefKind::Id(id) => pool.iter().find(|(_, _, pid)| pid == id),
                RefKind::Identifier {
                    project_code,
                    sequence_id,
                } => pool.iter().find(|(code, seq, _)| {
                    code.eq_ignore_ascii_case(project_code) && seq == sequence_id
                }),
                RefKind::Unresolved => None,
            };
            match hit {
                Some((code, seq, id)) => {
                    found.push(format!("{code}-{seq}"));
                    found_ids.push(id.to_string());
                }
                None => unresolved.push(unresolved_passthrough(raw)),
            }
        }
        let expected_found: Vec<&str> = golden["found_identifiers_in_order"]
            .as_array()
            .expect("found is array")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect();
        let expected_ids: Vec<&str> = golden["found_ids"]
            .as_array()
            .expect("ids is array")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect();
        let expected_unresolved: Vec<&str> = golden["unresolved_passthrough"]
            .as_array()
            .expect("unresolved is array")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect();
        assert_eq!(found, expected_found);
        assert_eq!(found_ids, expected_ids);
        assert_eq!(unresolved, expected_unresolved);
    }

    #[test]
    fn resolve_predicates_shape() {
        assert_eq!(resolve_id_predicate("i"), "i.\"id\" = :ref_id");
        assert_eq!(
            resolve_identifier_predicate("i", "p"),
            "UPPER(p.\"identifier\") = UPPER(:project_code) AND i.\"sequence_id\" = :sequence_id"
        );
        assert_eq!(RESOLVE_ID_PARAMS, &["ref_id"]);
        assert_eq!(RESOLVE_IDENTIFIER_PARAMS, &["project_code", "sequence_id"]);
        assert_params(&resolve_id_predicate("i"), RESOLVE_ID_PARAMS);
        assert_params(
            &resolve_identifier_predicate("i", "p"),
            RESOLVE_IDENTIFIER_PARAMS,
        );
    }

    /// `relate.before_after.json`: created shape, row mapping, unchanged
    /// and conflict goldens, verbatim target errors, enqueue payload.
    #[test]
    fn relate_created_matches_fixture() {
        let golden = fixture("relate.before_after.json");
        assert!(golden["live_pairs_before"]
            .as_array()
            .expect("array")
            .is_empty());
        let workspace = "99999999-aaaa-bbbb-cccc-000000004000";
        let source = resolved(FX4B_1, workspace, FX4_PROJECT_B, "FX4B", 1);
        let target = resolved(FX4A_1, workspace, FX4_PROJECT_A, "FX4A", 1);
        let targets = check_targets(&source, std::slice::from_ref(&target)).expect("targets ok");
        assert_eq!(targets, vec![target.clone()]);
        // First read finds no live pair: the handler attempts the INSERT.
        assert_eq!(
            relate_verdict(&[], &source.id, "blocked_by"),
            RelateVerdict::Create
        );
        let row = relate_insert_row(
            uuid(FX4_ACTOR),
            &source,
            "blocked_by",
            &target,
            uuid(FX4_ACTOR),
        );
        assert_eq!(row.issue_id, uuid(FX4B_1));
        assert_eq!(row.related_issue_id, uuid(FX4A_1));
        assert_eq!(row.relation_type, "blocked_by");
        // The write stores the SOURCE's project/workspace (fixture:
        // project FX4B, workspace fx4-workspace).
        assert_eq!(row.project_id, source.project_id);
        assert_eq!(row.workspace_id, source.workspace_id);
        assert_eq!(
            source.project_identifier.as_str(),
            golden["created_row_shape"]["project"]
                .as_str()
                .expect("str")
        );
        assert_eq!(row.created_by_id, uuid(FX4_ACTOR));
        assert_eq!(row.updated_by_id, None);
        assert_eq!(row.deleted_at, None);
        assert!(golden["created_row_shape"]["updated_by"].is_null());
        assert!(golden["created_row_shape"]["deleted_at"].is_null());
        let after = &golden["live_pairs_after"];
        assert_eq!(
            after[0],
            serde_json::json!([source.identifier(), row.relation_type, target.identifier()])
        );
        let result = RelateResult {
            issue: source.identifier(),
            relation_type: "blocked_by".to_owned(),
            created: vec![target.identifier()],
            unchanged: Vec::new(),
            conflicts: Vec::new(),
        };
        assert_eq!(
            compact(&to_value(&result)),
            compact(&golden["created_result"])
        );
    }

    #[test]
    fn relate_created_emit_matches_fixture() {
        let golden = fixture("relate.before_after.json");
        let emit = relate_created_emit(
            "blocked_by",
            &[uuid(FX4A_1)],
            &uuid(FX4_ACTOR),
            &uuid(FX4B_1),
            &uuid(FX4_PROJECT_B),
            FX4_EPOCH,
        );
        assert_eq!(emit.task_name(), ISSUE_ACTIVITY_TASK);
        assert!(emit.args().is_empty());
        let kwargs = emit.kwargs();
        let order: Vec<&str> = kwargs.keys().map(String::as_str).collect();
        assert_eq!(order, ISSUE_ACTIVITY_KWARG_ORDER);
        assert_eq!(
            compact(&serde_json::Value::Object(kwargs)),
            compact(&golden["created_activity_kwargs"][0])
        );
        // The `requested_data` rendering, byte for byte (CPython default
        // separators).
        assert_eq!(
            emit.requested_data,
            golden["created_activity_kwargs"][0]["requested_data"]
                .as_str()
                .expect("str")
        );
    }

    #[test]
    fn relate_unchanged_and_conflicts_match_fixture() {
        let golden = fixture("relate.before_after.json");
        let existing = vec![pair_row(FX4B_1, FX4A_1, "blocked_by")];
        // Same request from the storing side: unchanged.
        assert_eq!(
            relate_verdict(&existing, &uuid(FX4B_1), "blocked_by"),
            RelateVerdict::Unchanged
        );
        let unchanged = RelateResult {
            issue: "FX4B-1".to_owned(),
            relation_type: "blocked_by".to_owned(),
            created: Vec::new(),
            unchanged: vec!["FX4A-1".to_owned()],
            conflicts: Vec::new(),
        };
        assert_eq!(
            compact(&to_value(&unchanged)),
            compact(&golden["unchanged_result"])
        );
        // The direct 2-cycle refuses with the existing relation named
        // from the requester's side.
        assert_eq!(
            relate_verdict(&existing, &uuid(FX4A_1), "blocked_by"),
            RelateVerdict::Conflict {
                existing_relation: "blocking".to_owned()
            }
        );
        let cycle = RelateResult {
            issue: "FX4A-1".to_owned(),
            relation_type: "blocked_by".to_owned(),
            created: Vec::new(),
            unchanged: Vec::new(),
            conflicts: vec![RelateConflict {
                identifier: "FX4B-1".to_owned(),
                existing_relation: "blocking".to_owned(),
            }],
        };
        assert_eq!(
            compact(&to_value(&cycle)),
            compact(&golden["two_cycle_conflict_result"])
        );
        // A different type on the same pair conflicts identically.
        assert_eq!(
            relate_verdict(&existing, &uuid(FX4A_1), "relates_to"),
            RelateVerdict::Conflict {
                existing_relation: "blocking".to_owned()
            }
        );
        let other_type = RelateResult {
            issue: "FX4A-1".to_owned(),
            relation_type: "relates_to".to_owned(),
            created: Vec::new(),
            unchanged: Vec::new(),
            conflicts: vec![RelateConflict {
                identifier: "FX4B-1".to_owned(),
                existing_relation: "blocking".to_owned(),
            }],
        };
        assert_eq!(
            compact(&to_value(&other_type)),
            compact(&golden["different_type_conflict_result"])
        );
    }

    #[test]
    fn check_targets_errors_match_fixture() {
        let golden = fixture("relate.before_after.json");
        let workspace = "99999999-aaaa-bbbb-cccc-000000004000";
        let source = resolved(FX4A_1, workspace, FX4_PROJECT_A, "FX4A", 1);
        let err = check_targets(&source, std::slice::from_ref(&source)).expect_err("self errors");
        assert_eq!(
            err.message,
            golden["self_relation_error"].as_str().expect("str")
        );
        let foreign = resolved(
            "99999999-aaaa-bbbb-cccc-000000004201",
            "99999999-aaaa-bbbb-cccc-000000004099",
            "99999999-aaaa-bbbb-cccc-000000004098",
            "FX4X",
            1,
        );
        let err = check_targets(&source, &[foreign]).expect_err("workspace errors");
        assert_eq!(
            err.message,
            golden["cross_workspace_error"].as_str().expect("str")
        );
        // Duplicates collapse, first-seen order kept.
        let target = resolved(FX4B_1, workspace, FX4_PROJECT_B, "FX4B", 1);
        let unique = check_targets(&source, &[target.clone(), target.clone()]).expect("dedup ok");
        assert_eq!(unique, vec![target]);
    }

    #[test]
    fn relate_insert_sql_shape() {
        let sql = relate_insert_sql();
        // Column order is the db-crate column order, quoted.
        let expected_columns = issue_relation::COLUMNS
            .iter()
            .map(|column| format!("\"{column}\""))
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            sql.contains(&format!("({expected_columns})")),
            "columns in COLUMNS order: {sql}"
        );
        assert!(sql.contains("NULL, NULL"), "audit NULLs: {sql}");
        assert_params(&sql, RELATE_INSERT_PARAMS);
    }

    /// `relate_race.golden.json`: the hidden-pair attempt raises 23505,
    /// the re-read verdicts `Unchanged`.
    #[test]
    fn relate_race_matches_fixture() {
        let golden = fixture("relate_race.golden.json");
        // Two `_pair_rows` reads: the initial (hidden) one plus the
        // post-`IntegrityError` re-read.
        assert_eq!(golden["pair_rows_calls"].as_u64(), Some(2));
        // First read: empty, so the handler attempts the INSERT (which
        // the live pair turns into 23505).
        assert_eq!(
            relate_verdict(&[], &uuid(FX4A_4), "relates_to"),
            RelateVerdict::Create
        );
        // Re-read: the pair is there, carrying the requested type.
        let reread = vec![pair_row(FX4A_4, FX4A_5, "relates_to")];
        assert_eq!(
            relate_verdict(&reread, &uuid(FX4A_4), "relates_to"),
            RelateVerdict::Unchanged
        );
        let result = RelateResult {
            issue: "FX4A-4".to_owned(),
            relation_type: "relates_to".to_owned(),
            created: Vec::new(),
            unchanged: vec!["FX4A-5".to_owned()],
            conflicts: Vec::new(),
        };
        assert_eq!(compact(&to_value(&result)), compact(&golden["race_result"]));
    }

    /// `unrelate.golden.json`: exact-type removal, idempotent repeat,
    /// reverse-name removal, per-target emits.
    #[test]
    fn unrelate_wrong_type_matches_fixture() {
        let golden = fixture("unrelate.golden.json");
        // The live pair carries `relates_to`, not the requested type.
        let existing = vec![pair_row(FX4A_6, FX4A_7, "relates_to")];
        let shape = &golden["wrong_type_row_still_live"];
        assert_eq!(shape["issue"], "FX4A-6");
        assert_eq!(shape["related_issue"], "FX4A-7");
        assert_eq!(
            shape["stored_type"].as_str().expect("str"),
            existing[0].relation_type.as_str()
        );
        assert!(shape["deleted_at"].is_null());
        assert!(unrelate_matches(&existing, &uuid(FX4A_6), "blocked_by").is_empty());
        let result = UnrelateResult {
            issue: "FX4A-6".to_owned(),
            relation_type: "blocked_by".to_owned(),
            removed: Vec::new(),
            not_related: vec!["FX4A-7".to_owned()],
        };
        assert_eq!(
            compact(&to_value(&result)),
            compact(&golden["wrong_type_result"])
        );
    }

    #[test]
    fn unrelate_removed_and_repeat_match_fixture() {
        let golden = fixture("unrelate.golden.json");
        let existing = vec![pair_row(FX4A_6, FX4A_7, "relates_to")];
        let matched = unrelate_matches(&existing, &uuid(FX4A_6), "relates_to");
        assert_eq!(matched, vec![&existing[0]]);
        // The soft-delete stamps `deleted_at` (plus the full-save
        // `updated_at` / `updated_by_id`, quirk 2).
        assert_eq!(golden["removed_row_deleted_at_set"].as_bool(), Some(true));
        let removed = UnrelateResult {
            issue: "FX4A-6".to_owned(),
            relation_type: "relates_to".to_owned(),
            removed: vec!["FX4A-7".to_owned()],
            not_related: Vec::new(),
        };
        assert_eq!(
            compact(&to_value(&removed)),
            compact(&golden["removed_result"])
        );
        // Repeat over the now-empty pair: idempotent `not_related`.
        assert!(unrelate_matches(&[], &uuid(FX4A_6), "relates_to").is_empty());
        let repeat = UnrelateResult {
            issue: "FX4A-6".to_owned(),
            relation_type: "relates_to".to_owned(),
            removed: Vec::new(),
            not_related: vec!["FX4A-7".to_owned()],
        };
        assert_eq!(
            compact(&to_value(&repeat)),
            compact(&golden["repeat_result"])
        );
    }

    #[test]
    fn unrelate_reverse_name_matches_fixture() {
        let golden = fixture("unrelate.golden.json");
        // Stored `(FX4A-9 -> FX4A-8, blocked_by)` reads as `blocking`
        // from FX4A-8's side, so the reverse-named request matches.
        let existing = vec![pair_row(FX4A_9, FX4A_8, "blocked_by")];
        assert_eq!(
            type_from(&existing[0], &uuid(FX4A_8)),
            "blocking".to_owned()
        );
        let matched = unrelate_matches(&existing, &uuid(FX4A_8), "blocking");
        assert_eq!(matched, vec![&existing[0]]);
        let result = UnrelateResult {
            issue: "FX4A-8".to_owned(),
            relation_type: "blocking".to_owned(),
            removed: vec!["FX4A-9".to_owned()],
            not_related: Vec::new(),
        };
        assert_eq!(
            compact(&to_value(&result)),
            compact(&golden["reverse_name_remove_result"])
        );
    }

    #[test]
    fn unrelate_emits_match_fixture() {
        let golden = fixture("unrelate.golden.json");
        let first = unrelate_deleted_emit(
            "relates_to",
            &uuid(FX4A_7),
            &uuid(FX4_ACTOR),
            &uuid(FX4A_6),
            &uuid(FX4_PROJECT_A),
            FX4_EPOCH,
        );
        assert_eq!(first.task_name(), ISSUE_ACTIVITY_TASK);
        assert!(first.args().is_empty());
        assert_eq!(
            compact(&serde_json::Value::Object(first.kwargs())),
            compact(&golden["deleted_activity_kwargs"][0])
        );
        assert_eq!(
            first.requested_data,
            golden["deleted_activity_kwargs"][0]["requested_data"]
                .as_str()
                .expect("str")
        );
        assert_eq!(
            first.current_instance.as_deref(),
            golden["deleted_activity_kwargs"][0]["current_instance"].as_str()
        );
        let second = unrelate_deleted_emit(
            "blocking",
            &uuid(FX4A_9),
            &uuid(FX4_ACTOR),
            &uuid(FX4A_8),
            &uuid(FX4_PROJECT_A),
            FX4_EPOCH,
        );
        assert_eq!(
            compact(&serde_json::Value::Object(second.kwargs())),
            compact(&golden["deleted_activity_kwargs"][1])
        );
        assert_eq!(
            second.current_instance.as_deref(),
            Some("{\"relation_type\": \"blocking\"}")
        );
    }

    #[test]
    fn unrelate_delete_sql_shape() {
        let sql = unrelate_delete_sql();
        assert!(
            sql.contains("SET \"deleted_at\" = :now"),
            "deleted_at stamp: {sql}"
        );
        assert!(
            sql.contains("\"updated_at\" = :now"),
            "auto_now stamp: {sql}"
        );
        assert!(
            sql.contains("\"updated_by_id\" = :actor_id"),
            "save() stamp: {sql}"
        );
        assert!(sql.contains("WHERE \"id\" = :row_id"), "pk scope: {sql}");
        assert_params(&sql, UNRELATE_DELETE_PARAMS);
    }

    /// `grouped_relations.golden.json`: all-ten-keys grouping, legacy
    /// normalisation, visibility narrowing, sort order, 100-cap.
    fn grouped_issue(item: &serde_json::Value) -> GroupedIssue {
        let identifier = item["identifier"].as_str().expect("identifier is str");
        let (project, seq) = identifier.rsplit_once('-').expect("PROJ-seq");
        GroupedIssue {
            id: uuid(item["id"].as_str().expect("id is str")),
            project_identifier: project.to_owned(),
            sequence_id: seq.parse().expect("sequence_id is int"),
            name: item["name"].as_str().expect("name is str").to_owned(),
            state_name: item["state"].as_str().map(str::to_owned),
            state_group: item["state_group"].as_str().map(str::to_owned),
        }
    }

    fn pool_from_golden(grouped: &serde_json::Value) -> Vec<GroupedIssue> {
        let mut pool = Vec::new();
        for relation in RELATION_TYPES {
            for item in grouped[relation].as_array().expect("group is array") {
                pool.push(grouped_issue(item));
            }
        }
        pool
    }

    // The hub FX4A-10 (id unpinned by the fixture; any fixed value).
    const FX4_HUB: &str = "99999999-aaaa-bbbb-cccc-000000004110";

    /// The 12 live rows behind `grouped_unscoped`, from the hub's side:
    /// 11 forward/stored rows plus the legacy reverse-stored row for
    /// FX4A-12 (`(FX4A-12 -> FX4A-10, blocking)`).
    fn unscoped_rows() -> Vec<PairRow> {
        vec![
            pair_row(FX4_HUB, FX4A_1, "blocked_by"),
            pair_row(FX4_HUB, FX4A_11, "blocked_by"),
            pair_row(FX4A_12, FX4_HUB, "blocking"),
            pair_row(FX4A_7, FX4_HUB, "blocked_by"),
            pair_row(FX4_HUB, FX4A_2, "relates_to"),
            pair_row(FX4_HUB, FX4A_3, "duplicate"),
            pair_row(FX4_HUB, FX4A_6, "start_before"),
            pair_row(FX4A_9, FX4_HUB, "start_before"),
            pair_row(FX4_HUB, FX4A_8, "finish_before"),
            pair_row(FX4A_5, FX4_HUB, "finish_before"),
            pair_row(FX4_HUB, FX4A_4, "implemented_by"),
            pair_row(FX4B_1, FX4_HUB, "implemented_by"),
        ]
    }

    #[test]
    fn grouped_unscoped_matches_fixture() {
        let golden = fixture("grouped_relations.golden.json");
        let pool = pool_from_golden(&golden["grouped_unscoped"]);
        assert_eq!(pool.len(), 12);
        let grouped = grouped_relations(&uuid(FX4_HUB), &unscoped_rows(), &pool);
        assert_eq!(
            compact(&to_value(&grouped)),
            compact(&golden["grouped_unscoped"])
        );
        // Sort demo: numeric sequence order within the group.
        let blocked: Vec<&str> = grouped
            .blocked_by
            .iter()
            .map(|item| item.identifier.as_str())
            .collect();
        let expected: Vec<&str> = golden["sort_demo_blocked_by"]
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect();
        assert_eq!(blocked, expected);
        // Legacy row: stored reversed, read as `blocked_by` from the hub.
        let legacy = &golden["legacy_row"];
        assert_eq!(
            type_from(&pair_row(FX4A_12, FX4_HUB, "blocking"), &uuid(FX4_HUB)),
            legacy["reads_from_hub_as"].as_str().expect("str")
        );
        assert_eq!(blocked, {
            let expected: Vec<&str> = legacy["hub_blocked_by"]
                .as_array()
                .expect("array")
                .iter()
                .map(|v| v.as_str().expect("str"))
                .collect();
            expected
        });
    }

    #[test]
    fn grouped_visibility_narrowing_matches_fixture() {
        let golden = fixture("grouped_relations.golden.json");
        // The FX4A-member pool drops the FX4B other end.
        let pool: Vec<GroupedIssue> = pool_from_golden(&golden["grouped_unscoped"])
            .into_iter()
            .filter(|issue| issue.id != uuid(FX4B_1))
            .collect();
        let grouped = grouped_relations(&uuid(FX4_HUB), &unscoped_rows(), &pool);
        assert_eq!(
            compact(&to_value(&grouped)),
            compact(&golden["grouped_visible_member_FX4A"])
        );
        let narrowing = &golden["visibility_narrowing"];
        assert_eq!(narrowing["implements_unscoped"][0], "FX4B-1");
        assert!(narrowing["implements_visible"]
            .as_array()
            .expect("array")
            .is_empty());
        assert!(grouped.implements.is_empty());
    }

    #[test]
    fn grouped_keys_always_present() {
        let golden = fixture("grouped_relations.golden.json");
        let grouped = grouped_relations(&uuid(FX4_HUB), &[], &[]);
        let value = to_value(&grouped);
        let object = value.as_object().expect("grouped is object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected: Vec<&str> = golden["keys_always_present"]
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str"))
            .collect();
        expected.sort_unstable();
        assert_eq!(keys, expected);
        assert!(object
            .values()
            .all(|group| group.as_array().expect("group is array").is_empty()));
        // Junk stored types never surface (the `:295` guard).
        let junk = vec![pair_row(FX4_HUB, FX4A_1, "mystery-meat")];
        let pool = vec![GroupedIssue {
            id: uuid(FX4A_1),
            project_identifier: "FX4A".to_owned(),
            sequence_id: 1,
            name: "FX4 A one".to_owned(),
            state_name: Some("Todo".to_owned()),
            state_group: Some("unstarted".to_owned()),
        }];
        let grouped = grouped_relations(&uuid(FX4_HUB), &junk, &pool);
        assert!(to_value(&grouped)
            .as_object()
            .expect("object")
            .values()
            .all(|group| group.as_array().expect("array").is_empty()));
    }

    /// `_item` on a stateless issue renders explicit `null`s (the
    /// `None`-vs-absent-key trap, `relations.py:275-276`).
    #[test]
    fn grouped_item_stateless_renders_nulls() {
        let issue = GroupedIssue {
            id: uuid(FX4A_1),
            project_identifier: "FX4A".to_owned(),
            sequence_id: 1,
            name: "FX4 A one".to_owned(),
            state_name: None,
            state_group: None,
        };
        assert_eq!(
            compact(&to_value(&grouped_item(&issue))),
            format!(
                "{{\"id\":\"{FX4A_1}\",\"identifier\":\"FX4A-1\",\"name\":\"FX4 A one\",\
                 \"state\":null,\"state_group\":null}}"
            )
        );
    }

    #[test]
    fn grouped_cap_matches_fixture() {
        let golden = fixture("grouped_relations.golden.json");
        let cap = &golden["cap_case"];
        // 101 `relates_to` targets, sequences 14..=114.
        let mut rows = Vec::new();
        let mut pool = Vec::new();
        for sequence_id in 14..=114 {
            let id = uuid(&format!("99999999-aaaa-bbbb-cccc-{sequence_id:012x}"));
            rows.push(PairRow {
                id,
                issue_id: uuid(FX4_HUB),
                related_issue_id: id,
                relation_type: "relates_to".to_owned(),
            });
            pool.push(GroupedIssue {
                id,
                project_identifier: "FX4A".to_owned(),
                sequence_id,
                name: format!("FX4 cap {sequence_id}"),
                state_name: Some("Todo".to_owned()),
                state_group: Some("unstarted".to_owned()),
            });
        }
        let grouped = grouped_relations(&uuid(FX4_HUB), &rows, &pool);
        assert_eq!(grouped.relates_to.len(), GROUP_LIMIT);
        assert_eq!(
            grouped.relates_to.len() as u64,
            cap["len"].as_u64().expect("u64")
        );
        assert_eq!(
            grouped.relates_to.first().expect("first").identifier,
            cap["first"].as_str().expect("str")
        );
        assert_eq!(
            grouped.relates_to.last().expect("last").identifier,
            cap["last"].as_str().expect("str")
        );
    }

    #[test]
    fn pair_and_grouped_sql_shape() {
        let pair = pair_rows_sql();
        assert!(pair.contains("FROM \"issue_relations\" r"), "{pair}");
        assert!(
            pair.contains("r.\"issue_id\" = :a_id AND r.\"related_issue_id\" = :b_id"),
            "forward: {pair}"
        );
        assert!(
            pair.contains("r.\"issue_id\" = :b_id AND r.\"related_issue_id\" = :a_id"),
            "reverse: {pair}"
        );
        assert!(pair.contains("r.\"deleted_at\" IS NULL"), "live: {pair}");
        assert_params(&pair, PAIR_ROWS_PARAMS);
        let grouped = grouped_relations_sql();
        assert!(
            grouped.contains("r.\"issue_id\" = :issue_id OR r.\"related_issue_id\" = :issue_id"),
            "anchor: {grouped}"
        );
        assert!(
            grouped.contains("r.\"workspace_id\" = :workspace_id"),
            "workspace: {grouped}"
        );
        assert!(
            grouped.contains(
                "NOT (r.\"issue_id\" = :issue_id AND r.\"related_issue_id\" = :issue_id)"
            ),
            "self-pair exclusion: {grouped}"
        );
        assert!(
            grouped.contains("r.\"deleted_at\" IS NULL"),
            "live: {grouped}"
        );
        assert_params(&grouped, GROUPED_RELATIONS_PARAMS);
    }
}
