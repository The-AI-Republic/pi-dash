# Issue Run Continuation — Multi-Run Conversations Per Issue

> Directory: `.ai_design/issue_run_improve/`
>
> **Status:** discussion / pre-implementation. No code changes yet.
>
> **Scope:** how an issue continues across more than one `AgentRun` when the
> first run yields without finishing — and what that implies for run lifecycle,
> conversation continuity, dispatch routing, and user UX.
>
> **Related work**
>
> - `.ai_design/issue_runner/design.md` — pods, runners, the queue model, the
>   run-identity split (creator vs. billable owner). This doc builds on top of
>   that one and assumes its decisions.

## 1. Problem

Today, on `main` (commit `a99e736`):

- An issue moved to **In Progress** auto-creates an `AgentRun` and dispatches
  it to a runner via the pod queue (`orchestration/signals.py`,
  `orchestration/service.py`, `runner/services/matcher.py`).
- The runner spawns a fresh `claude --print …` (or codex equivalent) for every
  run. Each run is a brand-new agent session.
- When that run terminates — for any reason — the agent's working memory is
  gone. There is no native session resume, no transcript replay, no thread
  continuation across runs.
- A user comment on an issue is **not** a signal: nothing wakes the agent, no
  follow-up run is created.

The data model already partially supports continuation:

- `AgentRun.thread_id` (`runner/models.py:363`) stores the provider's session
  id (Claude `session_id` / Codex `thread_id`) when the runner reports it
  via `run_started` (`consumers.py:374-379`).
- `AgentRun.parent_run` (`runner/models.py:346-352`) chains follow-up runs.
  `orchestration/service.py:103` sets it to the latest prior run when a new
  run is created for an issue that already has one.
- The runner `RunPayload` struct has a `resume_thread_id: Option<String>` field
  (`agent/mod.rs:54`) but the cloud never populates it.
- Codex bridge supports `resume_thread()` (`runner/src/codex/bridge.rs:61-69`).
  The Claude Code path has no `--resume` plumbing.

So everything is wired *for storage* but nothing *consumes* it. This doc
proposes how to wire the consumption side.

## 2. Goal

For an issue whose work spans multiple agent runs, give the agent enough
context on each follow-up run that it can continue meaningfully — while
keeping the dispatch model (workspace pod queue, fair scheduling, runner
ownership) intact.

Non-goals:

