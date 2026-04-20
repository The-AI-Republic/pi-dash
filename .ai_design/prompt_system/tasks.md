# Pi Dash Prompt System — Implementation Tasks

This file turns the prompt-system design into a concrete MVP implementation checklist.

Related docs:
- `prompt-system-design.md`
- `workflow-handbook.md`

## Suggested rollout

### PR 1 — Models and scaffolding

Goal:
- land the schema and module skeleton without changing runtime behavior yet

Scope:
- add `PromptTemplate` model + migration
- add `AgentRun.parent_run`
- add `AgentRun.done_payload`
- align `AgentRun.status` enum
- add `Project.repo_url` and `Project.base_branch`
- create `apps/api/pi_dash/prompting/` module skeleton
- create `apps/api/pi_dash/orchestration/service.py` skeleton
- register `PromptTemplate` in admin

Why first:
- all later work depends on the schema and module boundaries existing

### PR 2 — Prompt rendering and preview

Goal:
- make prompt generation real and testable in isolation

Scope:
- implement `context.py`
- implement `renderer.py`
- implement `composer.build_first_turn(...)`
- add `templates/default.j2`
- implement default template seed logic
- implement preview endpoint
- add unit tests for:
  - context generation
  - template lookup
  - render success/failure
  - preview rendering

Why second:
- this validates the prompt contract before wiring state transitions and runner dispatch

### PR 3 — Orchestration trigger path

Goal:
- make `Todo -> In Progress` create and enqueue runs

Scope:
- implement `orchestration.service.handle_issue_state_transition(...)`
- wire it into the issue-state transition path
- enforce single-active-run guardrail
- implement follow-up run creation via `parent_run_id`
- persist rendered prompt onto the new run
- call the existing runner enqueue/dispatch interface
- add tests for:
  - trigger on `Todo -> In Progress`
  - no-op when active run exists
  - follow-up run on re-entry

Why third:
- this is the first end-to-end behavior change and should come only after prompt generation is stable

### PR 4 — Workpad and done-signal ingestion

Goal:
- make runs produce structured durable outcomes

Scope:
- implement workpad lookup/create/update behavior
- use dedicated agent/system user for workpad authorship
- implement `pi-dash-done` parsing
- normalize parsed output into `AgentRun.done_payload`
- define malformed/missing done-signal failure handling
- add tests for:
  - workpad create/reuse/update
  - done-signal parsing
  - malformed payload handling

Why fourth:
- this depends on real runs existing and gives the orchestration loop durable outputs and follow-up context

### PR 5 — Hardening and contract coverage

Goal:
- close gaps before broader rollout

Scope:
- add contract/integration tests across state transition -> run creation -> enqueue
- verify runner receives prompt verbatim
- verify `done_payload` normalization path
- tighten admin/auth behavior around preview if needed
- document re-seed and operational workflows

Why last:
- by this point the system exists; this PR reduces rollout risk

## Dependency order

1. Schema first:
   `PromptTemplate`, `AgentRun.parent_run`, `AgentRun.done_payload`, `Project.repo_*`
2. Prompt rendering next:
   context, renderer, composer, default template, seed
3. Orchestration after rendering:
   state transition hook -> create run -> enqueue runner
4. Outcome handling after runs exist:
   workpad behavior, done-signal parsing, `done_payload`
5. Hardening last:
   integration tests, auth cleanup, operational polish

## 1. Data model

- Add `PromptTemplate` model under `apps/api/pi_dash/prompting/models.py`.
- Add migration for `PromptTemplate`.
- Add `parent_run` field to `AgentRun`.
- Add `done_payload` field to `AgentRun`.
- Ensure `AgentRun.status` supports:
  - `queued`
  - `running`
  - `blocked`
  - `failed`
  - `completed`
  - `cancelled`
- Add project-level repo settings fields:
  - `Project.repo_url`
  - `Project.base_branch`
- Add or confirm indexes / constraints needed for:
  - one active prompt template per `(workspace, name)`
  - efficient lookup of issue runs by `work_item`, `status`, and `parent_run`

