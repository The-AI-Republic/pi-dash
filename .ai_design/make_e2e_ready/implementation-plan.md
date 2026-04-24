# Pi Dash E2E Readiness — Implementation Plan

Purpose:

- make the delegated local-agent workflow work end to end with the smallest runtime surface
- keep the runner lightweight (fire-and-forget) and let the agent drive all Pi Dash state changes itself
- define the minimum shippable path before broader rollout

Scope:

- Pi Dash CLI (`pidash`) as the agent's only channel back to Pi Dash
- runner injects the CLI + scoped credentials into the local Codex session
- prompt system advertises the CLI and drops the fenced-block completion contract
- minimal runner lifecycle reporting + server-side watchdog for crashed Codex processes
- operator readiness check and one documented smoke test

Non-goals:

- redesign the runner protocol beyond the lifecycle simplification below
- parse or validate a `pi-dash-done` fenced block in either runner or cloud
- build a cloud-side completion service that applies issue state transitions
- broaden delegation rules beyond the current MVP trigger
- expose per-run or per-issue scoped credentials (MVP uses the workspace-scoped `api_token`)

Related docs:

- `.ai_design/prompt_system/prompt-system-design.md`
- `.ai_design/prompt_system/workflow-handbook.md`
- `.ai_design/implement_runner/runner-design.md`
- `.ai_design/implement_runner/implementation-tasks.md`

## Problem statement

The branch already has most of the primitives:

- issue-state transitions can create `AgentRun` rows
- the runner can receive assignments and launch `codex app-server`
- the Pi Dash `/api/v1/workspaces/<slug>/...` REST API already exposes work items, comments, states, links, activities, etc.
- PR #10 plumbed `workspace_slug` into the runner config and made the runner mint an `api_token` at enrollment time, scoped to `/api/v1/`

What is missing is the connective tissue between those primitives. The design has also shifted: the runner is no longer the arbiter of "done." The agent is. The runner's job shrinks to launching Codex, handing it a working CLI, and reporting when the subprocess exits. All issue writes — state transitions, comments, the workpad — go through `pidash` and hit the existing REST API.

Six concrete gaps block end-to-end delivery:

1. The `pidash` CRUD subcommands do not exist.
   - `runner/src/cli/mod.rs` only defines daemon lifecycle subcommands (`configure`, `start`, `doctor`, etc.).
   - Warnings in `runner/src/cli/configure.rs:64-70` and `runner/src/tui/onboarding.rs:240-244` reference future `pidash issue` subcommands, but none are implemented.

2. The local Codex session cannot reach Pi Dash.
   - `runner/src/codex/app_server.rs:20-24` launches Codex with no environment, no token, and no binary injection.
   - The `api_token` lives in `Credentials` (`runner/src/config/schema.rs:96-106`) but is never passed to any process.

3. The prompt system still promises a runtime surface Codex does not have.
   - Prompt templates instruct the agent to maintain a workpad comment and emit a `pi-dash-done` fenced block.
   - The new runtime surface is `pidash …`; prompts must be updated accordingly and the fenced-block contract must be removed.

4. Runner completion reporting is over-specified.
   - `runner/src/cloud/protocol.rs:75-79` defines `RunCompleted { run_id, done_payload, ended_at }`.
   - `runner/src/codex/bridge.rs:286-310` synthesizes a placeholder `done_payload` from `turn/completed.params.done` when nothing meaningful is available.
   - Under fire-and-forget the runner cannot and should not claim a structured outcome. A minimal `RunTerminated { exit_code, ran_for_s }` event is enough.

5. There is no stall watchdog for orphaned runs.
   - `apps/api/pi_dash/runner/tasks.py:93-110` already reaps offline runners via a 90-second heartbeat grace.
   - Nothing reaps an `AgentRun` stuck in `RUNNING` when Codex crashes or the host is hard-killed. Under fire-and-forget this failure mode is more likely.

6. `pi-dash-runner doctor` is not aligned with the new shape.
   - `runner/src/cli/doctor.rs:157-175` runs `codex account status` and bails with no fallback.
   - It does not verify that `pidash` itself can authenticate against the cloud — which is the only thing that actually matters for the agent to do work.
   - No operator guide documents a reproducible smoke test.

