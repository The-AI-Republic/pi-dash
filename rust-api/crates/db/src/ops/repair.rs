//! D-37 data-repair commands: SQL + row mapping (stage 7, PIDASHCONV-808).
//!
//! Ports the database surface of the five repair commands
//! (`apps/api/pi_dash/db/management/commands/copy_issue_comment_to_description.py:1-53`,
//! `fix_duplicate_sequences.py:1-95`, `sync_issue_version.py:1-21`,
//! `sync_issue_description_version.py:1-23`,
//! `update_deleted_workspace_slug.py:1-71`; drift baseline
//! `01a93e17216faea7bfc156b0f864cbbe420d1c52`):
//! batch SQL, the advisory-lock key, the renumber rule, and the
//! version-sync payload shape. Decision logic (which branch, which
//! message) lives in `pidash_services::ops::repair`; prompts, printing,
//! exit codes and the Celery publish live in the binary's `ops::repair`.
//! Execution uses runtime `sqlx::query` (no `query!` macros): there is no
//! build-time database, per the merged precedent.
//!
//! Fixtures: `rust-api/fixtures/ops/commands/repair_sql.sql`,
//! `repair.rows.json` (F37-05), `version_sync.golden.json` (F37-06).
//!
//! # Ported semantics (translate, don't redesign)
//!
//! * `copy`: batches of 500 (`COPY_BATCH_SIZE`), `description_id IS NULL`
//!   ordered by `created_at ASC`, one transaction per batch. The explicit
//!   `created_at`/`updated_at` the constructor passes are overwritten by
//!   `auto_now_add`/`auto_now` (verified against live Django: new rows
//!   carry batch time, not the comment's), so the port stamps one `now`
//!   per batch for both columns. `bulk_update` does NOT bump `updated_at`
//!   (verified), so the link step writes `description_id` only.
//! * `fix`: `identifier__iexact` renders as
//!   `UPPER(identifier) = UPPER($1)`; the `workspace__slug` join carries
//!   no `deleted_at` filter (Django never applies the related model's
//!   default manager). `issues[1:]` rides `Meta.ordering -created_at`
//!   (newest keeps its id); the `IssueSequence` id-map iterates the same
//!   `-created_at` order with last-wins, so the oldest row wins a shared
//!   `issue_id` (verified). `bulk_update` writes `sequence_id`/`sequence`
//!   only, no `updated_at` bump (verified).
//! * `slug`: `all_objects` is the unfiltered manager, so the lookup has
//!   no `deleted_at` filter; `save(update_fields=["slug"])` writes the
//!   `slug` column only (verified — `updated_at` untouched).
//! * `sync_*`: `.delay(batch_size=<raw str>, countdown=int(<str>))` —
//!   `batch_size` stays a string (ported bug, see below); `countdown`
//!   follows CPython `int()` (surrounding whitespace, `+`/`-`, single
//!   underscores between digits).
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * `batch_size` reaches the task as the raw `input()` string while the
//!   task default is the int `5000` ([`version_sync_kwarg_pairs`]).
//! * `fix` with no `IssueSequence` rows raises
//!   `TypeError: unsupported operand type(s) for +: 'NoneType' and 'int'`
//!   ([`NONE_SEQUENCE_MESSAGE`]); the port emits the identical text.
//! * `slug`'s success message prints the already-mutated slug twice;
//!   the binary renders it verbatim (see its module docs).
//!
//! # Known divergences (untestable across implementations)
//!
//! * `strict_str_to_int` gates on ASCII digits; CPython `str.isdigit`
//!   also accepts non-ASCII decimal digits (e.g. `int("١٢٣") == 123`).
//! * `parse_py_int` accepts ASCII digits only and `i64` range; CPython
//!   accepts non-ASCII decimals and unbounded magnitudes (an overflowed
//!   countdown is rejected here, published there).
//! * `slug_already_stamped` checks ASCII digits; CPython `isdigit` is
//!   wider (unreachable via `SlugField` input).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Batch size of the copy loop
/// (`copy_issue_comment_to_description.py:18`).
pub const COPY_BATCH_SIZE: i64 = 500;

/// Celery task enqueued by `sync_issue_version`
/// (`bgtasks/issue_version_sync.py:234-235`, `@shared_task` default name).
pub const TASK_SCHEDULE_ISSUE_VERSION: &str =
    "pi_dash.bgtasks.issue_version_sync.schedule_issue_version";
