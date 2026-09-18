# Data layer: replacing the Django ORM and handing over schema ownership

Status: **draft for review** (PIDASHCONV-4). Design only — no code.
Public document: describes OSS structure and generic extension points only,
never private overlay internals.

This document decides how the FastAPI backend reads and writes the existing
Postgres database while Django still owns the schema, and when and how schema
ownership moves. It covers every bullet in PIDASHCONV-4. Parent rules
(PIDASHCONV-1) apply: no schema change and no Django migration during
coexistence; PRs touch only files under `pidash_refactor_muse/`.

All measurements below were taken 2026-09-18 on `apps/api/pi_dash` at `main`
(`ce3cf2ad`), excluding tests and migrations unless noted. Where the sibling
drafts (`00-inventory-and-slices.md`, `01-architecture.md`, both under review
at the time of writing) quote a different figure, the figure here is the
re-measured one and supersedes it for data-layer purposes.

Two corrections to numbers repeated from the parent issue:

- `transaction.atomic` measures **168 sites including tests, 140 in
  non-test code** (heaviest: `runner/views/chat.py` 17, `run_endpoints.py`
  13). The parent's "170" is within noise; use 140/168.
- Django signal receivers measure **7 `@receiver` handlers in 5 files**,
  repo-wide including tests, with zero `.connect()` registrations. The
  parent's "roughly 38" is not reproduced and should be retired. The 7 are
  enumerated in §4.

## 1. What the Django data layer looks like today

### 1.1 Base classes and the save() pipeline

Every persistent model funnels through one inheritance chain
(`db/models/base.py`, `db/mixins.py`):

- `TimeAuditModel`: `created_at` (`auto_now_add`), `updated_at` (`auto_now`).
- `UserAuditModel`: `created_by` / `updated_by` FKs to `db.User`
  (`SET_NULL`, nullable, `%(class)s_*` related names).
- `SoftDeleteModel`: `deleted_at` nullable; default manager
  `SoftDeletionManager` filters `deleted_at__isnull=True`; escape hatch
  `all_objects` (used at **48 non-test call sites**).
- `AuditModel` = the three above. `BaseModel(AuditModel)` adds a UUID
  primary key (`default=uuid.uuid4`, client-side) and a `save()` override
  that stamps `created_by`/`updated_by` from `crum.get_current_user()`
  (create sets `created_by`, update sets `updated_by`), with opt-outs
  `created_by_id=...` and `disable_auto_set_user=True`.
- `ProjectBaseModel(BaseModel)` (`db/models/project.py:300`) and
  `WorkspaceBaseModel(BaseModel)` (`db/models/workspace.py:185`) re-derive
  `self.workspace` from `self.project.workspace` inside `save()`. Every
  project-scoped write therefore depends on this backfill.

`delete()` on the mixin defaults to soft (`deleted_at = now()` + `save()`),
and enqueues the Celery task `soft_delete_related_objects(app_label,
model_name, pk)` (`bgtasks/deletion_task.py:18`), which cascades the
soft-delete to related rows asynchronously. Hard delete requires
`delete(soft=False)`; `SoftDeletionQuerySet.delete()` maps to
`update(deleted_at=now())` by default. `Workspace.delete()`
(`db/models/workspace.py:156`) extends this with its own cascade.

`ChangeTrackerMixin` (`db/mixins.py:92`) snapshots field values at
`__init__`, exposes `changed_fields` / `old_values` / `has_changed()`, and
stashes `_changes_on_save` before resetting on every `save()`. Consumers
(e.g. `IssueComment.save()`, which mirrors comment fields into a linked
`Description` row inside `transaction.atomic()`) read `_changes_on_save`
to decide what to propagate.

### 1.2 Custom managers: hidden query filters

| Manager | File | Hidden predicate (applies to every default query) |
|---|---|---|
| `SoftDeletionManager` | `db/mixins.py:56` | `deleted_at IS NULL` |
| `IssueManager` (`issue_objects`) | `db/models/issue.py:95` | excludes triage-state issues, `archived_at NOT NULL`, issues in archived projects, `is_draft=True` |
| `StateManager` (`objects`) | `db/models/state.py:79` | excludes `group = TRIAGE` |
| `TriageStateManager` | `db/models/state.py:86` | only `group = TRIAGE` |
| `PodManager` | `runner/models.py:39` | `deleted_at IS NULL` |
| `UserManager` | Django's `contrib.auth` | none (inherited default) |

