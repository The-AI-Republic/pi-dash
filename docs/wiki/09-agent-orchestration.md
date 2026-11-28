# 09 — Agent Orchestration

This page covers the cloud-side logic that turns a user's work item into a concrete run on a runner. The work happens in the Rust services:

```
rust-api/crates/services/src/orchestration/  ← phases, scheduling, workpad, done signals
rust-api/crates/services/src/prompting/      ← prompt fragments, composition, rendering, context
rust-api/crates/services/src/scheduler/      ← built-in scheduling rules
```

Plus the dispatch glue in `rust-api/crates/services/src/runner_*` (matcher, run lifecycle, outbox, sessions, enrollment).

## Concept map

- **Work item** — the user-facing thing (issue/task/sub-issue) served by the `app_*` domains.
- **Run** — one execution attempt against a work item, persisted as an `agent_run` row. A work item can have many runs (retries, follow-ups).
- **Phase** — runs progress through ordered phases (see the `PHASES` table in the services crate). Each phase has its own prompt template and expected outputs.
- **Workpad** — the durable scratchpad attached to a run that the agent reads/writes structured artifacts into.
- **Done signal** — explicit completion contract the agent emits to mark a phase done.
- **Pod** — a grouping primitive on the runner side. Think of it as the namespace a run lives in on a particular runner.

## `orchestration/`

```
creation.rs    ← high-level "create a run from a work item" entrypoint
dispatch.rs    ← decides when a run is eligible to dispatch
clock.rs       ← ticking clock + failure-policy backstops
blockers.rs / relations.rs / entries.rs  ← run graph support
```

`creation.rs` is the canonical entrypoint. Most product code that wants to "kick off an agent on this work item" goes through it; it composes prompt → phase → run → persists.

## `prompting/`

```
composer.rs    ← assembles a final prompt from fragments + context
context.rs     ← gathers context (work item, project state, prior runs)
renderer.rs    ← template render
fragments/     ← reusable prompt fragments
registry.rs / seed.rs  ← persisted prompt templates + seed defaults
```

Prompts are not hard-coded — they live in DB rows seeded by `seed.rs`. Instance admins can override fragments from the admin UI, which is why `prompting/` ships a full REST surface.

## `scheduler/`

`scheduler/builtins.rs` holds the default scheduling rules. This is the layer that asks "is now a good time to dispatch?" — it can defer runs based on quotas, project state, manual gating, etc.

The matcher is the bridge between this layer and the runner's HTTP long-poll: when a run becomes eligible, the matcher hands it to the next runner that asks.

## Runner-side dispatch services (`services/src/runner_*`)

```
runner_runs/       ← match an eligible run to a runner; state-machine transitions (assigned → running → completed/failed)
runner_sessions/   ← welcome frame (incl. version advisories — see 07); outbox + pubsub for live status updates
runner_enroll/     ← device-code enrollment; refresh/access token pair lifecycle
run_creation/      ← run creation guards (e.g. agent availability)
```

## End-to-end (cloud's view)

```
User opens work item  →  triggers run creation (work_item, phase)
                       ├─ prompting composer composes (work_item, phase, context)  → prompt text
                       ├─ persist agent_run row (status=pending) + workpad init
                       └─ scheduler rules fire → eligibility set

Run becomes eligible   →  matcher assigns (run, available_runners)  → run assigned to runner

Runner long-polls      →  GET /api/v1/runner/runs/   (per-runner, see 07)
                       ←  yields the assigned run + its prompt + workpad ref

Runner executes        →  POSTs events / approvals / outputs back
                       →  lifecycle transitions state
                       →  pubsub broadcasts to web UI for live progress

Agent emits done signal → orchestration parses + validates → phase complete
                       →  advance to next phase OR finalize run
```

## Run failure taxonomy and retry policy

Every failed run stores one canonical value in `agent_run.failure_reason`,
classified at write time by the failure classifier (`crates/types/src/runner_runs/failure.rs`) from the
runner protocol's `FailureReason` plus the raw error text (old runners that
only send the coarse protocol reasons still classify — the text rules fill
the gap). The stored strings are a wire contract for dashboards and API
filters: never rename one, only add.

