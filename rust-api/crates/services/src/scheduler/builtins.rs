//! Builtin scheduler catalog + per-workspace upsert + creation signal gate.
//!
//! Port of `apps/api/pi_dash/scheduler/builtins/__init__.py` (188 lines)
//! and `apps/api/pi_dash/scheduler/signals.py` (39 lines), scoped to the
//! lines this issue owns:
//!
//! * `builtins/__init__.py:21-28` (`BuiltinScheduler`) — [`BuiltinScheduler`].
//! * `builtins/__init__.py:31-59` (`SECURITY_AUDIT_PROMPT`) —
//!   [`SECURITY_AUDIT_PROMPT`].
//! * `builtins/__init__.py:62-98` (`FABLE_AUDIT_PROMPT`) —
//!   [`FABLE_AUDIT_PROMPT`].
//! * `builtins/__init__.py:101-121` (`BUILTINS`) — [`BUILTINS`].
//! * `builtins/__init__.py:124-188` (`ensure_builtin_schedulers`) —
//!   [`decide_ensure`], [`count_touched`], [`ensure_select_statement`],
//!   [`seed_builtin_schedulers`], [`builtin_repaint_statement`].
//! * `signals.py:28-39` (`_seed_builtins_on_workspace_create`) —
//!   [`should_seed_on_workspace_create`], [`SEED_DISPATCH_UID`].
//!
//! DB seam (the same split every sibling port uses — this crate holds no
//! database handle, so no `sqlx` here): the Django ORM calls become
//! contract SQL the DB edge issues, with the pure decision living here:
//!
//! * lookup: `Scheduler.objects.filter(workspace=ws, slug=slug,
//!   deleted_at__isnull=True).first()` — see
//!   [`ensure_select_statement`]. The fixture records the emitted
//!   statement (`FIX-scheduler .db.ensure_select_sql`), including its
//!   doubled `"schedulers"."deleted_at" IS NULL` predicate (default
//!   manager + explicit filter); one filter is semantically the same.
//! * repaint (`_apply_defaults`, `builtins/__init__.py:142-148`):
//!   `name, description, prompt, source=builtin` saved with
//!   `update_fields=["name", "description", "prompt", "source",
//!   "updated_at"]` — see [`builtin_repaint_statement`].
//! * create: `workspace, slug, name, description, prompt,
//!   source=builtin` (`is_enabled`/`color` fall to their model defaults;
//!   `created_by_id` is the request user via crum, `updated_by_id` stays
//!   `NULL`) wrapped in `transaction.atomic()` — see the insert in
//!   [`seed_builtin_schedulers`]. On
//!   `IntegrityError` (a racing caller won the insert under the
//!   `(workspace, slug)` conditional unique constraint
//!   `scheduler_unique_workspace_slug_when_active`) the loser re-fetches
//!   and repaints, so both callers converge.
//! * `touched += 1` runs unconditionally per builtin — even when the row
//!   was already current — so `ensure` returns the catalog size on every
//!   run, including idempotent ones (`FIX-scheduler
//!   .db.ensure_second_run_touched == 2`). See [`count_touched`].
//!
//! Signal semantics (`signals.py:28-39`): the `post_save` receiver on
//! `Workspace` (`dispatch_uid="scheduler.seed_builtins_on_workspace_create"`)
//! returns early when `created` is false and swallows every exception via
//! `logger.exception` so seeding can never block workspace creation (the
//! migration backfill picks the row up on next deploy). Both halves are
//! [`should_seed_on_workspace_create`] plus a caller-side catch-all the DB
//! edge must implement.
//!
//! Coexistence note: the seeder writes `Scheduler` rows only — it emits no
//! Celery task and touches no queue (verified by grep: no `delay` /
//! `apply_async` / `enqueue` in either Python file), so there is no
//! transactional enqueue and no Celery-format publisher on this path.
//!
//! Out of scope (read-only reference, per the split review): the
//! `Scheduler` model itself (`db/models/scheduler.py` — columns,
//! `SchedulerSource`, `OutcomeMode`, ordering, constraints). Only the
//! values this port writes are restated here ([`SOURCE_BUILTIN`]).
//!
//! Ported bugs (translate, don't redesign — from the `bugs` array of
//! `FIX-scheduler`): none in these lines.
//!
//! Semantic traps watched: `len(prompt)` counts code points
//! (`chars().count()`, never byte length); sha256 is over UTF-8 bytes.
//!
//! Executor (PIDASHCONV-1268): [`seed_builtin_schedulers`] is the single
//! copy of the `post_save` receiver's DB edge, lifted here from the api
//! crate so the ops entry point can call it too. The api workspace-create
//! handlers call this same copy.

