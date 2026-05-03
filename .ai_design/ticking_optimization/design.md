# Ticking Optimization — Workpad as Protocol, Drop Session Resume

> Directory: `.ai_design/ticking_optimization/`
>
> **Status:** design — implementation underway in the same branch. Supersedes
> the session-resume model assumed in
> `.ai_design/issue_ticking_system/design.md` §10 ("native session resume —
> kept unchanged"). Once this lands, that note in the ticking design should be
> updated to point here.
>
> **Scope:** how an agent run on an issue is prompted and dispatched on every
> trigger — initial delegation, periodic tick, Comment & Run. Replaces the
> current dual-path model (full template on first run / tiny continuation
> prompt on follow-ups via Codex thread/resume) with a single uniform path:
> every run renders the full template into a fresh agent session. The workpad
> comment is the only carrier of state between runs.
>
> **What this changes about today's code**
>
> - `pi_dash.prompting.composer.build_continuation` becomes dead code; deleted.
> - `pi_dash.orchestration.service._create_continuation_run` always renders via
>   `build_first_turn`. Naming should be revisited (it's no longer
>   "continuation"-distinct from a fresh dispatch).
> - `pi_dash.runner.services.matcher._refresh_continuation_prompt` is deleted.
> - On the runner side (Rust `runner/`), the Codex bridge no longer issues
>   `thread/resume` JSON-RPC calls; the Claude Code bridge no longer passes
>   `--resume <session_id>`. The `resume_thread_id` field is dropped from the
>   `Assign` server message and the `RunPayload` struct. `thread_id` is still
>   captured per-run (so multi-turn tool use within one dispatch works), it is
>   just not reused across `AgentRun` boundaries.
> - `_pinned_runner_for` semantics change from "pin so resume can succeed" to
>   "pin so the same disk checkout / branch is reused." Still useful, no longer
>   correctness-critical.
> - The `parent_run` foreign key on `AgentRun` is kept for lineage / auditing,
>   but stops gating prompt-rendering behavior. The `triggered_by` request /
>   scheduler parameter (`TRIGGER_TICK`, `TRIGGER_COMMENT_AND_RUN`, etc.;
>   not an `AgentRun` model field) keeps its bookkeeping role and is
>   unaffected.

## 1. Problem

The current ticking design treats follow-up runs as resumable conversations:

1. The first run on an issue gets the **full rendered template** (~3 KB across
   14 fragments) via `composer.build_first_turn`.
2. Every subsequent run — periodic ticks and Comment & Run alike — gets a
   **tiny continuation prompt** via `composer.build_continuation`: just the new
   human comments since the parent run started, or the literal sentinel
   `"(continuation requested with no new human input — proceed)"` when there
   are none.
3. The runner reattaches to the prior agent session via `thread/resume`
   (Codex) or `--resume` (Claude Code). This works only when the same runner
   is still online and still has the session locally; the model relies on its
   in-context memory of the original prompt to interpret the tiny new input.

This is correct on the happy path. There are three off-paths that range from
demonstrated to plausible-but-unmeasured. They are listed in descending order
of evidence strength.

### 1.1 Prompt-update staleness (demonstrated)

When we update a prompt fragment (e.g., PR #102 — task-type fork on
`code_change` vs `noncode`), the new template applies only on first runs.
Issues with active resumed sessions keep operating on the _original_ template
captured in their session memory, indefinitely. There is no signal in the
runtime that the agent is running on a stale prompt; the user sees the issue
behaving the way it did before the fix and assumes the fix didn't ship.

This is verifiable today by inspecting the dispatch path: `build_continuation`
returns at most the new comment text, so any in-flight issue is held to the
template version it was first dispatched with. For a system whose behavior is
largely defined by the prompt and that we expect to iterate on continuously,
this is a serious feedback-loop problem and the strongest single justification
for this design.

### 1.2 Failover-induced silent context loss (plausible, no observed incident yet)

Pi Dash's pod model lets any runner in the pod pick up an unpinned run.
`_pinned_runner_for` returns `None` when the original runner is offline,
revoked, or has lost its session. The new runner has no local agent session
for the captured `thread_id`. It gets handed the 50-byte continuation prompt
and launches a fresh agent process with effectively no instructions. The
agent either no-ops, hallucinates a task, or makes a wrong move. From the
user's perspective the issue silently stops progressing.

We have not yet seen a confirmed incident in production, but the architecture
admits this failure silently — there is no "session not found" signal that
would surface it. The Phase 0 instrumentation in §6.0 below adds the metrics
needed to quantify how often this is hitting users today; the design proceeds
on the assumption that even an occasional silent stall is unacceptable for
the product contract in §2.

### 1.3 Monotonic context growth (theoretically real, magnitude unmeasured)

By tick N, the resumed conversation has accumulated `N-1` prior turn outputs
in context — workpad edits, tool results, file reads, internal reasoning.
With a 24-tick cap (3 days at the default 3h cadence), an agent making a
decision on tick 24 is doing so against a longer context than tick 1. Models
exhibit "lost in the middle" degradation in long contexts; we should expect
some quality variance across an issue's lifetime, though the magnitude at the
24-tick scale has not been measured for this workload specifically.

The token-cost framing ("resume saves tokens") may also weaken under
inspection — modern API providers cache stable system-prompt prefixes, which
would absorb most of the cost of fresh-each-tick. Whether the agent runtime
in use (Codex CLI, Claude Code) actually exposes that caching is to be
confirmed during implementation; see §7.4.

## 2. Product goal (the contract that actually matters)

Pi Dash's user-facing promise is simple. A human creates an issue. An agent
works it end-to-end when it can. When it can't (genuine ambiguity, missing
auth, blocked dependency), it posts a focused comment and waits. When the
human responds, the agent picks up. The user sees an issue moving toward
done, or a clear question they need to answer.