## 2. Prompting module

- Create `apps/api/pi_dash/prompting/`.
- Implement `context.py`:
  - build the narrow prompt context
  - use raw markdown for `issue.description`
  - read repo data from `Project.repo_url` and `Project.base_branch`
- Implement `renderer.py`:
  - sandboxed Jinja2 environment
  - consistent render error handling via `PromptRenderError`
- Implement `composer.py`:
  - `build_first_turn(issue, run)`
  - `build_continuation(...)` stub raising `NotImplementedError`
- Add `templates/default.j2`.
- Sync `default.j2` content with `workflow-handbook.md`.

## 3. Seed and admin

- Implement seed logic for the global default template.
- Choose migration or post-migrate hook and wire it up.
- Add admin registration for `PromptTemplate`.
- Make the global default row read-only in admin if needed.
- Add a management command or documented workflow for re-seeding the default template.

## 4. Orchestration service

- Create `apps/api/pi_dash/orchestration/service.py`.
- Implement `handle_issue_state_transition(issue, from_state, to_state, actor)`.
- MVP trigger rule:
  - only `Todo -> In Progress` creates a run
- Enforce single-active-run guardrail:
  - active means `queued` or `running`
- Follow-up rule:
  - re-entering `In Progress` after any prior non-active run creates a new `AgentRun`
  - set `parent_run_id` to the prior run
  - never reopen or mutate an old run
- Call the prompting composer to render `run.prompt`.
- Persist the new run with `status="queued"`.
- Call the existing runner dispatch/enqueue interface.

## 5. Runner integration

- Identify the current enqueue/dispatch entry point used to send `Assign` envelopes.
- Wire `orchestration.service` into that entry point instead of duplicating logic elsewhere.
- Confirm runner receives `AgentRun.prompt` verbatim.
- Confirm no additional runner protocol changes are required for MVP.

## 6. Done-signal ingestion

- Define where terminal run output is captured as `AgentRunEvent`.
- Implement parser for fenced `pi-dash-done` blocks.
- Validate JSON strictly.
- Normalize parsed output into `AgentRun.done_payload`.
- Use `status` as the authoritative branch field.
- Treat `autonomy` as explanatory metadata, not the parser branch point.
- Handle malformed or missing done signals as a run failure path.

## 7. Workpad behavior

- Decide or create the dedicated agent/system user used for workpad authorship.
- Implement comment lookup for the single `## Agent Workpad` issue comment.
- Implement create-if-missing behavior.
- Implement update-in-place behavior across follow-up runs.
- Ensure the workpad remains stored as a normal `IssueComment` row.

## 8. Preview endpoint

- Add preview endpoint:
  - `POST /api/v1/workspaces/{slug}/prompt-templates/{id}/preview`
- Input:
  - `issue_id`
- Output:
  - rendered prompt string
- Enforce admin/authorized access policy.
- Ensure preview does not create an `AgentRun`.

## 9. Tests

- Unit tests for prompt context generation.
- Unit tests for Jinja rendering and render failures.
- Unit tests for template lookup order:
  - workspace override
  - global fallback
- Unit tests for orchestration trigger behavior:
  - `Todo -> In Progress`
  - no-op when active run exists
  - follow-up run creation on re-entry
- Unit tests for done-signal parsing:
  - valid `pi-dash-done`
  - malformed JSON
  - missing required fields
- Unit tests for workpad lookup/create/update behavior.
- Contract or integration test proving:
  - state transition creates run
  - prompt is rendered
  - run is enqueued

## 10. Open follow-ups after MVP

- Prompt size cap enforcement.
- Workspace-admin auth model for preview outside Django staff.
- Whether autonomy `score` drives notifications or orchestration policy.
- Whether checkpoint snapshots need first-class structured persistence beyond workpad + `AgentRunEvent`.
- Additional trigger states beyond `In Progress`:
  - `Todo` for investigation-prep runs
  - `In Review` for PR review runs