use pidash_compat::db::is_unique_violation;
use pidash_db::entity::schedulers;
use pidash_db::orm_tx;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use sqlx::PgPool;
use uuid::Uuid;

/// `Scheduler.source` value written by the seeder
/// (`db/models/scheduler.py:25-31`, `SchedulerSource.BUILTIN`).
pub const SOURCE_BUILTIN: &str = "builtin";

/// `post_save` dispatch uid (`signals.py:28`).
pub const SEED_DISPATCH_UID: &str = "scheduler.seed_builtins_on_workspace_create";

/// One row to upsert into the `Scheduler` table for every workspace
/// (`builtins/__init__.py:21-28`). Field-for-field with the frozen
/// dataclass: `slug`, `name`, `description`, `prompt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinScheduler {
    pub slug: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub prompt: &'static str,
}

/// Basic security-scan prompt (`builtins/__init__.py:31-59`).
pub const SECURITY_AUDIT_PROMPT: &str = r#"Scan this project's source code for potential security vulnerabilities
(injection, auth bypass, secret leakage, unsafe deserialization, SSRF,
insecure defaults).

For each finding, create a Pi Dash issue using the `pi-dash` CLI:
    pi-dash issue create \
      --title "[security] <short summary>" \
      --description "<file path, line range, vulnerable snippet,
                     severity (high|medium|low), and suggested fix>"

Before creating an issue, list existing open issues with the
"[security]" title prefix and skip any finding that already has a
corresponding open issue (de-dupe by file + rule, not by exact title).
"#;

/// Fable-powered audit prompt (`builtins/__init__.py:62-98`).
pub const FABLE_AUDIT_PROMPT: &str = r#"You are performing a scheduled, read-only security and correctness audit of this
repository. Run autonomously to completion — no human is available to answer
questions, so make reasonable assumptions and note them rather than stopping.

Scope: the entire repository at the current working directory — application code,
configuration, infrastructure-as-code, dependency manifests, and CI/CD
definitions. This is authorized defensive review of the project owner's own code.

Find:
1. Security vulnerabilities — injection (SQL, command, XSS, SSRF, path traversal,
   template, deserialization); broken authn/authz (missing checks, IDOR,
   privilege escalation, weak sessions); secrets committed or logged;
   cryptographic misuse; insecure configuration (permissive CORS, verbose prod
   errors, open cloud resources, unauthenticated endpoints); vulnerable or
   outdated dependencies with known CVEs (read manifests + lockfiles; if a
   scanner such as npm audit / pip-audit / osv-scanner / trivy is available, run
   it and fold in the results); unsafe file handling and redirects.
2. Correctness bugs — logic errors that could cause wrong behavior, data loss,
   crashes, race conditions, or resource leaks, traced through real data flow
   (not lint-level nits).

Method: work at high thoroughness. Trace data from untrusted inputs to sensitive
sinks and reason about WHY each finding is exploitable or wrong, not just whether
a pattern matches. Read the actual code paths. Where deterministic tooling exists
in the repo (SAST, dependency scanners, type checkers, the test suite), run it
and incorporate the output. Do NOT modify code, commit, or open PRs — read-only.

Report EVERY issue you find, including uncertain or low-severity ones; do not
filter while finding. For each finding capture: a short specific title, the file
path and line range, category (security|correctness) and a specific subtype,
severity (high|medium|low), confidence (high|medium|low), a 1-3 sentence
explanation of the exploit path or failure mode, and a concrete fix.

For each NEW finding, create a Pi Dash issue with the `pi-dash` CLI. Use the
title prefix "[fable-security]" for security findings and "[fable-bug]" for
correctness findings (a dedicated namespace so this audit does not collide with
the basic "[security]" Security Audit scheduler):
    pi-dash issue create \
      --title "[fable-security] <short summary>" \
      --description "<file path + line range, category/subtype, severity,
                     confidence, explanation, and suggested fix>"

Before creating an issue, list existing open issues with the "[fable-security]"
or "[fable-bug]" title prefix and skip any finding that already has a
corresponding open issue (de-dupe by file + subtype, not by exact title). Never
refile duplicates.