What the user does **not** see and does **not** care about:

- Whether tick N's agent is "the same agent process" as tick N-1's.
- Whether session memory persists across ticks.
- Whether tokens are saved by reusing a thread.

What the user **does** see — and where the current design breaks the contract:

- A prompt fix the team shipped yesterday hasn't taken effect on this issue.
- (Latent) The agent silently stops working after a runner failover.
- (Latent) The agent on a late tick makes a worse decision than on an early
  tick because the context tail is full of stale workpad drafts and old tool
  calls.

The architectural primitive that delivers the product contract is **durable
externally-visible state**, not in-process session memory. State that lives
in the issue thread, the workpad comment, and the repo survives ticks,
runner churn, prompt updates, and pod rebalancing. State that lives in an
agent session does not.

## 3. Proposal

**Every agent run is a fresh agent session with a freshly rendered full
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

A fresh agent process given the full template plus access to these three
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
tiny prompt — is a few KB of (likely cached) tokens per click. Negligible
compared to the cost of a single agent turn.

## 4. What changes

### 4.1 Pi Dash (Python)

| File                                 | Change                                                                                                                                                                                                                                                                                                                                                                                         |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `prompting/composer.py`              | Delete `build_continuation`. Keep `build_first_turn` as the sole entrypoint. Optionally rename to `build_run_prompt` to retire the "first turn" framing (deferred to a follow-up).                                                                                                                                                                                                             |
| `orchestration/service.py`           | `_create_continuation_run` calls `build_first_turn`. Once this lands, it can be merged into `_create_and_dispatch_run` (the bodies become near-identical aside from the `parent_run` field). Consider renaming to `_create_run` and parameterizing `parent_run`.                                                                                                                               |
| `runner/services/matcher.py`         | Delete `_refresh_continuation_prompt`. The dispatch path no longer needs to rebuild the prompt at dispatch time; the prompt rendered at run-creation time is the final prompt.                                                                                                                                                                                                                 |
| `runner/views/runs.py`               | The Comment & Run handler (`_post_comment_and_run`) no longer needs to flag the run as a "continuation" for prompt-rendering purposes. The `triggered_by` request parameter is still parsed and forwarded for telemetry / scheduler bookkeeping.                                                                                                                                               |
| `prompting/fragments/14_followup.md` | Folded into fragment 08's workpad reconciliation step (see §4.3). The standalone fragment is removed.                                                                                                                                                                                                                                                                                          |
| Tests                                | `test_build_continuation_*` tests in `tests/unit/prompting/test_composer.py` are removed. `test_drain_does_not_rebuild_first_turn_prompt` and the matcher tests around `_refresh_continuation_prompt` flip their expectations or are removed. Add a positive test asserting `run.prompt == build_first_turn(issue, run)` after `_create_continuation_run` (or its renamed equivalent) returns. |

### 4.2 Runner (Rust)