This plan closes those six gaps with the smallest coherent set of changes.

## Success criteria

The system is e2e-ready when all of the following are true:

- A delegated issue entering the trigger state creates an `AgentRun`, selects a runner, and starts a local Codex turn.
- The local Codex session has `pidash` on PATH and valid credentials on environment.
- The agent can, via `pidash …` alone, fetch the issue, list comments, post or update comments, list available states, and move the issue's state.
- The runner reports process termination back to the cloud via a minimal `RunTerminated` event.
- A Celery watchdog transitions `AgentRun` rows stuck in `RUNNING` beyond a configured window to `FAILED` with `reason=stalled`.
- Authoritative issue state lives on the `Issue` row (moved by the agent); `AgentRun.status` mirrors subprocess lifecycle, not workflow outcome.
- `pi-dash-runner doctor` verifies that both `codex` and `pidash` are installed, and that `pidash` can authenticate against the configured cloud.
- A documented smoke test covers register → doctor → delegate → observe the agent moving the issue in the UI.
- Automated tests cover CLI ↔ API contract, injection wiring, and stall-watchdog behavior.

## Committed decisions

### 1. The agent owns all Pi Dash writes

Decision:

- All state transitions, comments, and workpad updates are performed by the agent via `pidash`. Nothing else is allowed to mutate Pi Dash on the agent's behalf during a run.

Why:

- The existing `/api/v1/` REST surface already supports everything the agent needs.
- It removes the previous "cloud parses a fenced payload and applies it" coupling that was never implemented.
- It matches a prior-art pattern where the agent uses a tracker-native tool (analogous to a `linear_graphql` tool) and treats the tracker as the source of truth.

Implication:

- The `pi-dash-done` fenced-block contract is removed from the prompt. The cloud-side `orchestration/done_signal.py` parser becomes dead code for this workflow.
- The `Issue` row (and its state, comments, links) is the authoritative record of what the agent did. `AgentRun` is subprocess bookkeeping.

### 2. Runner is fire-and-forget

Decision:

- The runner launches Codex, forwards approval events as today, and on subprocess exit sends a single `RunTerminated { run_id, exit_code, ended_at }` event to the cloud.
- The runner no longer parses the final assistant message body, does not extract fenced blocks, and does not synthesize a structured completion payload.

Why:

- There is no longer a canonical completion payload to capture.
- Keeps the runner small and debuggable.
- Lets Codex exit semantics (clean vs. non-zero vs. signal) be the only signal the runner has to forward.

Implication:

- `protocol::ClientMsg::RunCompleted { done_payload }` is replaced by `RunTerminated { exit_code, ended_at }`. Elapsed time is computed server-side from `AgentRun.started_at` (already set in `on_run_started`), so the event does not carry a duration field.
- The cloud consumer maps `exit_code == 0 → AgentRun.status = COMPLETED`, non-zero or signal → `FAILED` with the exit info preserved in `AgentRun.error`.
- The existing `AgentRun.done_payload` column is left in place for backward compatibility but is no longer written by this workflow. New code paths must not read it either.
- During the migration window, the cloud consumer accepts **both** `RunCompleted` and `RunTerminated` message types: `RunCompleted` is treated as `exit_code=0` (for back-compat with any in-flight older runners). New runners emit `RunTerminated` only. The dual-accept window is removed in a follow-up cleanup once all deployed runners are on the new protocol version.

### 3. `pidash` CLI is the only agent-to-cloud surface

Decision:

- The agent talks to Pi Dash exclusively through `pidash` subcommands. No raw `curl`, no bespoke shell tooling, no direct token exposure in prompts.

Why:

- Narrow, auditable surface. Easy to add rate limits, logging, or confirmation prompts later.
- Lets the prompt advertise a stable command vocabulary instead of REST shapes.
- The runner's `api_token` stays inside the CLI process; the agent never reads it.

Implication:

- The runner injects `PIDASH_API_URL`, `PIDASH_WORKSPACE_SLUG`, and `PIDASH_TOKEN` into the Codex child environment and prepends the `pidash` binary's directory to `PATH`.
- The CLI reads those three env vars and never requires flags for them.

