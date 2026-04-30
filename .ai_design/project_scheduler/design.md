# Project Scheduler — Design

**Status:** Draft
**Date:** 2026-04-28
**Scope:** MVP. New project-level "scheduler" abstraction that periodically runs a prompt against a project's workspace via the agent. Backend (model + Beat + dispatcher + API) and a minimal CRUD web UI in `apps/web` for installing / editing / uninstalling bindings on a project.

---

## 1. Problem

We have two existing scheduling-shaped systems and they are both single-purpose:

- **`IssueAgentSchedule`** (`apps/api/pi_dash/db/models/issue_agent_schedule.py`) — per-issue tick clock that re-invokes the agent on a _single existing_ issue. Issue-consuming.
- **GitHub sync** (`apps/api/pi_dash/bgtasks/github_sync_task.py`) — hardcoded 4h Beat entry that runs `sync_all_repos` and fans out one task per `GithubRepositorySync`. Issue-producing, but only for one source.

There is no general way to say _"every 6 hours, run the agent against project X with this prompt to look for security bugs"_, and no way for a user to install / uninstall such a job on a project.

We want a **project-level scheduler layer**: reusable scheduler _definitions_ that can be installed on any project with optional per-install overrides, that periodically run a prompt against the project's workspace via the agent. The agent decides what to do with the result — typically: create new Pi Dash issues for findings.

## 2. Goals

- **Reusable definitions.** A scheduler (e.g. `security-audit`, `gdpr-compliance`) is defined once and can be installed on many projects.
- **Independent install lifecycle.** A `Scheduler` exists without any project; projects install/uninstall via a `SchedulerBinding`.
- **Per-install context.** Each install can carry extra prompt context (scope, severity threshold, project-specific notes) appended to the scheduler's base prompt at run time.
- **Per-install cadence.** Two projects can install the same scheduler at different intervals.
- **Issue-producing only.** Schedulers cause agent runs that _may_ create Pi Dash issues. The scheduler layer itself never creates issues — that's the agent's job via the same tools a user-driven run already uses.
- **Builtin-first.** MVP ships builtins in-tree (`source="builtin"`). 3rd-party / manifest-loaded schedulers come later, but the schema doesn't change when they do.

## 3. Non-Goals (MVP)

