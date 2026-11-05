# Rust raw SQL to SeaORM: migration playbook

Companion to [design.md](../../.ai_design/rust_orm/design.md), which moved the writes to the core
tables onto SeaORM domain services. This document covers everything else,
reads included.

## 1. Rule

Maintainer decision, 2026-11-03/04: **rust-api reads and writes go through
SeaORM entities and the query builder. Raw SQL stays only where SeaORM cannot
express a statement**, and each such site says why with a
`// raw-sql: <reason>` comment on the call line, or alone on the line above
it.

The trigger was the rustpidash incident: every work item returned 500. A
hand-written agent-run `SELECT` had two `llm_model` output columns. sqlx
resolves a duplicated name to the **last** column, so a by-name read got the
runner's NULL live model instead of the run's own, and the decode error became
a bare 500. A positional `try_get(25)` had also gone stale when a column was
dropped. Django never has this class of bug: the ORM maps joined columns into
separate objects. Hand-written SQL makes every handler re-solve that by hand.

`scripts/raw_sql_guard.py` enforces the rule (see `rust-api/README.md`,
"Local checks"):

- **No new raw SQL.** A file's count may never rise.
- **The count only shrinks.** Lower the allowlist entry in the PR that
  migrates the sites.
- **No new duplicate output columns** in a hand-written `SELECT`.

Starting point (2026-11-04): 4,163 raw-SQL sites in 259 production files.

## 2. What counts as raw SQL

| Counted | Not counted |
| --- | --- |
| `sqlx::query`, `query_as`, `query_scalar` (and `_with`), the `query!` macros, `raw_sql`, `QueryBuilder::new` | SeaORM `Entity::find()`, `ActiveModel`, `select_only().column_as(..)` |
| `Statement::from_string` / `from_sql_and_values`, `execute_unprepared`, `from_raw_sql`, `find_by_statement` | sea-query builder calls (`Query::select()`, `Expr::col`, `PgExpr` operators) |
| `Expr::cust*` (SQL text inside the builder) | test code (`#[cfg(test)]` items, files declared `#[cfg(test)] mod x;`, `tests/`), including test seeding |
| positional `try_get(<index>)` in any form (`.try_get(0)`, `.try_get::<T, _>(2)`, `Row::try_get(&r, 3)`) | a site marked `// raw-sql: <reason>` |

## 3. Patterns

Get a SeaORM connection over the existing pool with
`pidash_db::pool::sea_orm_connection(&pool)`. It is a wrapper with no I/O:
build it once per request render and pass `&DatabaseConnection` down. Prefer
`AppState::orm()` where the state is at hand.

### 3.1 One table, by key or filter

```rust
let state = states::Entity::find_by_id(id).one(&db).await?;
let open = issues::Entity::find()
    .filter(issues::Column::ProjectId.eq(project_id))
    .filter(issues::Column::DeletedAt.is_null())
    .order_by_asc(issues::Column::SequenceId)
    .all(&db)
    .await?;
```

Read only the columns you render. For a narrow read, use a
`DerivePartialModel` + `FromQueryResult` struct with
`.into_partial_model::<T>()`, or `select_only().columns([..]).into_tuple()`.
Large columns (`prompt`, description blobs, JSON payloads) should not cross
the wire just to be dropped.

### 3.2 Joins (the #2857 pattern)

Build one statement with `select_only()`. Name every joined column with
`column_as(.., "<prefix>_<column>")`, and decode by name into one
`#[derive(FromQueryResult)]` row struct. Every output name is then unique by
construction, and the row struct makes a missing alias a compile-time or
decode-time error, not a silent wrong value.

```rust
agent_run::Entity::find()
    .select_only()
    .columns([agent_run::Column::Id, agent_run::Column::LlmModel /* .. */])
    .column_as(runner::Column::Name, "runner_name")
    .column_as(runner_live_state::Column::LlmModel, "live_llm_model")
    .join(JoinType::LeftJoin, agent_run::Relation::Runner.def())
    .join(JoinType::LeftJoin, runner::Relation::RunnerLiveState.def())
    .filter(agent_run::Column::WorkItemId.eq(issue_id))
    .order_by_desc(agent_run::Column::CreatedAt)
    .into_model::<RunRow>()
    .one(&db)
    .await?;
```

- For two tables, `find_also_related(B)` returning `(A, Option<B>)` is also
  fine. Columns come back prefixed (`A_`, `B_`), so names cannot collide.
- **Do not** use `SelectThree::into_partial_model` in sea-orm 1.1.20: it
  never selects the third model's columns. Use the `select_only` +
  `column_as` + `into_model` form above.
- Add a unit test that builds the statement
  (`.build(DbBackend::Postgres).to_string()`) and pins the joins, filter,
  order and the unique output names, so CI covers it without a database.

### 3.3 Aggregates, `EXISTS`, `COUNT`

- `COUNT`: `Entity::find().filter(..).count(&db)`.
- `EXISTS`: `.select_only().column(Column::Id).limit(1).into_tuple::<Uuid>()`,
  then check for `Some`.
- Grouped aggregates: `select_only().column(..).column_as(Expr::col(..).count(), "n").group_by(..)`.
  Use sea-query functions (`Func::coalesce`, `Func::max`), not `Expr::cust`.
- JSON fields: `sea_orm::sea_query::extension::postgres::PgExpr`
  (`get_json_field` for `->`, `cast_json_field` for `->>`), applied to
  `Expr::col((Entity, Column))`.

