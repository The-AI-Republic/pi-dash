# Ticking Optimization — Workpad as Protocol, Drop Session Resume

> Directory: `.ai_design/ticking_optimization/`
>
> **Status:** design — no code changes yet. Supersedes the session-resume model
> assumed in `.ai_design/issue_ticking_system/design.md` §10 ("native session
> resume — kept unchanged"). Once this lands, that note in the ticking design
> should be updated to point here.
>
> **Scope:** how an agent run on an issue is prompted and dispatched on every
> trigger — initial delegation, periodic tick, Comment & Run. Replaces the
> current dual-path model (full template on first run / tiny continuation prompt
> on follow-ups via Codex `--resume`) with a single uniform path: every run
> renders the full template into a fresh Codex session. The workpad comment is
> the only carrier of state between runs.
>
> **What this changes about today's code**
>
> - `pi_dash.prompting.composer.build_continuation` becomes dead code; deleted.
> - `pi_dash.orchestration.service._create_continuation_run` always renders via
>   `build_first_turn`. Naming should be revisited (it's no longer
>   "continuation"-distinct from a fresh dispatch).
> - `pi_dash.runner.services.matcher._refresh_continuation_prompt` is deleted.
> - On the runner side (Rust `runner/`), `codex --resume <thread_id>` is no
>   longer invoked when picking up a follow-up `AgentRun`. Each `AgentRun`
>   spawns a fresh `codex` process with a new `thread_id`. `thread_id` is still
>   captured per-run (so multi-turn tool use within one dispatch works), it is
>   just not reused across `AgentRun` boundaries.
> - `_pinned_runner_for` semantics change from "pin so resume can succeed" to
>   "pin so the same disk checkout / branch is reused." Still useful, no longer
>   correctness-critical.
> - The `parent_run` foreign key on `AgentRun` is kept for lineage / auditing,
>   but stops gating prompt-rendering behavior.

## 1. Problem

The current ticking design treats follow-up runs as resumable conversations:

1. The first run on an issue gets the **full rendered template** (~3 KB across
   14 fragments) via `composer.build_first_turn`.
2. Every subsequent run — periodic ticks and Comment & Run alike — gets a
   **tiny continuation prompt** via `composer.build_continuation`: just the new
   human comments since the parent run started, or the literal sentinel
   `"(continuation requested with no new human input — proceed)"` when there
   are none.
3. The runner reattaches to the prior Codex session via `--resume <thread_id>`.
   This works only when the same runner is still online and still has the
   session locally; the model relies on its in-context memory of the original
   prompt to interpret the tiny new input.

This is correct on the happy path. It is silently broken on three off-paths
that we hit in real usage:

### 1.1 Failover-induced silent context loss

Pi Dash's pod model lets any runner in the pod pick up an unpinned run.
`_pinned_runner_for` returns `None` when the original runner is offline,
revoked, or has lost its session. The new runner has no local session for the
captured `thread_id`. It gets handed the 50-byte continuation prompt and
launches a fresh Codex process with effectively no instructions. The agent
either no-ops, hallucinates a task, or makes a wrong move. From the user's
perspective the issue silently stops progressing.

This is the failure mode that motivated the investigation: a non-coding test
issue ("run `pwd` and post a comment via pidash CLI") was assumed to fail
because of the prompt's unconditional `git fetch origin`, but the same class
of failure can manifest any time pod rebalancing breaks the resume path.

### 1.2 Prompt-update staleness

When we update a prompt fragment (e.g., PR #102 — task-type fork on
`code_change` vs `noncode`), the new template applies only on first runs.
Issues with active resumed sessions keep operating on the _original_ template
captured in their session memory, indefinitely. There is no signal in the
runtime that the agent is running on a stale prompt; the user sees the issue
behaving the way it did before the fix and assumes the fix didn't ship.

For a system whose behavior is largely defined by the prompt and that we
expect to iterate on continuously, this is a serious feedback-loop problem.

### 1.3 Monotonic context growth

By tick N, the resumed conversation has accumulated `N-1` prior turn outputs
in context — workpad edits, tool results, file reads, internal reasoning.
With a 24-tick cap (3 days at the default 3h cadence), an agent making a
decision on tick 24 is doing so against a noticeably noisier context than
tick 1. Models exhibit "lost in the middle" degradation in long contexts;
decision quality is not uniform across an issue's lifetime under this design.

The token-cost framing ("resume saves tokens") also weakens under inspection:
prompt caching on the system prompt absorbs most of the cost of fresh-each-tick,
while the accumulated turn tail under resume keeps paying full token cost
turn-over-turn.

## 2. Product goal (the contract that actually matters)

Pi Dash's user-facing promise is simple. A human creates an issue. An agent
works it end-to-end when it can. When it can't (genuine ambiguity, missing
auth, blocked dependency), it posts a focused comment and waits. When the
human responds, the agent picks up. The user sees an issue moving toward done,
or a clear question they need to answer.