### 4. CLI output is JSON by default

Decision:

- Every `pidash` subcommand prints a single JSON document to stdout on success. Errors print a JSON object with an `error` field to stderr and exit non-zero.

Why:

- Agents reason over JSON more robustly than tabular output.
- A uniform output shape is easier to teach in the prompt.
- Context cost is modest — the normal issue payload is well under 2 KB.

Implication:

- No `--pretty` or human-table default in the MVP. A future flag can be added without breaking the agent contract.

### 5. Issue is the source of truth; `AgentRun.status` mirrors subprocess lifecycle

Decision:

- `AgentRun.status` reflects whether the Codex process ran to clean exit, not whether the issue was resolved.
- The issue's own state (moved by the agent via `pidash issue patch`) is what the board, reporting, and humans see as the workflow outcome.

Why:

- Aligns with fire-and-forget: the runner does not know the workflow outcome, only the exit status.
- Avoids the cloud needing to decide whether "subprocess exited cleanly but agent did nothing" means `COMPLETED` or `FAILED`.
- Matches Symphony's reconciliation model: the tracker is the source of truth; the worker row is bookkeeping.

Implication:

- Blocked and noop semantics live on the issue itself (state + comment), not on `AgentRun`.
- The Django signal that already fires on `Issue.state` change can be used later to decorate runs if we want a tighter mirror, but it is not required for MVP.

### 6. Blocked = state move + comment, performed by the agent

Decision:

- When the agent cannot progress, it moves the issue to a "Blocked" state (if one exists in the project) and posts a comment explaining why.
- If the project has no Blocked state, the agent falls back to posting a clearly-tagged blocker comment and leaves the issue in its current state.

Why:

- Makes blocked issues visible on the board without requiring a new Pi Dash domain concept.
- Keeps the information in the tracker where humans already look.

Implication:

- The prompt must instruct the agent: "To block, move state to Blocked and post a comment. If no Blocked state exists, post a comment prefixed `Blocked:` and stop."
- No default "Blocked" state is seeded automatically in MVP. A future improvement can add onboarding seeding.

### 7. Readiness check validates the whole chain

Decision:

- `pidash doctor` verifies, in order:
  1. `codex` binary is on PATH and executes (`codex --version`).
  2. `pidash` binary self-check (`pidash --version`).
  3. `pidash workspace me` succeeds against the stored credentials. This single call validates token presence, token validity, cloud reachability, and the bot user's active membership in one shot.

Why:

- E2E readiness means the agent's chain works, not just that Codex has auth.
- The current probe (`codex account status`) does not tell us whether the agent will be able to do any work.
- A separate cloud-heartbeat probe would be redundant: if `workspace me` succeeds, cloud reachability is already proven.

Implication:

- `doctor` no longer requires a version-specific Codex auth subcommand; it only checks that the Codex binary exists and that `pidash`'s own cloud credentials work end-to-end against the REST API.

## Runtime surface specification

### `pidash` subcommand inventory (MVP)

All subcommands accept no auth flags; they read `PIDASH_API_URL`, `PIDASH_WORKSPACE_SLUG`, `PIDASH_TOKEN` from the environment. All subcommands emit a single JSON document to stdout on success.

| Command                                                   | Backing endpoint                                | Purpose                                                                                                              |
| --------------------------------------------------------- | ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | --------------------------- | ------------------------------------------------------- |
| `pidash issue get <IDENT>`                                | `GET /work-items/<PROJ>-<num>/` (by-identifier) | Fetch full issue payload (title, description, state, comments, links)                                                |
| `pidash issue patch <IDENT> --state <STATE_NAME_OR_UUID>` | `PATCH /work-items/<uuid>/ {state: <uuid>}`     | Move issue state. Accepts either a state UUID or a state name; names are resolved locally by calling `states` first. |
| `pidash issue patch <IDENT> [--title                      | --description                                   | --priority ...]`                                                                                                     | `PATCH /work-items/<uuid>/` | Update other issue fields when the workflow requires it |
| `pidash comment list <IDENT>`                             | `GET /work-items/<uuid>/comments/`              | Return the comment list for an issue                                                                                 |
| `pidash comment add <IDENT> --body <MD>`                  | `POST /work-items/<uuid>/comments/`             | Post a new comment                                                                                                   |
| `pidash comment update <COMMENT_ID> --body <MD>`          | `PATCH /work-items/<uuid>/comments/<cid>/`      | Edit a comment the agent owns (used for the workpad)                                                                 |
| `pidash state list`                                       | `GET /projects/<pid>/states/`                   | List states available in the current project with `name`, `description`, `group`                                     |
| `pidash workspace me`                                     | `GET /workspaces/<slug>/members/me/`            | Used by `doctor` to verify auth; returns the authenticated user                                                      |