`State` additionally keeps `all_state_objects`. Anything migrating an
`Issue` / `State` / soft-deletable query must reproduce the exact
predicate — these are the easiest parity bugs to introduce because the
filter is invisible at the call site.

### 1.3 Columns: UUID keys, JSON, constraints, indexes

- **UUID primary keys** throughout (`default=uuid.uuid4`, generated
  client-side in Python — no `pgcrypto` / DB-side default involved).
- **`JSONField` × 122** in `db/` (e.g. `Account.metadata`,
  comment/description JSON bodies). Postgres-native `jsonb`.
- **Declarative constraints/indexes × 100** (`UniqueConstraint`,
  `CheckConstraint`, `GinIndex`, `Index`) plus **`unique_together` × 47**.
  Two GIN indexes are load-bearing for query plans (§3, spike 3):
  `issues_fts_idx` (`Issue.Meta`, `SearchVector("name",
  "description_stripped", config="english")`) and
  `issue_comments_fts_idx` (`IssueComment.Meta`,
  `SearchVector("comment_stripped", ...)`).
- Non-default table names exist (e.g. `accounts` for the auth `Account`
  model, `Meta.db_table`); any re-mapping must preserve them byte for
  byte. Model `Meta.ordering` (e.g. `("-created_at",)`) leaks into
  subqueries unless cleared with `.order_by()` — see
  `search/issue.py:_matching_comment_issue_ids`.

### 1.4 ORM features used in queries (non-test, non-migration counts)

| Feature | Count | Notes |
|---|---|---|
| `annotate()` | 662 | heaviest: `app/views/issue/base.py` 60, `module/base.py` + `module/archive.py` 56 each |
| `Subquery` / `OuterRef`+`Exists` | 114 / 276 | per-row existence and aggregate subqueries in list endpoints |
| `Window()` | 4 | grouped cursor paginator (`utils/paginator.py:250,455`) + `bgtasks/cleanup_task.py:325,361` (dedup `RowNumber`) |
| `prefetch_related` / `select_related` | 89 / 288 | N+1 control is manual and per-view |
| Full-text search | 15 | `search/issue.py` (`SearchQuery websearch`, `SearchRank`, `SearchHeadline`) + the two GIN indexes |
| Aggregate/window expressions (`ArrayAgg`, `RowNumber`, …) | 88 | analytics and grouped views |
| `select_for_update` | 109 | concentrated in `runner/` (chat 15, enrollment 10, matcher 6) + `orchestration/scheduling.py` 4 + `bgtasks/agent_ticker.py` 4 |
| `distinct()` | 112 | |
| `values()` / `values_list()` | 617 | dict-projections used as lightweight DTOs |
| `F()` / `Case`+`When` / `Coalesce`+`Cast` | 210 / 132 / 164 | atomic counters, conditional updates, type coercion |
| `bulk_create` / `bulk_update` | 101 | bypass `save()` and signals — see §4 |
| `QuerySet.update()` | 170 | bypasses `save()`/`auto_now` handling notes (cf. `bgtasks/scheduler.py:250`); signals do not fire |
| `get_or_create` / `update_or_create` / `in_bulk` | 55 | |
| `only()` / `defer()` / `iterator()` | 21 / 8 | |
| `.raw()` / `RawSQL` / cursor `execute()` | 15 | confined to `db/management/commands/wait_for_db.py` — no business-logic raw SQL |
| `.extra()` / `.using()` | 0 / 0 | no legacy escape hatches; explicit replica routing goes through the router |

Migrations: 215 numbered files repo-wide (174 under `db/migrations/`).
Concrete models: ~145 classes (`db/models/` + `runner/models.py` 18 +
`assistant/models.py` 6 + `prompting` 2 + `license`), consistent with the
parent's "165 models / 219 migrations" within counting-method noise.

## 2. Side stores: MongoDB and Redis

Both travel with the migration; neither is replaced by this design.