What the user does **not** see and does **not** care about:

- Whether tick N's agent is "the same Codex process" as tick N-1's.
- Whether session memory persists across ticks.
- Whether tokens are saved by reusing a thread.

What the user **does** see — and where the current design breaks the contract:

- The agent silently stops working after a runner failover.
- A prompt fix the team shipped yesterday hasn't taken effect on this issue.
- The agent on tick 14 makes a worse decision than it would have on tick 1
  because the context is full of stale workpad drafts and old tool calls.

The architectural primitive that delivers the product contract is **durable
externally-visible state**, not in-process session memory. State that lives
in the issue thread, the workpad comment, and the repo survives ticks, runner
churn, prompt updates, and pod rebalancing. State that lives in a Codex
session does not.

## 3. Proposal

**Every agent run is a fresh Codex session with a freshly rendered full
template. The workpad is the only carrier of state across runs. Session
resume is removed for run-to-run continuity.**

This applies uniformly to all three triggers: initial delegation, periodic
tick, Comment & Run. There is one path through the orchestration and runner
layers, parameterized only by _when_ a new run is created (which differs by
trigger), not by _how_ it is prompted.

### 3.1 The protocol the agent follows on every run

Already substantially encoded in fragments 07 (Step 0.5 — Analyze & scope)
and 14 (Follow-up run context). Made explicit and uniform:

1. Read the issue description.
2. Read all comments via `pidash comment list`, in chronological order.
3. Locate and read the existing `## Agent Workpad` comment end-to-end.
   - If present: this is the prior state. Reconcile the plan, check off items
     already done, expand for new scope. Do not restart from scratch.
   - If absent: this is a true first run. Create one.
4. Look at repo state (current branch, recent commits) when relevant.
5. Decide one move: continue / ask / wait / escalate / done.
6. Externalize the decision and any new findings to the workpad **before**
   exiting. Anything not in the workpad is lost.
7. Take the bounded step (one comment, one commit, one investigation, one
   question), update the workpad checkpoints, exit cleanly.

The completion of the issue is a property of the workpad converging, not a
property of any single run finishing the work.

### 3.2 Why this works without resume