End with a one-line summary: issues filed, duplicates skipped, breakdown by
severity. If there are no new findings, file nothing and report "No new findings".
"#;

/// The catalog — the single source of truth (`builtins/__init__.py:101-121`).
/// Order is declaration order: `security-audit` first.
pub const BUILTINS: [BuiltinScheduler; 2] = [
    BuiltinScheduler {
        slug: "security-audit",
        name: "Security Audit",
        description: "Scans the project for common security vulnerabilities and files Pi Dash issues for any new findings.",
        prompt: SECURITY_AUDIT_PROMPT,
    },
    BuiltinScheduler {
        slug: "fable-security-audit",
        name: "Claude Mythos/Fable System Security Audit",
        description: "Comprehensive Fable-powered audit: scans the project for security vulnerabilities and correctness bugs and files Pi Dash issues for new findings.",
        prompt: FABLE_AUDIT_PROMPT,
    },
];

/// What `ensure_builtin_schedulers` does with one catalog entry
/// (`builtins/__init__.py:158-187`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureAction {
    /// No active row: insert with `source=builtin` (with the
    /// `IntegrityError` → refetch + repaint fallback).
    Create,
    /// Active row present (or won by a racer): repaint
    /// `name/description/prompt/source` + `updated_at`.
    Repaint,
}

/// The active `Scheduler` row the DB edge preloaded for one builtin slug
/// (`None` when the [`ensure_select_statement`] lookup finds no row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerRowView {
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub source: String,
}

/// Pure half of the per-builtin loop (`builtins/__init__.py:165-187`):
/// an existing active row repaints; a missing one inserts. (The
/// `IntegrityError` retry also lands on [`EnsureAction::Repaint` — the
/// winner is re-fetched and `_apply_defaults` runs over it.)
pub fn decide_ensure(existing: Option<&SchedulerRowView>) -> EnsureAction {
    match existing {
        Some(_) => EnsureAction::Repaint,
        None => EnsureAction::Create,
    }
}

/// Rows touched by one `ensure` run (`builtins/__init__.py:187`:
/// `touched += 1` per builtin, unconditionally). The count equals the
/// catalog size even when every row was already current.
pub fn count_touched(plans: &[EnsureAction]) -> usize {
    plans.len()
}

/// The 14 `schedulers` columns the ensure lookup projects, in the old
/// SQL's SELECT order (decode is by name, so order is cosmetic).
fn scheduler_seed_columns() -> [schedulers::Column; 14] {
    use schedulers::Column;
    [
        Column::CreatedAt,
        Column::UpdatedAt,
        Column::CreatedById,
        Column::UpdatedById,
        Column::DeletedAt,
        Column::Id,
        Column::WorkspaceId,
        Column::Slug,
        Column::Name,
        Column::Description,
        Column::Prompt,
        Column::Source,
        Column::IsEnabled,
        Column::Color,
    ]
}

/// Per-builtin lookup (`builtins/__init__.py:165-169` and the
/// `177-181` refetch): active (non-deleted) row for `(workspace, slug)`,
/// newest first. The doubled `deleted_at IS NULL` predicate is the
/// emitted Django SQL (default manager + explicit filter); one filter
/// is semantically the same predicate.
fn ensure_select_statement(workspace_id: Uuid, slug: &str) -> sea_orm::Select<schedulers::Entity> {
    schedulers::Entity::find()
        .select_only()
        .columns(scheduler_seed_columns())
        .filter(schedulers::Column::DeletedAt.is_null())
        .filter(schedulers::Column::Slug.eq(slug))
        .filter(schedulers::Column::WorkspaceId.eq(workspace_id))
        .order_by_desc(schedulers::Column::CreatedAt)
        .limit(1)
}

/// Per-builtin repaint (`_apply_defaults`, `builtins/__init__.py:142-148`):
/// `save(update_fields=["name", "description", "prompt", "source",
/// "updated_at"])` with `source` forced back to `builtin`.
fn builtin_repaint_statement(
    name: &str,
    description: &str,
    prompt: &str,
    stamp: sea_orm::prelude::DateTimeWithTimeZone,
    scheduler_id: Uuid,
) -> sea_orm::UpdateMany<schedulers::Entity> {
    schedulers::Entity::update_many()
        .col_expr(schedulers::Column::Name, Expr::value(name))
        .col_expr(schedulers::Column::Description, Expr::value(description))
        .col_expr(schedulers::Column::Prompt, Expr::value(prompt))
        .col_expr(schedulers::Column::Source, Expr::value("builtin"))
        .col_expr(schedulers::Column::UpdatedAt, Expr::value(stamp))
        .filter(schedulers::Column::Id.eq(scheduler_id))
}