- **MongoDB** is optional and append-only. `MongoConnection`
  (`settings/mongo.py`) is a singleton `pymongo.MongoClient` guarded by
  `is_configured()` — when `MONGO_DB_URL`/`MONGO_DB_DATABASE` are unset,
  everything degrades to Postgres or to dropping the write. Users:
  `bgtasks/logger_task.py` (`api_activity_logs` collection),
  `bgtasks/cleanup_task.py`, `bgtasks/webhook_task.py`,
  `middleware/logger.py`. Recommendation: FastAPI reuses the identical
  pattern — a small sync `pymongo` helper module behind the same
  `is_configured()` guard, called from services (or `run_sync` from async
  paths). No async Motor migration; the writes are fire-and-forget logs.
- **Redis** is required. `settings/redis.py` already maintains **both** a
  sync client and an async (`redis.asyncio`) client with lazy singletons.
  Used by the assistant runtime (`assistant/runtime/`, SSE/event fan-out),
  runner session/machine views, Celery tasks, and `config/registry.py`.
  Recommendation: FastAPI uses `redis.asyncio` exclusively (it already
  exists in the dependency tree), sharing the same key space and URL
  settings. Key-prefix ownership per backend during coexistence is defined
  in the pilot-slice design, not here.
- **Celery stays Django-side** until its own slice migrates
  (`settings/celery.py`, `DatabaseScheduler` via django-celery-beat,
  78 tasks). FastAPI slices that need today's tasks (notably
  `soft_delete_related_objects`) enqueue by task name over the shared
  broker with a thin sender — no Django import, no behavior change.

## 3. Library decision

### Recommendation: SQLAlchemy 2.0 async + psycopg 3 + (later) Alembic

- **SQLAlchemy 2.0, async style** (`create_async_engine`,
  `async_sessionmaker`, `AsyncSession`, `select()`/ORM-mapped classes).
  It covers the whole measured surface: `Exists`/`scalar_subquery` for the
  276 `OuterRef`/`Exists` sites, `func.row_number().over()` for the 4
  `Window` sites, `with_loader_criteria` / repository base filters for the
  hidden manager predicates, `with_for_update()` for the 109 row-lock
  sites, `func.to_tsvector`/`websearch_to_tsquery`/`ts_rank_cd` for FTS,
  `insert().on_conflict_*` (via the `sqlalchemy.dialects.postgresql`
  dialect) for the get/update-or-create sites, and `after_commit`-style
  hooks for the `on_commit` sites.
- **Driver: psycopg 3 async** (`psycopg==3.3.0` is already pinned in
  `requirements/base.txt`). One driver serves both backends during
  coexistence (Django uses it synchronously today), so no new native
  dependency enters the image. `asyncpg` is rejected for exactly that
  reason: a second driver doubles connection behavior to qualify under a
  tight budget (§7) for no measured need.
- **Alembic only after handover** (§6). It must not run, and must not even
  be configured to run, during coexistence — Django migrations are the
  single owner until the named handover point.

### Options rejected

- **SQLModel.** Couples table definition to API schemas; the codebase
  needs them separate (257 serializers vs 145 models, per-view serializer
  subclasses). It also hides exactly the constraint/index/partition
  details this migration must preserve, and its Pydantic-version coupling
  is a liability when `pydantic-ai-slim` already forces Pydantic 2.x
  upgrades on its own schedule.
- **Query builder only (e.g. raw `psycopg` + `asyncpg`-style SQL).**
  Adequate for reads, but the 140 `atomic` sites and the per-model
  `save()` side effects (§4) need unit-of-work/identity-map semantics to
  port safely. Hand-rolled UoW re-implements SQLAlchemy badly.
- **Synchronous SQLAlchemy (`psycopg` sync) under `anyio.to_thread`.**
  Works, but every concurrent request holds a full thread + connection,
  doubling the connection math in §7. The codebase already runs on
  `UvicornWorker` with `asgiref` async usage (`sync_to_async` in
  `assistant/`, `runner/`, `cloud_agent/`); async-first matches the
  serving model chosen in `01-architecture.md` §11, which reserves the
  sync/async session decision to this document. **Decision: async.**

### 3.1 Spike results (representative hard queries)

Three queries were chosen to cover the riskiest constructs; each was
mapped to its SQLAlchemy 2 equivalent (sketches, not committed code).