The ticker consults `policy_for(reason)` when a run fails without yielding
(`orchestration/clock.rs`):

| Policy                        | Ticker behaviour                                                                                                                                                                                                                                  |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Retry soon, free**          | Re-queue a free entry on a short backoff (`FREE_RETRY_BACKOFF_SECONDS`, 120 s). Does not spend a tick. Capped at `FREE_RETRY_MAX_ATTEMPTS` (3) consecutive tries, then the backstop stops the clock.                                              |
| **Retry on the normal clock** | Today's behaviour: keep ticking, spend a tick per retry.                                                                                                                                                                                          |
| **Needs a human**             | Stop the clock with disarm reason `failure_needs_human`; no further ticks are spent. The failure comment names the reason and the fix. Never auto-pauses — Re-tick (re-arms without granting budget), Run AI, or a state move resumes once fixed. |
| **Fresh session**             | Clear the failed run's `thread_id` so the next continuation starts a fresh session instead of resuming the one that will fail again. Ticks on the normal clock.                                                                                   |

Backstop regardless of policy: `REPEATED_FAILURE_LIMIT` (3) runs in a row
failing with the same reason stops the clock with disarm reason
`repeated_failure` (free-retry reasons get their retry allowance first, so
their backstop sits at `FREE_RETRY_MAX_ATTEMPTS + 1`). Same recovery as
`failure_needs_human`.

| Reason                                        | Side     | Policy                           |
| --------------------------------------------- | -------- | -------------------------------- |
| `workspace_setup`                             | platform | needs a human                    |
| `git_auth`                                    | platform | needs a human                    |
| `network`                                     | platform | retry soon, free                 |
| `runner_offline`                              | platform | retry soon, free                 |
| `daemon_restart`                              | platform | retry soon, free                 |
| `assign_rejected_busy`                        | platform | retry soon, free                 |
| `timeout`                                     | platform | normal clock                     |
| `max_turns`                                   | platform | normal clock                     |
| `internal`                                    | platform | normal clock                     |
| `agent_error.provider_auth_or_access`         | agent    | needs a human                    |
| `agent_error.provider_quota_limit`            | agent    | needs a human                    |
| `agent_error.provider_capacity_or_rate_limit` | agent    | normal clock                     |
| `agent_error.provider_server_error`           | agent    | normal clock                     |
| `agent_error.provider_network`                | agent    | retry soon, free                 |
| `agent_error.context_overflow`                | agent    | fresh session                    |
| `agent_error.model_not_found_or_unavailable`  | agent    | needs a human                    |
| `agent_error.missing_executable`              | agent    | needs a human                    |
| `agent_error.missing_config`                  | agent    | needs a human                    |
| `agent_error.unsupported_version`             | agent    | needs a human                    |
| `agent_error.empty_or_unparseable_output`     | agent    | normal clock                     |
| `agent_error.process_failure`                 | agent    | normal clock                     |
| `agent_error.unknown`                         | agent    | normal clock (fail-safe default) |

Classifier notes: HTTP status codes only match on digit boundaries (so
`402913 tokens` / `15290ms` / `exit status 4030` never land in a provider
bucket); pre-launch failures route platform-side by construction and never
reach the agent-text rules; capacity wording is checked before the auth
status codes because Anthropic-compatible providers use 403 for concurrency
rejections; anything unrecognised falls into the normal-clock bucket, never
the free-retry one. Refusals are out of this taxonomy — `REFUSED` +
`refusal_category` model those separately.

## Extending the system

- **New phase** — add to the phase table in the services crate and a matching prompt template in `prompting/fragments/` + `seed.rs`.
- **Custom scheduling rule** — add a rule to `scheduler/builtins.rs`.
- **New agent** — implement the agent trait in `runner/src/agent/` (Rust). The cloud side is agent-agnostic: it only cares about the prompt and the done-signal contract.

## Where to read next

- [06 — Backend architecture](./06-backend-architecture.md) — the modules referenced here in context
- [07 — Runner architecture](./07-runner-architecture.md) — the runner's view of dispatch
- [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md) — the wire schema runs flow over
- `rust-api/crates/services/src/orchestration/creation.rs` — the canonical entrypoint
