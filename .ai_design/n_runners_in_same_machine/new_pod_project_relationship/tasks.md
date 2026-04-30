# Pod ↔ Project ↔ Runner Refactor — Tasks

Companion to `./design.md`. Tracks the implementation needed to land the refactor end-to-end on the same PR (`feat/multi-runner-daemon`).

How to use:

- Keep status in-place with checkboxes.
- Tick off as each task lands. Commit + push after each phase.
- Phase A is a precondition for B/C/D — others can interleave once A is in main.

## Phases

- [ ] **Phase A** — Cloud schema + dispatch
- [ ] **Phase B** — Runner config + CLI registration
- [ ] **Phase C** — Cloud-side pod CRUD API + project listing
- [ ] **Phase D** — TUI + CLI multi-runner UX (selectors, status, doctor)
- [ ] **Phase E** — End-to-end smoke test + docs

---

## Phase A — Cloud schema + dispatch

### A.1 Schema migration

- [ ] New migration `runner/migrations/0007_pod_project_relationship.py`
  - [ ] Add `Pod.project = ForeignKey(db.Project, on_delete=CASCADE, related_name='pods', null=True)` (initially nullable)
  - [ ] Drop `pod_one_default_per_workspace_when_active` constraint
  - [ ] Drop `pod_unique_name_per_workspace_when_active` constraint
- [ ] Data step inside the same migration
  - [ ] **NULL out every `issues.assigned_pod_id`** before the wipe — `Issue.assigned_pod` is `on_delete=PROTECT` and will block pod deletion otherwise. The next `Issue.save()` re-resolves to the project default; the dispatch path also has the `issue.assigned_pod or Pod.default_for_project_id(...)` fallback.
  - [ ] Hard-fail with a clear message + cleanup SQL if any non-deleted Pod / Runner / AgentRun rows exist (per `decisions.md` Q2). Operator runs `DELETE FROM agent_run; DELETE FROM runner; DELETE FROM pod;` (in that order) and reruns.
- [ ] Schema migration step 2 (separate file or inside the same one, after data step)
  - [ ] `Pod.project` → NOT NULL
  - [ ] Add `pod_one_default_per_project_when_active`
  - [ ] Add `pod_unique_name_per_project_when_active`
- [ ] **Required backfill** in the same migration (RunPython, after the constraints land)
  - [ ] For every existing `Project` row, idempotently create `Pod(workspace=project.workspace, project=project, name=f"{project.identifier}_pod_1", is_default=True)`. This is mandatory: the `post_save(Project)` signal only fires on create, so without backfill, projects that pre-date this migration would have no pods and the next runner registration would 404.

### A.2 Pod model + lifecycle

- [ ] `Pod.workspace`: keep but enforce `pod.workspace_id == pod.project.workspace_id` in `Pod.clean()` / `pre_save`
- [ ] Replace `Pod.default_for_workspace()` / `Pod.default_for_workspace_id()` with `default_for_project()` / `default_for_project_id()`
- [ ] Remove the `Runner.save()` auto-pod-resolution branch
- [ ] Add naming validation: regex `^{re.escape(project.identifier)}_[A-Za-z0-9._-]{1,96}$`, 1–128 chars, suffix can't match `pod_\d+` for user-supplied names
- [ ] **Remove legacy workspace-pod lifecycle hooks** (otherwise they re-introduce or crash on the new `Pod.project` NOT NULL):
  - [ ] Delete `runner/signals.py:create_default_pod_for_new_workspace` (the `post_save(Workspace)` handler) and its receiver registration in `runner/apps.py`
  - [ ] Delete or rewrite `runner/management/commands/ensure_workspace_pods.py`. If kept, rename to `ensure_project_pods.py` and rewrite to iterate over Projects (not Workspaces) and call `Pod.default_for_project_id(...)`-style get_or_create.
- [ ] Connect new `post_save(sender=Project)` handler in `runner/signals.py` (and register in `runner/apps.py`) — auto-create one default pod per project on create. Idempotent (`get_or_create`).

### A.3 `register-under-token/` rewrite

- [ ] Body schema: add required `project` (string, identifier), optional `pod` (string, full pod name)
- [ ] Resolve project: scoped to `token.workspace`, 404 on miss
- [ ] Resolve pod: project's default pod when omitted, named pod otherwise; 404 on miss, 400 if soft-deleted
- [ ] Response carries `pod_id` alongside `runner_id`
- [ ] Update unit + contract tests

### A.4 Legacy `register/` rewrite

- [ ] Body gains required `project`, optional `pod`
- [ ] Same resolution rules as A.3
- [ ] Response unchanged externally except for added `pod_id`

### A.5 Dispatch path