- Multi-agent collaboration on the same issue (one conversation per chain).
- Cross-issue memory (a runner's recall of issue A is not available on issue
  B; that's a separate, larger problem).
- Long-term memory beyond a single issue's lifecycle.

## 3. The Three Sub-Problems

This problem decomposes cleanly into three independent decisions. Each can be
chosen separately; each has its own MVP/full-featured spectrum.

| #   | Question                                                          | Where the design lives |
| --- | ----------------------------------------------------------------- | ---------------------- |
| A   | What does it mean for a run to "yield" instead of "finish"?       | §4 — Run Lifecycle     |
| B   | What triggers a follow-up run?                                    | §5 — Continuation Triggers |
| C   | How does the follow-up run inherit the prior run's context?       | §6 — Conversation Continuity |

A separate question — whether to introduce an `IssueConversation` entity that
owns the thread of runs — is covered in §7.

## 4. Run Lifecycle: Yield vs. Finish

### 4.1 Today's terminal states

`AgentRunStatus` (`runner/models.py:101`) currently distinguishes
`QUEUED → ASSIGNED → RUNNING → SUCCEEDED | FAILED | CANCELLED | TIMED_OUT`.
All four terminals are flat: "done, don't expect more."

This is too coarse. An agent can stop without having finished, in several
materially different ways:

| Termination kind          | What it means                                              | Should follow-up auto-trigger? |
| ------------------------- | ---------------------------------------------------------- | ------------------------------ |
| Completed cleanly         | Agent reports done; the work is finished                   | No                             |
| Yielded — needs input     | Agent emits a structured "I need X from the human" signal  | Yes, when the human responds   |
| Yielded — needs approval  | Agent paused on an approval gate                           | Yes, when approval resolves    |
| Hit budget cap            | Run exceeded a token / wall-clock budget                   | Optional — policy decision     |
| Crashed / disconnected    | Runner died, lease expired, network gone                   | Maybe — recovery is intra-run  |
| User cancelled            | A human stopped the run                                    | No                             |

### 4.2 Proposed lifecycle additions

Introduce an explicit **paused / awaiting** state distinct from terminal:

- `PAUSED_AWAITING_INPUT` — agent yielded cleanly with a question for the
  human; no further dispatch until a continuation trigger fires.
- `PAUSED_AWAITING_APPROVAL` — already partially modeled via the approvals
  table; promote to a first-class run status so the matcher's "non-terminal"
  predicate is honest.

`PAUSED_*` is **not terminal**. The run holds its lease, retains its
`thread_id`, and is the natural attachment point for the follow-up.

### 4.3 The "I'm yielding" protocol

For `PAUSED_AWAITING_INPUT` to exist, the *agent* needs a way to emit a clean
yield. Three options, in increasing order of integration cost:

1. **Heuristic on output** — sniff the agent's final stdout for "needs input"
   markers. Cheap, brittle, fights the model.
2. **Tool call** — give the agent a `request_user_input` tool. The runner sees
   the tool call and reports it as a yield to the cloud. Aligned with how
   approvals already work.
3. **Prompt protocol** — instruct the agent (in the system prompt) to end its
   final turn with a structured JSON envelope (`{"status": "yielded",
   "reason": "...", "question": "..."}`). The runner parses it.

Recommendation: **(2) tool call**. It piggybacks on the approval path's
existing event plumbing, and tool calls are naturally first-class in both
Claude Code and Codex SDKs.

## 5. Continuation Triggers

### 5.1 Candidate triggers

| Trigger                                       | Carries new info? | UX feel             | Implementation cost                 |
| --------------------------------------------- | ----------------- | ------------------- | ----------------------------------- |
| User comment on the issue                     | Yes               | Conversational      | New `IssueComment` post_save signal |
| Issue state transition back to In Progress    | No                | Coarse, button-y    | Already wired                       |
| Explicit "Continue" / "Reply" action          | Optional          | Explicit, button-y  | New endpoint + UI                   |
| Approval grant (resumes paused run)           | No                | Invisible           | Already partially wired             |
| Scheduled retry after timeout                 | No                | System-driven       | New scheduler                       |

### 5.2 Recommendation

Make **comment-triggered continuation** the primary user path. It maps to the
mental model the user already has ("I just leave a comment and the agent will
read it"), and it carries the new information (the comment body) inline.

Add **explicit "Continue"** as a secondary path for cases where the user has
nothing to add but wants the agent to keep going (e.g., agent yielded on a
budget cap; user accepts the additional spend).

Treat **approval grant** as intra-run, not a new run — the run was holding its
lease the whole time. This is already most of the way there.

### 5.3 Comment-triggered flow (sketch)

```
User adds comment to Issue I
        │
        ▼
post_save(IssueComment)
        │
        ├─► Skip if author is a runner / agent (avoid self-trigger loops)
        ├─► Skip if no prior AgentRun for I (no conversation to continue)
        ├─► Find latest AgentRun R_prev for I
        │     │
        │     ├─► R_prev is PAUSED_*    → create R_next with parent_run=R_prev
        │     ├─► R_prev is terminal    → create R_next with parent_run=R_prev (resume a finished issue)
        │     └─► R_prev is RUNNING     → append comment to live run via WS frame, do NOT create R_next
        │
        ▼
R_next dispatched through normal pod queue
```

### 5.4 Race conditions to think about

- **Two comments before R_next dispatches.** Coalesce: the dispatcher reads
  *all* comments since `R_prev.completed_at` at dispatch time. Don't create
  one R_next per comment.
- **Comment arrives while R_prev is still RUNNING.** Don't queue a new run.
  Either (a) push the comment to the live run as an inbound user message via
  the WS bridge, or (b) hold the comment until the run terminates and then
  dispatch. Option (a) is closer to the desired UX but requires runner-side
  support for mid-run inbound messages.
- **Agent comments on its own issue.** Filter by author type: if the comment
  is an agent-authored comment (logged on behalf of an `AgentRun`), do not
  re-trigger.

## 6. Conversation Continuity

This is the hardest of the three sub-problems. The follow-up run needs the
prior run's context. Three mechanisms.

### 6.1 Mechanism A — Native provider-side resume

`claude --resume <session_id>` (Claude Code) and `resume_thread(...)` (Codex)
let the provider runtime re-attach to a previous session.

**Pros**

- Zero token overhead — no transcript replay.
- Perfect memory — the agent has its full prior turn-by-turn state.
- Simplest semantics; trivially supports tool/file state if the provider
  supports it.

**Cons**

- **Locality.** Claude Code stores its session on the local disk of the runner
  process. The follow-up run *must* hit the same runner, and that runner's
  session storage must still be intact (no restart, no GC, no reinstall).
- **Couples runs to runner identity** — fights the pod queue's "any runner in
  the pod" model.
- **Provider-specific.** Codex resume works differently; cross-provider runs
  on the same conversation are out of scope.

### 6.2 Mechanism B — Cloud transcript replay

We already capture every meaningful event into `AgentRunEvent`
(`runner/models.py:424`). On a follow-up run we synthesize a system-prompt
preamble that summarizes — or replays — the prior run's user/assistant turns,
and start a *fresh* provider session.

**Pros**

- Provider-agnostic.
- Runner-agnostic — the follow-up can land on any idle runner in the pod.
- Durable — the cloud is the source of truth.
- Auditable — the same transcript drives both replay and human review.

**Cons**

- Token cost on every continuation. For long conversations this compounds.
- Tool/file state not perfectly restored. The agent "remembers" what it did
  but doesn't have the same in-memory caches, file handles, etc.
- Replay quality is sensitive to how we summarize. Naive concat of all events
  blows the context window quickly.

### 6.3 Mechanism C — Hybrid

Try (A) first; fall back to (B) when (A) is unavailable.

```
follow-up run dispatched
        │
        ▼
Was R_prev assigned to a runner that is still online & in the pod?
        ├─ Yes → set resume_thread_id on RunPayload; runner attempts native resume
        │           ├─ Native resume succeeds → done
        │           └─ Native resume fails    → fall back to replay path
        └─ No  → skip native; use replay path
```

This is what we should ship for v1. It captures the cheap-and-fast common case
(same runner is still around) without sacrificing correctness when the runner
is gone.

### 6.4 Pinning policy

Hybrid raises a real scheduling question: how hard do we *try* to keep the
follow-up on the same runner?

Three positions:

- **Strict pin.** Follow-up run is exclusively dispatchable to the original
  runner. If that runner is busy, the run waits. Maximum native-resume hit
  rate; worst queue fairness; users wait longer.
- **Soft pin (recommended).** Prefer the original runner; wait up to N seconds
  (e.g., 30s) for it to free up; then release the pin and dispatch to any
  runner using the replay path. Tunable knob.
- **No pin.** Always dispatch to whoever is free; native resume is just a
  performance hint that succeeds opportunistically. Best fairness; loses the
  native-resume benefit when runners are busy.

Soft pin is the right default because it aligns with the digital-employee
model: "I'd like the same person who worked on this last time, but I'd rather
make progress than wait forever."

### 6.5 Where the resume hint lives

Two storage options:

1. Read at dispatch time from `parent_run.thread_id` and `parent_run.runner_id`.
2. Promote to a first-class field on the new run: `resume_thread_id`,
   `pinned_runner_id`.

Option 1 keeps the model lean (no new columns), at the cost of always walking
the chain. Option 2 lets us snapshot the resume hint once and stop caring
about whether it's still derivable later (e.g., if `parent_run.runner_id` was
nulled out for some reason).

Recommendation: **Option 1 for MVP**, promote to Option 2 if we find ourselves
patching the same data lookup in three places.

## 7. Optional: An `IssueConversation` Entity

Today, "the conversation on an issue" is implicit — it's the chain of
`AgentRun` rows whose `parent_run` walks back to a root.

A first-class `IssueConversation` (or `WorkThread`) model could own:

- `issue_id` — the issue this conversation is about.
- `native_session_id` — the canonical provider session id (latest non-empty
  `thread_id` in the chain).
- `pinned_runner_id` — soft affinity for native-resume.
- `head_run_id` — the latest run in the chain.
- `status` — `active` (work continuing), `closed` (done), `abandoned`.

### 7.1 Why we'd add it

- **Multiple conversations per issue.** "I want to start over" or "let me ask a
  side question" become natural — each gets its own conversation. The walk-
  the-parent_run-chain model has no place for branching.
- **Cleaner ownership.** Pinning, transcript caching, and the resume hint all
  live in one place instead of being recomputed from the chain.
- **Cleaner permissions.** Conversation-level subscribe / mute / reassign
  actions become possible.

### 7.2 Why we'd skip it (for now)

- The chain model is sufficient for one-conversation-per-issue. Promoting now
  is speculative.
- New entity = new migrations, new permission rules, new API surface.

Recommendation: **defer.** Walk-the-chain works for v1. Promote to
`IssueConversation` only if the multi-thread-per-issue use case becomes real.

## 8. Recommended MVP Shape

This is the minimum coherent slice that delivers the user-visible benefit.

1. **Run lifecycle** (§4)
   - Add `AgentRunStatus.PAUSED_AWAITING_INPUT` (and `PAUSED_AWAITING_APPROVAL`
     if not already represented as a status).
   - Define the `request_user_input` tool that the agent calls to yield.
   - Runner reports the yield to the cloud; cloud transitions `R_prev` to
     `PAUSED_AWAITING_INPUT` and stores the agent's question in `R_prev.done_payload`.

2. **Continuation trigger** (§5)
   - Add `post_save` on `IssueComment`:
     - Skip agent-authored comments.
     - Find latest `AgentRun` for the issue.
     - If `RUNNING`: forward the comment to the live run via WS (out of MVP
       scope — okay to defer and queue instead).
     - If `PAUSED_*` or terminal: create a new `AgentRun` with `parent_run`
       set, dispatch through the normal pod queue.
   - Add `POST /api/v1/runner/runs/<id>/continue/` endpoint as the explicit
     button-driven path.

3. **Conversation continuity — hybrid** (§6)
   - At dispatch, the cloud computes `resume_thread_id = parent_run.thread_id`
     and `preferred_runner_id = parent_run.runner_id`.
   - Matcher implements **soft pin**: prefer `preferred_runner_id`, wait up to
     `RUN_PIN_GRACE_SECONDS` (default 30), then release.
   - Runner side: if `resume_thread_id` is set, attempt native resume first
     (Claude Code: pass `--resume`; Codex: call `resume_thread()`). On failure,
     fall back to replay.
   - Replay path: load `R_prev` events, render a transcript preamble
     (compact format — user turns, assistant turns, key tool calls, dropping
     internal scratchpad), prepend to the new run's prompt.

4. **Defer**
   - `IssueConversation` entity (§7).
   - Mid-run inbound user messages (forwarding a comment into a `RUNNING`
     run). Workaround: queue the comment, dispatch on terminate.
   - Cross-issue / cross-conversation memory.

## 9. Open Questions

| #   | Question                                                                                                                                         |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| Q1  | What's the right TTL on a `PAUSED_AWAITING_INPUT` run? After 7 days with no comment, do we GC it to terminal?                                    |
| Q2  | Replay format: full turn-by-turn vs. summarize-the-prior-run? At what conversation length do we switch?                                          |
| Q3  | Can we forward an inline comment into a `RUNNING` run without making the runner stateful in dangerous ways? Or do we always wait for terminate? |
| Q4  | Soft-pin grace window: 30s reasonable? Tunable per pod?                                                                                          |
| Q5  | If native resume succeeds but the resumed agent's working directory was wiped (runner reinstalled), the agent has memory but no files. How do we detect and handle? |
| Q6  | Do we expose conversation history (the run chain) as a first-class API for the UI's issue panel, or keep it as `issue.agent_runs.order_by(...)`? |
| Q7  | Billing: a follow-up run costs tokens; who pays? Same `runner.owner` semantics as the primary run, or attribute to the comment author?           |
| Q8  | If `IssueComment` triggers a run but the issue is no longer in `In Progress` (user moved it back to Backlog), do we still dispatch? Probably not — define the gating rule. |

## 10. Summary of Tradeoffs

The central tradeoff is **runner affinity vs. queue fairness**. Native resume
is dramatically cheaper and higher-fidelity, but it ties a run to a specific
runner; the pod queue's whole point is that any runner can pick up any work.

Soft pin with replay fallback is the pragmatic middle: we capture the common-
case win (the original runner is usually still around) and degrade
gracefully when it isn't.

The other meaningful tradeoff is **trigger surface**. Comment-triggered
continuation matches user intent best but introduces an implicit dispatch
path, which is harder to reason about than an explicit button. Shipping both
gives users a fallback for surprising cases.