The workpad is designed to hold everything the next agent needs to pick up
the work: the analysis (incl. Task type from PR #102), the plan with
checkpoints, the autonomy assessment, blocking questions, validation status,
and free-form notes. The repository is the second carrier (branch, commits,
tests). The comment thread is the third (questions, answers, decisions).

A fresh Codex process given the full template plus access to these three
durable surfaces has everything a resumed session would have, minus the
ephemeral in-context state the prior session had loaded but never wrote down
— and that ephemeral state is precisely what we want to discourage the agent
from relying on.

### 3.3 Why uniform across all triggers

Two paths (resume for Comment & Run, fresh for ticks) was tempting because
Comment & Run has a shorter human-perceived loop. But:

- Comment & Run can land on a different runner via failover too. Same silent
  context-loss bug, just less visibly.
- "Short loop" is not guaranteed — a human comment may arrive hours after the
  agent finished its prior tick and went idle.
- Two paths means two sets of test cases, two debug surfaces, two failure
  modes. The reduction to one path is a meaningful simplification.
- Prompt updates apply uniformly, immediately, on every trigger.

The cost of unifying — Comment & Run pays a full template render instead of a
tiny prompt — is ~3 KB of cached tokens per click. Negligible.

## 4. What changes

### 4.1 Pi Dash (Python)

| File                                 | Change                                                                                                                                                                                                                                                                                                                                                                         |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `prompting/composer.py`              | Delete `build_continuation`. Keep `build_first_turn` as the sole entrypoint. Optionally rename to `build_run_prompt` to retire the "first turn" framing.                                                                                                                                                                                                                       |
| `orchestration/service.py`           | `_create_continuation_run` calls `build_first_turn`. Once this lands, it can be merged into `_create_and_dispatch_run` (the bodies become near-identical aside from the `parent_run` field). Consider renaming to `_create_run` and parameterizing `parent_run`.                                                                                                               |
| `runner/services/matcher.py`         | Delete `_refresh_continuation_prompt`. The dispatch path no longer needs to rebuild the prompt at dispatch time; the prompt rendered at run-creation time is the final prompt.                                                                                                                                                                                                 |
| `runner/views/runs.py`               | The Comment & Run handler (`_post_comment_and_run`) no longer needs to flag the run as a "continuation" for prompt-rendering purposes. The trigger type is still recorded for telemetry / scheduler bookkeeping.                                                                                                                                                               |
| `prompting/fragments/14_followup.md` | Currently gated on `run.attempt > 1`. The intent stays the same but it now fires on every non-first run, which is the steady state. Consider folding its content into fragment 08 (workpad reconciliation) since it's no longer a special case.                                                                                                                                |
| Tests                                | `test_build_continuation_*` tests in `tests/unit/prompting/test_composer.py` are removed. `test_drain_does_not_rebuild_first_turn_prompt` and the matcher tests around `_refresh_continuation_prompt` flip their expectations or are removed. Add a positive test that every follow-up run's `prompt` equals what `build_first_turn(issue, run)` would produce at that moment. |

### 4.2 Runner (Rust)

| Area                      | Change                                                                                                                                                                                                                                                           |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Codex invocation          | Drop the `--resume <thread_id>` flag from the launch command for follow-up runs. Always invoke `codex` fresh. `thread_id` capture remains useful within a single `AgentRun` (multi-turn tool use, log correlation) and continues to be reported back to Pi Dash. |
| Local session storage     | The runner can stop preserving Codex session state across `AgentRun` boundaries. Sessions are now ephemeral to a single dispatch and can be pruned when the dispatch terminates.                                                                                 |
| Pinning / queue selection | Unchanged in shape — still prefer the previously-pinned runner. The reason changes (repo locality, not session locality), but the mechanism is the same.                                                                                                         |

### 4.3 Prompt fragments

Most existing fragments are already written for fresh-each-tick because they
tell the agent to read durable state via `pidash` CLI calls. Specific updates:

- **Fragment 14** — currently positioned as "follow-up run context (attempt N
  > 1)." Re-frame as the steady-state workpad-reconciliation rule that
  > applies whenever a workpad exists, regardless of attempt number. Or fold
  > into Fragment 08 step 2 (which already does workpad reconciliation when
  > one is found). Decide one home for the rule.
- **Fragment 07** — Step 0.5 step 1 already says "Pull all prior comments via
  `pidash comment list`." Reinforce: the comment list is authoritative; do
  not assume any prior comment is already in your context.
- **Fragment 04** — Default posture mentions "Maintain exactly one
  `## Agent Workpad` comment as your source of truth." Strengthen to:
  "Anything not written to the workpad is lost between runs."

## 5. What stays the same

- The issue lifecycle model (states, comments-as-conversation,
  agent-self-assesses-each-turn). See `MEMORY.md` →
  `project_issue_lifecycle_model.md`.
- The ticking system (3h cadence, 24-tick cap, Paused state, Comment & Run
  button). See `.ai_design/issue_ticking_system/design.md`.
- The autonomy / escalation model and the workpad template structure. The
  workpad becomes _more_ important under this design, not less.
- The pinning mechanism (still preferred for repo locality).
- `parent_run` lineage on `AgentRun` (kept for auditing, no longer drives
  prompt selection).
- All trigger sources — initial delegation, periodic tick, Comment & Run
  button. They differ in _when_ a new `AgentRun` is created; they no longer
  differ in _how_ it is prompted.

## 6. Migration and rollout

This change is observably behavior-changing for any issue with a live
resumed session at deploy time. Recommended rollout:

1. **Land Pi Dash side first.** `composer.build_continuation` removal,
   `_create_continuation_run` switch to `build_first_turn`, matcher
   simplification, test updates. At this point, the runner still tries to
   `--resume`, but the prompt it sends is now self-sufficient. The agent will
   work correctly with or without resume succeeding.
2. **Land runner side second.** Drop `--resume` invocation. Once Pi Dash is
   sending self-sufficient prompts, dropping resume on the runner is a no-op
   for correctness.