**Spike 1 — annotated list queryset with existence subqueries.**
Representative: `app/views/issue/base.py` (60 `annotate()` sites; per-row
`Exists`/`OuterRef` subqueries for subscription, relation, and label
membership layered under the `IssueManager` hidden predicates).

```python
# Django (shape): Issue.issue_objects.filter(...).annotate(
#     is_subscribed=Exists(sub_q), label_ids=ArraySubquery(...))
stmt = (
    select(Issue, exists(sub_q).label("is_subscribed"))
    .where(Issue.deleted_at.is_(None))          # ex-SoftDeletionManager
    .where(Issue.state.has(State.group != "triage"))  # ex-IssueManager
    .where(~Issue.is_draft, Issue.archived_at.is_(None))
)
```

Verdict: direct. `Exists`/`scalar_subquery` compose identically; the only
work is making the four hidden `IssueManager` predicates explicit in one
shared repository function so all 60 sites keep them.

**Spike 2 — grouped cursor pagination with a window function.**
Representative: `utils/paginator.py:250` (`RowNumber()` partitioned by the
group-by field, then `row_number > offset AND < stop`).

```python
rownum = func.row_number().over(
    partition_by=getattr(model, group_field),
    order_by=(key_col.desc().nulls_last(), model.created_at.desc()),
).label("row_number")
subq = select(model, rownum).subquery()
stmt = select(subq).where(subq.c.row_number > offset, subq.c.row_number < stop)
```

Verdict: direct, with one caveat — Django renders the filter-then-order
shape in one statement while the portable SA form needs the explicit
subquery above; `EXPLAIN` parity on a staging-size dataset is a checklist
item for the pilot slice, not a blocker.

**Spike 3 — full-text search with GIN-index parity.**
Representative: `search/issue.py` (`SearchQuery websearch`,
`SearchRank`, `SearchHeadline`; `issues_fts_idx` on
`to_tsvector('english', name || ' ' || description_stripped)`).

```python
vector = func.to_tsvector("english", Issue.name + " " + Issue.description_stripped)
query = func.websearch_to_tsquery("english", user_text)
stmt = (
    select(Issue, func.ts_rank_cd(vector, query).label("_rank"))
    .where(vector.op("@@")(query))
    .order_by(desc("_rank"))
)
```

Verdict: feasible with a hard constraint — the `to_tsvector` expression
must stay **byte-for-byte identical** to the index expression (the Django
code comments say the same about `SearchVector`), or the planner drops
`issues_fts_idx`. The CI drift check (§5) compares rendered index
expressions, not just column sets, for the two FTS tables. `ts_headline`
options (`start_sel <<`, `stop_sel >>`, `max_words 20`) map 1:1 to
`func.ts_headline`. The comment-subquery branch
(`_matching_comment_issue_ids`, with its `order_by()`-clearing and
`access`-filter limitation) ports as a second `exists()` OR-branch, bugs
included and documented at port time.

## 4. Behavior hidden in save() and signals: full inventory and replacements

Rule: **no implicit behavior in the FastAPI data layer.** Everything below
moves to explicit service/repository calls. `bulk_*`/`update()` call sites
(101 + 170) already bypass `save()`/signals today, so making the
remaining paths explicit *reduces* behavioral variance rather than adding
it — but each ported slice must still audit whether its path relied on a
bypassed or a fired hook.

### 4.1 save() overrides (23 model sites + 2 framework sites)

| Model(s) | Hidden behavior | Replacement |
|---|---|---|
| `BaseModel` (`db/models/base.py:23`) | crum `created_by`/`updated_by` stamping | explicit `actor_id` param from request scope (§4.3); stamped by repository |
| `ProjectBaseModel`, `WorkspaceBaseModel` | backfill `workspace` from `project` | repository invariant, tested once |
| `Issue` (`issue.py:267`) | default pod resolution + default-state resolution (2 extra queries on create) | service function `create_issue()` performing the same lookups in order |
| `IssueComment` (`issue.py:598`) | strip-tags + linked `Description` create/sync in `transaction.atomic()` | service function owning both writes in one transaction |
| `Project` ×3, `Workspace`, `State`, `Module`, `Cycle`, `Page` ×2, `Description` ×2, `Draft`, `Favorite`, `Label`, `Sticky`, `View`, `User`, `runner/models.py` ×3 | per-model defaults/denormalization (sequence counters, slugs, sort orders, derived flags) | per-entity service functions; each migrated slice lists the exact override it replaces |
| `ChangeTrackerMixin.save()` | `_changes_on_save` snapshot | SQLAlchemy attribute history (`attributes.get_history`) read in the service before flush |