/// Celery task enqueued by `sync_issue_description_version`
/// (`bgtasks/issue_description_version_sync.py:123-124`).
pub const TASK_SCHEDULE_ISSUE_DESCRIPTION_VERSION: &str =
    "pi_dash.bgtasks.issue_description_version_sync.schedule_issue_description_version";

/// `TypeError` text of `None + index + 1` when a project has duplicate
/// issues but no `IssueSequence` rows (`fix_duplicate_sequences.py:80`,
/// wrapped by `:94-95`); verified against live Django.
pub const NONE_SEQUENCE_MESSAGE: &str = "unsupported operand type(s) for +: 'NoneType' and 'int'";
/// `strict_str_to_int` rejection (`fix_duplicate_sequences.py:22-25`).
pub const INVALID_INTEGER_MESSAGE: &str = "Invalid integer string";
/// Identifier shape rejection (`fix_duplicate_sequences.py:43-44`).
pub const INVALID_IDENTIFIER_MESSAGE: &str = "Invalid issue identifier format";

/// Why a repair database call failed.
#[derive(Debug, thiserror::Error)]
pub enum RepairError {
    /// Postgres or pool failure (Django would traceback; the binary
    /// renders this on stderr with exit 1 instead).
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

/// `convert_uuid_to_integer` (`utils/uuid.py:19-26`): `sha256(str(uuid))`,
/// first 8 digest bytes as big-endian signed int64. Feeds
/// `pg_advisory_xact_lock` (`fix_duplicate_sequences.py:61-66`).
pub fn convert_uuid_to_integer(id: &Uuid) -> i64 {
    let digest = Sha256::digest(id.to_string().as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

/// `strict_str_to_int` (`fix_duplicate_sequences.py:22-25`): all digits,
/// or `-` plus digits. Values past `i64` saturate: the `issues.sequence_id`
/// column is `int4`, so saturation never matches a row — the same
/// observable outcome as CPython's unbounded filter that matches nothing.
pub fn strict_str_to_int(s: &str) -> Result<i64, String> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    let gated = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    if !gated {
        return Err(INVALID_INTEGER_MESSAGE.to_owned());
    }
    match s.parse::<i64>() {
        Ok(value) => Ok(value),
        Err(e) if *e.kind() == std::num::IntErrorKind::PosOverflow => Ok(i64::MAX),
        Err(e) if *e.kind() == std::num::IntErrorKind::NegOverflow => Ok(i64::MIN),
        Err(_) => Err(INVALID_INTEGER_MESSAGE.to_owned()),
    }
}

/// Split `issue_identifier` on `-` (`fix_duplicate_sequences.py:41-47`).
/// Returns `(project_identifier, sequence)`.
pub fn parse_issue_identifier(s: &str) -> Result<(String, i64), String> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 2 {
        return Err(INVALID_IDENTIFIER_MESSAGE.to_owned());
    }
    let sequence = strict_str_to_int(parts[1])?;
    Ok((parts[0].to_owned(), sequence))
}

/// CPython `int()` for the `sync_*` countdown (`sync_issue_version.py:19`,
/// `sync_issue_description_version.py:21`): surrounding whitespace is
/// stripped, one `+`/`-` sign allowed, single underscores between digits.
/// Failure renders CPython's `invalid literal` text over the raw input.
pub fn parse_py_int(s: &str) -> Result<i64, String> {
    let invalid = || format!("invalid literal for int() with base 10: '{s}'");
    let trimmed = s.trim();
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if digits.is_empty() {
        return Err(invalid());
    }
    let mut cleaned = String::with_capacity(digits.len());
    let mut prev_underscore = false;
    for (i, c) in digits.chars().enumerate() {
        if c == '_' {
            if i == 0 || prev_underscore {
                return Err(invalid());
            }
            prev_underscore = true;
            continue;
        }
        if !c.is_ascii_digit() {
            return Err(invalid());
        }
        prev_underscore = false;
        cleaned.push(c);
    }
    if prev_underscore || cleaned.is_empty() {
        return Err(invalid());
    }
    let signed = if trimmed.starts_with('-') {
        format!("-{cleaned}")
    } else {
        cleaned
    };
    signed.parse::<i64>().map_err(|_| invalid())
}

/// Renumber rule (`fix_duplicate_sequences.py:79-82`): `issues[1:]` (in
/// the `-created_at` order the caller fetched) becomes `last + index + 1`,
/// `index` 0-based over the duplicates. `None` (no `IssueSequence` rows)
/// is the `TypeError` path. Saturates near `i64::MAX` (the `int4` write
/// then range-errors, as Django's out-of-range write does).
pub fn renumber_plan(
    last_sequence: Option<i64>,
    duplicate_issue_ids: &[Uuid],
) -> Result<Vec<(Uuid, i64)>, String> {
    let last = last_sequence.ok_or_else(|| NONE_SEQUENCE_MESSAGE.to_owned())?;
    Ok(duplicate_issue_ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let next = last.saturating_add(index as i64).saturating_add(1);
            (*id, next)
        })
        .collect())
}