3. **Spot-check a handful of in-flight issues** at each step. Confirm the
   workpad is rich enough for the agent to recover. If a workpad is too thin,
   that's a prompt-discipline gap to fix in the prompt fragments, not a
   reason to keep resume.

Expected user-visible effects post-rollout:

- Failover stops causing silent context loss.
- Prompt fixes take effect on next tick, not "next time the issue restarts."
- Per-tick agent decisions become reproducible from `(workpad@T, comments@T,
repo@T, current template)`.

No data migration. No state-format change. No downtime.

## 7. Risks and open questions

### 7.1 Workpad-discipline gaps may surface

If any current prompt path lets the agent leave important state in
session-memory only, that issue will regress: the next tick won't find the
state in the workpad and may redo work or get confused. Mitigation:
spot-check before rollout, audit the existing fragments for "the agent might
remember X" assumptions, fix discovered gaps.

### 7.2 Larger per-tick cost

Without resume, every tick re-renders ~3 KB and re-pays the system-prompt
tokens (mostly absorbed by prompt caching on Anthropic and similar providers
— need to confirm Codex-CLI's caching behavior). Acceptable trade for the
correctness wins.

### 7.3 Loss of "warm" Codex sessions

Some users may have been benefiting from short Comment & Run loops where the
session was still warm with file reads etc. They'll now pay one extra
file-read per click. If this is a real complaint, it can be mitigated by
keeping a short-lived Codex session warm on the runner side without using it
for prompt continuity (i.e., the prompt is still full and self-sufficient).
Defer until there's evidence of a problem.

### 7.4 Codex-CLI semantics

This design assumes:

- `codex` started without `--resume` produces a clean session bound to a
  fresh `thread_id`.
- The full rendered template is the initial input (system or first user
  message — the runner already handles this distinction).
- Captured `thread_id` is unique per `AgentRun` and not reused across runs.

These match the runner's current behavior on first runs. Confirm during
implementation that nothing in the runner side implicitly depends on
`thread_id` reuse for non-resume reasons (e.g., logging, telemetry). If it
does, those callers need updating.

### 7.5 Stale `parent_run` interpretation

`AgentRun.parent_run` is currently a load-bearing pointer for resume. Once
resume is gone, downstream code that branches on `parent_run is not None`
should be audited — most of it is fine (lineage, audit), but anything that
infers "this is a resumable continuation" from `parent_run` is now wrong.

## 8. Alternatives considered

### 8.1 Keep resume; render full template every tick on top of resume

Sends the full template as a new user message into the resumed conversation.
Combines the worst of both designs: prompt-cache prefix is broken (template
appears at varying conversation positions), instruction-duplication ambiguity
arises (the model sees "Step 0.5 — analyze and decide" twice and has to
guess which is authoritative), monotonic context growth is unaffected.
Rejected.

### 8.2 Keep resume for ticks but render full template only when failover is

detected

Failover-only fallback is the minimal fix for the silent-context-loss bug,
but does nothing for prompt staleness or context growth. Also keeps two
paths in the orchestration layer with subtly different semantics, which is
the kind of complexity that breeds future bugs. Rejected in favor of the
uniform fresh-each-run model.

### 8.3 Drop resume for ticks, keep for Comment & Run

Discussed and rejected during design. The asymmetry has no strong
justification beyond "Comment & Run usually has a short gap" — which isn't
guaranteed and doesn't change the architectural primitive. The unifying win
(one path, immediate prompt updates everywhere, replayable inputs
everywhere) outweighs the marginal token saving on Comment & Run.

## 9. PR sequencing

Suggested split:

1. **PR A — Pi Dash side.** Composer cleanup, orchestration switch, matcher
   simplification, test updates. Self-contained; behavior change is "every
   follow-up run now gets a self-sufficient prompt." The runner still does
   what it does today (tries to resume); when resume succeeds the agent
   ignores the redundant context, when it fails the agent uses the workpad.
2. **PR B — Runner side.** Drop `--resume` invocation. Cleanup of session
   storage. Pure simplification on top of PR A.
3. **PR C — Prompt fragment refresh (optional).** Fold Fragment 14's content
   into Fragment 08's workpad reconciliation step, strengthen
   workpad-as-source-of-truth language in Fragment 04. Defer until A and B
   have soaked.

Each PR is independently revertible. PR A by itself is enough to fix the
failover bug; PRs B and C are cleanup.
