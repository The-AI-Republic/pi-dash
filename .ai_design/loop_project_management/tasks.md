# Loop (Auto Project Management) — Implementation Tasks

This file turns `design.md` into a concrete MVP implementation checklist.

Related docs:

- `design.md`
- `.ai_design/project_scheduler/design.md` (scheduling idioms reused here)
- `.ai_design/integrate_ai_agent/` (assistant runtime this builds on)

## Suggested rollout

### PR 1 — Schema and seeding (design §6)

Goal: land all database changes with no runtime behavior change.

- `pi_dash/db/models/loop.py`: `LoopJob`, `LoopTarget` (+ `SkipReason`), `LoopUserPreference` per §6.1–6.3; export from `db/models/__init__.py`
- `assistant` migration `0002_thread_kind.py`: `ThreadKind` choices + `kind` column (§6.4)
- `kind=CHAT` filter in `AssistantThreadListCreateEndpoint.get` (`assistant/views/threads.py:22`) — behaviorally inert until loop threads exist, so it belongs here
- `pi_dash/loop/builtins.py` (Django-free, §8.1) + `db` migration creating tables and upserting the builtin job with `enabled=False` (§13)
- tests: `test_models.py`, `test_thread_visibility.py` (§12)
- note: CI/local test DBs need one `--create-db` run after this lands

### PR 2 — Loop-mode runtime seam (design §7.7)

Goal: the assistant runtime understands unattended turns; nothing dispatches them yet.

- `AssistantDeps.mode` + `created_via` property (`runtime/deps.py:20`); `mode=thread.kind` in `_load_context` (`assistant/tasks.py:70`)
- `LOOP_INSTRUCTIONS` block in `dynamic_instructions` (`runtime/instructions.py:52`), formatted with `LOOP_MAX_WRITES`
- `created_via=ctx.deps.created_via` in `tools/issues.py:182` (and the update path)
- kind-aware history cap in `runtime/history.py:27` + `ASSISTANT_LOOP_HISTORY_MAX_TURNS` setting
- resolve open question §14.3 (orchestration side-effect of completed-group transitions under loop mode)
- tests: `test_runtime_seam.py` (§12)

### PR 3 — Eligibility, scanner, fire, dispatch (design §7.1–7.6, §7.8)

Goal: due targets dispatch real assistant turns end-to-end.

- `pi_dash/loop/eligibility.py`: `llm_available_q()` (ee-overridable), `eligible_due_targets(now)`, `check(target)` with deterministic reason precedence (§7.8)
- `pi_dash/loop/dispatch.py`: `dispatch_loop_turn` + `_ensure_thread` rotation (§7.5)
- `pi_dash/bgtasks/loop.py`: `scan_due_targets` (`_reconcile_targets` throttled, `_fan_out_due`, `_advance_ineligible_due`) and `fire_loop_target` (§7.2–7.4); `_stagger` helper (§7.3)
- `scan-due-loop-targets` Beat entry (`pi_dash/celery.py`, next to line 118)
- settings block (§11): `LOOP_ENABLED`, `LOOP_STAGGER_WINDOW_MINUTES`, `LOOP_MAX_DISPATCH_PER_TICK`, `LOOP_RECONCILE_EVERY_MINUTES`, `LOOP_ROTATION_HEADROOM`, `LOOP_MAX_WRITES`
- tests: `test_eligibility.py`, `test_scanner.py`, `test_fire_dispatch.py` (§12)

### PR 4 — `get_pull_request_status` tool (design §8.2)

Goal: the builtin job can actually establish merge state.

- `pi_dash/assistant/tools/github.py` per the §8.2 contract (parse → creds via `_scoping.member_projects` → httpx → never-raise mapping → per-run budget); register in `tools/__init__.py:11`
- `LOOP_PR_LOOKUPS_PER_RUN` setting
- tests: `test_github_tool.py` (§12, mocked httpx)

### PR 5 — User settings API + web UI (design §9.1, §10.A)

Goal: users can see and toggle "Auto Project Management".

- `pi_dash/loop/views.py` + `pi_dash/loop/urls.py`; include in `pi_dash/urls.py` (`path("api/", include("pi_dash.loop.urls"))`)
- GET/PATCH contracts per §9.1 — five-key whitelist, `interval_label` derived server-side, `enabled`-only PATCH
- `packages/types/src/auto-pm.ts`, `packages/services/src/auto-pm/auto-pm.service.ts`, page component `auto-project-management.tsx`, tab registration in `packages/constants/src/settings/profile.ts` + sidebar icon, route case in `[profileTabId]/page.tsx` (§10.A table)
- i18n keys `auto_pm.*` in every locale
- tests: `test_user_api.py` (§12)

### PR 6 — Instance admin API + admin UI (design §9.2, §10.B)

Goal: the operator can manage jobs and observe targets.

- `pi_dash/loop/admin_views.py` + `admin_urls.py`; `path("loop/", include(...))` in `pi_dash/license/urls.py`; `InstanceAdminPermission` throughout
- job serializer + RRULE validation incl. hourly floor (§6.1, §9.2); targets listing with filters
- `apps/admin` `loop/` pages + sidebar entry + `InstanceLoopService` (§10.B)
- tests: `test_admin_api.py` (§12)
- launch act: flip the seeded job to enabled on the dogfooding instance, watch the targets table per §13, then production

## Out of scope (tracked in design §15)

- visible loop transcripts, digests, per-workspace preferences, event-driven jobs, user-authored jobs, additional builtins, PR-state materialization, rdates/exdates