/// `issue_sequence_map` (`fix_duplicate_sequences.py:76`): `issue_id` to
/// row over the `-created_at` fetch order, last write wins (the oldest
/// row wins a shared `issue_id`). Rows with `NULL issue_id` are inert
/// (Python keys them under `None`, which no issue id ever equals).
pub fn build_sequence_map(rows: &[SequenceRow]) -> HashMap<Uuid, SequenceRow> {
    let mut map = HashMap::with_capacity(rows.len());
    for row in rows {
        if let Some(issue_id) = row.issue_id {
            map.insert(issue_id, row.clone());
        }
    }
    map
}

/// `bulk_issue_sequences` selection (`fix_duplicate_sequences.py:84-88`):
/// for each renumbered issue, the mapped sequence row (if any) takes the
/// same new value. Returns `(sequence_row_id, new_sequence)` pairs.
pub fn plan_sequence_updates(
    map: &HashMap<Uuid, SequenceRow>,
    issue_updates: &[(Uuid, i64)],
) -> Vec<(Uuid, i64)> {
    issue_updates
        .iter()
        .filter_map(|(issue_id, new_sequence)| map.get(issue_id).map(|row| (row.id, *new_sequence)))
        .collect()
}

/// Already-stamped guard (`update_deleted_workspace_slug.py:44`):
/// `"__" in slug and slug.split("__")[-1].isdigit()`.
pub fn slug_already_stamped(slug: &str) -> bool {
    slug.contains("__")
        && slug
            .rsplit("__")
            .next()
            .is_some_and(|last| !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()))
}

/// `new_slug` (`update_deleted_workspace_slug.py:53-56`):
/// `{slug}__{int(deleted_at.timestamp())}`.
pub fn stamped_slug(slug: &str, deleted_at: &DateTime<Utc>) -> String {
    format!("{slug}__{}", deleted_at.timestamp())
}

/// `.delay` kwargs in call order (`sync_issue_version.py:19`,
/// `sync_issue_description_version.py:21`): `batch_size` is the raw
/// `input()` string (ported bug), `countdown` the converted int. The
/// caller assembles the insertion-ordered map.
pub fn version_sync_kwarg_pairs(
    batch_size: &str,
    countdown: i64,
) -> [(String, serde_json::Value); 2] {
    [
        (
            "batch_size".to_owned(),
            serde_json::Value::String(batch_size.to_owned()),
        ),
        (
            "countdown".to_owned(),
            serde_json::Value::Number(countdown.into()),
        ),
    ]
}

/// Comment columns the copy consumes
/// (`copy_issue_comment_to_description.py:29-42`). `created_at`/`updated_at`
/// are not projected: the values the port inserts are always batch `now`
/// (see the module docs), and ordering needs no projection.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentRow {
    pub id: Uuid,
    pub comment_json: serde_json::Value,
    pub comment_html: String,
    pub comment_stripped: Option<String>,
    pub project_id: Uuid,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub workspace_id: Uuid,
}

/// A `descriptions` row to insert: the comment mapping plus the fresh id
/// and batch `now` (Django assigns `uuid4` client-side and `auto_now`).
#[derive(Debug, Clone, PartialEq)]
pub struct NewDescription {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub description_json: serde_json::Value,
    pub description_html: String,
    pub description_stripped: Option<String>,
    pub project_id: Option<Uuid>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub workspace_id: Uuid,
}