| Area                                                     | Change                                                                                                                                                                                                     |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Codex bridge (`runner/src/codex/bridge.rs`)              | Remove the `thread/resume` JSON-RPC call path; always start a fresh thread via `thread/start`. Drop the `if let Some(thread_id) = …` resume branch.                                                        |
| Claude Code bridge (`runner/src/claude_code/process.rs`) | Drop the `--resume <session_id>` argv injection. Always launch fresh.                                                                                                                                      |
| Protocol (`runner/src/protocol.rs` or similar)           | Remove `resume_thread_id` field from `ServerMsg::Assign` and `RunPayload`. Update test fixtures (`runner/tests/protocol_roundtrip.rs`, `codex_bridge_fake.rs`, `claude_bridge_fake.rs`) to drop the field. |
| Supervisor plumbing                                      | Remove parameter passing of `resume_thread_id` through internal function signatures.                                                                                                                       |
| Pinning / queue selection                                | Unchanged in shape — still prefer the previously-pinned runner. The reason changes (repo locality, not session locality), but the mechanism is the same.                                                   |

### 4.3 Prompt fragments

Most existing fragments are already written for fresh-each-tick because they
tell the agent to read durable state via `pidash` CLI calls. Specific updates:

- **Fragment 14** — currently positioned as "follow-up run context (attempt
  count > 1)". Its "read the existing workpad end-to-end and resume from there"
  rule is now the steady-state every-non-first-run rule, not a recovery-only
  rule. **Decision: fold its content into fragment 08 step 2** (which already
  performs workpad reconciliation when one is found) and delete the standalone
  fragment. The fragment 14 framing as a "special case" is itself a relic of
  the resume-is-default mental model that this design rejects.
- **Fragment 07** — Step 0.5 step 1 already says "Pull all prior comments via
  `pidash comment list`." Reinforce: the comment list is authoritative; do
  not assume any prior comment is already in your context.
- **Fragment 04** — Default posture mentions "Maintain exactly one
  `## Agent Workpad` comment as your source of truth." Strengthen to:
  "The workpad is the only carrier of state between runs. Anything not
  written to the workpad before this run exits is lost — the next run starts
  from a fresh agent session with no memory of this one."

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
- The `triggered_by` request / scheduler parameter (`TRIGGER_TICK`,
  `TRIGGER_COMMENT_AND_RUN`, etc., used in `runner/views/runs.py` and
  `orchestration/scheduling.py`). It still drives ticker bookkeeping and
  Comment & Run scheduler resets. Trigger source is no longer involved in
  prompt or session semantics — every trigger results in the same
  `build_first_turn` render and a fresh agent session.
- All trigger sources — initial delegation, periodic tick, Comment & Run
  button. They differ in _when_ a new `AgentRun` is created; they no longer
  differ in _how_ it is prompted.

## 6. Migration and rollout

This change is observably behavior-changing for any issue with a live
resumed session at deploy time.

### 6.0 Phase 0 — Pre-rollout instrumentation (recommended)

Before merging the load-bearing change, add lightweight cloud-side
observability so we have a baseline to compare against. Restrict to fields
that are cheap to record at dispatch time without round-tripping the runner:

- Per-`AgentRun` log/metric: `(triggered_by, parent_run_id is None,
pinned_runner_resolved, thread_id_supplied)`. This lets us count how
  often the cloud expected resume to be possible (`thread_id_supplied`)
  versus how often the pinned runner was actually available
  (`pinned_runner_resolved`). The gap between those two is the cloud-side
  proxy for failover rate.
- Per-`AgentRun` outcome (post-run): did the agent post a new comment?
  touch the repo? update the workpad? A run that started after a `paused`
  parent and produced no observable side-effect is a soft signal of
  context loss.
- Whether the runner _actually used_ the resume hint is not cheaply
  knowable cloud-side today (the runner discovers this by trying resume
  and reporting `resume_unavailable`). The cloud-side fields above are
  the affordable proxy; richer telemetry would require a new runner
  message and is out of scope for this design.

How long to run in baseline mode is a judgment call, not a correctness
requirement — even a few days of data is useful for "before/after" claims.
In a hurry, Phase 0 can be skipped entirely; the design's correctness
argument does not depend on it. The cost is forfeiting numerical claims
in any future regression debate.

### 6.1 Phase 1 — Pi Dash side

Land the Pi Dash changes:

1. `composer.build_continuation` removal.
2. `_create_continuation_run` switches to `build_first_turn`.
3. **`_build_assign_msg` stops emitting `resume_thread_id` in the WS
   envelope.** This is the load-bearing safety property of Phase 1: the
   runner's resume code path can remain intact, but it cannot fire because
   no thread id is delivered.