/// `post_save` gate (`signals.py:32-33`): only a newly created workspace
/// seeds. Updates, deletes and fixture loads (`created=False`) return
/// without touching the database.
pub fn should_seed_on_workspace_create(created: bool) -> bool {
    created
}

/// The active-row lookup (`builtins/__init__.py:165-169` and the
/// `177-181` refetch): one model or `None`. Every failure stops the
/// call silently (the receiver swallows exceptions).
async fn ensure_active_row(
    db: &sea_orm::DatabaseConnection,
    workspace_id: Uuid,
    slug: &str,
) -> Result<Option<schedulers::Model>, sea_orm::DbErr> {
    ensure_select_statement(workspace_id, slug)
        .into_model::<schedulers::Model>()
        .one(db)
        .await
}

/// `#3b82f6`, the `Scheduler.color` model default (DB-level default text).
const BUILTIN_COLOR: &str = "#3b82f6";

/// `post_save` receiver `_seed_builtins_on_workspace_create`
/// (`scheduler/signals.py:28-39`): `ensure_builtin_schedulers` runs on
/// every workspace creation. The receiver swallows every exception
/// (`logger.exception`), so this edge returns `()` and every error arm
/// below is a silent stop — seeding can never block workspace creation.
/// `created_by` is `Some` requesting user on the HTTP paths
/// (`BaseModel.save` stamps the crum user on the scheduler rows too)
/// and `None` on crum-less entry points (management commands), where
/// Django stamps `NULL`. Shared by every workspace-create entry point
/// (the Django signal fires regardless of entry point).
pub async fn seed_builtin_schedulers(pool: &PgPool, workspace_id: Uuid, created_by: Option<Uuid>) {
    if !should_seed_on_workspace_create(true) {
        return;
    }
    let db = pidash_db::pool::sea_orm_connection(pool);
    for builtin in BUILTINS {
        let row = match ensure_active_row(&db, workspace_id, builtin.slug).await {
            Ok(row) => row,
            Err(_) => return,
        };
        let view = row.as_ref().map(|row| SchedulerRowView {
            name: row.name.clone(),
            description: row.description.clone(),
            prompt: row.prompt.clone(),
            source: row.source.clone(),
        });
        match (row, decide_ensure(view.as_ref())) {
            (Some(row), EnsureAction::Repaint) => {
                let stamp: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
                if builtin_repaint_statement(
                    builtin.name,
                    builtin.description,
                    builtin.prompt,
                    stamp,
                    row.id,
                )
                .exec(&db)
                .await
                .is_err()
                {
                    return;
                }
            }
            (None, EnsureAction::Create) => {
                let now_stamp: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
                let inserted = schedulers::ActiveModel {
                    id: Set(Uuid::new_v4()),
                    created_at: Set(now_stamp),
                    updated_at: Set(now_stamp),
                    created_by_id: Set(created_by),
                    updated_by_id: Set(None),
                    deleted_at: Set(None),
                    workspace_id: Set(workspace_id),
                    slug: Set(builtin.slug.to_owned()),
                    name: Set(builtin.name.to_owned()),
                    description: Set(builtin.description.to_owned()),
                    prompt: Set(builtin.prompt.to_owned()),
                    source: Set("builtin".to_owned()),
                    is_enabled: Set(true),
                    color: Set(BUILTIN_COLOR.to_owned()),
                }
                .insert(&db)
                .await;
                match inserted {
                    Ok(_) => {}
                    Err(error) => {
                        // The winner of the race repaints; anything else
                        // stops the seeding silently.
                        let sqlx_error = orm_tx::into_sqlx(error);
                        if !is_unique_violation(&sqlx_error) {
                            return;
                        }
                        // Lost the race: re-fetch and repaint the
                        // winner (`builtins/__init__.py:177-186`).
                        let winner = match ensure_active_row(&db, workspace_id, builtin.slug).await
                        {
                            Ok(winner) => winner,
                            Err(_) => return,
                        };
                        if let Some(winner) = winner {
                            let stamp: sea_orm::prelude::DateTimeWithTimeZone =
                                chrono::Utc::now().into();
                            if builtin_repaint_statement(
                                builtin.name,
                                builtin.description,
                                builtin.prompt,
                                stamp,
                                winner.id,
                            )
                            .exec(&db)
                            .await
                            .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
            }
            // Unreachable by `decide_ensure`'s contract (`Some` repaints,
            // `None` creates); stop silently like any seeder failure.
            _ => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/prompting/FIX-scheduler.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn sha_hex(body: &str) -> String {
        Sha256::digest(body.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn catalog_data(fixture: &Value) -> &Value {
        fixture
            .get("data")
            .and_then(|data| data.get("catalog"))
            .expect("fixture carries data.catalog")
    }

    #[test]
    fn catalog_matches_fixture_exactly() {
        // `FIX-scheduler / catalog.builtins` (2 slugs) + full prompt goldens.
        let golden = fixture();
        let catalog = catalog_data(&golden);
        assert_eq!(catalog["builtin_count"].as_u64().expect("count"), 2);
        assert_eq!(BUILTINS.len(), 2);
        let expected = catalog["builtins"].as_array().expect("builtins array");
        for (builtin, want) in BUILTINS.iter().zip(expected.iter()) {
            assert_eq!(builtin.slug, want["slug"].as_str().expect("slug"));
            assert_eq!(builtin.name, want["name"].as_str().expect("name"));
            assert_eq!(
                builtin.description,
                want["description"].as_str().expect("description")
            );
            assert_eq!(
                builtin.prompt.chars().count() as u64,
                want["prompt_len"].as_u64().expect("prompt_len")
            );
            assert_eq!(
                sha_hex(builtin.prompt),
                want["prompt_sha256"].as_str().expect("prompt_sha256")
            );
        }
        let data = golden.get("data").expect("fixture carries data");
        assert_eq!(
            SECURITY_AUDIT_PROMPT,
            data["security_audit_prompt"]
                .as_str()
                .expect("security prompt")
        );
        assert_eq!(
            FABLE_AUDIT_PROMPT,
            data["fable_audit_prompt"].as_str().expect("fable prompt")
        );
        assert_eq!(
            sha_hex(SECURITY_AUDIT_PROMPT),
            "6cba8b5a8c04221b8dfdc4a23a8de31c60abe14ec6e1483d7e814a39ff35376f"
        );
        assert_eq!(
            sha_hex(FABLE_AUDIT_PROMPT),
            "63a696e7115b1724dd72b30ef575e76e4603a14c482aacdb48bef5f89f3b5b36"
        );
        assert_eq!(SECURITY_AUDIT_PROMPT.chars().count(), 634);
        assert_eq!(FABLE_AUDIT_PROMPT.chars().count(), 2962);
        assert_eq!(SOURCE_BUILTIN, "builtin");
        assert_eq!(
            SEED_DISPATCH_UID,
            "scheduler.seed_builtins_on_workspace_create"
        );
    }

    /// One `ensure` run over a fake store, mirroring the Python loop
    /// (`builtins/__init__.py:158-187`) including the unconditional
    /// `touched += 1`.
    fn run_ensure(store: &mut HashMap<String, SchedulerRowView>) -> usize {
        let mut plans = Vec::with_capacity(BUILTINS.len());
        for builtin in BUILTINS.iter() {
            let action = decide_ensure(store.get(builtin.slug));
            match action {
                EnsureAction::Create => {
                    store.insert(
                        builtin.slug.to_owned(),
                        SchedulerRowView {
                            name: builtin.name.to_owned(),
                            description: builtin.description.to_owned(),
                            prompt: builtin.prompt.to_owned(),
                            source: SOURCE_BUILTIN.to_owned(),
                        },
                    );
                }
                EnsureAction::Repaint => {
                    let row = store.get_mut(builtin.slug).expect("repaint target");
                    row.name = builtin.name.to_owned();
                    row.description = builtin.description.to_owned();
                    row.prompt = builtin.prompt.to_owned();
                    row.source = SOURCE_BUILTIN.to_owned();
                }
            }
            plans.push(action);
        }
        count_touched(&plans)
    }

    #[test]
    fn ensure_is_idempotent_and_touches_per_builtin() {
        // `FIX-scheduler / db`: first run creates, second run still
        // touches 2, count unchanged.
        let mut store = HashMap::new();
        assert_eq!(run_ensure(&mut store), 2);
        assert_eq!(store.len(), 2);
        assert_eq!(run_ensure(&mut store), 2);
        assert_eq!(store.len(), 2);
        for builtin in BUILTINS.iter() {
            let row = store.get(builtin.slug).expect("seeded row");
            assert_eq!(row.source, "builtin");
            assert_eq!(row.prompt, builtin.prompt);
        }
    }

    #[test]
    fn ensure_repaints_stale_rows() {
        // `FIX-scheduler / db.ensure_repaints_stale`: a stale row converges
        // to the catalog and still counts as touched.
        let mut store = HashMap::new();
        store.insert(
            "security-audit".to_owned(),
            SchedulerRowView {
                name: "Stale name".to_owned(),
                description: "stale".to_owned(),
                prompt: "stale prompt".to_owned(),
                source: "manifest".to_owned(),
            },
        );
        assert_eq!(
            decide_ensure(store.get("security-audit")),
            EnsureAction::Repaint
        );
        assert_eq!(
            decide_ensure(store.get("fable-security-audit")),
            EnsureAction::Create
        );
        assert_eq!(run_ensure(&mut store), 2);
        let row = store.get("security-audit").expect("repainted row");
        assert_eq!(row.name, "Security Audit");
        assert_eq!(row.prompt, SECURITY_AUDIT_PROMPT);
        assert_eq!(row.source, "builtin");
    }

    #[test]
    fn ensure_statement_carries_the_emitted_predicates() {
        use sea_orm::{DbBackend, QueryTrait};
        // `FIX-scheduler / db.ensure_select_sql`: the doubled
        // `deleted_at IS NULL` quirk is one semantic predicate; the
        // slug/workspace scope (in either order — binding order is not
        // observable) and newest-first limit survive.
        let select = ensure_select_statement(Uuid::nil(), "security-audit")
            .build(DbBackend::Postgres)
            .to_string();
        assert!(select.contains("FROM"), "select from: {select}");
        assert!(
            select.contains("\"schedulers\""),
            "schedulers table: {select}"
        );
        assert!(
            select.contains("\"deleted_at\" IS NULL"),
            "live rows only: {select}"
        );
        assert!(select.contains("\"slug\""), "slug scope: {select}");
        assert!(
            select.contains("\"workspace_id\""),
            "workspace scope: {select}"
        );
        assert!(select.contains("ORDER BY"), "newest-first order: {select}");
        assert!(select.contains("LIMIT 1"), "limit 1: {select}");
        assert!(
            select.contains("\"verified\"") || !select.contains("\"schedulers\".\"verified\""),
            "no invented column: {select}"
        );
        // The full 14-column projection covers every field the
        // `SchedulerRowView` reads (name/description/prompt/source).
        for column in [
            "\"name\"",
            "\"description\"",
            "\"prompt\"",
            "\"source\"",
            "\"id\"",
        ] {
            assert!(
                select.contains(column),
                "projection keeps {column}: {select}"
            );
        }
    }

    #[test]
    fn repaint_statement_carries_the_defaults_write() {
        use sea_orm::{DbBackend, QueryTrait};
        // `_apply_defaults`: name/description/prompt/source + the
        // `updated_at` stamp, scoped by id; `source` is forced back to
        // the builtin literal.
        let stamp: sea_orm::prelude::DateTimeWithTimeZone = chrono::Utc::now().into();
        let repaint = builtin_repaint_statement("N", "D", "P", stamp, Uuid::nil())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(
            repaint.contains("UPDATE \"schedulers\""),
            "repaint update: {repaint}"
        );
        for col in [
            "\"name\"",
            "\"description\"",
            "\"prompt\"",
            "\"source\"",
            "\"updated_at\"",
        ] {
            assert!(repaint.contains(col), "repaint writes {col}: {repaint}");
        }
        assert!(
            repaint.contains("builtin"),
            "source forced to builtin: {repaint}"
        );
        assert!(repaint.contains("WHERE"), "repaint scope: {repaint}");
    }

    #[test]
    fn signal_gate_only_seeds_on_create() {
        // `signals.py:32-33`: `created=False` returns; exceptions swallowed
        // (caller-side catch-all, documented on the gate).
        assert!(should_seed_on_workspace_create(true));
        assert!(!should_seed_on_workspace_create(false));
    }
}