/// Issue columns the renumber consumes.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueRow {
    pub id: Uuid,
    pub sequence_id: i32,
}

/// `issue_sequences` columns the renumber consumes.
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceRow {
    pub id: Uuid,
    pub issue_id: Option<Uuid>,
    pub sequence: i64,
}

/// Project columns the fix consumes (the id feeds the lock key and the
/// downstream filters).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRow {
    pub id: Uuid,
}

/// Workspace columns the slug command consumes.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceRow {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// `filter(description_id__isnull=True).order_by("created_at")[:500]`
/// (`copy_issue_comment_to_description.py:21-23`) over the default
/// manager (`deleted_at IS NULL`).
pub const COMMENT_BATCH_SQL: &str = "SELECT id, comment_json, comment_html, \
    comment_stripped, project_id, created_by_id, updated_by_id, workspace_id \
    FROM issue_comments WHERE deleted_at IS NULL AND description_id IS NULL \
    ORDER BY created_at ASC LIMIT $1";

/// `bulk_update(..., ["description_id"])`
/// (`copy_issue_comment_to_description.py:51`): one column, no
/// `updated_at` bump (verified against live Django).
pub const LINK_COMMENT_SQL: &str = "UPDATE issue_comments SET description_id = $1 WHERE id = $2";

/// `Project.objects.get(identifier__iexact=..., workspace__slug=...)`
/// (`fix_duplicate_sequences.py:50`): no `LIMIT` — `get()` needs the full
/// count for the 0/1/N decision, and no `deleted_at` filter on the joined
/// `workspaces` (Django never applies the related default manager).
pub const FIND_PROJECT_SQL: &str = "SELECT p.id FROM projects p \
    INNER JOIN workspaces w ON w.id = p.workspace_id \
    WHERE p.deleted_at IS NULL AND UPPER(p.identifier) = UPPER($1) \
    AND w.slug = $2";

/// `Issue.objects.filter(project=..., sequence_id=...)`
/// (`fix_duplicate_sequences.py:53`) in `Meta.ordering -created_at`.
pub const DUPLICATE_ISSUES_SQL: &str = "SELECT id, sequence_id FROM issues \
    WHERE deleted_at IS NULL AND project_id = $1 AND sequence_id = $2 \
    ORDER BY created_at DESC";

/// `SELECT pg_advisory_xact_lock(%s)`
/// (`fix_duplicate_sequences.py:66`).
pub const ADVISORY_LOCK_SQL: &str = "SELECT pg_advisory_xact_lock($1)";

/// `Max("sequence")` over the default manager
/// (`fix_duplicate_sequences.py:68-71`).
pub const MAX_SEQUENCE_SQL: &str = "SELECT MAX(sequence) FROM issue_sequences \
    WHERE deleted_at IS NULL AND project_id = $1";

/// The id-map queryset (`fix_duplicate_sequences.py:76`) in `Meta.ordering
/// -created_at` (order feeds the last-wins rule).
pub const PROJECT_SEQUENCES_SQL: &str = "SELECT id, issue_id, sequence \
    FROM issue_sequences WHERE deleted_at IS NULL AND project_id = $1 \
    ORDER BY created_at DESC";

/// `bulk_update(bulk_issues, ["sequence_id"])`
/// (`fix_duplicate_sequences.py:90`): one column, no `updated_at` bump
/// (verified against live Django).
pub const UPDATE_ISSUE_SEQUENCE_ID_SQL: &str = "UPDATE issues SET sequence_id = $1 WHERE id = $2";

/// `bulk_update(bulk_issue_sequences, ["sequence"])`
/// (`fix_duplicate_sequences.py:91`): one column, no `updated_at` bump
/// (verified against live Django).
pub const UPDATE_SEQUENCE_SQL: &str = "UPDATE issue_sequences SET sequence = $1 WHERE id = $2";

/// `Workspace.all_objects.get(slug=...)`
/// (`update_deleted_workspace_slug.py:31`): the unfiltered manager, so no
/// `deleted_at` predicate. `slug` is unique, hence `LIMIT 1`.
pub const FIND_WORKSPACE_SQL: &str =
    "SELECT id, name, slug, deleted_at FROM workspaces WHERE slug = $1 LIMIT 1";