4. `_refresh_continuation_prompt` deletion + dispatch-time call sites cleaned.
5. Test updates.

The §8.1 "send the full template into a resumed session" hybrid is **not**
an intermediate state of this rollout, because step 3 above prevents resume
from happening at all. With `resume_thread_id` absent from `Assign`, the
runner's `payload.resume_thread_id` deserializes to `None` (via
`#[serde(default)]`) and both the Codex and Claude bridges fall through to
their fresh-session paths. Phase 1 is therefore safe to ship without Phase
2 — the runner's resume code becomes unreachable dead code, not an
active-but-redundant alternate path.

### 6.2 Phase 2 — Runner side

Drop `thread/resume` (Codex bridge) and `--resume <session_id>` (Claude
Code bridge). Remove the `resume_thread_id` field from `ServerMsg::Assign`,
`RunPayload`, and supervisor plumbing. Pure dead-code removal on top of
Phase 1.

### 6.3 Spot-check protocol

At each phase, spot-check 3–5 in-flight issues. Concrete checklist for each:

- Does the workpad's `### Plan` describe the remaining work in enough detail
  that a fresh agent could pick it up without re-doing investigation?
- Are `### Acceptance Criteria` and `### Validation` populated with concrete
  items (not "(extracted from comments)" placeholders)?
- Does `### Notes` capture any non-obvious decisions or constraints discovered
  in this issue's prior runs?
- If the issue is currently `safe_to_continue: false`, does `Question for
human` clearly state what's blocking, and is the `### Notes` section honest
  about what was tried?
- If the agent recovered from a workpad on the previous tick, did it produce
  a coherent next step or a confused one?

Any "no" on the first three items is a workpad-discipline gap and should be
addressed in the prompt fragments before further rollout — not by reverting
to the resume path.

### 6.4 Expected user-visible effects post-rollout

- Prompt fixes take effect on next tick, not "next time the issue restarts."
- (When failover does fire) the agent picks up the work via the workpad
  instead of stalling.
- Per-tick agent decisions become reproducible from `(workpad@T, comments@T,
repo@T, current template)`.

No data migration. No state-format change. No downtime.

## 7. Risks and open questions

### 7.1 Workpad-discipline gaps may surface (highest-priority risk)

If any current prompt path lets the agent leave important state in
session-memory only, that issue will regress: the next tick won't find the
state in the workpad and may redo work or get confused. This is the single
most consequential risk because if the workpad is not actually sufficient
state, the design fails.

**Workpad completeness contract.** Before exiting any run, the agent must
have populated:

- `### Phase` set to a current value (not stale from a prior run).
- `### Progress Checkpoints` reflect what's actually done in the repo, with
  any inapplicable items marked `n/a`.
- `### Analysis` populated (Restated problem, Acceptance criteria, Proposed
  approach, Task type, Risks/assumptions, Decision).
- `### Plan` is the current plan, with checked-off items reflecting current
  reality.
- If the agent is exiting in a blocked state: `### Autonomy / Escalation`
  has `safe_to_continue: false` and `Question for human` set to a specific
  question (not `null`).
- `### Notes` is updated with anything material learned this run that future
  runs need to know.

This contract is enforced at the prompt level: fragment 08 (workpad setup)
and fragment 13 (ending the run) should each end with an explicit checklist
the agent verifies before exiting.

**Pre-rollout audit.** Before Phase 1, grep all 14 fragments for verbs that
imply the agent has session memory of prior turns: "remember", "earlier",
"recall", "as you saw before", "your previous", "you already". Each hit is
either rewritten to point at the workpad/comments/repo as the source of
truth, or marked as a true intra-run reference (within a single dispatch,
where session memory is fine).

### 7.2 Larger per-tick cost

Without resume, every tick re-renders ~3 KB and re-pays the system-prompt
tokens. Whether the agent runtime caches this depends on its provider (see
§7.4). Even without caching, the cost is bounded — 24 ticks × ~3KB = ~72KB
of duplicated prompt over an issue's full lifetime — and is dwarfed by the
cost of any single non-trivial agent turn. Acceptable trade for the
correctness wins.

### 7.3 Loss of "warm" agent sessions

Some users may have been benefiting from short Comment & Run loops where the
session was still warm with file reads etc. They'll now pay one extra
file-read per click. If this turns out to be a real complaint, it can be
mitigated by keeping a short-lived agent session warm on the runner side
without using it for prompt continuity (i.e., the prompt is still full and
self-sufficient). Defer until there's evidence of a problem.