- [ ] Locate every `Pod.default_for_workspace*` call and switch to the project-scoped variant
- [ ] **`Issue.save()` auto-resolution** (`apps/api/pi_dash/db/models/issue.py:199`): change the auto-fill from `Pod.default_for_workspace_id(workspace_id)` to `Pod.default_for_project_id(self.project_id)`
- [ ] **`Issue` serializer cross-check** (`apps/api/pi_dash/app/serializers/issue.py:180`): replace the `pod.workspace_id == issue.workspace_id` guard with `pod.project_id == issue.project_id`. Update the error message to "pod is in a different project". Keep the soft-delete check.
- [ ] AgentRun creation paths: `pod = issue.assigned_pod or Pod.default_for_project_id(issue.project_id)`. Issues without a project surface as a hard error (no silent workspace-default fallback).
- [ ] Update orchestration tests — at least one happy-path covering "two projects, one issue per project, two pods, two runners → each runner gets its own project's run"
- [ ] Add a regression test for the cross-project escape hatch: assigning an Issue from Project P a `Pod` belonging to Project Q (same workspace) must 400.

### A.6 Cloud-side WS Hello cross-check

- [ ] Add optional `project_slug: Option<str>` to the `Hello` body shape (Rust + Python sides)
- [ ] In `_handle_token_hello`, if `project_slug` is present, resolve and verify it matches `runner.pod.project`. Mismatch → emit `RemoveRunner` (existing path) with `reason="project_mismatch"`.
- [ ] Test for the mismatch path

### A.7 Tests