/// `save(update_fields=["slug"])`
/// (`update_deleted_workspace_slug.py:63-64`): the `slug` column only
/// (verified — `updated_at` untouched).
pub const UPDATE_WORKSPACE_SLUG_SQL: &str = "UPDATE workspaces SET slug = $1 WHERE id = $2";

/// Columns of the copy `INSERT`, in bind order.
const DESCRIPTION_INSERT_COLUMNS: &str = "(id, created_at, updated_at, \
    description_json, description_html, description_stripped, project_id, \
    created_by_id, updated_by_id, workspace_id)";

/// Multi-row `INSERT INTO descriptions ... RETURNING id` for `bulk_create`
/// (`copy_issue_comment_to_description.py:44`): one statement per batch,
/// Postgres `$N` placeholders, zip-aligned `RETURNING id` like Django's.
pub fn descriptions_insert_sql(row_count: usize) -> String {
    let mut sql = format!("INSERT INTO descriptions {DESCRIPTION_INSERT_COLUMNS} VALUES ");
    for row in 0..row_count {
        if row > 0 {
            sql.push_str(", ");
        }
        let base = row * 10;
        sql.push('(');
        for col in 0..10 {
            if col > 0 {
                sql.push_str(", ");
            }
            sql.push_str(&format!("${}", base + col + 1));
        }
        sql.push(')');
    }
    sql.push_str(" RETURNING id");
    sql
}

fn map_comment_row(row: &sqlx::postgres::PgRow) -> Result<CommentRow, sqlx::Error> {
    let comment_json: sqlx::types::Json<serde_json::Value> = row.try_get("comment_json")?;
    Ok(CommentRow {
        id: row.try_get("id")?,
        comment_json: comment_json.0,
        comment_html: row.try_get("comment_html")?,
        comment_stripped: row.try_get("comment_stripped")?,
        project_id: row.try_get("project_id")?,
        created_by_id: row.try_get("created_by_id")?,
        updated_by_id: row.try_get("updated_by_id")?,
        workspace_id: row.try_get("workspace_id")?,
    })
}

fn map_sequence_row(row: &sqlx::postgres::PgRow) -> Result<SequenceRow, sqlx::Error> {
    Ok(SequenceRow {
        id: row.try_get("id")?,
        issue_id: row.try_get("issue_id")?,
        sequence: row.try_get("sequence")?,
    })
}