Notes:

- `pidash issue patch --state <NAME>` does the name-to-UUID lookup client-side on the CLI. This is a UX affordance for the agent; the agent is not expected to track UUIDs across turns.
- All CLI errors exit non-zero with JSON on stderr: `{"error": "<short>", "detail": "<optional>"}`.

### URL resolution sequence

The REST API exposes the issue-by-identifier route as GET-only (`/workspaces/<slug>/work-items/<PROJ>-<num>/`). Mutating routes (PATCH, comments, relations, attachments) all require `project_id` in the URL: `/workspaces/<slug>/projects/<project_id>/work-items/<uuid>/...`. The CLI therefore performs a three-step resolution whenever it is given an issue identifier for a mutating operation:

1. `GET /workspaces/<slug>/work-items/<PROJ>-<num>/` — returns the full issue payload including the `project` UUID and the issue `id` UUID.
2. For `--state <NAME>` only: `GET /workspaces/<slug>/projects/<project_id>/states/` — returns the project's state list; the CLI matches `<NAME>` case-insensitively against `state.name` and errors if there are zero or multiple matches.
3. `PATCH /workspaces/<slug>/projects/<project_id>/work-items/<issue_uuid>/` (or `POST /comments/`, etc.) — the actual mutation.

Results from steps 1 and 2 are cached in-memory for the lifetime of a single CLI invocation. The CLI does not persist any cache across invocations; agents re-resolve on every command, which is fine because the cost is a few extra GETs per mutation.

The GET-only `pidash issue get <IDENT>` and `pidash state list` subcommands use only step 1 (and, for `state list`, a follow-up step 2).

### Prompt-time state guidance

The prompt composer must include, at minimum:

- the issue's current state (`name`, `group`)
- the full state list for the issue's project, each with `name`, `description`, `group`

The existing `StateLiteSerializer` (`apps/api/pi_dash/api/serializers/state.py:44-55`) omits `description`. Either extend it with `description` or switch the prompt-side query to the full `StateSerializer`. The former is lighter-weight.

### Runner → Cloud protocol change

Replace:

```rust
ClientMsg::RunCompleted { run_id, done_payload: Value, ended_at }
```

With:

```rust
ClientMsg::RunTerminated { run_id, exit_code: i32, ended_at }
```

Cloud handling:

- `exit_code == 0` → `AgentRun.status = COMPLETED`
- any non-zero or signal exit → `AgentRun.status = FAILED`, populate `AgentRun.error` with `"exit_code=<n>"` (signal info when available)
- `ended_at` is set unconditionally from the event timestamp

`AgentRun.done_payload` column is retained (DB compatibility) but left null for new runs.

**Migration:** The cloud consumer accepts both `RunCompleted` and `RunTerminated` for one release. `RunCompleted` from an older runner is treated as `exit_code=0` (the old semantics); any `done_payload` field on such a message is discarded. New runners emit only `RunTerminated`. The dual-accept code path is removed in a follow-up cleanup once telemetry confirms no live runner still emits `RunCompleted`.

### Stall watchdog

New Celery task in `apps/api/pi_dash/runner/tasks.py`, scheduled every 60s. The watchdog keys off **runner-level heartbeat**, not `AgentRun.updated_at`, because under fire-and-forget a long solo turn may not tick the run row at all:

```
runner.reap_orphaned_runs:
  for each Runner where status == OFFLINE:
    for each AgentRun owned by that runner where status in {RUNNING, ASSIGNED, AWAITING_APPROVAL}:
      set status = FAILED, error = "runner offline", ended_at = now
```