### 7.4 Agent runtime semantics

This design assumes:

- A fresh `codex app-server` session (or fresh `claude` invocation) without
  a resume hint produces a clean session bound to a fresh `thread_id` /
  `session_id`.
- The full rendered template is the initial input.
- Captured `thread_id` is unique per `AgentRun` and not reused across runs.

These match the runner's current behavior on first runs. Confirm during
implementation that nothing in the runner side implicitly depends on
`thread_id` reuse for non-resume reasons (e.g., logging, telemetry).

Whether the underlying API path leverages prompt caching for the static
system prompt is provider-dependent; it does not affect the design's
correctness, only its cost profile. If caching is not in play, §7.2's cost
estimate is the hard ceiling, which remains acceptable.

### 7.5 Stale `parent_run` interpretation

`AgentRun.parent_run` is currently a load-bearing pointer for resume. Once
resume is gone, downstream code that branches on `parent_run is not None`
should be audited — most of it is fine (lineage, audit), but anything that
infers "this is a resumable continuation" from `parent_run` is now wrong.

### 7.6 Mid-run crash leaves a partial workpad

If a run crashes after partially updating the workpad (or after pushing a
commit but before recording the push in the workpad), the next tick reads
inconsistent state. This is a workpad-protocol issue independent of resume:
it exists today and would exist under any state model.

The new design is arguably _better_ for this case because there is no
in-context session memory masking the inconsistency — the next tick reads
the durable artifacts (workpad + comments + repo) and any inconsistency is
visible and resolvable. Fragment 08 step 2 ("reconcile the workpad before
editing further") is the existing place that handles this; reinforce it
during the audit in §7.1.

### 7.7 Rust runner protocol field removal

Removing `resume_thread_id` from `ServerMsg::Assign` is a wire-format change.
Pi Dash and the runner must deploy in lockstep, or one side must tolerate the
field's presence/absence during the rollout window. The simplest approach is
to land Phase 1 (Pi Dash sends a self-sufficient prompt but still populates
`resume_thread_id` for any runner that expects it) and Phase 2 (runner stops
reading it; Pi Dash stops sending it) as separate deploys with a backwards-
compatible window in between. Within this PR, the field is removed in the
same change to avoid drift; the deploy ordering must put Pi Dash first.

## 8. Alternatives considered

### 8.1 Keep resume; render full template every tick on top of resume

Sends the full template as a new user message into the resumed conversation.
Combines the worst of both designs: the prompt-cache prefix is broken
(template appears at varying conversation positions), instruction-duplication
ambiguity arises (the model sees "Step 0.5 — analyze and decide" twice and
has to guess which is authoritative), and monotonic context growth is
unaffected. Rejected.

### 8.2 Keep resume for ticks, full re-render only on detected failover

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

Suggested split. The ordering is **safety-critical**: Phase 1 must include
dropping `resume_thread_id` from the assign envelope so the runner cannot
resume — leaving the runner's resume code paths intact while Pi Dash sends
the full template would land us in the §8.1 hybrid (template re-sent into a
resumed session, with instruction-duplication ambiguity and unbroken
context growth). With Phase 1 dropping the field, Phase 2 is dead-code
removal on top of an already-correct system.

1. **PR A — Pi Dash side.** Composer cleanup, orchestration switch,
   matcher simplification (including dropping `resume_thread_id` from
   `_build_assign_msg`), prompt fragment refresh (fold fragment 14 into
   fragment 08, strengthen fragment 04), test updates. Self-contained
   behavior change: every follow-up run now ships a self-sufficient prompt
   AND no resume hint, so any current runner deserializes
   `resume_thread_id` to `None` and starts a fresh session. **This PR
   alone fixes the prompt-staleness bug AND makes Phase 2 pure cleanup.**
2. **PR B — Runner side.** Delete the now-dead `thread/resume` /
   `--resume` code paths and the `resume_thread_id` field from
   `ServerMsg::Assign`, `RunPayload`, and supervisor plumbing.

This PR bundles A and B together since the runner change is small (~60–80
lines) and the deploy is straightforward when the changes ship together.
The two-phase framing remains useful as a mental model for the safety
order; the implementation reflects "Phase 1 must include dropping
`resume_thread_id`" by removing it from the assign builder in the same
commit as the composer change.

Each commit within the PR is independently revertible.
