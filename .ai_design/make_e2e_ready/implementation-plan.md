# Pi Dash E2E Readiness — Implementation Plan

Purpose:

- make the current prompt-system + runner stack actually work end to end with a local Codex installation
- close the contract gaps between prompt, runner, and cloud
- define the minimum shippable path before broader rollout

Scope:

- local Codex runner execution
- Pi Dash workpad and completion flow
- done-signal ingestion and issue state transition application
- operator and developer verification

Non-goals:

- redesign the runner protocol from scratch
- add multi-turn orchestration
- build a polished admin UX for prompt-template editing
- broaden delegation rules beyond the current MVP trigger

Related docs:

- `.ai_design/prompt_system/prompt-system-design.md`
- `.ai_design/prompt_system/workflow-handbook.md`
- `.ai_design/implement_runner/runner-design.md`
- `.ai_design/implement_runner/implementation-tasks.md`

## Problem statement

The current branch has most of the primitives needed for a delegated local-agent workflow:

- issue-state transitions can create `AgentRun`s
- the runner can receive assignments and launch `codex app-server`
- the runner can forward completion and approval events back to the cloud

But the system is not yet truly end to end. The current implementation has four blocking gaps:

1. Prompt-to-runtime capability mismatch
   - The prompt tells the agent it can read and update Pi Dash issues/comments/workpad state.
   - The local runner currently launches `codex app-server` without exposing any Pi Dash tool surface, API token, or helper client.

2. Completion-contract mismatch
   - The prompt contract requires a terminal `pi-dash-done` fenced JSON block.
   - The runner currently treats `turn/completed.params.done` as the source of truth.
   - The cloud-side fenced-block parser exists, but it is not wired into the active runtime path.

3. Outcome application gap
   - The system persists `done_payload`, but it does not yet apply the requested issue state transition from that payload.
   - As a result, successful or blocked runs do not complete the orchestration loop.

4. Operational verification gap
   - `pi-dash-runner doctor` assumes an older Codex CLI auth subcommand and currently fails on the local CLI version in use.
   - This makes readiness checks noisy and weakens installation confidence.

This plan closes those gaps with the smallest coherent set of changes.

## Success criteria

The system is considered e2e-ready when all of the following are true:

- A delegated issue entering the trigger state creates an `AgentRun`, selects a runner, and starts a local Codex turn.
- Codex can read and update the Pi Dash workpad using a supported runtime surface rather than prompt fiction.
- The agent's final outcome is ingested through one canonical completion contract.
- The cloud applies the normalized outcome to both:
  - `AgentRun.status`
  - the issue state transition requested by the done payload, when valid
- Blocked runs remain actionable and preserve the escalation metadata.
- An operator can run a local readiness check that correctly validates Codex binary presence and login state.
- Automated tests cover the state transition -> run execution -> completion ingestion -> issue update path.

## Committed decisions

### 1. One canonical completion contract

Decision:

- The cloud-side `pi-dash-done` payload remains the canonical completion contract.

Why:

- The prompt system, workpad contract, and orchestration metadata are already modeled around this payload.
- It gives Pi Dash a stable, structured contract that is decoupled from whatever `codex app-server` happens to emit natively.
- It lets the cloud branch on one domain-specific schema rather than a Codex transport detail.

Implication:

- The runner must capture the final assistant text for the turn, extract the fenced done block, and send the normalized result back to the cloud.
- `turn/completed.params.done` can remain as a transport optimization only if it is populated from the exact same canonical payload; otherwise it is ignored for Pi Dash workflow purposes.

### 2. Real Pi Dash capability surface, not prompt-only claims

Decision:

- The local Codex environment must be given an explicit Pi Dash capability surface.

Preferred MVP shape:

- expose a small local helper CLI or MCP-like tool surface for:
  - get issue details
  - list issue comments
  - create or update the single workpad comment
  - optionally fetch workspace/project metadata already present in the run context

Why:

- It matches the prompt contract.
- It avoids leaking raw long-lived credentials directly into arbitrary shell usage.
- It gives a narrow and auditable interface for the agent.

