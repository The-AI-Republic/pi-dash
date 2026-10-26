# Rust ORM adoption: SeaORM entities and OSS-owned domain write services

Status: **accepted** (maintainer decision, 2026-10-26). Stage 8 of the Django → Rust
conversion (PIDASHCONV-1). Issues are filed from §8; each issue names its unit id
(`ORM-xx`).

Public document: it describes OSS crates and the extension contract a downstream
overlay relies on. It does not describe private overlay internals; those live in the
overlay's own issues.

## 1. Problem

Django gave every caller the write side effects for free. `Issue.save()` assigns the
sequence id, sort order and default state; `post_save` receivers create notification
preferences, seed schedulers and fire the orchestration transition; serializers queue
activity and webhook jobs. Any code that called `.save()` (an OSS view, a Celery task,
or an overlay package such as a cloud edition) got all of it, so nobody re-implemented it.

The Rust port has no equivalent layer. `rust-api` uses sqlx 0.8 with hand-written SQL
and sea-query 0.32 for dynamic statements. Write logic lives inside each handler, and
every new caller copies it. Measured on `rust-dev` at `f7459d74c`:

| Write | Independent implementations | Notes |
|---|---|---|
| Issue create (`Issue.save()` create branch) | 8+ | app and v1 handlers, drafts, three intake paths, agent tool, git/GitHub sync, seed and demo data |
| "Next sequence id" rule alone | 6 | `dispatch/tools.rs`, `assistant/tools_issues.rs`, `dummy_data.rs`, `models_user.rs`, `app_views_search/models.rs`, `handlers_project.rs` |
| Issue comment insert | 7 | app, v1, space, runner lifecycle, agent tool, ticker bounce comment, git sync |
| Issue move between projects | 2 near-identical `PoolMoveStore` impls | `app_issues/handlers_versions_move.rs`, `v1_work_items/handlers_move_search.rs` |
| User signup | 8 call sites | only 4 insert `user_notification_preferences`; GitHub, Google and Gitea OAuth signups do not |
| Workspace / project / member creation | 3 / 4 / 10+ | each re-calls the post-save effects by hand |
| Activity / webhook task-name constants | ~15 copies | one per services module plus api copies |

Two consequences follow:

1. **OSS parity bugs.** A side effect that one copy misses is a divergence from Django,
   for example the missing notification-preference rows above.
2. **Overlays duplicate OSS logic.** An overlay composes the OSS crates
   (`build_router_with_overlay`, `register_oss_handlers`, the seams in
   `rust-api/README.md`). None of those seams cover domain writes, and the write kernels
   are private handler functions. Some are `pub(crate)`, such as
   `api::issue_transition::{capture_prior_state_pool, fire_issue_transition}`. So an
   overlay that creates an issue, adds a comment or provisions a user has to copy the
   SQL and the side effects. Every OSS behaviour change then has to be mirrored by hand,
   and missed side effects show up later as overlay bugs. That is the drift the
   OSS/overlay split exists to prevent.

## 2. Decision

**D1. Adopt SeaORM 1.1 (`~1.1.20`).** SeaORM is built on sqlx and sea-query. The 1.1
line requires `sqlx ^0.8.4` and `sea-query ^0.32.5`, and the workspace locks
`sqlx 0.8.6` and `sea-query 0.32.7`, so the dependency graph unifies and nothing else
moves. SeaORM 2.0 (current 2.0.4) requires sqlx 0.9 and sea-query 1.0. Upgrading to it
is a separate, later change that moves the whole workspace and every overlay together.

Features: `default-features = false`, `["sqlx-postgres", "runtime-tokio", "macros",
"with-uuid", "with-chrono", "with-json"]`. This matches the workspace sqlx features. No
`sea-orm-migration`.

Why not Diesel: Diesel brings its own connection layer (libpq or `diesel-async` on
tokio-postgres). That means a second pool, and no transaction shared with the existing
sqlx code, which matters during incremental adoption. Diesel also has no lifecycle
hooks, and adds heavy compile cost to a workspace of over 1M lines. Its main advantage,
compile-checked queries, is already covered here by the Django contract oracle.