/// One copy batch (`copy_issue_comment_to_description.py:20-26`).
pub async fn fetch_comment_batch(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<CommentRow>, RepairError> {
    let rows = sqlx::query(COMMENT_BATCH_SQL)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(map_comment_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(RepairError::Db)
}

/// One copy batch write (`copy_issue_comment_to_description.py:28-51`):
/// `bulk_create` then the `description_id` backfill in a single
/// transaction. Returns the inserted ids, zip-aligned with `descriptions`.
pub async fn apply_copy_batch(
    pool: &PgPool,
    descriptions: &[NewDescription],
    links: &[(Uuid, Uuid)],
) -> Result<Vec<Uuid>, RepairError> {
    if descriptions.is_empty() {
        return Ok(Vec::new());
    }
    let mut tx = pool.begin().await?;
    let insert_sql = descriptions_insert_sql(descriptions.len());
    let mut query = sqlx::query(&insert_sql);
    for desc in descriptions {
        query = query
            .bind(desc.id)
            .bind(desc.created_at)
            .bind(desc.updated_at)
            .bind(sqlx::types::Json(&desc.description_json))
            .bind(&desc.description_html)
            .bind(&desc.description_stripped)
            .bind(desc.project_id)
            .bind(desc.created_by_id)
            .bind(desc.updated_by_id)
            .bind(desc.workspace_id);
    }
    let inserted = query.fetch_all(&mut *tx).await?;
    let mut ids = Vec::with_capacity(inserted.len());
    for row in &inserted {
        ids.push(row.try_get::<Uuid, _>("id")?);
    }
    for (comment_id, description_id) in links {
        sqlx::query(LINK_COMMENT_SQL)
            .bind(description_id)
            .bind(comment_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(ids)
}

/// Project candidates for `get()` (`fix_duplicate_sequences.py:50`).
/// The caller applies the 0/1/N decision (Django's `get()` semantics).
pub async fn find_project_rows(
    pool: &PgPool,
    identifier: &str,
    workspace_slug: &str,
) -> Result<Vec<ProjectRow>, RepairError> {
    let rows = sqlx::query(FIND_PROJECT_SQL)
        .bind(identifier)
        .bind(workspace_slug)
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(|row| {
            Ok(ProjectRow {
                id: row.try_get("id")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(RepairError::Db)
}

/// Duplicate issues in `-created_at` order
/// (`fix_duplicate_sequences.py:53,79`).
pub async fn fetch_duplicate_issues(
    pool: &PgPool,
    project_id: &Uuid,
    sequence_id: i64,
) -> Result<Vec<IssueRow>, RepairError> {
    let rows = sqlx::query(DUPLICATE_ISSUES_SQL)
        .bind(project_id)
        .bind(sequence_id)
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(|row| {
            Ok(IssueRow {
                id: row.try_get("id")?,
                sequence_id: row.try_get("sequence_id")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(RepairError::Db)
}

/// The renumber transaction (`fix_duplicate_sequences.py:59-91`):
/// advisory lock, `MAX(sequence)`, and the id-map rows, all inside one
/// transaction. Dropping without [`FixTx::commit_plan`] rolls back.
pub struct FixTx<'c> {
    tx: sqlx::Transaction<'c, sqlx::Postgres>,
    max_sequence: Option<i64>,
    sequences: Vec<SequenceRow>,
}

impl FixTx<'_> {
    /// `MAX(sequence)` read under the lock (`:68-71`).
    pub fn max_sequence(&self) -> Option<i64> {
        self.max_sequence
    }

    /// Id-map rows in `-created_at` order (`:76`).
    pub fn sequences(&self) -> &[SequenceRow] {
        &self.sequences
    }

    /// The two `bulk_update`s (`:90-91`) plus commit. Each pair is
    /// `(row_id, new_value)`.
    pub async fn commit_plan(
        mut self,
        issue_updates: &[(Uuid, i64)],
        sequence_updates: &[(Uuid, i64)],
    ) -> Result<(), RepairError> {
        for (issue_id, new_sequence) in issue_updates {
            sqlx::query(UPDATE_ISSUE_SEQUENCE_ID_SQL)
                .bind(new_sequence)
                .bind(issue_id)
                .execute(&mut *self.tx)
                .await?;
        }
        for (row_id, new_sequence) in sequence_updates {
            sqlx::query(UPDATE_SEQUENCE_SQL)
                .bind(new_sequence)
                .bind(row_id)
                .execute(&mut *self.tx)
                .await?;
        }
        self.tx.commit().await?;
        Ok(())
    }
}

/// Open [`FixTx`]: `BEGIN`, `pg_advisory_xact_lock`, `MAX(sequence)`, the
/// id-map fetch (`fix_duplicate_sequences.py:59-76`).
pub async fn begin_fix_tx<'a>(
    pool: &'a PgPool,
    project_id: &Uuid,
) -> Result<FixTx<'a>, RepairError> {
    let mut tx = pool.begin().await?;
    let lock_key = convert_uuid_to_integer(project_id);
    sqlx::query(ADVISORY_LOCK_SQL)
        .bind(lock_key)
        .execute(&mut *tx)
        .await?;
    let max_row = sqlx::query(MAX_SEQUENCE_SQL)
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await?;
    let max_sequence: Option<i64> = max_row.try_get("max")?;
    let seq_rows = sqlx::query(PROJECT_SEQUENCES_SQL)
        .bind(project_id)
        .fetch_all(&mut *tx)
        .await?;
    let sequences = seq_rows
        .iter()
        .map(map_sequence_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FixTx {
        tx,
        max_sequence,
        sequences,
    })
}

/// `Workspace.all_objects.get(slug=...)`
/// (`update_deleted_workspace_slug.py:31`).
pub async fn find_workspace(
    pool: &PgPool,
    slug: &str,
) -> Result<Option<WorkspaceRow>, RepairError> {
    let row = sqlx::query(FIND_WORKSPACE_SQL)
        .bind(slug)
        .fetch_optional(pool)
        .await?;
    row.map(|row| {
        Ok(WorkspaceRow {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            slug: row.try_get("slug")?,
            deleted_at: row.try_get("deleted_at")?,
        })
    })
    .transpose()
    .map_err(RepairError::Db)
}

/// The slug write (`update_deleted_workspace_slug.py:62-64`) in a
/// transaction: `slug` column only.
pub async fn apply_slug_update(
    pool: &PgPool,
    workspace_id: &Uuid,
    new_slug: &str,
) -> Result<(), RepairError> {
    let mut tx = pool.begin().await?;
    sqlx::query(UPDATE_WORKSPACE_SLUG_SQL)
        .bind(new_slug)
        .bind(workspace_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_key_matches_python_vector() {
        // `convert_uuid_to_integer(uuid.UUID("12345678-..."))` from the
        // repo-pinned CPython: first 8 sha256 bytes, big-endian signed.
        let id = Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap();
        assert_eq!(convert_uuid_to_integer(&id), 8400349069047396436);
        let zero = Uuid::nil();
        assert_eq!(
            convert_uuid_to_integer(&zero),
            convert_uuid_to_integer(&zero)
        );
    }

    #[test]
    fn strict_str_to_int_gates_like_python() {
        assert_eq!(strict_str_to_int("5"), Ok(5));
        assert_eq!(strict_str_to_int("007"), Ok(7));
        assert_eq!(strict_str_to_int("-0"), Ok(0));
        assert_eq!(strict_str_to_int("-5"), Ok(-5));
        for bad in [
            "", "-", "+5", " 5", "5 ", "1_0", "0x10", "3.0", "--5", "- 5",
        ] {
            assert_eq!(
                strict_str_to_int(bad),
                Err(INVALID_INTEGER_MESSAGE.to_owned()),
                "{bad:?}"
            );
        }
        // Unbounded magnitudes saturate instead of erroring (the `int4`
        // filter then matches nothing, as in Python).
        assert_eq!(strict_str_to_int("99999999999999999999999"), Ok(i64::MAX));
        assert_eq!(strict_str_to_int("-99999999999999999999999"), Ok(i64::MIN));
    }

    #[test]
    fn identifier_split_requires_two_parts() {
        assert_eq!(parse_issue_identifier("PRB-5"), Ok(("PRB".to_owned(), 5)));
        assert_eq!(parse_issue_identifier("-5"), Ok(("".to_owned(), 5)));
        for bad in ["PRB", "A-B-C", ""] {
            assert_eq!(
                parse_issue_identifier(bad),
                Err(INVALID_IDENTIFIER_MESSAGE.to_owned()),
                "{bad:?}"
            );
        }
        assert_eq!(
            parse_issue_identifier("PRB-"),
            Err(INVALID_INTEGER_MESSAGE.to_owned())
        );
        assert_eq!(
            parse_issue_identifier("PRB-abc"),
            Err(INVALID_INTEGER_MESSAGE.to_owned())
        );
    }

    #[test]
    fn py_int_matches_cpython_successes() {
        assert_eq!(parse_py_int("300"), Ok(300));
        assert_eq!(parse_py_int(" 300 "), Ok(300));
        assert_eq!(parse_py_int("+5"), Ok(5));
        assert_eq!(parse_py_int("1_0"), Ok(10));
        assert_eq!(parse_py_int("\t42\n"), Ok(42));
        assert_eq!(parse_py_int("-0"), Ok(0));
    }

    #[test]
    fn py_int_rejects_like_cpython() {
        // (Non-ASCII decimal digits like "５" are accepted by CPython
        // and rejected here; documented divergence, see module docs.)
        for bad in [
            "", "   ", "0x10", "3.0", "abc", "1__0", "_1", "1_", "+-5", "--5",
        ] {
            assert_eq!(
                parse_py_int(bad),
                Err(format!("invalid literal for int() with base 10: '{bad}'")),
                "{bad:?}"
            );
        }
        // Past `i64`: CPython would publish the big int; `serde_json`
        // cannot represent it, so this is rejected (documented).
        assert!(parse_py_int("99999999999999999999999").is_err());
    }

    #[test]
    fn renumber_rule_is_last_plus_index_plus_one() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert_eq!(renumber_plan(Some(50), &[a, b]), Ok(vec![(a, 51), (b, 52)]));
        assert_eq!(renumber_plan(Some(50), &[]), Ok(vec![]));
        assert_eq!(
            renumber_plan(None, &[a]),
            Err(NONE_SEQUENCE_MESSAGE.to_owned())
        );
        assert_eq!(renumber_plan(Some(i64::MAX), &[a]), Ok(vec![(a, i64::MAX)]));
    }

    #[test]
    fn sequence_map_last_wins_and_nulls_skip() {
        let issue = Uuid::new_v4();
        let other = Uuid::new_v4();
        let newer = SequenceRow {
            id: Uuid::new_v4(),
            issue_id: Some(issue),
            sequence: 7,
        };
        let older = SequenceRow {
            id: Uuid::new_v4(),
            issue_id: Some(issue),
            sequence: 7,
        };
        let null = SequenceRow {
            id: Uuid::new_v4(),
            issue_id: None,
            sequence: 1,
        };
        let solo = SequenceRow {
            id: Uuid::new_v4(),
            issue_id: Some(other),
            sequence: 9,
        };
        // `-created_at` fetch order: newest first.
        let rows = vec![newer.clone(), solo.clone(), older.clone(), null];
        let map = build_sequence_map(&rows);
        assert_eq!(map.len(), 2);
        assert_eq!(map[&issue].id, older.id);
        assert_eq!(map[&other].id, solo.id);

        let updates = plan_sequence_updates(&map, &[(issue, 51), (Uuid::new_v4(), 52)]);
        assert_eq!(updates, vec![(older.id, 51)]);
    }

    #[test]
    fn stamped_guard_matches_python() {
        assert!(slug_already_stamped("ws__1714976889"));
        assert!(slug_already_stamped("a__b__12"));
        for plain in ["ws", "ws__", "ws__x", "ws__1x", "__", "ws-1"] {
            assert!(!slug_already_stamped(plain), "{plain:?}");
        }
    }

    #[test]
    fn stamped_slug_appends_epoch() {
        let deleted_at = DateTime::parse_from_rfc3339("2024-05-06T07:08:09Z")
            .unwrap()
            .to_utc();
        assert_eq!(
            stamped_slug("probe808del", &deleted_at),
            "probe808del__1714979289"
        );
    }

    #[test]
    fn sync_kwargs_keep_call_order_and_str_bug() {
        let pairs = version_sync_kwarg_pairs("5000", 300);
        assert_eq!(pairs[0].0, "batch_size");
        assert_eq!(pairs[0].1, serde_json::Value::String("5000".to_owned()));
        assert_eq!(pairs[1].0, "countdown");
        assert_eq!(pairs[1].1, serde_json::json!(300));
    }

    #[test]
    fn insert_builder_numbers_placeholders() {
        let one = descriptions_insert_sql(1);
        assert!(one.starts_with("INSERT INTO descriptions ("), "{one}");
        assert!(
            one.contains("$1, $2, $3, $4, $5, $6, $7, $8, $9, $10"),
            "{one}"
        );
        assert!(one.ends_with(" RETURNING id"), "{one}");
        let two = descriptions_insert_sql(2);
        assert!(two.contains("($11, $12, $13"), "{two}");
        assert!(two.contains("$20) RETURNING id"), "{two}");
    }

    #[test]
    fn static_sql_pins_clauses() {
        assert!(COMMENT_BATCH_SQL.contains("description_id IS NULL"));
        assert!(COMMENT_BATCH_SQL.contains("ORDER BY created_at ASC"));
        assert!(LINK_COMMENT_SQL.contains("SET description_id = $1"));
        assert!(!LINK_COMMENT_SQL.contains("updated_at"));
        assert!(FIND_PROJECT_SQL.contains("UPPER(p.identifier) = UPPER($1)"));
        assert!(FIND_PROJECT_SQL.contains("INNER JOIN workspaces"));
        assert!(DUPLICATE_ISSUES_SQL.contains("ORDER BY created_at DESC"));
        assert!(MAX_SEQUENCE_SQL.contains("MAX(sequence)"));
        assert!(PROJECT_SEQUENCES_SQL.contains("ORDER BY created_at DESC"));
        assert!(!UPDATE_ISSUE_SEQUENCE_ID_SQL.contains("updated_at"));
        assert!(!UPDATE_SEQUENCE_SQL.contains("updated_at"));
        assert!(FIND_WORKSPACE_SQL.contains("SELECT id, name, slug, deleted_at"));
        assert!(!FIND_WORKSPACE_SQL.contains("deleted_at IS NULL"));
        assert!(!UPDATE_WORKSPACE_SLUG_SQL.contains("updated_at"));
    }
}