### 3.4 Dynamic filters and list endpoints

- Build filters with `Condition::all()` / `Condition::any()` and
  `.add_option(..)`, not by concatenating strings. `format!`-built SQL is the
  hardest kind of raw SQL to review, and it goes first in any module.
- Avoid N+1 queries in list endpoints: load the page with one query, then the
  related rows with one `is_in(ids)` query per relation (or
  `LoaderTrait::load_many`), and stitch them in memory.
- Keep Django's ordering, including its *lack* of a tiebreaker where Django
  has none, and its cursor and pagination semantics. The contract suites pin
  these.

### 3.5 Writes

Writes to the design §4 tables go through `pidash_services::domain`
(`.ai_design/rust_orm/design.md`). Any other write uses
`ActiveModel { .. }.insert/update/delete` or `Entity::update_many()` /
`delete_many()` with a filter.

**Transactions:** a SeaORM transaction cannot adopt an `sqlx::Transaction`,
and sqlx cannot join a SeaORM one (`.ai_design/rust_orm/design.md`). Migrate a transaction's whole
body together, or keep it on one side. Never split one transaction across
the two.

## 4. Output parity

The migration changes how a row is fetched, never what the API returns.
Parity tiers 1-2 (PIDASHCONV-1 rule 13) apply:

- **Datetimes:** render through the same helpers
  (`serialize_iso_datetime`, `serialize_drf_datetime`). SeaORM returns
  `DateTime<FixedOffset>`; convert with `.with_timezone(&Utc)` first.
- **NULL vs JSON null:** an `Option<Json>` column gives `None` for SQL NULL
  and `Some(Value::Null)` for a JSON `null`. Keep whatever the old reader did.
- **Django properties over SQL:** when Django reads a value through a model
  property (for example `AgentRun.error` is
  `(error_details or {}).get("message") or ""`), port the property, not the
  old SQL expression.
- **Contract suites** for the module must pass **unedited**. A needed edit
  means behaviour changed: stop and explain it in the PR.

## 5. Entities

Every OSS table has an entity (`pidash-db` `entity/`, #2882), generated with
`sea-orm-cli` 1.1.20. The regeneration steps and cleanups are in
`entity/mod.rs`. The two that bite:

- **Partial unique indexes.** The generator turns a partial unique index (one
  with a `WHERE`) into `#[sea_orm(unique)]` on the column. Those flags are
  removed. Never re-add one without checking `pg_index.indpred`.
- **Trimmed relations.** Some `has_many` / `belongs_to` edges are deleted (the
  list is in `mod.rs`). A migration that needs one of those joins adds both
  sides.

## 6. Allowed exceptions (`// raw-sql: <reason>`)

Keep raw SQL only for statements SeaORM/sea-query genuinely cannot build, or
where the builder form would be clearly worse and is measured to be:

- DDL and the Rust job queue's own schema (`CREATE TABLE IF NOT EXISTS`,
  advisory locks such as `pg_advisory_xact_lock`), and `LISTEN`/`NOTIFY`.
- Recursive CTEs and window functions the builder cannot express.
- Row-locking forms the builder lacks, for example `FOR UPDATE SKIP LOCKED`
  with a CTE `RETURNING` chain.
- A measured hot path where the builder form is shown to be slower, with the
  numbers in the PR.

"It was already written this way" and "the SQL is long" are not reasons.

## 7. Per-PR checklist

1. One module (or one related group) per PR.
2. Each raw site becomes SeaORM, or keeps a `// raw-sql: <reason>`.
3. Lower the allowlist with `python3 scripts/raw_sql_guard.py --seed` and
   commit the diff. Only that module's lines should change.
4. `cargo fmt --check` and `cargo clippy --workspace --all-targets -D warnings`
   pass.
5. Unit tests pin every new statement's shape (§3.2) without a database.
6. The module's contract suites pass unedited in CI.
7. The PR body lists the before/after count and any exceptions with reasons.

## 8. Order

Ranked by risk: duplicate output names first, then positional reads and
joins (the bug class behind the incident), then volume. Counts are as of
2026-11-04; recompute with `--report` before each batch.

| Batch | Scope | Why first |
| --- | --- | --- |
| 0 | The 20 `[dup]` statements: `app_intake`, `app_issues/handlers_engage`, `auth_oauth`, `prompting`, `v1_assets`, `v1_cli_auth`, `jobs/assistant`, `oss_worker`, `jobs/tasks_mail`, `app_scheduler/queries` (12 clashing columns), `app_views_search`, `dispatch/tools`, `orchestration/creation`, `read/work_items` | Each may be a live bug like #2850. Triage: alias, or migrate. |
| 1 | `api/runner_enroll` (246 sites, 123 positional) | Most positional reads |
| 2 | `api/app_issues` (250, 112 joins) | The issue detail surface |
| 3 | `db/src` shared queries (286, 125 joins) | Used by many modules |
| 4 | `api/app_cycles`, `app_analytics`, `v1_work_items`, `app_project`, `app_workspace` | Join-heavy |
| 5 | `api/runner_runs`, `app_intake`, `app_scheduler`, `jobs/oss_worker` | Positional reads |
| 6+ | The remaining modules, largest first (`jobs/orchestration`, `v1_cycles_modules`, `app_integrations`, `services/domain` internals, then the rest) | Volume |

The end state is that the allowlist holds only `// raw-sql:` exceptions (the
guard then lists no counted sites), and the guard keeps it that way.