### 4.2 Signal handlers (7 total — the "~38" figure is retired)

| Signal | File | Effect | Replacement |
|---|---|---|---|
| `pre_save` + `post_save`, `Issue` | `bgtasks/github_signals.py` | snapshot + enqueue GitHub completion-comment sync | service emits domain event after commit; worker subscribes |
| `pre_save` + `post_save`, `Issue` | `orchestration/signals.py` | agent state-transition dispatch (with re-entrancy guards) | explicit `dispatch_agent_transition()` call in the issue service |
| `post_save`, `Project` | `runner/signals.py` | auto-create default `Pod` | `create_project()` creates the pod in the same transaction |
| `post_save`, `Workspace` | `scheduler/signals.py` | seed builtin schedulers | `create_workspace()` seeds them explicitly |
| `post_save`, `User` | `db/models/user.py:307` | create `UserNotificationPreference` row | `create_user()` creates it in the same transaction |

Pattern for all seven: the service function performs the write **and**
emits a named domain event / enqueues the follow-up **after commit**
(§7, `on_commit` mapping). Cross-process follow-ups keep going through
the existing Celery tasks during coexistence (§2) — the trigger moves,
the worker does not.

### 4.3 Audit fields without django-crum

`django-crum==0.7.9` (`CurrentRequestUserMiddleware`, 17 non-test
`get_current_user`/`impersonate` uses) is thread-local request magic with
no async-native equivalent. Replacement:

- A `ContextVar`-based request scope (the pattern already exists in
  `utils/core/request_scope.py`, which uses `asgiref.local.Local`
  precisely for async-safe isolation) carries the authenticated actor for
  the request. The auth dependency sets it; the repository reads it as an
  explicit argument, never as ambient state inside model code.
- `created_at`/`updated_at`: SQLAlchemy `default=`/`onupdate=` (Python
  side, matching `auto_now_add`/`auto_now` semantics exactly — wall-clock
  at flush, not statement time). No DB defaults are added: that would be
  a schema change.
- `created_by`/`updated_by`: explicit columns on write; background paths
  (Celery, no request) pass an explicit system/bot actor id, mirroring
  today's `impersonate` uses in `assistant/tools/` and `cloud_agent/`.
  `disable_auto_set_user=True` and `created_by_id=` call sites map 1:1 to
  "omit the actor argument" / "pass this id".

## 5. Models while Django owns the schema

**Decision: hand-written SQLAlchemy models, generator-assisted, checked
in; never reflected at runtime, never `create_all` against the shared
database.**

- Each model declares `__tablename__` (preserving `Meta.db_table`
  overrides like `accounts`), explicit column types matching the
  Django-rendered DDL (`UUID(as_uuid=True)` with client-side
  `default=uuid.uuid4` — **not** a DB default; `JSONB` for the 122 JSON
  fields; identical `nullable`, `unique`, `CheckConstraint`,
  `UniqueConstraint`, and index declarations including the two FTS GIN
  indexes with byte-identical expressions).
- Initial drafts may be produced with a reflection generator
  (`sqlacodegen` against a Django-migrated scratch DB) to avoid
  transcription slips across ~145 classes, but the output is
  human-reviewed, hand-edited, and committed. Runtime reflection is
  rejected: it loses column comments, constraint names, and the
  relationship shapes the repositories need, and it makes drift silent.
- `metadata.create_all` is forbidden outside throwaway test databases
  (§8). The shared database is migrated by Django only (§6).

**CI drift detection** (added with the scaffold; this design mandates the
checks, the scaffold implements them):

1. `manage.py makemigrations --check` — fails if Django models changed
   without a migration (Django-side drift).
2. Metadata-vs-database comparator — spins up scratch Postgres, applies
   Django migrations to head, reflects it, and diffs table/column sets,
   nullability, and **rendered index expressions** against the FastAPI
   `MetaData`. Any delta fails the job. This is the check that protects
   the FTS index parity in spike 3 and catches a Django migration the
   port missed.

