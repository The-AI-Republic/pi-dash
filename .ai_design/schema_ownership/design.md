# Schema ownership: Rust owns the schema, with a defined Django fallback window

Status: design for maintainer decision. Implements the maintainer decisions of
2026-11-22 on NEWPIDASH-379 (Rust owns the schema; native OSS migrator) and the
follow-up (OSS/cloud combine like the Django `INSTALLED_APPS` graph; full model
in NEWPIDASH-380). This doc fills in the remaining details: fresh-install
baseline, fallback-window constraints, cloud history adoption, the `pidash-sdk`
dependency path, `ensure_schema`, the startup gate, `verify-source`/port-check
retirement, and the NEWPIDASH-372 closure. It ends with the proposed amended
NEWPIDASH "Schema" rule text.

Research basis: OSS `ed077eaf5`, private `2f24d30` (2026-11-21), i.e. the
NEWPIDASH-375 research basis. File:line references below are carried over from
the NEWPIDASH-375/379/380/372/378 issue bodies at that basis. This run had no
repository checkout, so nothing below re-verifies them against live main; the
implementing run (NEWPIDASH-380, branched from `origin/main` at `ed077eaf5`)
owns that recheck before changing anything, per the epic rule.

## 1. Source of truth (recommendation: A, as decided)

The maintainer has decided: the Rust migration owns the schema. Of the three
options in the issue, that is (A): move `pidash-migrate`'s approach into OSS as
a native `pidash-api migrate` under `rust-api/`, still writing
`django_migrations` rows while the Django fallback is kept.

- Not (B) (Rust migrations are the truth and Django only follows with
  state-only or no-op migrations): during the fallback window Django must be
  able to run `manage.py migrate` for real on a database the Rust migrator
  built. State-only Django migrations would make Django's own history a fiction
  and keep Django as the authoring bottleneck for every schema change, while
  the proven mechanism — the cloud migrator writing `django_migrations` rows
  after each port (`rust-cloud/crates/cloud-migration/lib.rs:539-544`) and
  skipping migrations Django already applied — already delivers a truthful
  shared history with no Django-side fakery.
- Not (C) (merge the gates first, then move): the gates are already being fixed
  in parallel without serializing on this design. NEWPIDASH-378 replaces the
  waiting gate with a fail-fast check (the six OSS entrypoints
  `rust-api/bin/docker-entrypoint-{api,worker,beat}{,-local}.sh` currently run
  `pidash-api ops wait_for_migrations`, which polls forever from
  `rust-api/crates/services/src/ops/boot.rs`, against stale hard-coded
  `LEAF_MIGRATIONS` at `boot.rs:77-87`), NEWPIDASH-382 owns running the
  migrator before services, and NEWPIDASH-380 swaps the interim check for
  `migrate check`. No gate blocks the move.

Concretely: one migration crate under `rust-api/` holds every OSS migration,
grouped by app (`db`, `runner`, `license`, `prompting`, `assistant`, the Django
contrib tables, the Rust-only tables). Each migration declares its
dependencies, like Django's `dependencies`. `migrate up` applies them in
dependency order under one advisory lock, one transaction per migration
(except documented concurrent-index phases), records each Django-equivalent
migration in `django_migrations`, imports migrations Django already applied
without rerunning them, and exits nonzero naming the failing migration on any
failure. This reuses what works in `rust-cloud/crates/cloud-migration`
(SeaORM, advisory lock, owner/app URL split).

## 2. Fresh installs: baseline snapshot plus forward ports

An empty OSS database is created with no Python, in two parts:

1. **Baseline snapshot at the current leaf.** A checked-in baseline creates the
   full current OSS schema on an empty database in one step: all OSS app tables
   at the leaf the implementation lands on (`db/0183`, `runner/0043` plus
   `runner/0044` once NEWPIDASH-372 merges — the 380 run already assumes this
   rebase), the Django contrib tables (`auth`, `contenttypes`, `sessions`,
   `django_celery_beat` and the other installed contrib apps), and the
   Rust-only tables (see section 4). The baseline is produced from a
   Django-migrated database (schema dump taken under the same procedure as the
   entity generation in `rust-api/crates/db/src/entity/mod.rs:4-32`), not
   hand-written, so it cannot drift from Django's output by construction.
2. **Stamp, then forward ports.** Applying the baseline also writes the full
   set of `django_migrations` rows up to the baseline leaf, so a fallback
   Django started on the database runs `migrate --check` with nothing to
   apply. Every migration after the baseline is a forward port applied
   normally. A database Django migrated to the leaf is a no-op; a partly
   migrated one gets the rest finished.