Rationale:

- The existing `mark_offline_runners` task (`tasks.py:93-110`) already flips `Runner.status` to OFFLINE after 90s of missed heartbeats.
- If the runner is OFFLINE, any run it owns is by definition orphaned — Codex might still be running locally, but the cloud has lost visibility, and the issue either has been moved by the agent (outcome observable on the `Issue` row) or is stuck.
- Keying off `Runner.status` reuses an already-battle-tested signal and avoids adding a per-run liveness protocol.

Optional grace for slow operators: add a setting `RUNNER_ORPHAN_GRACE_SECONDS` (default 0) that delays the reap for N seconds after the runner goes OFFLINE, in case a runner is bouncing and will reconnect quickly.

Watchdog task body mirrors the pattern in `expire_stale_approvals` (`tasks.py:35-90`) for select_for_update + atomic update semantics.

### Doctor reshape

`runner/src/cli/doctor.rs` becomes a short sequence of three checks, each printing a line and returning non-zero on failure:

1. `which codex` succeeds and `codex --version` runs.
2. `pidash --version` runs (self-check).
3. `pidash workspace me` succeeds against the stored credentials.

Check 3 subsumes what a separate heartbeat probe would validate: it proves token validity, cloud reachability, HTTPS/TLS correctness, and the bot user's workspace membership in one round-trip.

Each check outputs a single JSON line so the TUI / ops can consume the output verbatim.

## Proposed rollout

### PR 1 — `pidash` CRUD subcommands

Scope:

- **Django prerequisite:** mark the runner-minted `api_token` as a service token so CLI traffic routes through the 300/min throttle instead of the default 60/min. Change `APIToken.objects.create(...)` at `apps/api/pi_dash/runner/views/register.py:106-111` to include `is_service=True`. This keys into `ServiceTokenRateThrottle` (`apps/api/pi_dash/api/rate_limit.py:53-55`).
- add Rust subcommand modules: `issue get`, `issue patch`, `comment list|add|update`, `state list`, `workspace me`
- shared HTTP client reading `PIDASH_API_URL`, `PIDASH_WORKSPACE_SLUG`, `PIDASH_TOKEN`
- implement the three-step URL resolution described in "URL resolution sequence" with per-invocation in-memory cache
- JSON output (success → stdout, errors → stderr + non-zero exit)

Files likely touched:

- `apps/api/pi_dash/runner/views/register.py:106-111` — set `is_service=True` on minted api_token
- `apps/api/pi_dash/tests/contract/runner/test_registration.py` — assert the minted token is a service token
- `runner/src/cli/mod.rs` — new subcommand enum variants
- `runner/src/cli/issue.rs`, `comment.rs`, `state.rs`, `workspace.rs` — new
- `runner/src/api_client.rs` — new thin HTTP wrapper (do not reuse `cloud/` module which is WS-specific)
- `runner/Cargo.toml` — add `reqwest` if not already present
- `runner/tests/pidash_cli_contract.rs` — new

Acceptance:

- against a local `docker compose -f docker-compose-local.yml up` stack plus a seeded workspace, each subcommand round-trips correctly
- contract test covers success paths, 401/403 mapping, and 404 mapping
- rate-limit headers from the service-token throttle are observable in responses (300/min)
- no new env vars required beyond the three documented above

### PR 2 — Runner injection of `pidash` into Codex session

Scope:

- at run-assignment time, compute `PATH` additions and the three `PIDASH_*` env vars from the runner's persisted `Credentials` and `workspace_slug`
- pass them into `AppServer::spawn`
- remove any remaining fenced-block/done_payload inspection in the bridge (leave transport-level `turn/completed` handling for liveness only)

Files likely touched:

- `runner/src/codex/app_server.rs:20-24` — add `.env(...)` calls
- `runner/src/daemon/supervisor.rs` — compute injected env from `Credentials` and pass through
- `runner/src/codex/bridge.rs:286-310` — stop synthesizing `done_payload`

Acceptance:

- integration test (fake `codex app-server`) verifies the three env vars and PATH prefix are passed to the child
- running a real Codex turn locally, `pidash workspace me` from inside the Codex shell returns the bot user

### PR 3 — Prompt update + protocol simplification

Scope:

- prompt template: `apps/api/pi_dash/prompting/templates/default.j2` teaches the agent about `pidash` (already landed on this branch — verify against plan during review).
- prompt context builder: populate `issue.project_states` so the template can render the "Available states" block. In `apps/api/pi_dash/prompting/context.py::build_context`, add to the returned `issue` dict:
  ```python
  "project_states": [
      {"name": s.name, "description": s.description or "", "group": s.group}
      for s in State.objects.filter(project=project).order_by("sequence")
  ],
  ```
  Prefer inline dict construction over the full `StateSerializer` — smaller payload, and `StateLiteSerializer` would need a `description` field to be equivalent.
- protocol: replace `RunCompleted { done_payload }` with `RunTerminated { exit_code }` on both runner and cloud sides. Add transitional dual-accept in the cloud consumer per "Runner → Cloud protocol change" above.
- `runner/consumers.py::on_run_completed` renamed/adjusted to handle `RunTerminated` and set `AgentRun.status` from exit code. Keep a small compatibility handler for `RunCompleted` that delegates to the new logic with `exit_code=0`.
- superseded-docs banner: append a "Superseded by `.ai_design/make_e2e_ready/implementation-plan.md` — fenced-block contract removed" note to the top of `.ai_design/prompt_system/prompt-system-design.md` and `.ai_design/prompt_system/workflow-handbook.md`.

Files likely touched:

- `apps/api/pi_dash/prompting/context.py` — populate `project_states`
- `apps/api/pi_dash/prompting/templates/default.j2` — already edited on this branch; review for coherence
- `runner/src/cloud/protocol.rs:75-79` — replace message type
- `apps/api/pi_dash/runner/consumers.py:221-441` — handle both `RunTerminated` (new) and `RunCompleted` (compat) during migration
- `apps/api/pi_dash/orchestration/done_signal.py` — add deprecation note; do not delete in this PR to keep the diff small
- `.ai_design/prompt_system/prompt-system-design.md`, `.ai_design/prompt_system/workflow-handbook.md` — superseded banners

Acceptance:

- rendered prompt for a sample issue includes the CLI capability block and the project's state list with descriptions
- snapshot test on `build_context` output confirms `project_states` is populated with `name`, `description`, `group`
- contract test: runner sends `RunTerminated{exit_code=0}` → `AgentRun.status == COMPLETED`
- contract test: runner sends `RunTerminated{exit_code=137}` → `AgentRun.status == FAILED`, `error` includes the exit code
- contract test: cloud receives a legacy `RunCompleted{done_payload: {...}}` and treats it as `exit_code=0` during the migration window

### PR 4 — Watchdog, doctor, operator docs

Scope:

- Celery task `runner.reap_orphaned_runs` keyed off `Runner.status == OFFLINE`, scheduled in beat every 60s.
- reshape `doctor` to run the three checks (codex, pidash, workspace me).
- operator guide at `runner/README.md` or `.ai_design/implement_runner/operator-guide.md` documenting register → doctor → delegate → observe happy-path smoke test. Call out explicitly: the user who mints the runner registration code must have issue-edit permission in every project that will delegate to this runner.
- add a `make smoke` or equivalent script if useful.

Files likely touched:

- `apps/api/pi_dash/runner/tasks.py` — new `reap_orphaned_runs` task
- `apps/api/pi_dash/celery.py` — beat schedule entry
- `apps/api/pi_dash/settings/*.py` — `RUNNER_ORPHAN_GRACE_SECONDS` with default 0
- `runner/src/cli/doctor.rs` — rewrite to three-check form
- `runner/README.md` or the operator guide doc
- `apps/api/pi_dash/tests/unit/runner/test_tasks.py` — reaper test

Acceptance:

- unit test: a runner flipped to OFFLINE causes its owned `RUNNING` runs to be marked `FAILED` with `runner offline` reason
- unit test: a runner that is ONLINE does not have its `RUNNING` runs reaped, regardless of how long they've been running
- `pidash doctor` on a freshly registered runner reports all three checks green
- the smoke test recipe runs top-to-bottom on a local docker stack without edits