## 6. Schema ownership and the handover

**Django migrations remain the single owner until a named handover
point.** No Alembic, no DDL from FastAPI, no exceptions during
coexistence.

- **Handover point H0** is defined as: *the release in which the last
  Django-served write path is cut over to FastAPI and Django enters
  read-only/shadow mode for one full release.* H0 is a routing fact, not
  a date — the pilot-slice plan (a later design) must show the ordered
  list of slices whose completion constitutes H0.
- **Alembic baseline procedure at H0** (works for a self-hoster upgrading
  from *any* recent migration level):
  1. Freeze: record the Django migration head `N` (all apps) at H0.
     `db/migrations/` is frozen — kept in the tree, never edited again.
  2. Add one Alembic revision `h0_baseline` whose `upgrade()` and
     `downgrade()` are **empty (no DDL)**. It is generated from a
     reflection of a Django-migrated-at-`N` database so its metadata
     snapshot equals production schema by construction.
  3. Deploy order in the H0 release: run `manage.py migrate` first
     (brings a database at *any* older level to `N` — Django migrations
     already handle that; this is why the handover works from any recent
     level), assert `django_migrations` contains every leaf up to `N`,
     then `alembic stamp h0_baseline` (records without executing DDL).
  4. After H0: all schema changes are Alembic revisions; `env.py`
     refuses to run unless the baseline stamp is present. Django
     `migrations/` remain readable history for forks upgrading across
     H0 and are removed in a later release, never before.
- **Rollback across H0:** re-point routing to Django (Rule: rollback is
  a routing change) — valid only while no post-H0 Alembic revision has
  run. The first real Alembic revision past H0 closes the rollback
  window; that revision's design must say so explicitly.

## 7. Transactions, session lifecycle, and connection pooling

### Transaction mapping

| Django today | FastAPI |
|---|---|
| `transaction.atomic()` (140 non-test sites) | service-owned `async with session.begin():`; nesting via savepoints (`begin_nested()`) where Django nests atomics |
| `select_for_update()` (109 sites) | `select(...).with_for_update(nowait/skip_locked=...)`, same lock strength per call site |
| `transaction.on_commit(...)` (~10 sites: celery `.delay`, event publish, pod drain) | after-commit callback list drained by the request dependency after successful commit; cross-process effects keep at-least-once semantics via the existing Celery tasks |
| `QuerySet.update()` / `bulk_*` (271 sites, skip `save()`/signals) | `update()`/`insert()` Core statements in repositories — same skip, now visible instead of accidental |

Isolation level stays at Postgres default (`read committed`) unless a
ported call site proves otherwise; any exception is recorded in that
slice's design.

### Session lifecycle

One `AsyncSession` per request, created by a FastAPI dependency:
open → run endpoint/services → commit on success, rollback on exception
→ close, draining after-commit callbacks only on the commit path.
Repositories receive the session as an argument (the boundary fixed in
`01-architecture.md` §11); services own transaction boundaries; HTTP and
ORM-session-factory imports never leak into services. `expire_on_commit`
is `False` (read-after-write without implicit re-selects; DTO conversion
at the service boundary makes staleness visible). Background tasks open
their own short-lived sessions; never share a request session across a
task boundary.

### Connection budget (small self-hosted Postgres)

Today: `CONN_MAX_AGE` is unset (Django default `0` — connections open
per request, bounded in practice by `$GUNICORN_WORKERS` Uvicorn workers
plus Celery worker/beat processes). FastAPI must fit inside the same
`max_connections` (stock Postgres default 100, often lowered on small
hosts):

- One async engine per process; `pool_size=5`, `max_overflow=5`,
  `pool_timeout=30`, `pool_pre_ping=True`, `pool_recycle=1800`.
  Single uvicorn worker per container during the pilot; scale workers
  only with a measured `pg_stat_activity` ceiling (Django workers +
  Celery concurrency + FastAPI `(pool_size + overflow) × workers` +
  replica + headroom ≥ 20%).
- Keep `idle_in_transaction_session_timeout=60s` parity
  (`settings/common.py:186` sets it for Django) on FastAPI sessions so a
  stalled await cannot hold row locks indefinitely on either backend.
- Statement timeout is set per-engine, not per-database, so Django's
  behavior is untouched.