Cloud adds baselines for its own two apps (`cloud_assistant`,
`platform_federation`) the same way; the cloud binary's combined `migrate up`
runs the OSS baseline plus the cloud baselines, then forward migrations.

Proof (NEWPIDASH-380 Done when, kept): `migrate up` on an empty Postgres
produces a schema identical (tables, columns, types, defaults, indexes,
constraints) to Django `migrate` at the same leaf, checked by a comparison in
CI in the style of the current `oracle.py compare`; the cloud combined
migrator is compared against Django with OSS plus `pi_dash_cloud` settings.
Until that comparison merges, the parity-stack contract suites plus the
`oracle.py compare` it replaces are the evidence, and the workpad names which
one was used.

## 3. Fallback window: what every Rust schema change must satisfy, and when it ends

While the Django fallback is kept, the shared database must stay usable by the
old system, and a fallback Django must still `manage.py migrate` for real.
So every Rust schema change, without exception, satisfies all of these:

- **Additive only.** New tables, columns, indexes and constraints may be
  added. Never drop, rename or retype a column or table the old system still
  uses; mark it unused instead and leave the cleanup to the switchover. (This
  carries over unchanged from today's rule.)
- **A database-level default on every new NOT NULL column**, so inserts that
  do not name it keep working — from either system.
- **A matching schema-only Django model field plus Django migration, authored
  in the same issue**: the Django model field and the migration only
  (`apps/api/pi_dash/db/models` and `migrations`), no serializers, views or
  behaviour. The Django migration is the portable spec of the change; the Rust
  migration ports it (DDL-identical, as NEWPIDASH-372's `runner/0044` is to
  cloud `native_runner_0040.sql`). Small changes may land both halves in one
  PR; otherwise the Django half lands first.
- **Entities regenerated** from the migrated schema as the backend README
  describes (`rust-api/README.md:411,516` procedure, amended to say the Rust
  migrator builds the reference database).

This is exactly how NEWPIDASH-372 closed its gap (schema-only `runner/0044`
plus model fields, no Python behaviour, allowed in the frozen tree under the
Schema exception), and that is the template for every schema change during the
window.

**End signal.** The requirement ends when the maintainer retires the Django
fallback — the later switchover epic that NEWPIDASH-375 explicitly defers
("Deprecating the old system … is a later epic, opened when this one is
Done"). Until the maintainer declares the fallback retired, the constraints
above hold on every schema change, no matter how long the window lasts. When
the fallback retires, the migrator keeps its history and stops writing
`django_migrations` rows; no migration is rewritten.

The amended NEWPIDASH "Schema" rule text is proposed in section 8.

## 4. Boot-time DDL: yes, `ensure_schema` becomes migrations

The two `ensure_schema` sites become versioned migrations in the OSS migration
set, owned by the migrator instead of the serve/worker boot path:

- `rust_job_queue` and `rust_job_schedule` in
  `rust-api/crates/jobs/src/queue.rs:229-319`;
- `rust_runner_model_catalog` in
  `rust-api/crates/api/src/runner_models/schema.rs:248-300`.

They are folded into the baseline (section 2) for fresh installs, and added as
idempotent guarded migrations (`IF NOT EXISTS` / `DO` blocks in the style
372 used) for the upgrade path, so existing databases that already had the
tables created at boot adopt them without error and without recreating
anything. NEWPIDASH-380 Done when keeps the hard requirement: after the move,
no service process issues DDL at startup — no `ensure_schema` left on the
serve or worker path.

## 5. Startup gate: `migrate check` replaces `wait_for_migrations` and the leaf list

- Services never run migrations, never wait for them, and issue no DDL. At
  most they run the read-only `migrate check`: one probe comparing the applied
  history against the migration set; exit 0 if current, otherwise print what
  is missing and exit nonzero immediately. A missing history table or an
  unreachable database also exits nonzero. No polling, no sleeps.
- This replaces both halves of NEWPIDASH-378: the `wait_for_migrations` calls
  in all six OSS entrypoints (378 swaps them for its interim fail-fast check
  first — `pidash-api ops check_migrations`, one probe of
  `django_migrations` — and 380 then replaces that interim check with the
  native `migrate check`), and the hard-coded `LEAF_MIGRATIONS`
  (`boot.rs:77-87`, already stale: it pins `db/0182_project_settings` and
  `runner/0042_agentrun_work_type` while main is at `db/0183` and
  `runner/0043`). The leaf list disappears entirely; there is nothing left to
  go stale because `check` reads the migration set, not a copy of it.
- Migration itself runs once, before any service starts, as a release/install
  step (NEWPIDASH-382 owns the compose/`setup.sh` wiring). If it fails or does
  not finish, the release or install fails and the old services keep running.
  Cloud already works this way (`release.yml:731` runs
  `compose run --rm --no-deps migrator` in an `&&` chain before recreating
  services); OSS converges on it. The Rust image needs no Python at any step:
  this also retires `rust-api/bin/docker-entrypoint-migrator.sh:12`, which
  cannot work in the Rust image, and updates its golden test
  (`rust-api/contract-tests/ops/test_image.py:88,425-433,466-497`).

## 6. One migrator or two: one (decided; this section records the consequences)

One graph, one `migrate up`, one history — the same combination model as
Django, where the cloud image runs the OSS Django code with the cloud apps
added to `INSTALLED_APPS` (`private-pi-dash/pi_dash_cloud/settings/cloud.py:58-65`)
and one `manage.py migrate` sees OSS apps plus `cloud_assistant` (7
migrations) and `platform_federation` (1, depending on OSS `db/0148`),
recorded in one `django_migrations` table:

- OSS owns the migrator and all OSS migrations under `rust-api/`. Cloud adds
  only its own apps' migrations on top. Cloud migrations may depend on OSS
  migrations; OSS never depends on cloud, and OSS never knows the cloud apps
  exist.
- The OSS binary runs OSS migrations only; the cloud binary builds the
  migrator from the OSS set plus its own and runs the combined set. Same
  commands (`up`, `status`, `check`) in both.
- No hand-porting of OSS migrations into cloud, ever again. An OSS schema
  change reaches cloud with the pin bump, as with Django.

Retired when the cloud rebuild lands (private PR after the OSS PR and pin
bump, per the epic's merge order): the 23 hand-ported OSS migrations in
`rust-cloud/crates/cloud-migration/sql/`, `verify-source` and `sources.json`,
the pin-bump fingerprint gate in `release.yml`, OSS
`.github/workflows/cloud-migration-port-check.yml` (and the
`cloud-port-tracked` label it enforces), and the "no Rust migrations, ever"
statements (`rust-cloud/README.md:6`, `cloud-db/src/lib.rs:11`), plus the
`rust-api/README.md:411,516`, `.ai_design/rust_orm/design.md:65-71,158-159`
and ~25 module-doc statements that say Django owns the schema.

**Existing cloud history is adopted, not rewritten.** Databases already
migrated by `pidash-migrate` carry rows in `pidash_cloud_schema_migrations`
and `django_migrations`. The combined migrator maps both to the combined
history without rerunning anything: every migration Django already applied
(including the 23 formerly hand-ported ones, whose `django_migrations` rows
the cloud migrator wrote itself) is imported, and the old
`pidash_cloud_schema_migrations` table is left behind once the combined run
confirms parity (380 proves: on a copy of the current test database the
combined migrator applies nothing and rewrites no history).

**Dependency path.** The overlay reaches OSS only through `pidash-sdk`
(`rust-cloud/Cargo.toml:26-35`, PIDASHCONV-1551). The migration set and the
migrator builder are exposed through `pidash-sdk`; if that proves unworkable,
the 380 run records a reviewed exception for the migration crate instead of
quietly depending around the boundary. Either way the exception-or-path is
written down in the 380 PR, not assumed.

## 7. Columns only cloud creates: closed with NEWPIDASH-372

The gap — OSS entities using `agent_run.assignment_token`,
`assignment_protocol`, `capacity_wait` and `runner_session.capabilities`, which
only cloud `native_runner_0040.sql` created — is closed in two steps already
in flight:

1. NEWPIDASH-372 (In Test, `pi-dash/newpidash-372`, PR #3459) adds the missing
   columns as one schema-only OSS Django migration, `runner/0044` on top of
   `runner/0043_desktop_handoff`, plus the matching model fields, with DDL
   identical to `native_runner_0040.sql` (including the persistent-DB-default
   pattern and the `agent_run_assignment_protocol_check` `IN (0, 1)`
   constraint) and `IF NOT EXISTS` guards so cloud adopts it cleanly. The 372
   run labeled the PR `cloud-port-tracked` under the current port-check rule
   and superseded the schema part of PDASHOSS01-372 / PR #3310.
2. NEWPIDASH-380 moves that SQL into the OSS migration set as the ported
   `runner/0044` (this is the "OSS schema lives in OSS" step: cloud keeps only
   migrations for cloud-only tables), and the cloud rebuild deletes the
   hand-port. On adopted databases the `django_migrations` row for
   `runner/0044` is already present, so the combined migrator imports it and
   applies nothing.

The 380 run's recorded assumption stands: its baseline is cut at `db/0183` /
`runner/0043` and rebased to include `runner/0044` once 372 merges. No separate
backfill issue is needed; if 372 had failed, its gap would have become a 380
blocker, but it is in Test with a review pass recorded.

## 8. Proposed amended NEWPIDASH "Schema" rule text

For the maintainer to record as the decision comment on this issue (replacing
the current "Schema" section, whose first sentence "Django's migrations stay
the single schema history" is superseded):

> **Schema.** Rust owns the schema. The native Rust migrator
> (`rust-api/crates/migration`, `pidash-api migrate`) is the only thing that
> applies schema changes in any deployment. It lives in OSS, needs no Python,
> and handles both fresh installs (baseline snapshot at the current leaf,
> stamped with the full `django_migrations` history) and upgrades (forward
> ports, one transaction per migration, fail-fast with the migration named).
> It runs once as a release/install step before any service starts; if it
> fails or does not finish, the release fails. Services never migrate, never
> wait, and issue no DDL; they run only the read-only `migrate check`, which
> exits nonzero immediately when the schema is not current.
>
> While the Django fallback is kept, every Rust schema change must keep Django
> runnable: additive changes only (new tables, columns, indexes, constraints;
> never drop, rename or retype anything the old system uses), a
> database-level default on every new NOT NULL column, and a matching
> schema-only Django model field plus migration (`apps/api/pi_dash/db/models`
> and `migrations`; no other Django code) authored in the same issue, which may
> land ahead of or with the Rust and frontend PR. Regenerate the Rust entities
> from the migrated schema as the backend README describes. This fallback
> requirement ends when the maintainer retires the Django fallback (a later
> switchover epic); until then it binds every schema change.
>
> Cloud builds the same migrator from the OSS migration set plus its own apps'
> migrations (`cloud_assistant`, `platform_federation`); cloud migrations may
> depend on OSS migrations, never the reverse. No hand-porting of OSS
> migrations into cloud. The cloud binary runs the combined set; the OSS binary
> runs OSS migrations only.

## Appendix: evidence index (research basis `ed077eaf5` / `2f24d30`)

OSS Django ownership today: 255 migration files (`db` 193, leaf `0183`;
`runner` 45, leaf `0043`; `license` 6; `prompting` 7; `assistant` 4); every
deployment path runs `python manage.py migrate` (root and community compose,
AIO `supervisor.conf`, `rust-api/bin/docker-entrypoint-migrator.sh:12`); Rust
has no migrations, only boot-time `ensure_schema` DDL
(`crates/jobs/src/queue.rs:229-319`,
`crates/api/src/runner_models/schema.rs:248-300`); entities generated via
`sea-orm-cli` from a Django-migrated database
(`crates/db/src/entity/mod.rs:4-32`); ownership statements at
`rust-api/README.md:411,516`, `.ai_design/rust_orm/design.md:65-71,158-159`,
~25 module docs, and the current NEWPIDASH "Schema" rule.

Cloud migrator today: `rust-cloud/crates/cloud-migration` (SeaORM) hand-ports
23 Django migrations (`db/0172–0183`, `runner/0034–0043`) plus cloud-only
`native_runner_0040.sql`; writes `django_migrations` rows
(`lib.rs:539-544`); advisory lock plus `pidash_cloud_schema_migrations`
history; rejects empty databases (needs Django-applied baseline `db/0170`,
`runner/0033`, `cloud_assistant/0007`, `platform_federation/0001`, contrib
apps — `lib.rs:25-40,85-94`); CI comparison
(`rust-cloud-migrations.yml`, `oracle.py compare`); 270-file fingerprints in
`verify-source`; "no Rust migrations, ever"
(`rust-cloud/README.md:6`, `cloud-db/src/lib.rs:11`); OSS port-check workflow
`.github/workflows/cloud-migration-port-check.yml` with the
`cloud-port-tracked` label.

Related work this design builds on (no duplication): NEWPIDASH-380 body (the
full one-graph model, scope, and Done when — this doc confirms it; the 380
body needs no update); NEWPIDASH-378 (interim fail-fast check and stale-leaf
test; 380 later swaps the check); NEWPIDASH-372 (PR #3459, `runner/0044`;
contract proof on the Rust binary left for its Test stage on the parity
stack); NEWPIDASH-382 (compose/`setup.sh` wiring, runs after 380).
