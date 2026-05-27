# 09 — Agent Orchestration

This page covers the cloud-side logic that turns a user's work item into a concrete run on a runner. The work happens across three Django modules:

```
pi_dash/orchestration/   ← phases, scheduling, workpad, done signals
pi_dash/prompting/       ← prompt fragments, composition, rendering, context
pi_dash/scheduler/       ← built-in scheduling hooks + signals
```

Plus the runner-side glue in `pi_dash/runner/services/` (matcher, run lifecycle, outbox, scheduler hook).

## Concept map

- **Work item** — the user-facing thing (issue/task/sub-issue) tracked in `app/`.
- **Run** — one execution attempt against a work item, persisted in `runner/models.py`. A work item can have many runs (retries, follow-ups).
- **Phase** — runs progress through ordered phases (`orchestration/agent_phases.py`). Each phase has its own prompt template and expected outputs.
- **Workpad** — the durable scratchpad attached to a run that the agent reads/writes structured artifacts into (`orchestration/workpad.py`).
- **Done signal** — explicit completion contract the agent emits to mark a phase done (`orchestration/done_signal.py`).
- **Pod** — a grouping primitive on the runner side (`runner/models.py`, `services/pod_naming.py`). Think of it as the namespace a run lives in on a particular runner.

## `orchestration/`

```
agent_phases.py   ← phase definitions + transitions
scheduling.py     ← decides when a run is eligible to dispatch
service.py        ← high-level "create a run from a work item" entrypoint
signals.py        ← Django signals fired during the run lifecycle
done_signal.py    ← parsing / validating the agent's "I'm done" frame
workpad.py        ← workpad CRUD + lifecycle
```

`service.py` is the canonical entrypoint. Most product code that wants to "kick off an agent on this work item" goes through it; it composes prompt → phase → run → persists.

## `prompting/`

```
composer.py       ← assembles a final prompt from fragments + context
context.py        ← gathers context (work item, project state, prior runs)
renderer.py       ← template render
fragments/        ← reusable prompt fragments
models.py         ← persisted prompt templates / fragment overrides
seed.py           ← seed default prompts
views.py / urls.py / serializers.py  ← admin API for editing prompts
```

Prompts are not hard-coded — they live in DB rows seeded by `seed.py`. Instance admins can override fragments from the admin UI, which is why `prompting/` ships a full REST surface.

## `scheduler/`

`scheduler/builtins/` holds the default scheduling rules; `signals.py` wires them into the run lifecycle. This is the layer that asks "is now a good time to dispatch?" — it can defer runs based on quotas, project state, manual gating, etc.

The runner-side scheduler hook (`pi_dash/runner/services/scheduler_hook.py`) is the bridge between this layer and the runner's HTTP long-poll: when a run becomes eligible, the matcher hands it to the next runner that asks.

## Runner-side dispatch services (`pi_dash/runner/services/`)

```
matcher.py            ← match an eligible run to a runner (project, capabilities, availability)
run_lifecycle.py      ← state-machine transitions (assigned → running → completed/failed)
session_service.py    ← welcome frame (incl. version advisories — see 07)
outbox.py             ← outbound message queue for cloud → runner notifications
pubsub.py             ← Redis pub/sub bridge for live status updates to web UI
chat.py               ← in-run chat messages
permissions.py        ← who can mutate which run / runner
pod_naming.py         ← derive pod names from project / runner / config
tokens.py             ← refresh/access token pair lifecycle
validation.py         ← payload validation helpers
runner_delete.py      ← runner removal — coordinates cloud + runner-side cleanup
```

## End-to-end (cloud's view)

```
User opens work item  →  triggers orchestration.service.create_run(work_item, phase)
                       ├─ prompting.composer.compose(work_item, phase, context)  → prompt text
                       ├─ persist Run row (status=pending) + workpad init
                       └─ scheduler.signals fire → eligibility set

Run becomes eligible   →  matcher.assign(run, available_runners)  → Run.assigned_to = runner

Runner long-polls      →  GET /api/v1/runner/runs/   (per-runner, see 07)
                       ←  yields the assigned Run + its prompt + workpad ref

Runner executes        →  POSTs events / approvals / outputs back
                       →  run_lifecycle transitions state
                       →  pubsub broadcasts to web UI for live progress

Agent emits done_signal → orchestration parses + validates → phase complete
                       →  advance to next phase OR finalize run
```

## Extending the system

- **New phase** — add to `orchestration/agent_phases.py` and a matching prompt template in `prompting/fragments/` + `seed.py`.
- **Custom scheduling rule** — add a rule under `scheduler/builtins/` and wire it via `scheduler/signals.py`.
- **New agent** — implement the agent trait in `runner/src/agent/` (Rust) and a matching dispatch path in `runner/src/codex/`-equivalent module. The cloud side is agent-agnostic: it only cares about the prompt and the done-signal contract.

## Where to read next

- [06 — Backend architecture](./06-backend-architecture.md) — the modules referenced here in context
- [07 — Runner architecture](./07-runner-architecture.md) — the runner's view of dispatch
- [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md) — the wire schema runs flow over
- `pi_dash/orchestration/service.py` — the canonical entrypoint