Fallback if helper tooling is deferred:

- reduce the prompt contract to only the capabilities that truly exist in-session
- do not instruct the agent to maintain the workpad if the runtime cannot support it

Note:

- The preferred path is to implement the helper surface, not weaken the prompt.

### 3. Cloud owns workflow application

Decision:

- The cloud remains the authority for:
  - parsing the done payload
  - updating `AgentRun`
  - applying requested issue state transitions

Why:

- Issue state is a Pi Dash domain concern, not a runner concern.
- This keeps local Codex runs stateless with respect to workflow branching.
- It preserves an auditable server-side outcome path.

### 4. Readiness check must track the current Codex CLI

Decision:

- `doctor` must support the actual locally installed Codex CLI auth commands rather than assuming one legacy subcommand.

Why:

- E2E readiness is only meaningful if the health check reflects reality on operator machines.

## Proposed rollout

## Phase 1 — Completion contract alignment

Goal:

- make run completion deterministic and compatible with the prompt system

Changes:

- extend the runner bridge or history pipeline to capture the final assistant message body
- parse the terminal `pi-dash-done` fenced block before marking the run complete
- send normalized `done_payload` to the cloud in `RunCompleted`
- if the fence is missing or malformed:
  - mark the run failed
  - include a parse error detail

Implementation notes:

- reuse `apps/api/pi_dash/orchestration/done_signal.py` as the source of truth for normalization rules
- avoid implementing a second independent parser in Rust unless there is a strong operational reason
- if Rust-side parsing is needed for latency or robustness, define fixture parity tests against the Python parser

Deliverables:

- final-turn capture path in runner
- canonical done-payload transport to cloud
- contract tests for valid, missing, and malformed done fences

## Phase 2 — Pi Dash workpad runtime surface

Goal:

- make the prompt's workpad instructions executable by the agent

Changes:

- add a narrow Pi Dash helper interface available to local Codex sessions
- authenticate that interface using a runner-scoped or run-scoped credential minted by the cloud
- implement endpoints or helper commands for:
  - fetch issue
  - list comments
  - get-or-create workpad
  - update workpad in place

Preferred architecture:

- runner launches Codex with access to a local helper binary or local API wrapper
- helper speaks to Pi Dash using a scoped token that only permits the current run's allowed issue operations

Security requirements:

- no broad workspace token exposed directly to shell by default
- credentials should be least-privilege and revocable
- operations must be scoped to the current `AgentRun` or current issue when feasible

Deliverables:

- helper interface contract doc or README
- runner injection path
- API/auth implementation on the cloud side
- tests for workpad create, reuse, and update

## Phase 3 — Cloud-side outcome application

Goal:

- complete the orchestration loop after a run finishes

Changes:

- on `RunCompleted`, parse or validate the canonical done payload
- update:
  - `AgentRun.status`
  - `AgentRun.done_payload`
  - `AgentRun.ended_at`
- apply `state_transition.requested_group` when valid
- preserve blocked/noop semantics

Rules:

- `completed` status:
  - mark run completed
  - allow transition to `completed` group if requested and valid
- `blocked` status:
  - mark run blocked
  - do not auto-transition to a terminal completed state
  - preserve blockers and autonomy fields for UI and follow-up handling
- `noop` status:
  - mark run terminal without implying code change
  - transition only if explicitly allowed by policy

Validation rules:

- reject invalid state-group transitions cleanly
- log and persist outcome-application failures
- never lose the original done payload even if issue transition application fails

Deliverables:

- server-side completion application service
- tests covering:
  - completed -> issue moved
  - blocked -> issue not advanced
  - invalid requested transition -> run preserved with error handling

## Phase 4 — Operational readiness and verification

Goal:

- make local installation and smoke testing reliable

Changes:

- update `pi-dash-runner doctor` to support current Codex CLI auth discovery
- add a documented smoke-test path:
  - register runner
  - verify doctor
  - delegate issue
  - confirm workpad update
  - confirm done-payload ingestion
  - confirm issue state transition

