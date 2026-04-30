# Project Scheduler — Implementation Tasks

This file turns `design.md` into a concrete MVP implementation checklist.

Related docs:

- `design.md`

## Suggested rollout

### PR 1 — Schema and model scaffolding

Goal:

- land the database changes and model wiring with no runtime behavior change yet

Scope:

- add `Scheduler` model (`apps/api/pi_dash/db/models/scheduler.py`) with conditional `UniqueConstraint(workspace, slug, deleted_at__isnull=True)`
- add `SchedulerBinding` model (sibling file or same file) with conditional `UniqueConstraint(scheduler, project, deleted_at__isnull=True)` and `Index(enabled, next_run_at)`
- add `AgentRun.scheduler_binding` FK (`SET_NULL`, nullable) in `apps/api/pi_dash/runner/models.py`
- migration `0131_project_scheduler_mvp.py` — creates both tables and the `AgentRun` column
- export `Scheduler`, `SchedulerBinding` from `pi_dash/db/models/__init__.py`
- add `croniter` to `apps/api/requirements/base.txt`

Why first:

- every later layer (Beat task, dispatcher, API, UI) touches one of these models; no behavior changes in this PR keeps it small and reviewable

### PR 2 — Project-scoped dispatcher

Goal:

- make `dispatch_scheduler_run` real before anything calls it

Scope:

- add `dispatch_scheduler_run(binding, prompt) -> Optional[AgentRun]` in `apps/api/pi_dash/orchestration/service.py` (or `scheduling.py` next to its sibling)
- helper resolves pod via `Pod.default_for_workspace_id(binding.workspace_id)`
- helper resolves `created_by` from `binding.actor` with fallback to the workspace's agent-system user (mirror `_resolve_creator_for_trigger`'s tick branch)
- creates an `AgentRun` with `work_item=None`, `parent_run=None`, `scheduler_binding=binding`, `prompt=<resolved>`, status `QUEUED`
- registers `transaction.on_commit(drain_pod_by_id)` the same way `_create_continuation_run` does
- unit tests: dispatch with no default pod → returns `None`; dispatch happy-path → returns run with correct FKs and `pod_id`; mutually-exclusive invariant (`work_item` and `scheduler_binding` cannot both be set) is enforced by the helper

Why second:

- the Beat task in PR 3 is a thin shell over this helper; isolating dispatcher tests from Beat scheduling makes both PRs easier to review

### PR 3 — Beat scanner and per-binding fire

Goal:

- periodically fan out runs for due bindings using the three-phase claim/dispatch/rollback pattern

Scope:

- add `scan_due_bindings` and `fire_scheduler_binding` in `apps/api/pi_dash/bgtasks/scheduler.py`
- wire `scan-due-scheduler-bindings` into `apps/api/pi_dash/celery.py` (`crontab(minute="*")`)
- add `SCHEDULER_ENABLED` setting (default `True`) and short-circuit `scan_due_bindings` when off, mirroring `_is_enabled` in `github_sync_task.py`
- implement Phase 1 (SFU claim + cron-advance + commit), Phase 2 (dispatch outside tx), Phase 3 (post-dispatch rollback re-acquires SFU)
- skip + log when `binding.last_run.status` is non-terminal (`{queued, assigned, running, awaiting_approval, awaiting_reauth, paused_awaiting_input, blocked}`)
- extend the run-terminate hook in `apps/api/pi_dash/runner/consumers.py` (~line 699): when `run.scheduler_binding` is set, clear/populate `binding.last_error` based on terminal status
- unit tests: skip-when-non-terminal without advancing `next_run_at`, successful claim advances `next_run_at`, three-phase rollback, terminate-hook updates

Why third:

- depends on PR 1 (schema) and PR 2 (dispatcher); Beat is a singleton concern (one beat per env) so it's worth landing in its own PR with the concurrency tests

### PR 4 — Builtin registry and seeding

Goal:

- ensure every workspace (existing and future) has the `security-audit` builtin in its catalog

Scope:

- create `apps/api/pi_dash/scheduler/builtins/__init__.py` with `BUILTINS` list and `ensure_builtin_schedulers(workspace)` helper
- ship the `security-audit` `BuiltinScheduler` record (slug, name, description, prompt — see §6.6)
- data migration `0132_seed_builtin_schedulers.py` — iterates existing `Workspace` rows and calls the helper
- `post_save` signal on `Workspace` (`created=True`) calls the helper for new workspaces
- unit tests: helper is idempotent; signal fires on creation; backfill migration is idempotent under repeat-apply