- **No 3rd-party plugin loader.** No manifest format, no `scripts/` or `assets/`, no executable code shipped from user-supplied schedulers. The `source` enum is wired so this ships later without a migration.
- **No issue-contract / dedupe at the scheduler layer.** A scheduler does not declare "I produce issues with these fingerprints," and the framework does not dedupe scheduler-produced issues. If the agent posts duplicates, the _prompt_ fixes that (e.g. "do not duplicate findings already present as open issues with `[security]` prefix").
- **No third-party / manifest-loaded definitions in the UI.** MVP UI CRUDs `source="builtin"` definitions only — manifest-loaded ones come later (§11). Builtins seeded by the migration are editable like any other definition (the prompt text is a workspace's to refine).
- **No retry policy.** A run that errors logs and waits for the next cron tick.
- **No quotas, rate limits, or backpressure** per project. Cadence is whatever the binding's cron says.
- **No event-driven schedulers** — cron only.
- **No "Run now" button** in MVP (UI work is separate). Manual triggering can be done via Django admin / shell.

## 4. Relationship to existing systems

|                     | `IssueAgentSchedule`                  | GitHub sync                             | **Scheduler (this design)**     |
| ------------------- | ------------------------------------- | --------------------------------------- | ------------------------------- |
| Bound to            | Single Issue                          | `GithubRepositorySync` (project + repo) | Project, via `SchedulerBinding` |
| Direction           | Consumes (re-runs on existing issues) | Produces (creates issue mirrors)        | Produces (agent creates issues) |
| Reusable definition | No (1:1 with issue)                   | No (hardcoded task)                     | **Yes (`Scheduler` row)**       |
| Cadence             | Per-issue interval                    | Single fixed 4h                         | Per-install cron                |
| Who creates issues  | n/a                                   | The Celery task                         | The agent itself                |

These three coexist; this design adds the third row, it does not replace either of the others.

## 5. Schema

### `Scheduler` — the definition

```python
class Scheduler(BaseModel):
    workspace = ForeignKey("db.Workspace", on_delete=CASCADE, related_name="schedulers")
    slug = CharField(max_length=64)                    # e.g. "security-audit"
    name = CharField(max_length=255)
    description = TextField(blank=True)
    prompt = TextField()                               # base prompt
    source = CharField(
        max_length=16,
        choices=[("builtin", "builtin"), ("manifest", "manifest")],
        default="builtin",
    )
    is_enabled = BooleanField(default=True)            # workspace-level kill switch

    class Meta:
        # BaseModel inherits SoftDeleteModel (deleted_at), so a plain
        # unique_together would collide with tombstones on uninstall/reinstall.
        # Match the GithubRepositorySync pattern (db/models/integration/github.py:50).
        constraints = [
            UniqueConstraint(
                fields=["workspace", "slug"],
                condition=Q(deleted_at__isnull=True),
                name="scheduler_unique_workspace_slug_when_active",
            ),
        ]
```

**Why workspace-scoped, not global:** matches existing pattern (`WorkspaceIntegration`). Workspace admins control which schedulers exist in their catalog before any project can install one.

### `SchedulerBinding` — the install

```python
class SchedulerBinding(BaseModel):
    scheduler = ForeignKey("db.Scheduler", on_delete=CASCADE, related_name="bindings")
    project   = ForeignKey("db.Project",   on_delete=CASCADE, related_name="scheduler_bindings")
    workspace = ForeignKey("db.Workspace", on_delete=CASCADE)   # denorm for filtering

    cron          = CharField(max_length=64)               # 5-field cron
    extra_context = TextField(blank=True)                  # appended to scheduler.prompt
    enabled       = BooleanField(default=True)

    next_run_at = DateTimeField(null=True, blank=True)
    # Single source of truth for "last run state": the AgentRun itself.
    # The binding does NOT carry a duplicated status enum — there are
    # 10 non-trivial states in AgentRunStatus (queued, assigned, running,
    # awaiting_approval, awaiting_reauth, paused_awaiting_input, blocked,
    # completed, failed, cancelled — runner/models.py:101) and
    # collapsing them to ok|error|running loses operator information and
    # makes "is the previous run still in flight?" ambiguous. Read the
    # status off `last_run.status` instead.
    last_run = ForeignKey(
        "db.AgentRun", on_delete=SET_NULL, null=True, blank=True,
        related_name="+",
    )
    last_error = TextField(blank=True, default="")  # short-circuit errors that never produced a run

    actor = ForeignKey("db.User", on_delete=SET_NULL, null=True)
    # Audit identity for agent actions taken on behalf of this binding —
    # mirrors GithubRepositorySync.actor for sync-authored writes.

    class Meta:
        # Conditional unique to survive uninstall/reinstall (see Scheduler).
        constraints = [
            UniqueConstraint(
                fields=["scheduler", "project"],
                condition=Q(deleted_at__isnull=True),
                name="scheduler_binding_unique_per_project_when_active",
            ),
        ]
        indexes = [Index(fields=["enabled", "next_run_at"])]
```

### `AgentRun` change — back-pointer to the binding

```python
# Added to AgentRun (apps/api/pi_dash/runner/models.py).
scheduler_binding = ForeignKey(
    "db.SchedulerBinding", on_delete=SET_NULL,
    null=True, blank=True, related_name="agent_runs",
)
```

`AgentRun` already carries `work_item` (`Issue`, nullable), `run_config` (JSON), and `required_capabilities` (JSON) but has **no generic `metadata` field**. We could piggyback on `run_config["scheduler_binding_id"]`, but a real FK is cheaper to query, lets the run-terminate hook (§6.5) do `select_related("scheduler_binding")`, and survives the eventual deletion of `run_config` ad-hoc keys. Schedulers and issue-bound runs are mutually exclusive sources for a run — exactly one of `work_item` / `scheduler_binding` should be set; this is enforced in the dispatcher, not via a DB check (matches existing patterns).

### Migration

`0131_project_scheduler_mvp.py` — creates both tables and adds `AgentRun.scheduler_binding`. No changes to `Issue` or `Project`.

## 6. Execution

### 6.1 Beat scanner

```python
# apps/api/pi_dash/celery.py
"scan-due-scheduler-bindings": {
    "task": "pi_dash.bgtasks.scheduler.scan_due_bindings",
    "schedule": crontab(minute="*"),
},
```

`scan_due_bindings` selects `SchedulerBinding` rows where:

- `enabled=True`
- `scheduler.is_enabled=True`
- `next_run_at <= now` (NULL means "first run, due immediately on next scan")

…and fans out one `fire_scheduler_binding(binding_id)` per row. Same shape as `scan_due_schedules` in `agent_schedule.py`.

### 6.2 Per-binding fire — three-phase pattern

`fire_scheduler_binding` follows the same three-phase claim/dispatch/rollback shape as `agent_schedule.fire_tick` (`apps/api/pi_dash/bgtasks/agent_schedule.py:84-204`). **Dispatch must run outside the SFU transaction** — the dispatcher registers `transaction.on_commit(drain_pod_by_id)`, and holding the binding row lock across a Celery / network dispatch extends contention and breaks the visibility assumption other tasks rely on.

**Phase 1 — Claim (inside transaction, holds SFU on the binding row):**

1. `SELECT FOR UPDATE` the binding.
2. Re-check `enabled`. If `binding.last_run` exists and `binding.last_run.status` is non-terminal (anything outside `{completed, failed, cancelled}`), skip + log and return — concurrency policy (§9).
3. Capture pre-claim values (`prev_next_run_at`, `prev_last_error`) for the rollback path.
4. Compute the next fire time from `binding.cron` via `croniter`. Write `next_run_at = <next>`, clear `last_error`. Save and commit.

**Phase 2 — Dispatch (no transaction, no row lock):**

5. Resolve the prompt: `scheduler.prompt + ("\n\n" + binding.extra_context if binding.extra_context else "")`.
6. Call `dispatch_scheduler_run(binding, prompt)` — see §6.3 — which creates a fresh `AgentRun` with `work_item=None`, `parent_run=None`, `scheduler_binding=binding`. The dispatcher's internal `transaction.on_commit(drain_pod_by_id)` now fires correctly because we are _not_ inside an outer SFU transaction.
7. On dispatch success: open a short transaction, set `binding.last_run = run`, save. Status is read off `run.status` from this point on; the binding never duplicates it.

**Phase 3 — Rollback (only if Phase 2 returns `None`):**

8. Open a transaction, `SELECT FOR UPDATE` the binding again, restore `next_run_at = prev_next_run_at`, set `last_error` to a short reason ("no default pod", "no actor", etc.). This mirrors `agent_schedule.fire_tick`'s post-dispatch rollback (`agent_schedule.py:177-204`).

### 6.3 Project-scoped dispatcher (new)

The existing `dispatch_continuation_run` (`orchestration/scheduling.py:231`) is **not reusable**: it requires an `Issue`, requires `_latest_prior_run(issue) is not None`, and resolves the pod via `_resolve_pod_for_issue(issue)`. None of those preconditions hold for a project-scoped run.

Add a sibling helper:

```python
def dispatch_scheduler_run(binding: SchedulerBinding, prompt: str) -> Optional[AgentRun]:
    """Fresh AgentRun for a project-scoped scheduler tick.
       - work_item=None, parent_run=None, scheduler_binding=binding
       - pod = Pod.default_for_workspace_id(binding.workspace_id)
       - created_by = binding.actor or workspace agent-system user
       - prompt = resolved prompt (already includes extra_context)
       Returns None when no default pod is available."""
```

This is a real new path in `orchestration/service.py`, not a config tweak; the design promotes the §8 "open question (a)" to a required deliverable.

`"scheduler"` is a **dispatch-time origin label**, not a new persisted `AgentRun` column in MVP. The helper may log it and/or thread it through internal service helpers for observability, but the durable link from a run back to its scheduler source is the new `AgentRun.scheduler_binding` FK from §5, not a `triggered_by` field.

**Repo / workspace selection is the runner's problem, not the scheduler's.** The scheduler dispatches a project-scoped run and trusts the runner to do the right thing — including projects with no `GithubRepositorySync`. Whether the runner clones the project's bound repo (if any), uses a fresh sandbox, or refuses the assignment is out of scope for this design and stays inside the existing runner protocol. The scheduler layer's contract ends at "an `AgentRun` was created and matched"; what the runner does with it is governed by the runner's own design docs (`.ai_design/implement_runner/`).

### 6.4 Cron handling

Use `croniter` to parse `binding.cron` and compute `next_run_at`. Validate at write time in the serializer; reject malformed cron with a 400.

**Dependency add:** `croniter` is not currently in the repo (verified — zero matches under `apps/api/`). Add it to `apps/api/requirements/base.txt`.

### 6.5 Run-terminate hook extension

The existing terminate hook (`runner/consumers.py:699-741`) only handles `run.work_item` (issue-schedule cap-hit pause + pod drain). When `run.scheduler_binding` is set instead, after the AgentRun has been updated to its terminal status, the hook also:

1. Reads `binding = run.scheduler_binding` via `select_related`.
2. Updates `binding.last_error`: cleared on `completed`, set to a short summary on `failed`/`cancelled`.

`binding.last_run` already points at this run (set at dispatch in §6.2 step 6), so `last_run.status` reflects the terminal state automatically — no separate status mirror to update.

### 6.6 Builtin registry and seeding

`pi_dash/scheduler/builtins/__init__.py` exposes a list of `BuiltinScheduler(slug, name, description, prompt)` records, plus a single helper:

```python
def ensure_builtin_schedulers(workspace) -> None:
    """Idempotent upsert of every BUILTINS entry for one workspace.
       Safe to call concurrently — relies on the (workspace, slug) conditional
       unique constraint to make racing inserts collapse to update_or_create."""
```

Two call sites, **both required** (a single one is insufficient — see Codex finding 4):

1. **Backfill data migration** for existing workspaces — runs `ensure_builtin_schedulers(ws)` for every `Workspace` at deploy time. Idempotent so re-runs and re-deploys are safe.
2. **`post_save` signal on `Workspace`** (`created=True`) — calls `ensure_builtin_schedulers(instance)` so every newly-created workspace gets the catalog without waiting for the next deploy.

A startup hook in `apps.ready()` was considered and rejected: with multiple processes (web + worker + beat + admin) it races on every boot, and idempotency via the conditional unique constraint converts those races into wasted writes rather than correctness bugs but still adds noise. The signal is the deterministic single-call path.

**MVP builtin** — exactly one, to validate the wiring. The prompt assumes the agent has the **Pi Dash CLI** available in its session (this is how a user-driven run authors issues today). The scheduler layer does not create issues itself; the prompt instructs the agent to use the CLI:

```
slug:  security-audit
name:  Security Audit
prompt:
  Scan this project's source code for potential security vulnerabilities
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
```

The exact CLI invocation and flag names should be filled in from the current `pi-dash` CLI surface at implementation time (out-of-tree relative to this design, so the doc points to the contract rather than pinning a specific syntax).

More builtins (GDPR, dead-code, dependency-audit, …) ship later as code, no schema work.

## 7. API

```
# Definitions — workspace-addressed; drives the workspace Schedulers tab (§8.A)
GET    /api/workspaces/<slug>/schedulers/
POST   /api/workspaces/<slug>/schedulers/
PATCH  /api/workspaces/<slug>/schedulers/<sid>/
DELETE /api/workspaces/<slug>/schedulers/<sid>/

# Bindings — project-addressed; drives the project Schedulers tab (§8.B)
GET    /api/workspaces/<slug>/projects/<id>/scheduler-bindings/
POST   /api/workspaces/<slug>/projects/<id>/scheduler-bindings/
PATCH  /api/workspaces/<slug>/projects/<id>/scheduler-bindings/<bid>/
DELETE /api/workspaces/<slug>/projects/<id>/scheduler-bindings/<bid>/
```

Permissions:

- Scheduler definition CRUD: **workspace admin only**.
- Scheduler binding CRUD: **project admin only** (matches the project's existing admin boundary; same shape `GithubRepositorySync` follows).
- `GET /schedulers/` is readable by any workspace member so the project Schedulers tab can populate its "install" picker without granting workspace-admin scope.

The two URL shapes mirror the two UI surfaces: definitions live at the workspace, bindings live at the project. The `Scheduler` serializer includes an `active_binding_count` field so the workspace Schedulers list can show "installed on N projects" without a second round-trip.

## 8. Web UI (`apps/web`)

Two new UI surfaces, one per user role:

- **§8.A Workspace → Schedulers tab** (workspace admin) — author and edit scheduler definitions.
- **§8.B Project Settings → Schedulers tab** (project admin) — install scheduler definitions onto this project, edit cron, uninstall.

The two surfaces are independent: the workspace tab never lists bindings, the project tab never lets you create or edit a definition. They communicate only through the workspace catalog endpoint (`GET /schedulers/`) that the project tab reads when populating its install picker.

---

### 8.A Workspace → Schedulers (definitions)

A **new top-level item in the workspace left navigation**, **sibling** to the existing `Prompts` entry (which lives at `apps/web/app/(all)/[workspaceSlug]/prompts/`). Not nested under `Prompts`.

```
Workspace
  ├── Prompts        (existing)
  ├── Schedulers     (new — this section)
  └── …other tabs
```

#### 8.A.1 Routes

- `apps/web/app/(all)/[workspaceSlug]/schedulers/page.tsx` — list of all `Scheduler` rows in the workspace.
- `apps/web/app/(all)/[workspaceSlug]/schedulers/[schedulerId]/page.tsx` — detail/edit view for one scheduler.
- `apps/web/app/(all)/[workspaceSlug]/schedulers/layout.tsx` — wrapper that uses the **same shell shape** as `prompts/layout.tsx` (`Outlet` inside the standard full-height workspace page container) but with a **stricter workspace-admin access gate**. This does **not** mirror `prompts/layout.tsx`'s member-readable permission model; only the layout structure is mirrored.

#### 8.A.2 List view

One row per `Scheduler` (active, `deleted_at IS NULL`):

| column       | source                                                                                                                        |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------- |
| Name         | `scheduler.name`                                                                                                              |
| Slug         | `scheduler.slug` (small, monospace)                                                                                           |
| Source       | `scheduler.source` (badge: `builtin` / `manifest`)                                                                            |
| Installed on | `scheduler.active_binding_count` (read-only number; clicking does _not_ navigate to bindings — those live on the project tab) |
| Enabled      | `scheduler.is_enabled` toggle (workspace-level kill switch)                                                                   |
| Actions      | Open detail, Delete                                                                                                           |

**"+ New scheduler"** button → modal with `slug`, `name`, `description`, `prompt` fields. `source` is forced to `builtin` in MVP.

#### 8.A.3 Detail / edit view

Single editable form for one scheduler:

- Name, description, prompt textarea (multi-line, monospace).
- Slug shown but immutable after create (part of the unique constraint).
- `is_enabled` toggle.
- "Save" (PATCH) / "Delete" (soft-delete; conditional unique allows re-create with the same slug).

This view does **not** show or manage bindings. A workspace admin who wants to know "which projects use this scheduler" sees the count on the list view; to actually change that, they go to the relevant project's Schedulers tab.

#### 8.A.4 Permissions

Workspace admin only. The layout guard rejects non-admins; modify endpoints reject at the API layer too (`POST/PATCH/DELETE /schedulers/`).

---

### 8.B Project Settings → Schedulers (bindings)

A new tab inside Project Settings, alongside the existing project-settings tabs.

#### 8.B.1 Route

- `apps/web/app/(all)/[workspaceSlug]/(settings)/settings/projects/[projectId]/schedulers/page.tsx` — list of `SchedulerBinding` rows for this project.

This follows the app's existing project-settings route convention (`/${workspaceSlug}/settings/projects/${projectId}/...`), not a separate `/projects/<id>/settings/...` tree.

#### 8.B.2 List view

| column     | source                                                               |
| ---------- | -------------------------------------------------------------------- |
| Scheduler  | `binding.scheduler.name`                                             |
| Cron       | `binding.cron`                                                       |
| Enabled    | `binding.enabled` toggle (PATCH inline)                              |
| Last run   | `binding.last_run.status` + `binding.last_run.ended_at` (or "never") |
| Last error | `binding.last_error` if non-empty, else dash                         |
| Actions    | Edit, Uninstall                                                      |

**"+ Install scheduler"** modal — picker over `GET /api/workspaces/<slug>/schedulers/` (only `is_enabled=True` schedulers shown), cron text input, extra-context textarea. Submits `POST .../scheduler-bindings/`.

**Edit binding** modal — cron, extra_context, enabled. Scheduler choice locked; uninstall + re-install to swap.

**Uninstall** — confirm → DELETE. Soft-deletes; conditional unique on `(scheduler, project)` allows re-install.

#### 8.B.3 Permissions

Project admin only — same boundary the project's other settings tabs use.

---

### 8.C Stack alignment (shared)

Standard for this repo (per `CLAUDE.md` §"Frontend composition"):

- API client in `@pi-dash/services` — new module `services/scheduler.ts` covering both definition and binding endpoints.
- State in `@pi-dash/shared-state` (MobX) — a `SchedulerDefinitionStore` keyed by workspace and a `SchedulerBindingStore` keyed by project. Two stores, not one, because the two surfaces have disjoint lifetimes and disjoint permission scopes.
- Navigation wiring in `@pi-dash/constants` — add one workspace settings entry and one project settings entry to the existing `WORKSPACE_SETTINGS` / `PROJECT_SETTINGS` maps so the sidebars and Power-K settings menus surface the new tabs through the same mechanism as the rest of settings.
- Components from `@pi-dash/ui`: table, modal, badge, toggle. Cron input is a plain text field with server-side validation in MVP (no JS cron parser pulled into the bundle); the 400 from the serializer surfaces as an inline form error.
- i18n: every new string lands in `packages/i18n/src/locales/<lang>/` for **every** locale (per `AGENTS.md` parity rule), English as placeholder where translations don't yet exist.

### 8.D Out of scope for the UI MVP

- No "Run now" button (§3).
- No run history / detail drilldown — `binding.last_run` summary only.
- No manifest-loaded scheduler import UI (§11).
- No cross-project bulk install (e.g. "install this scheduler on all projects in the workspace") — that's a workspace-admin power tool we can ship later.
- No prompt-template library or sharing across workspaces (§11).

## 9. Open questions (to resolve before merge)

1. **Concurrency.** If the previous run on a binding is non-terminal when the next cron tick fires: skip (default), queue, or kill-and-restart? Recommend **skip + log** — implemented in §6.2 step 2 by reading `binding.last_run.status`. "Non-terminal" = anything outside `{completed, failed, cancelled}`, which correctly treats `awaiting_approval`, `awaiting_reauth`, `paused_awaiting_input`, and `blocked` as still-in-flight.
2. **Run identity.** Confirm `binding.actor` is the right identity for agent-authored issues, vs. a synthetic "Scheduler" service user. Recommend `binding.actor` for consistency with the `GithubRepositorySync.actor` pattern.
3. **Cron timezone.** UTC only in MVP, matching the existing Beat schedule. Per-workspace TZ is a follow-up.
4. **Cron input UX.** §8.2 leaves open whether the UI ships a structured cron picker or a plain text field with server-side validation. Recommend plain text for MVP — keeps bundle cost low and matches how `croniter` validates server-side.

(Project-scoped dispatch was previously listed here; it has been promoted to a required deliverable — see §6.3.)

## 10. Rollout

- Settings flag `SCHEDULER_ENABLED` (default `True`); kill switch in `scan_due_bindings`, mirrors `_is_enabled` in `github_sync_task`.
- Catalog seeded with one builtin (`security-audit`) via the required backfill migration + `Workspace.post_save` signal from §6.6.
- Zero installs created automatically — every binding is user-created.
- One data backfill is required for existing workspaces: seed builtin `Scheduler` rows only. No backfill is required for `Project`, `Issue`, or historical `AgentRun` rows beyond the additive nullable `AgentRun.scheduler_binding` column.

## 11. Future work (explicitly _not_ MVP)

- **Manifest-based schedulers.** A scheduler package = a directory with `manifest.yaml` + `prompt.md` + optional `scripts/` + `assets/`, modeled on Anthropic Skills. Loaded into the `Scheduler` table with `source="manifest"`. Executable scripts run inside the runner sandbox (cloned workspace, scoped token), never in the API process — that's the trust boundary.
- **Issue-contract layer.** Optional: a scheduler can declare a fingerprint function (`(file, rule) → key`) and the framework dedupes/updates instead of creating duplicates. Ships when we observe duplicate-issue pain.
- **Run-now button & run history UI.**
- **Cross-project bulk install** from the workspace Schedulers tab (e.g. "install on all projects") — MVP keeps install one-project-at-a-time on the project tab.
- **Per-workspace cron timezone.**
- **Quotas / per-project run budgets.**