- PgBouncer is **not** introduced by this design. If the pilot's
  measurements exhaust the budget, the follow-up decision is a
  transaction-pooler sidecar evaluated against `select_for_update` and
  advisory-lock usage — called out here as the expected pressure valve,
  not a silent addition.

## 8. Test database strategy and fixtures

- **Same Postgres, same tables.** FastAPI tests run against a database
  created by the Django test runner (`pytest.ini`: `pytest-django`,
  `--reuse-db --nomigrations`, markers `unit/contract/smoke/slow`).
  `metadata.create_all` is allowed only on throwaway local databases,
  never as the suite's schema source — otherwise the suite would pass
  against a schema Django never built, masking exactly the drift §5
  exists to catch.
- **Isolation:** per-test transaction rollback (open transaction in a
  fixture, roll back after the test) mirroring Django `TestCase`
  semantics; session-scoped `--reuse-db` equivalent keeps the suite fast
  across 215 migrations.
- **Fixtures:** port `tests/factories.py` + `tests/conftest.py`
  (`create_user`, `api_token`, workspace/project graphs) to async factory
  functions returning ORM instances bound to the test session. Keep the
  same object graphs so parity tests are meaningful.
- **Parity tests (the core of the strategy):** for each migrated slice,
  contract tests drive the same scenario through the Django endpoint and
  the FastAPI endpoint against identically-factored data and diff the
  responses (status, body shape, ordering, soft-delete visibility,
  audit stamps). Disagreements fail the build; known-intentional
  differences are allow-listed per slice with an expiry (the slice's
  H0 contribution), never globally.
- **Load/parity for the three spikes:** `EXPLAIN`-level plan check for
  spike 2 on staging-size data; `issues_fts_idx` usage assertion for
  spike 3 (`EXPLAIN` shows the bitmap index scan); predicate-equivalence
  tests for spike 1 (hidden-manager predicates return identical row sets
  on a fixture containing triage, archived, draft, and soft-deleted rows).

## 9. Schema-change call-out (for Rich)

**No schema change and no Django migration is required by this design
during coexistence.** Specifically: no new columns, indexes, defaults,
or constraints; UUIDs stay client-generated; timestamps stay
Python-side; the FTS indexes are reused as-is; soft-delete and audit
columns already exist on every table that needs them.

Two foreseeable pressures that would each need a **separate decision**,
not a silent addition here:

1. If the connection budget (§7) proves too tight in the pilot, adding a
   transaction-pooler sidecar or a column/index for a rewritten hot query.
2. If any slice cannot reproduce a `Meta.ordering`- or
   `unique_together`-dependent behavior without a supporting index, that
   index ships as its own proposal.

## 10. Risks and rollback

- **Silent predicate loss** (hidden manager filters, `all_objects`
  escapes, `Meta.ordering`). Mitigation: shared repository functions +
  predicate-equivalence fixtures (§8) + the drift comparator (§5).
- **Write-write races during coexistence.** Both backends write the same
  tables; Django has no knowledge of FastAPI's after-commit hooks and
  vice versa. Mitigation: slices are cut so that one entity's writes
  live on one backend at a time (routing-level mutual exclusion); the
  pilot proves the pattern before wider cutover.
- **Transaction-scope mismatch** (`on_commit` → after-commit,
  `select_for_update` lock strength, `bulk_*` skip semantics).
  Mitigation: per-slice mapping table against §7; parity tests cover the
  locked paths (`runner/` enrollment/chat are the densest).
- **Rollback of this design:** documentation only — revert the PR. wasted
  work is bounded to review time. Rollback of built slices stays a
  routing change per parent Rule (constraints), with the H0 window
  defined in §6.

## 11. Deliverable and process

Deliverable: this document at
`pidash_refactor_muse/django_to_fastapi/docs/02-data-layer.md`, via pull
request touching only `pidash_refactor_muse/`, with a summary comment on
PIDASHCONV-4, the issue moved to In Review, and Rich's approval awaited
before any `backend/` code or scaffold work begins. The scaffold and the
pilot slice are separate, later issues that implement what this design
approves — including the CI drift checks (§5), the repository base with
the soft-delete/audit/scope primitives (§4–§5), and the session/pooling
wiring (§7).