**D2. Entities mirror the Django schema; Django stays the schema owner.** Entities are
generated with `sea-orm-cli generate entity` against a database migrated by
`apps/api` at the current `origin/main`, then committed under
`rust-api/crates/db/src/entity/`. No SeaORM migrations exist. A parity test compares
each entity's table, columns, types and nullability with `information_schema` on the
migrated contract database, and fails on drift. Regenerating after a Django sync is
part of the drift re-sync work.

**D3. OSS owns every write to OSS tables through public domain services.** A new module
tree, `pidash_services::domain::<aggregate>`, is the only code that writes the tables
it covers (§4). Each operation is a public async function:

```rust
pub async fn create(ctx: &WriteCtx, txn: &mut WriteTxn, input: CreateIssue)
    -> Result<issue::Model, DomainError>;
```

- `WriteCtx` carries the actor (`crum` user equivalent), request origin/host, the
  workspace and project scope, and per-caller options (§D6).
- `WriteTxn` wraps a SeaORM `DatabaseTransaction` plus the after-commit queue (the
  existing `pidash_db::tx::AfterCommit` pattern). Callers either pass their own
  `WriteTxn` or use `domain::run(ctx, pool, |txn| …)`, which begins, commits and then
  drains the queue.
- The functions are re-exported from `pidash_services` and documented as an extension
  surface in `rust-api/README.md` next to the existing seams. Overlays call them; they
  never execute OSS SQL text constants themselves.

**D4. Where each Django behaviour lives.**

| Django mechanism | Rust home | Examples |
|---|---|---|
| `Model.save()` override, row-local and in-transaction | `ActiveModelBehavior::before_save` | issue `sequence_id` + `issue_sequences` row, `sort_order`, default state, `completed_at`, `description_stripped` |
| `post_save` receiver that only writes rows | `ActiveModelBehavior::after_save` (same transaction) | `user_notification_preferences` for non-bot users, built-in scheduler seeding on workspace create |
| Effects that need request context or must run after commit | the domain service, queued on `WriteTxn`'s after-commit list | issue/model activity and webhook jobs, notifications, orchestration state transition, completion comment, pod drains |
| `QuerySet.update()` / bulk writes (no signals in Django either) | service functions that use `update_many` or sea-query explicitly | bulk dates, archive sweeps |

SeaORM runs the hooks only through `ActiveModelTrait::insert`, `update` and `save`
(verified in 1.1.20 `entity/active_model.rs`). `Entity::insert(..).exec`,
`update_many`, `insert_many` and raw statements skip them, just as Django bulk paths
skip signals. Services must use the hook-firing calls wherever Django fires the signal.

**D5. One pool; SeaORM-owned transactions.** The SeaORM `DatabaseConnection` is built
from the existing primary pool with
`SqlxPostgresConnector::from_sqlx_postgres_pool(pools.primary().clone())` and carried on
`AppState` and the worker context. There is no second pool.

A SeaORM transaction cannot adopt an `sqlx::Transaction`, and sqlx cannot join a SeaORM
transaction: `InnerConnection` is `pub(crate)` in 1.1.20. So every statement inside a
domain write runs on the SeaORM `DatabaseTransaction`:

- entity calls directly;
- existing sea-query builders through `txn.execute(DbBackend::Postgres.build(&stmt))`;
- the remaining raw SQL (advisory locks such as `pg_advisory_xact_lock`, `SELECT … FOR
  UPDATE`, existing SQL constants) through `Statement::from_sql_and_values`.

Shared helpers that take `&mut PgConnection` today gain a `ConnectionTrait` variant when
a service needs them. Reads stay on sqlx.

**D6. Translate, don't redesign, still holds.** Consolidating copies must not change any
behaviour the contract suites observe. Where the copies differ today, the difference is
classified before merging:

- **Django has the same difference** (different views do different things): keep it as
  an explicit option on `WriteCtx` or the input struct, with a doc comment that cites the
  Python file:line.
- **Django has one behaviour and a Rust copy diverges:** that copy is a Rust bug. File a
  stage-8 "Rust bug:" issue with the usual byte-level evidence; the consolidation adopts
  the Django behaviour only if the bug issue is fixed in the same PR.

Overlay-specific behaviour that Django's overlay package had (for example a write tool
that stamps `updated_by` where the OSS view omits it) is also a `WriteCtx` option. The
option lives in OSS because the behaviour is a parameter of the OSS write, not
overlay logic.