Why fourth:

- the catalog has nothing useful in it without a builtin, but you don't want to ship the builtin until the dispatch path is proven (PRs 2 + 3)

### PR 5 — REST API

Goal:

- expose scheduler-definition CRUD (workspace-addressed) and binding CRUD (project-addressed) over HTTP

Scope:

- views, serializers, URL wiring under `apps/api/pi_dash/app/views/scheduler/` (mirror existing module layout)
- definition endpoints:
  - `GET /api/workspaces/<slug>/schedulers/` readable by any workspace member
  - `POST /api/workspaces/<slug>/schedulers/` restricted to workspace admin
  - `PATCH|DELETE /api/workspaces/<slug>/schedulers/<sid>/` restricted to workspace admin
- binding endpoints (project admin):
  - `GET|POST /api/workspaces/<slug>/projects/<id>/scheduler-bindings/`
  - `PATCH|DELETE /api/workspaces/<slug>/projects/<id>/scheduler-bindings/<bid>/`
- `Scheduler` serializer includes an `active_binding_count` field so the workspace list view doesn't need a second round-trip
- `GET /schedulers/` is readable by any workspace member so the project Schedulers tab can populate its install picker
- serializer validates `cron` via `croniter` at write time → 400 on malformed
- DELETE soft-deletes; conditional unique constraints (§5) allow re-create / re-install
- unit + contract tests for each endpoint, including permission boundaries

Why fifth:

- once the backend can fire scheduled runs, exposing CRUD lets the UI actually drive it

### PR 6 — Web UI: Workspace Schedulers tab + Project Settings Schedulers tab

Goal:

- ship the two CRUD surfaces in §8.A and §8.B together (they share the services and store layers, so one PR is cheaper than two)

Scope (shared):

- add `services/scheduler.ts` in `@pi-dash/services` covering both definition and binding endpoints
- add `SchedulerDefinitionStore` (keyed by workspace) and `SchedulerBindingStore` (keyed by project) in `@pi-dash/shared-state`
- extend the existing settings/navigation constants in `@pi-dash/constants` (`WORKSPACE_SETTINGS`, `PROJECT_SETTINGS`, and associated icon/menu wiring) so both tabs show up in the normal settings sidebars and Power-K menus
- cron input is plain text + server-side validation (no JS cron parser in the bundle)
- i18n strings added to **every** locale under `packages/i18n/src/locales/` (English as placeholder where translations don't yet exist)

Scope (Workspace Schedulers — §8.A):

- new left-nav entry **sibling** to the existing `Prompts` item (not nested under it)
- new routes: `apps/web/app/(all)/[workspaceSlug]/schedulers/page.tsx`, `.../schedulers/[schedulerId]/page.tsx`, `.../schedulers/layout.tsx`
- layout uses the same page shell pattern as `prompts/layout.tsx` (`Outlet` inside the standard workspace container) but with a stricter workspace-admin access gate
- list view per §8.A.2 (rows show `active_binding_count` as a read-only number — no drill-through to bindings; that's the project tab's job)
- detail/edit view per §8.A.3 — single editable form, no binding management
- "+ New scheduler" modal: slug / name / description / prompt textarea
- delete confirm dialog
- inline toggle for `Scheduler.is_enabled`

Scope (Project Settings Schedulers — §8.B):

- new tab inside Project Settings, peer to existing project-settings tabs
- route lives under the existing project-settings tree: `apps/web/app/(all)/[workspaceSlug]/(settings)/settings/projects/[projectId]/schedulers/page.tsx`
- list view per §8.B.2
- "+ Install scheduler" modal: scheduler picker (from workspace catalog, only `is_enabled=True`), cron text input, extra-context textarea
- "Edit" modal: cron + extra_context + enabled (scheduler locked)
- "Uninstall" confirm dialog
- inline toggle for `SchedulerBinding.enabled`

UI tests:

- workspace tab: create scheduler, edit prompt, delete, kill-switch toggle
- project tab: install on project, edit cron, uninstall, picker excludes disabled schedulers

Why sixth:

- last because everything earlier needs to be merged for the UI to have something real to call

## Out of scope (do not include in any of the PRs above)

- 3rd-party / manifest-based schedulers (`source="manifest"`)
- Issue-contract / fingerprint dedupe at the framework layer
- "Run now" button
- Run history / per-run UI drilldown
- Per-workspace cron timezone
- Quotas / per-project run budgets

These are tracked under design.md §11 Future work.