Deliverables:

- updated doctor checks
- operator guide additions
- one explicit happy-path test recipe in repo docs

## Work breakdown

## PR 1 — Canonical done-signal ingestion

Scope:

- capture final assistant output in runner
- derive canonical done payload from fenced block
- send canonical payload to cloud
- align completion tests

Files likely touched:

- `runner/src/codex/bridge.rs`
- `runner/src/daemon/supervisor.rs`
- `runner/src/history/jsonl.rs`
- `runner/tests/codex_bridge_fake.rs`
- possibly a shared parser fixture area

Acceptance:

- run does not complete successfully without a valid final done payload
- malformed done payload produces a failed run with a clear error

## PR 2 — Pi Dash workpad tool surface

Scope:

- define and implement the helper surface
- mint scoped credentials if required
- expose helper to local Codex session
- integrate workpad operations

Files likely touched:

- `runner/src/codex/app_server.rs`
- `runner/src/daemon/supervisor.rs`
- `apps/api/pi_dash/orchestration/workpad.py`
- new API endpoints or a small helper module

Acceptance:

- agent can create and update exactly one `## Agent Workpad` comment through supported runtime tooling

## PR 3 — Cloud completion application

Scope:

- centralize run-finalization logic in the cloud
- apply issue state transitions from the normalized done payload
- preserve blocked and invalid-transition behavior

Files likely touched:

- `apps/api/pi_dash/runner/consumers.py`
- `apps/api/pi_dash/orchestration/done_signal.py`
- new orchestration completion service module
- tests under `apps/api/pi_dash/tests/unit/orchestration/`

Acceptance:

- completed runs can advance issue state
- blocked runs remain blocked with intact metadata

## PR 4 — Doctor and operator hardening

Scope:

- modernize Codex auth detection
- document the smoke test
- add contract coverage where missing

Files likely touched:

- `runner/src/cli/doctor.rs`
- `runner/README.md`
- `.ai_design/implement_runner/operator-guide.md`

Acceptance:

- readiness checks pass on a valid local Codex install
- operator docs describe one verified e2e flow

## Open design questions

These must be resolved before Phase 2 implementation finishes:

1. What is the Pi Dash runtime surface for local Codex?
   - local helper CLI
   - local proxy daemon
   - MCP server
   - direct HTTP helper commands

2. What credential shape should be used?
   - run-scoped token
   - runner-scoped token with issue-level authorization
   - short-lived signed capability token

3. Where should done-signal parsing live?
   - Python only, with runner shipping raw final text
   - Rust only, with mirrored fixtures
   - both, with one side authoritative and the other defensive

4. Should workpad failure block task completion?
   - recommended answer: yes for this workflow, because the workpad is part of the prompt contract and audit trail

## Risks

- If we keep both `turn/completed.params.done` and fenced output as competing sources of truth, the system will drift and become difficult to debug.
- If we expose overly broad Pi Dash credentials to the local shell, we create unnecessary security risk.
- If we weaken the prompt instead of adding the missing runtime surface, the resulting workflow will be less useful and will regress the original product intent.
- If issue state transition application is embedded ad hoc in the websocket consumer, the completion path will be hard to test and evolve.

## Recommended implementation order

1. Land canonical done-signal ingestion first.
2. Add the Pi Dash runtime helper surface second.
3. Centralize cloud-side completion application third.
4. Finish with doctor hardening and smoke-test docs.

This order keeps the system debuggable:

- first ensure the outcome contract is stable
- then ensure the agent can perform the promised workpad actions
- then let the cloud act on the result

## Exit checklist

- [ ] Runner captures and forwards canonical done payload
- [ ] Cloud finalizes runs from the canonical payload path
- [ ] Issue state transition application is implemented and tested
- [ ] Local Codex has a real Pi Dash workpad capability surface
- [ ] Workpad create/update path is covered by tests
- [ ] `doctor` correctly validates current Codex CLI auth
- [ ] One documented e2e smoke test succeeds on a fresh machine