**D7. Table ownership and the write guard.** A table is owned by the repository whose
Django models define it. Overlay code writes its own tables directly. It writes OSS
tables only by calling `pidash_services::domain`. Inside OSS, the tables in §4 are
written only from `pidash_services::domain` (and test fixtures).

Both rules are enforced by a ratchet check in CI. A script lists every
`INSERT`/`UPDATE`/`DELETE` against a covered table outside the allowed module and
compares the list with a committed allowlist. New sites fail the check. The allowlist
only shrinks, and it reaches empty when the migration issues are Done. Test modules
(`#[cfg(test)]`, `tests/`, `contract-tests/`, `fixtures/`) are exempt.

**D8. SQL text constants stop being an extension surface.** Public SQL constants
(`SEQUENCE_CREATE_SQL`, `COMMENT_INSERT_SQL`, `builtin_insert_sql`, …) remain for OSS
internal use while callers migrate. Once a domain service covers a write, the constant
becomes `pub(crate)` or private, so an overlay cannot re-assemble the write from SQL
fragments.

## 3. Non-goals

- No schema change and no migration change. Django keeps `makemigrations`/`migrate`
  until switchover.
- Reads are not ported to SeaORM. About 1M lines of tuned sqlx/sea-query read code
  stay; a read moves only when its service already holds the entity.
- No SeaORM 2.0 or sqlx 0.9 upgrade in this stage.
- No `apps/api` changes. The quarantine rule holds: implementation PRs touch only
  `rust-api/` and `.github/workflows/rust-api*.yml`.

## 4. Covered tables (stage-8 scope)

| Aggregate | Tables | Operations |
|---|---|---|
| `domain::issues` | `issues`, `issue_sequences`, `issue_assignees` (fallback assignee), `issues.workpad` column | create, update (full and partial), state change, archive/unarchive, workpad set, move between projects |
| `domain::comments` | `issue_comments`, `descriptions` | create, update (with Description sync), soft delete |
| `domain::relations` | `issue_relations` | get-or-create, soft delete |
| `domain::identity` | `users`, `profiles`, `accounts`, `user_notification_preferences` | create user (signup, admin, OAuth, federation-style provisioning), update profile and user fields, account upsert and token rotation |
| `domain::workspaces` | `workspaces`, `workspace_members` | create (with built-in schedulers), rename, add/update/deactivate member |
| `domain::projects` | `projects`, `project_members`, `states` | create (default states, admin member, model activity), add/update/deactivate member, create state |
| `domain::tokens` | `api_tokens`, `machine_token`, `runner_session` | issue API key, touch last-used, revoke |
| `domain::assistant` | `assistant_user_llm_config`, `assistant_message` | config CRUD and verify stamp, message create and finalize |

Seed and demo-data writers (`workspace_seed.rs`, `dummy_data.rs`) migrate last. They
may keep bulk inserts, but they must reach the same rows as the services.

## 5. Shape of the code

```
rust-api/crates/db/src/entity/          generated entities (+ hand-written ActiveModelBehavior impls)
rust-api/crates/db/src/entity/parity.rs  information_schema parity test
rust-api/crates/services/src/domain/     WriteCtx, WriteTxn, DomainError, run()
    issues.rs comments.rs relations.rs identity.rs workspaces.rs projects.rs tokens.rs assistant.rs
    activity.rs                          the one copy of the activity/webhook task names + enqueue helpers
rust-api/scripts/write_guard.py          the ratchet check (§D7) + write_guard_allowlist.txt
.github/workflows/rust-api-write-guard.yml
```

The `ActiveModelBehavior` impls live next to the entities in `pidash-db`, because
SeaORM requires the impl in the entity's crate. Hooks that need jobs or services code
(scheduler seeding) call through a small trait object installed on `WriteTxn`, so
`pidash-db` does not depend on `pidash-services`.

## 6. Overlay contract after this stage

An overlay binary:

1. builds its `AppState` and worker context from the OSS constructors, which now carry
   the SeaORM `DatabaseConnection`;
2. calls `pidash_services::domain::*` for every write to an OSS table, passing a
   `WriteCtx` with its options;