## Open design questions

These can be resolved inside the listed PRs but should be called out during implementation review:

1. Error mapping from REST to CLI exit codes.
   - Proposal: 400 → 2 (invalid), 401/403 → 3 (auth), 404 → 4 (not found), 5xx → 5 (server). Documented in CLI `--help`.

2. Whether `pidash issue patch --state <NAME>` is case-sensitive.
   - Proposal: case-insensitive match; error if ambiguous.

3. How much of the current `orchestration/done_signal.py` to delete vs. keep.
   - Proposal: leave intact in PR 3 (zero risk); delete in a later cleanup PR once nothing imports it.

4. Whether to seed a default "Blocked" state per project during workspace setup.
   - Proposal: out of scope for this plan; track as a follow-up. MVP relies on the fallback (blocker-tagged comment).

5. Stall grace default.
   - Proposal: 15 minutes. Long enough for a slow turn, short enough that a crashed Codex does not hold an issue hostage. Revisit after observing real runs.

## Risks

- Rate limiting is addressed in PR 1 by marking the runner-minted `api_token` as a service token (300/min). A misbehaving agent in a tight loop could still exhaust 300/min — watch for this during the first real runs and tighten if needed.
- The `api_token` minted at registration is workspace-broad. A compromised runner host can mutate any issue in the workspace. Known trade-off for MVP; scoped tokens are a future enhancement, not a blocker.
- The runner's effective permissions are inherited from the user who minted the registration code (`reg.created_by` at `runner/views/register.py:86-108`). If that user is not a project member with issue-edit permission, all PATCH and comment calls will 403 at runtime. **Operator guidance (PR 4):** registration codes should be minted by a workspace admin, or by a user who is a full member of every project that will delegate to this runner. A future improvement can introduce an explicit workspace-level bot user for runner auth.
- Dropping the `pi-dash-done` fenced contract means we lose structured blocker metadata. If that turns out to be load-bearing for downstream tooling, restore it as a first-class field on the Comment model instead of as a fenced block.
- Without a tight `Issue.state` → `AgentRun.status` mirror, the UI can briefly show a run in `RUNNING` after the issue is already `Done`. Acceptable for MVP; tighten later via a Django signal.
- Protocol migration: during the dual-accept window a new-runner vs old-cloud deployment could race. Sequence the rollout as cloud-first (accepts both) → runner-second (emits `RunTerminated`). If a runner starts emitting `RunTerminated` before the cloud understands it, the cloud will log an unknown-message warning and the run will be reaped by the orphan watchdog — not silently lost, but delayed.

## Recommended implementation order

1. PR 1 — ship the CLI. Nothing else can be tested without it.
2. PR 2 — wire it into Codex. Unlocks end-to-end manual runs.
3. PR 3 — prompt + protocol. Makes the workflow correct and clean.
4. PR 4 — watchdog + doctor + docs. Hardens operations.

This order keeps every PR independently useful: PR 1 gives operators a CLI to poke the API with; PR 2 lets Codex use it; PR 3 makes the agent actually good at it; PR 4 makes the system safe to run unattended.

## Exit checklist

- [ ] `pidash` has `issue get/patch`, `comment list/add/update`, `state list`, `workspace me` subcommands, JSON output, env-based auth
- [ ] Runner injects `PIDASH_API_URL`, `PIDASH_WORKSPACE_SLUG`, `PIDASH_TOKEN`, and PATH entry into the Codex child process
- [ ] Prompt advertises the CLI and no longer references the `pi-dash-done` fenced contract
- [ ] State list (with descriptions) is included in the rendered prompt
- [ ] Runner sends `RunTerminated { exit_code, ran_for_s }` and the cloud maps it to `AgentRun.status`
- [ ] Celery task reaps `RUNNING` runs older than the configured grace
- [ ] `pidash doctor` validates codex, pidash, workspace auth, and heartbeat
- [ ] Operator smoke-test recipe runs on a fresh local docker stack without edits
- [ ] `orchestration/done_signal.py` is either removed or documented as dead for this workflow