- [ ] Migration test: pre-existing pod rows abort with the documented message
- [ ] Migration test: existing Project rows get a backfilled default pod
- [ ] Migration test: existing `issues.assigned_pod_id` are NULLed before pod wipe (so PROTECT FK doesn't block)
- [ ] Pod default-per-project unique-constraint test
- [ ] Pod-naming validation test (good names / bad names / reserved suffix)
- [ ] `register-under-token/` happy path with `--project` + default pod
- [ ] `register-under-token/` with explicit `--pod` (existing user-created pod)
- [ ] `register-under-token/` rejects unknown project / cross-workspace project / soft-deleted pod
- [ ] AgentRun dispatch routes by `issue.assigned_pod or issue.project.default_pod`
- [ ] Issue serializer rejects cross-project `assigned_pod` (Project P issue + Project Q pod, same workspace) → 400
- [ ] `Issue.save()` auto-fills `assigned_pod` from project default for new issues
- [ ] Removed `post_save(Workspace)` handler: workspace creation no longer creates a workspace-level pod
- [ ] If `ensure_project_pods` is kept, idempotency test (running it twice creates no duplicates)

## Phase B — Runner config + CLI registration

### B.1 `RunnerConfig` schema (Rust)

- [ ] Add `project_slug: String` (required) to `[[runner]]` block in `runner/src/config/schema.rs`
- [ ] Add `pod_id: Option<Uuid>` (informational, written by registration response)
- [ ] `Config::validate()` rejects empty `project_slug`
- [ ] Existing `working_dir` uniqueness check stays; add explicit comment that it's the per-machine "workspace value" uniqueness

### B.2 `pidash configure`

- [ ] Add `--project <SLUG>` (required when registering)
- [ ] Add `--pod <NAME>` (optional)
- [ ] Pass them on the `register/` (legacy) call body
- [ ] Update Hello fan-out to include `project_slug`
- [ ] Persist the registration response's `pod_id` into the `[[runner]]` block

### B.3 `pidash token add-runner`

- [ ] `--project <SLUG>` required
- [ ] `--pod <NAME>` optional
- [ ] Wire to `register-under-token/` endpoint with the new body
- [ ] Persist `pod_id` to the new `[[runner]]` block

### B.4 `pidash token list-projects` (new)

- [ ] CLI verb that calls `/api/runners/projects/?workspace=<token.workspace>` (Phase C also adds the endpoint)
- [ ] Prints `identifier` + `name` + per-project pod counts so the user can pick

### B.5 Tests

- [ ] Round-trip a v2 `config.toml` with `project_slug` + `pod_id`
- [ ] `Config::validate()` rejects missing `project_slug`
- [ ] `pidash configure` with `--project` produces a config block matching the cloud's response

## Phase C — Cloud-side pod CRUD API + project listing

### C.1 Project listing endpoint

- [ ] `GET /api/runners/projects/` (session auth or token auth via `X-Token-Id` for daemon use)
- [ ] Returns `[{ id, identifier, name, default_pod_id }]` scoped to `token.workspace` (token auth) or `request.user`'s workspaces (session auth)

### C.2 Pod CRUD endpoints

- [ ] `POST /api/runners/pods/` — create non-default pod under a project. Body: `{ project_id, name, description? }`. Server validates the name regex.
- [ ] `GET /api/runners/pods/?project_id=…` — list pods for a project (existing pods.py mostly; tweak filter)
- [ ] `PATCH /api/runners/pods/<id>/` — rename. Validates new name keeps the project prefix.
- [ ] `DELETE /api/runners/pods/<id>/` — soft-delete. Refuse if pod has any non-revoked runner; refuse if pod is the project's default.

### C.3 UI surface (cloud web app)

- [ ] _Optional_ — the design doesn't strictly require shipping a UI in this PR. Stub: surface project's pods read-only on the project's settings page so the user can see them and copy pod names for CLI use. Full pod-management UI can wait for a follow-up.

### C.4 Tests

- [ ] Project-listing scoped correctly by token / session
- [ ] Pod creation validates the name prefix
- [ ] Pod creation rejects duplicate names within a project
- [ ] Pod soft-delete refuses if runners attached or if pod is default

## Phase D — TUI + CLI multi-runner UX

This is the original "F4 deferred" work, plus the per-runner project/pod surfacing.

### D.1 IPC reshape

- [ ] `StatusSnapshot` → `{ daemon: DaemonInfo, runners: Vec<RunnerStatusSnapshot> }`
- [ ] `RunnerStatusSnapshot` carries `runner_id`, `name`, `project_slug`, `pod_name`, `status`, `current_run`, `approvals_pending`, `last_heartbeat`
- [ ] Bump IPC version constant
- [ ] Add `runner: Option<String>` selector to: `RunsList`, `RunsGet`, `ApprovalsList`, `ApprovalsDecide`, `ConfigUpdate`
- [ ] `IpcServer` resolves the selector to a `RunnerInstance` (by name; default to "all" for read endpoints when omitted; require for write endpoints when N>1)

### D.2 Supervisor wiring

- [ ] Pass the full `Vec<RunnerInstance>` (or a `HashMap<Uuid, Arc<RunnerInstance>>`) into `IpcServer`
- [ ] Per-instance `state` / `approvals` / `paths` accessed via the map

### D.3 TUI

- [ ] Status tab: render N rows, one per runner
- [ ] Runs tab: top-of-tab runner picker (`<` / `>` to cycle; `1`/`2`/… for direct selection up to 9 runners)
- [ ] Approvals tab: same picker, filters approvals to selected runner
- [ ] Config tab: same picker; `display_value()` and `set_text_value()` operate on the selected runner's config slice. Daemon-level fields (cloud_url, log_level) live in a fixed pseudo-runner section that sits above the picker.

### D.4 CLI selectors

- [ ] `pidash status` lists all runners with project/pod columns; `pidash status --runner <name>` filters to one
- [ ] `pidash issue --runner <name> ...`, `pidash comment --runner <name> ...`, etc.
- [ ] `pidash doctor` walks every runner and reports per-runner; `pidash doctor --runner <name>` for a single check
- [ ] When N>1 and a verb that targets a runner is invoked without `--runner`, hard-error with a hint listing runner names

### D.5 Tests

- [ ] `StatusSnapshot` serialization roundtrip
- [ ] IPC server: read/write resolution with selector / without / ambiguous
- [ ] TUI render snapshot (or smoke) for 2-runner config
- [ ] `pidash doctor` covers two runners

## Phase E — End-to-end smoke + docs

- [ ] Update `cloud_ws_fake.rs` to support two runners under one fake token, verify the full lifecycle (Hello-Welcome × 2, Heartbeat × 2 with separate rids, Assign routed by rid)
- [ ] Add an integration test: spin up two `RunnerInstance` configs (different `project_slug`, different `working_dir`), the supervisor sends two Hellos, fake cloud sends one Assign each, both run to completion independently
- [ ] Update parent `design.md` to reference this refactor (one-line footnote pointing here)
- [ ] Update parent `tasks.md` checkboxes for the multi-runner phases that this work also closes (§4.4, §6.2, §6.4)
- [ ] Write a deployment note in `runner/README.md`: "after a fresh install, run `pidash token list-projects` to find your project identifier; then `pidash configure --project <ID>` …"
- [ ] Run the manual test checklist from the previous setup notes against the new code (token install / add-runner / per-runner deregister / token revoke / heartbeat cleanup)

## Open follow-ups (not blocking this PR)

- [ ] Routing rule for non-default pods (`decisions.md` Q7)
- [ ] Default-pod transfer flow (promote a non-default pod to default)
- [ ] Pod-level config inheritance (`decisions.md` Q9)
- [ ] Cross-workspace runners (parent design Q7)
- [ ] Per-pod / per-project runner cap (today's cap is per-machine only)
- [ ] Capability tags on pods (perf / staging / prod) once routing rules land