3. writes only its own tables directly;
4. runs the same write guard against its own tree, with OSS tables as the forbidden set.

When an overlay needs a write the domain services do not offer, the fix is an OSS issue
that adds it, filed under PIDASHCONV-1 with `blocked_by` wired. The overlay never adds
the write itself.

## 7. Risks

| Risk | Mitigation |
|---|---|
| Django defaults are Python code, not DB defaults (`sequence_id`, `sort_order`, `created_by` from middleware, soft-delete filters) | Hooks reproduce them with Python file:line citations; contract suites on both binaries are the gate |
| Consolidation silently changes one caller's behaviour | §D6 classification; each migration PR runs every contract suite whose routes it touches, on the OSS and overlay binaries |
| Transaction split between sqlx and SeaORM code | §D5: the whole domain write runs on the SeaORM transaction; review rejects mixed-transaction code |
| Generated entities drift after a Django sync | parity test fails; regeneration is part of the re-sync issue |
| Compile time | SeaORM 1.1 shares sqlx/sea-query with the workspace; entities only for §4 tables |

## 8. Units (filed as stage-8 issues under PIDASHCONV-1)

| Unit | Issue | Repo | Summary | Blocked by |
|---|---|---|---|---|
| ORM-01 | PIDASHCONV-1519 | OSS | SeaORM dependency, `DatabaseConnection` on `AppState`/worker ctx, entities for §4 tables, parity test | — |
| ORM-02 | PIDASHCONV-1520 | OSS | `domain` framework: `WriteCtx`, `WriteTxn`, `DomainError`, `run()`, one activity/webhook enqueue module, public orchestration transition (lift `issue_transition` out of `pub(crate)`) | ORM-01 |
| ORM-03 | PIDASHCONV-1521 | OSS | `domain::issues` create/update/state/archive/workpad + hooks; migrate app and v1 issue handlers | ORM-02 |
| ORM-04 | PIDASHCONV-1522 | OSS | migrate the remaining issue writers: drafts, intake ×3, agent tools, git/GitHub sync, automation, ticker dispatch | ORM-03 |
| ORM-05 | PIDASHCONV-1523 | OSS | `domain::issues::move_to_project`: one public `MoveStore` impl replacing both `PoolMoveStore` copies, options for caller deltas | ORM-03 |
| ORM-06 | PIDASHCONV-1524 | OSS | `domain::comments` + `domain::relations`; migrate all seven comment writers and the relation writers | ORM-02 |
| ORM-07 | PIDASHCONV-1525 | OSS | `domain::identity`, `domain::workspaces`, `domain::projects`; migrate signups, admin, ops, invites, project/state create | ORM-02 |
| ORM-08 | PIDASHCONV-1526 | OSS | `domain::tokens` + `domain::assistant`; migrate their writers | ORM-02 |
| ORM-09 | PIDASHCONV-1527 | OSS | write guard (ratchet) + allowlist + workflow | — |
| ORM-10 | PIDASHCONV-1528 | OSS | Rust bug: GitHub/Google/Gitea OAuth signups skip `user_notification_preferences` | — |
| ORM-11 | PIDASHCONV-1529 | overlay | overlay write guard (ratchet) | — |
| ORM-12 | PIDASHCONV-1530 | overlay | move the overlay's issue/comment/relation/workpad/token writes onto `domain` | ORM-03, ORM-05, ORM-06, ORM-08 |
| ORM-13 | PIDASHCONV-1531 | overlay | move the overlay's identity/workspace/member/instance writes onto `domain` | ORM-07, ORM-08 |
| ORM-14 | PIDASHCONV-1532 | overlay | move the overlay's assistant writes onto `domain` | ORM-08 |
| ORM-15 | PIDASHCONV-1533 | both | gate: both allowlists empty, SQL constants de-publicised (§D8), all contract suites green on both binaries, README extension-surface section, Porting guide updated | all of the above |

## 9. Done (stage-8 ORM gate)

- Both write-guard allowlists are empty.
- Every contract suite passes on the OSS binary and on the overlay binary.
- `rust-api/README.md` documents `pidash_services::domain` as an extension surface.
- The Porting guide's Django idiom → Rust pattern table maps `Model.save()`,
  `post_save` and serializer `save()` to §D4.
