# Pi Dash agent-run token efficiency: design v4 (final, after critique round 3)

Repository: The-AI-Republic/pi-dash at main `9cc85ccd0` (after the agent-naming refactor, PR #3261). Paths are relative to the repo root and all file:line references are to that commit. This document follows AGENTS.md "Agent vocabulary": the native Codex-based agent is the **Pi Dash Agent**; third-party agents are Codex, Claude Code, Cursor Agent, Muse Code, Grok and OpenClaw. Stored and wire values (`engine`, `managed_runner`) keep their spelling.

v4 is the only implementation reference. It does not depend on v1, v2 or v3 for any content; earlier versions are kept only as history. No further review round will run, so every remaining choice that needs a person is listed in "Decisions for the owner" at the end of the appendix.

Owner's direction for this pass: streamlined but safe. Every saving is activated through one path: a live canary that randomizes **tickets** (not pods or workspaces) with a sticky deterministic hash, is evaluated intention-to-treat over every enrolled ticket, and becomes default-on only after the decision rule that Richard recorded before the canary started passes. An emergency stop contains a failed canary, including runs that are already queued or running.

**Deviation from the brief.** The brief asks for "flags, A/B on replayed tickets" in rollout. v4 replaces replayed-ticket A/B with live per-ticket canaries against concurrent controls, as an owner-requested streamlining. The replay harness is kept as Later item L-01 with its requirements written down. This is a requirements change, so it is the first item in "Decisions for the owner": Vinh confirms it or asks for L-01 to come back into the core.

## 0. Critique round 3 dispositions

Codex raised 8 MUST and 3 SHOULD findings in round 3. Every code claim was checked at `9cc85ccd0` and held; in addition, this pass found that a run can be re-queued in place and assigned again (`apply_run_resume_unavailable`, `apps/api/pi_dash/runner/services/run_lifecycle.py:247-295`) and that reconnect redelivery rebuilds the assign frame from the run (`runner/services/session_service.py:474-525`; Rust `services/src/runner_sessions/session_service.rs:1427`), which strengthens MUST 5. All 11 findings are accepted. None is rejected. The fixes follow the owner's direction and pick the simplest safe option where Codex offered two.

| # | Finding | Verified | Disposition | What changed in v4 |
|---|---|---|---|---|
| M1 | Comparison is not intention-to-treat; treatment mismatches and missing reports drop whole tickets | yes: v3 5.1.5 required `treatment_match = true` for a complete ticket | Accept | Outcome metrics (merged rate, guardrails, sendbacks, blocked runs) are computed over every enrolled ticket. Treatment compliance is reported separately and never filters. If any enrolled ticket lacks a known cost, the economic verdict is "unavailable"; there is no survivor-only comparison. If any enrolled ticket has an unknown merge outcome, the merged-rate verdict is "unavailable". A run that ended before its agent made any model request counts zero tokens as `reported` (5.1.2), so early failures do not make cost unknown. Section 5.1.5; test `test_treatment_failure_cannot_improve_canary_by_excluding_ticket` (T-11b). |
| M2 | Inference ignores the assignment unit (pod or workspace clusters, ticket-level intervals) | yes: v3 assigned by `sha256(experiment_id + unit_id)` with pod or workspace units | Accept, option "randomize by ticket" | The unit is the ticket. Arm = `canary` when the first 8 hex digits of `sha256(experiment_id + ":" + issue_id)`, as an integer mod 10,000, are below the share in basis points. The share is immutable for an experiment, so the arm is a pure function of experiment and ticket and is also stored in each run's manifest. Pod moves, profile changes and reassignment cannot change it. Ticket-level intervals are then valid. Pod and workspace units are removed. Test `test_ticket_arm_is_sticky_hash_independent_of_pod_and_profile` (T-10). |
| M3 | Rate model omits cache-write pricing | yes: `runner/src/daemon/observability.rs:170-178` folds cache reads and writes into `input`; v3's table had no write rate | Accept | Four disjoint billable categories: uncached input, cache read, cache write (with 5-minute and 1-hour tiers where the provider prices them differently) and output (reasoning is a subset of output and is never billed again). A missing tier split, or cache-write tokens on a model whose entry has no write rate, gives "cost unknown". The rate table is an append-only versioned file; every experiment records its `rate_table_version` before it starts and a closed report is computed with that version only. Section 5.1.4; test `test_cache_creation_is_charged_once_at_write_rate` (T-06a). |
| M4 | Step one cannot meet 40 KiB | yes: measured below | Accept, budgets revised to measured values | Baseline renders were measured for every recipe from the section files at `9cc85ccd0` (5.4.4). Exact extraction ranges are given by file and line. Measured results: coding-task (software) 91.9 KiB today, 64.4 KiB after step one, 55.3 KiB after step two. Budgets are set from these numbers (66, 35 and 41 KiB for coding, review and test after step one; 57 KiB for coding after step two). 20 KiB is not reachable by extraction alone, so it is dropped as a target and recorded as Later L-10 and owner decision O-2. T-18a adds baseline size and hash fixtures for every recipe before any marker is placed. |
| M5 | Assignment snapshot has no consistent redelivery rule | yes: `_build_assign_msg` takes only the run (`matcher.py:492-517`) and runs after the assignment transaction (`:276-284`, `:337`); Rust `plan_assignment` uses prompt and run config only (`drain.rs:550-558`); in-place requeue exists (`run_lifecycle.py:247-295`) | Accept | Delivery is decided per assignment attempt. In the assignment transaction the server derives the delivery from the immutable creation snapshot and the chosen runner's version, builds the full assign frame with a `delivery_attempt` number, and stores the exact frame in a new column `AgentRun.delivery`. The post-commit send and every redelivery send that stored frame byte for byte. A requeue closes the attempt into `delivery.history`; the next assignment is a new attempt and may derive a different delivery (an old runner gets the inline prompt). Section 5.4.3; T-18b (Django), T-18c (Rust); test `test_slim_assignment_reassigned_to_old_runner_gets_inline_with_correct_delivery_identity`. |
| M6 | Rollback does not contain a guardrail incident | yes: v3 rollback affected only new runs | Accept | Emergency stop, `canary_stop <experiment_id>`: marks the experiment stopped (new runs get baseline settings in both backends), both backends refuse to dispatch queued canary runs of a stopped experiment, queued canary runs are cancelled with code `experiment_stopped` and recreated with baseline settings, and running canary runs follow an explicit policy (cancel on a guardrail stop, finish otherwise). Attribution, arm and costs are preserved. Section 7.4; T-27a (Django), T-27b (Rust); test `test_failed_canary_prevents_queued_treatment_dispatch`. |
| M7 | T-11 and T-18 are not credible single PRs | yes | Accept | T-11 is split into T-11a (cohort selection), T-11b (evaluation) and T-11c (report command and closed-report pinning). T-18 is split into T-18a (extraction mechanism, composition and baseline fixtures, both backends), T-18b (Django dispatch integration with the per-attempt delivery record) and T-18c (Rust dispatch integration). T-06 is also split, into T-06a (rate model) and T-06b (ticket breakdown payload), because the rate model grew. Estimates were redone including fixtures and both backends' tests. Each ticket has its own dependencies and named failing test. |
| M8 | Live canaries silently replace the brief's replay A/B | yes: brief rollout asks for "A/B on replayed tickets" | Accept | The header states the deviation plainly, section 7 repeats it, it is owner decision O-1 for Vinh, and replay stays as Later L-01 with its requirements. v4 does not claim to satisfy the unchanged brief on this point. |
| S9 | Static-prefix promise too broad | yes: overrides that interpolate run values vary the band | Accept | The promise covers only runs without volatile overrides; with one, the stable prefix ends where the first volatile override begins, and the manifest records that byte offset as `stable_prefix_end`. Both cases are tested in T-17. |
| S10 | Existing guidance contradicts the digest | yes: `sections/analyze-and-scope.md:12` says the full comment thread is shown and the in-prompt snapshot is authoritative | Accept | T-23 adds a digest-mode wording of that step (in both mirrors) that says older agent and system comments are index lines and must be read with `pidash comment list` when their decisions or dependencies matter. A test asserts that no digest-mode render claims to contain the full thread. |
| S11 | Sendback proxy counts normal review waiting | yes: v3 counted every implementation run after a review run | Accept | A sendback is now an explicit hand-back: an `IssueActivity` state change from a `review` or `test` group state to a `started` group state. Review ticks while the issue waits are not sendbacks. The sendback rate is reported but is not a hard gate; rework cost is already inside cost per merged ticket. Test case `test_review_waiting_ticks_are_not_sendbacks` (T-11a). |

Decision judgments from round 3 and how v4 answers them:

- **D1 (storage), agreed.** JSON keys on `AgentRun.usage`; normalization and coverage rules (5.1.2) are fixed before baseline collection.
- **D2 (delivery), agreed with conditions.** Both conditions are met: per-attempt delivery identity with a persisted envelope (5.4.3) and an emergency stop (7.4). It now costs one more column, `AgentRun.delivery`.
- **D3 (effort), agreed.** Unchanged.
- **D4 (budgets), definition agreed, feasibility disputed.** Budgets are now the measured, reachable numbers in 5.4.4. 20 KiB is dropped.
- **D5 (external reviewer), agreed.** Every total states "excludes external review" and no claim implies total system savings.
- **D6 (decision rule), disagreed as written.** Fixed by ticket randomization, intention-to-treat over every enrolled ticket, an unavailable economic verdict when any cost is missing, and an evidence-based sendback count outside the hard gate. Richard still records the parameters before a canary starts (7.2).

## 1. Goal

Cut the model cost per merged ticket, counting every run on the ticket (first pass, ticks, comment-triggered runs, review and test runs, hand-backs, retries, failures and cancellations), without lowering the rate at which runs finish tickets. The Pi Dash Agent runs on OpenHub's free quota (`runner/src/pidash_agent/free_quota.rs:1-22`), so its waste is AI Republic's money; third-party agents spend the user's quota.

The economic metric is "runner-attributable modeled cost per merged ticket, excluding external review". In a canary it is computed over every enrolled ticket of an arm; if any enrolled ticket has no known cost, the economic result is "unavailable" (5.1.5). Merged means a linked code review whose provider merge time is at or before the cutoff (5.1.6).

Engineering targets:

- Every run stores a `RunUsage` record with tokens (or an explicit absence), requests, tool calls, turns, a coverage state and its execution identity, for every agent kind.
- The product shows modeled cost per run and per ticket, split into first pass and rework, and never shows unknown as zero.
- Generic instruction bytes (rendered UTF-8 on the baseline fixture context, excluding the run band and override bodies) meet the measured budgets in 5.4.4: after slim step one at most 66 KiB for coding-task, 35 KiB for review and 41 KiB for test; after step two at most 57 KiB for coding-task. Total prompt bytes and guide-read bytes are reported beside them.
- Within one ticket, runs that share recipe, work type, section versions and overrides and have no volatile override get a byte-identical static band; with a volatile override the band is identical up to `stable_prefix_end` (5.3).
- These are budgets, not promised savings. A saving is claimed only when its canary passes (section 7).

## 2. Non-goals and anti-goals

Non-goals: changing models offered, billing plans, OpenHub quota rules or gateway routing; touching the cloud agent (it never runs issues, AGENTS.md rule 1); redesigning the external PR reviewer, which lives outside this repo; replacing the agent harness with a custom loop; changing reviewer thresholds, reviewer count or final review gates; moving the Rust HTML converter; adding a comment retrieval command; an offline replay harness (Later L-01, owner decision O-1); changing reasoning effort for any phase (Later).

Anti-goals (any of these makes the epic a failure):

- A cheaper run creates enough extra rework to raise ticket cost.
- A budget hit is counted as a finished run.
- Missing usage shows as zero, or free quota shows as zero cost.
- A partially observed run or ticket counts toward a saving.
- Mandatory instructions, permissions, approval policy, sandbox mode or review gates get weaker.
- Tools, model or effort change in the middle of a run.
- Django and the Rust API render different prompts.
- A resumed session inherits another issue's context, privileges or worktree.
- Output is truncated only in Pi Dash's event mirror after the model already consumed it, and reported as a saving.
- Economic telemetry stores prompt text, tool content, reasoning or credentials.
- A run is delivered a prompt that refers to files or variables its runner cannot provide.
- A canary verdict is computed over a subset of the enrolled tickets, or a missing cost is treated as a low cost.
- A failed canary keeps dispatching treatment runs that were created before it failed.

## 3. Current state

### 3.1 Prompt construction

- Every issue run gets its prompt from `build_first_turn` (`apps/api/pi_dash/prompting/composer.py:390-424`; Rust `rust-api/crates/services/src/prompting/context.rs:1249`, called from `orchestration/creation.rs:2225`), on all three Django creation paths (`apps/api/pi_dash/orchestration/service.py:593`, `:732`, `:922`). The text is stored in `AgentRun.prompt` (`apps/api/pi_dash/runner/models.py:1353`) and sent verbatim in `assign` (`_build_assign_msg`, `runner/services/matcher.py:492-517`; Rust `pidash_types::runner_sessions::envelopes::build_assign_msg`, used by `services/src/runner_sessions/drain.rs:133`).
- A prompt kind maps to an ordered recipe (`apps/api/pi_dash/prompting/recipes.py:58-115`; Rust `recipes.rs:90-94`). Sections are joined and rendered once by sandboxed Jinja (`composer.py:204-230`, `:309-328`). All local agent kinds share one recipe; the Pi Dash Agent's recipe is an alias (`recipes.py:196`). A user, workspace, default override chain lets a workspace replace a section body (`composer.py:150-201`, loaded at `:313-325`; Rust `composer.rs:250`).
- Rust section bodies are byte mirrors embedded with `include_str!` (`registry.rs:1-9`, `:217-226`). They have drifted: commit `fb00b0529` (NEWPIDASH-260) changed only the Rust copies of `pidash-cli.md` (`:79`, `:159`) and `workpad-setup.md` (`:34`). The identity test exists (`rust-api/contract-tests/prompting/test_data_plane_dual_engine.py:107`) and fails at this commit, and no PR workflow runs it (`rust-api-contract.yml` triggers on push to main only, `:7-11`).
- Local-run manifests are a bare list of section entries; cloud runs already store `{"v": 2, "executor_kind", "kind", ...}` (`composer.py:404-423`).
- `rust-api/README.md:116-149` describes a cutover edge that proxies every prefix to Django unless a per-prefix env var routes it to Rust, so every prompt and API change lands in both trees.

### 3.2 Size and layout

The coding-task recipe is `intro, software.context, relationships, session-framing, pidash-cli, task-lifecycle, default-posture, autonomy, state-routing, analyze-and-scope, workpad-setup, software.execute, implementation, blocking, guardrails, workpad-template, ending-run` (`recipes.py:59-77`). Template bodies total about 120 KB (largest: `pidash-cli` 37.7 KB in Django and 38.2 KB in the Rust mirror, `analyze-and-scope` 18.0 KB, `software.execute` 11.1 KB, `ending-run` 10.6 KB); the review recipe totals about 74 KB. The observed rendered prompt on jarvis was about 101 KB, 499 lines.

- The most variable text (issue description, full comment log) follows a one-paragraph preamble (`sections/intro.md:6-30`). Run-specific text (trigger, tick counters, prior workpad body) sits at recipe positions 2, 3, 4, 6 and 11.
- Generic sections interpolate `{{ issue.identifier }}` about 39 times across 13 files and `{{ run.id }}` three times (`sections/pidash-cli.md:18`, `:69`, `:167`). `analyze-and-scope.md:15` interpolates the parent identifier and the parent title, which can change between runs. Recipes differ by phase (`recipes.py:58-115`), so a coding run and a review run never share a full prefix.

### 3.3 Markdown loss

- Django: `prompting/context.py:22-32` returns `description_stripped`, which is `strip_tags(description_html)` (`db/models/issue.py:367-373`). Comments use `comment_stripped` (`context.py:318`; `issue.py:645`), and the parent description uses the same stripped text (`context.py:588-599`).
- Rust: `IssueContextInput.description_stripped` is filled from the issue view (`services/src/orchestration/creation.rs:2156`) and returned unchanged (`prompting/context.rs:209-214`). Every loader projects stripped columns only: the issue projection (`creation.rs:174-176`), comments (`:498-503`), the parent description (`:538-539`), the jobs crate's `IssueRow` (`jobs/src/orchestration/creation_jobs.rs:206-245`), its comments query (`:1319-1338`) and its parent fetch (`:1624-1642`). The API-side store is `api/src/runner_runs/dispatch.rs:205-244`.
- A tested Tiptap HTML-to-Markdown converter exists in Django (`apps/api/pi_dash/utils/markdown_converter.py:174`, never raises). Its Rust port is in the `api` crate (`rust-api/crates/api/src/v1_work_items/handlers_labels_pages.rs:4161` and helpers past `:5044`). `services` and `jobs` cannot import `api`. All modes run from one binary, `rust-api/bin/pidash-api` (`serve` and `worker`, `src/main.rs:81-125`), which depends on all three crates.

### 3.4 Sessions and repeated context

- Fresh session per issue run is the documented design (`matcher.py:499-502`). The issue lane calls `spawn_from_config` with no resume id (`runner/src/agent/mod.rs:407-413`); resume exists only on the chat lane (`supervisor.rs:2937-2948`).
- An issue may use up to 10 ticks by default (`db/models/issue_agent_ticker.py:36`), every 3 hours per stage (`db/models/project.py:169-172`). Each run resends the full prompt, and the comment log grows with every run because the agent's own comments are included (`context.py:297-329`; Rust `context.rs:547`, no size budget). Comments labelled `fold` are excluded. Each comment has `speaker_type` (`human`, `agent`, `system`, `integration`, `issue.py:590-594`) and `speaker_agent_run_id` (`issue.py:629`). `pidash comment list <identifier>` prints every comment as JSON (`runner/src/cli/comment.rs:119-131`); the CLI has only `list`, `add` and `update` (`:45-80`).
- The full prior workpad body is inlined (`context.py:650-653`) and the agent is also told to `pidash workpad get` it (`sections/pidash-cli.md:159`).
- Review and test prompts inline the whole parent `done_payload` as sorted, indented JSON (`context.py:401-416`, rendered by `sections/review-intro.md:25` and `sections/test-intro.md:30`; Rust `context.rs:696`). For Claude Code that payload includes `usage` and `total_cost_usd`. Review runs have `parent_run = None` and fall back to the ticker's `resume_parent_run`.
- Free-quota retries reuse the thread and start "a new turn with the same prompt" (`supervisor.rs:4010-4013`).

### 3.5 Pi Dash Agent configuration

- The daemon writes `CODEX_HOME/config.toml` (`runner/src/pidash_agent/codex_config.rs:84-115`) with provider, model, approval policy, sandbox mode, `web_search = "disabled"` and command-backed auth. No effort, output limit, compaction limit or project-doc cap is set, so engine defaults apply (pinned `rust-v0.153.4`, `runner/ENGINE_VERSION`).
- The profile carries only `available, lane, base_url, model, reason_code, engine_sign_in_state` (`runner/src/pidash_agent/api.rs:108-120`; Django `runner/views/engine.py:86-108`; Rust `rust-api/crates/api/src/pidash_agent/mod.rs`). The daemon refreshes it and rewrites the config at any time (`manager.rs:286-334`); each run starts a new engine process, which reads the config once.
- Effort is cleared for the Pi Dash Agent (`runner/src/pidash_agent/agent.rs:61-62`; `runner/src/agent/mod.rs:452-458` passes `None, None`). `turn/start` sends `effort: self.effort_default` (`runner/src/codex/bridge.rs:309-327`). `thread/start` sets sandbox and approval and shadows the TOML values (`bridge.rs:274-293`), so the effective posture must be tested at `thread/start`.
- No parallel-tool-call setting appears anywhere in `runner/src/pidash_agent` or `runner/src/codex`.

### 3.6 Usage telemetry

- `TokenUsage` has required `input`, `output`, `total` (`u64`) plus optional `cache_read`, `cache_write`, `reasoning`, `raw` (`pidash-ipc/src/dto.rs:142-154`). It rides on live state (`dto.rs:190`) and on `RunCompleted`, `RunPaused`, `RunFailed`, `RunCancelled` (`runner/src/cloud/protocol.rs:171`, `:185`, `:194`, `:204`).
- `AgentRun.usage` is a JSON bag `{input, output, total, cache_read, cache_write, reasoning, raw}` with generated token columns (`runner/models.py:1427-1438`; shape `runner/services/usage.py:5-37`; Rust `rust-api/crates/types/src/runner_runs/usage.rs:43`). Finalization merges live state, `done_payload["usage"]` and terminal `tokens` and also writes `llm_model` (`run_lifecycle.py:67-100`; Rust `services/src/runner_runs/lifecycle.rs:177`). Live state has `turn_count` (`dto.rs:194`; `RunnerLiveState.turn_count`, `runner/models.py:1943`), which finalization does not persist, and the next run replaces the live-state row.
- The runner parses only Codex usage frames (`observability.rs:106-190`). Frames carry thread-cumulative and per-request (`last`) counters and no request id (`:85-141`). `RunTokenMeter` takes its baseline as `cumulative - last` on the first frame (`:192-212`), which drops earlier requests if the first observed frame already covers several. The meter is created inside `pump_events` (`supervisor.rs:4206`), so each free-quota retry starts a new meter and loses earlier tokens, and a retry-exhausted failure sends `tokens: None` (`supervisor.rs:4127-4135`). `turn_count` counts Codex `turn/started` (`supervisor.rs:4471-4473`). Exec hints recognise only Codex `commandExecution` and Claude Bash `tool_use` (`observability.rs:255-300`).
- Claude Code's error branch emits `Failed` with no usage or cost (`claude_code/bridge.rs:280-289`); only success copies `usage` and `total_cost_usd` (`:291-297`).
- Cache writes: `observability.rs:170-178` folds Anthropic `cache_read_input_tokens` and `cache_creation_input_tokens` into `input` so that `input` means the same for every provider, and keeps the totals in `cache_read` and `cache_write`. The provider's split of cache writes into 5-minute and 1-hour tiers (`usage.cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens`) is not parsed into any field.

| Agent kind | Live usage | Final usage | Requests, tool calls, turns | Cost | Evidence |
|---|---|---|---|---|---|
| Pi Dash Agent, Codex | yes | yes, lost on retry | `last` frames, `item/started` with `id`, `turn/started` | no | `observability.rs:96-137` |
| Claude Code | no (assistant `message.usage` ignored) | success only | `message.id`, `tool_use` id, `num_turns` dropped | `total_cost_usd` on success only | `claude_code/bridge.rs:280-297` |
| Cursor Agent | no | no | `tool_call` with `call_id` | no | `cursor_agent/schema.rs:30-35` |
| Muse Code | no | no (raw passthrough) | `tool_call/started` with `call_id` | no | `muse_code/bridge.rs:423`, `:448`, `:484`, `:494` |
| Grok, OpenClaw | no | no | partial | no | `grok/bridge.rs:331-409`, `openclaw/bridge.rs:319-370` |

The product shows one cumulative issue "Tokens" tile (`apps/web_new/src/features/issues/detail/agent/AgentStatusPanel.tsx:114`, `:172-177`) and a runner activity panel (`features/runners/detail/ActivityPanel.tsx:81-90`). Both analytics backends serve a run-token sum, not ticket cost (`app/views/analytic/advance.py:67-80`; `rust-api/crates/services/src/app_analytics/queries.rs`).

### 3.7 Merge evidence

- `GitCodeReviewLink` is the live store; the legacy GitHub webhook write is removed ("Sunset 4/6", `app/views/integration/github.py:956`) and `GithubPullRequestLink` is only mirrored from it (`integrations/git/code_reviews.py:63-103`). Fields: `merged`, `state`, `remote_updated_at`, `metadata` (`integration/git.py:226-246`). There is no merge timestamp. `remote_updated_at` is overwritten by every newer webhook (`github.py:966-985`), so it is not a merge time.
- The provider's raw object is kept in `metadata`: the GitHub webhook writes it under `metadata["remote"]` (`github.py:972`; Rust `api/src/app_integrations/handlers_github_app.rs:2308-2309`), and attach or refresh through the adapters stores the full PR or MR object (`integrations/git/adapters/github.py:243`, `gitlab.py:384-395`). GitHub PRs and GitLab MRs both carry `merged_at` in that object. `merged` itself is derived from `merged` or `merged_at` (`utils/github_client.py:295`).
- Links are soft-deleted (`deleted_at`, `db/mixins.py:72-78`; partial-unique constraints use `deleted_at IS NULL`).

### 3.8 Matching, ticker and failure policy

- `next_for_runner` (`matcher.py:164-243`) builds eligibility from executor kind, pin kind (`dedicated`, `locality`), `pod_dispatch` and engine pod rules, then applies the usage rule `filter_runs_usable_by_runner` (`permissions.py:83-160`, ownership, visibility and engine team-run rules). Runners report `runner_version` from hello `version` (`runner/models.py:676`; `protocol.rs:97`). Stale locality pins are released after `RUNNER_PIN_AUTO_RELEASE_SECS` (default 600, `settings/common.py:562`; `matcher.py:375-470`; Rust `runner_sessions/unsatisfiable_pins.rs`).
- The ticker spends one tick when it claims a machine-started run (`agent_ticker.py:209-232`); a human's free entry does not.
- A failed run is classified into `RunFailureReason` (`runner/failure.py:49-127`; runner reasons mapped first, `:386-430`), and the reason maps to a `FailurePolicy` (`:134-184`): `RETRY_FREE` (one free entry after a backoff, at most `FREE_RETRY_MAX_ATTEMPTS = 3` in a row), `NEEDS_HUMAN` (stop the clock with `FAILURE_NEEDS_HUMAN`), `FRESH_SESSION`, or the default `RETRY_NORMAL`. A backstop stops the clock after `repeated_failure_limit_for(reason)` failures in a row (`failure.py:376-381`). The ticker applies it in `_apply_failure_policy` (`orchestration/scheduling.py:539-612`, called from `_on_run_ended`, `:615`). Rust twins: `rust-api/crates/types/src/runner_runs/failure.rs`, `services/src/orchestration/clock.rs`.
- `PIDASH_RUN_ID` is set for every agent kind (`runner/src/util/shell.rs:81-86`). `PIDASH_ISSUE_IDENTIFIER` is read by the CLI (`cli/workpad.rs:93`, `:117`; `cli/state.rs:50`) but never set; `Assign` carries only `work_item_id` (`protocol.rs:288-303`).
- `_build_assign_msg(run)` takes only the run (`matcher.py:492-517`). It is called after the assignment transaction commits, from `drain_pod` (`:276-284`) and `drain_for_runner` (`:337`), and again for reconnect redelivery of `ASSIGNED` and `WAITING_FOR_WORKTREE` runs (`session_service.py:474-525`). Rust builds the frame from prompt and run config in `plan_assignment` (`services/src/runner_sessions/drain.rs:550-558`) and for redelivery in `session_service.rs:1427`. Nothing records which frame was sent.
- `apply_run_resume_unavailable` re-queues an assigned run in place: status back to `QUEUED`, runner and `assigned_at` cleared (`run_lifecycle.py:247-295`; Rust `services/src/runner_runs/lifecycle.rs`). One `AgentRun` can therefore be assigned more than once, to different runners.
- Cancels go out through `_send_cancel_best_effort` (`runner/views/runs.py:41-61`) and are re-sent on poll and on reconnect while the run is `CANCEL_REQUESTED` (`session_service.py:236-270`, `:498-511`). A cancelled run keeps the issue's clock ticking (`orchestration/scheduling.py:641-652`).

### 3.9 Runtime configuration and state history

- `InstanceConfiguration` stores key and value rows (`apps/api/pi_dash/license/models/instance.py:72-83`, table `instance_configurations`) that both backends read (Rust `rust-api/crates/db/src/config/accessor.rs`). A row can change without a deploy.
- Every state change writes an `IssueActivity` row with `field = "state"`, old and new state names and ids (`db/models/issue.py:553`; written in `bgtasks/issue_activities_task.py:215-223`). One automated path writes only the new id (`:640-649`). State groups include `started`, `review` and `test` (`db/models/state.py:14-22`).

## 4. Options and trade-offs

- **Order of work.** Telemetry first, because every saving is a hypothesis until per-run usage exists. The store and wire types already exist, so this is coverage gaps plus ticket economics. Correctness fixes that claim no saving (mirror parity, Markdown, retry accounting) can run in parallel.
- **Telemetry storage (D1).** New keys in the `AgentRun.usage` JSON bag versus a checkpoint table with source identity and sequence. The bag is several tickets smaller; its weakness, double counting, is handled by making the runner meter the single reducer with fixed ordering and dedup rules. Chosen: the bag.
- **Prompt layout.** Static-first reordering is cheap but its cache value is unproven (Codex may key the cache per thread; Anthropic caches for 5 minutes per account; open question 2). Shrinking inline text saves on every request of every run for every agent kind, cached or not. Chosen: both, reorder first, claim only measured savings.
- **Reference delivery (D2).** An on-demand API endpoint needs a round trip and a route in two backends. A daemon-pinned bundle needs the daemon to get it from somewhere. Chosen: the server renders the bundle with the run's overrides at creation; the runner reads it locally through `pidash guide`.
- **Old and ineligible runners.** Predicting at creation who can take the run, or storing two renders, both add machinery. Chosen: store one full inline prompt plus extraction spans, and derive the slim prompt and bundle at each assignment attempt for the runner actually chosen, if capable.
- **Delivery record (v4).** Rebuilding the frame on every send is cheap but means the record cannot say what a runner received, and a reassigned run may be described wrongly. Storing only a summary leaves redelivery to rebuild. Chosen: persist the exact frame per attempt in one new JSON column, replay it verbatim, and keep earlier attempts as summaries. The cost is one extra copy of the prompt per run.
- **Randomization unit (v4).** Pod or workspace units need cluster-aware inference and many clusters, which AI Republic does not have. Ticket units give independent assignments with a deterministic hash and need no new state. Chosen: tickets. The cost is possible interference between tickets that share runners, noted in section 8.
- **Missing cost (v4).** Conservative bounds for missing costs need an upper cost bound per run that does not exist. Chosen: if any enrolled ticket lacks cost, the economic verdict is unavailable; experiments are scoped to agent kinds that report usage.
- **Rust Markdown.** Moving the converter into a shared crate is a 900-plus-line move. Chosen: a small seam in `services`, filled once in the `pidash-api` binary's `main` for both modes, plus explicit HTML loading in every loader.
- **Comment digest.** A resolution-aware digest needs resolution metadata that does not exist. Chosen: a speaker-and-phase digest that shortens only agent and system noise.
- **Pi Dash Agent tuning.** The machine profile can be refreshed at any time and is not tied to a run. Chosen: tuning is part of the run's creation snapshot, sent in `assign`, applied per run; the machine profile never carries saving keys.
- **Activation (owner streamlining, deviation from the brief).** The brief asks for A/B on replayed tickets. A sound replay needs a pre-work repository state, isolated write targets and fresh review and merge outcomes, which is a project of its own. A live canary with concurrent controls uses real outcomes and existing data. Chosen, at the owner's request: live canary only; replay is Later L-01. Vinh confirms this as decision O-1.
- **Emergency stop (v4).** A settings flip that affects only new runs leaves queued treatment runs dispatchable. Re-rendering queued runs in place would break the immutable snapshot. Chosen: block dispatch, cancel and recreate queued canary runs with baseline settings, and stop running ones by an explicit policy.
- **Merge time.** `remote_updated_at` is overwritten by later updates. Chosen: one nullable `merged_at` column written once, backfilled from `metadata`.
- **Rework.** Fresh session with a compact packet versus resume. A resumed thread carries old tool output, needs the same machine, worktree and a live session. Chosen: fresh plus packet; resume is Later.

## 5. Chosen design

### 5.1 Telemetry and economics

#### 5.1.1 Usage envelope

A new optional wire struct in `pidash-ipc/src/dto.rs`, carried as `run_usage: Option<RunUsage>` beside the existing `tokens` on live state and on all four terminal messages. Old servers ignore it; new servers fall back to `tokens` when it is absent. `TokenUsage` is unchanged.

```
RunUsage {
  tokens: Option<TokenUsage>,           // None when the agent reports no tokens
  cache_write_tiers: Option<{ ttl_5m: u64, ttl_1h: u64 }>,  // only when the provider reports the split
  requests: Option<u64>,   requests_basis: Option<"exact" | "partial">,
  tool_calls: Option<u64>, tool_calls_basis: Option<"exact" | "partial">,
  turns: Option<u64>,      turns_basis: Option<"exact" | "partial">,
  reported_cost_usd: Option<String>,    // decimal string as the agent reported it
  coverage: "reported" | "partial" | "unsupported" | "unavailable_after_failure",
  execution: {
    agent_kind, agent_version, meter_version, model,
    effort,                              // "engine_default" when none was set
    tuning_sha256: Option<String>,       // of the tuning actually applied
    delivery_attempt: Option<u32>,       // echoed from assign
    delivered_layout: "inline" | "slim",
    bundle_sha256: Option<String>,
    sandbox, approval_policy             // effective values at thread start, where known
  }
}
```

Storage stays the `AgentRun.usage` JSON bag (D1). It gains `cache_write_tiers`, `requests`, `tool_calls`, `turns` (each with a basis), `reported_cost_usd`, `coverage`, `execution`, `prompt_bytes` and, at finalization, `treatment_match` (5.1.5). Unknown stays null; tokens are never zero-filled except by the zero-request rule in 5.1.2. Canonical meaning: `input` includes cache reads and cache writes (as `observability.rs:170-178` already normalizes), and `output` includes reasoning; `cache_read`, `cache_write` and `reasoning` are subsets and are never added again. `merge_usage` (`runner/services/usage.py`; Rust `types/src/runner_runs/usage.rs:43`) prefers `run_usage` from the terminal message, then live state, then `done_payload["usage"]` for old runners. Because the terminal message carries `turns`, the count survives the next run replacing the live-state row.

#### 5.1.2 Counting rules

Fixed before baseline collection.

- **Scope.** One meter per run, owned by the run and living across free-quota retries (it moves out of `pump_events`, `supervisor.rs:4206`). Frames are grouped by agent thread; a retry that reuses the thread continues the same group.
- **Baseline.** An issue run always starts its thread with `thread/start` (3.4), so its baseline is zero and the first frame's cumulative counters count in full. If a future lane resumes a thread, the meter needs a baseline captured before the run's first request; without one the run's coverage is `partial`.
- **Ordering and dedup.** Frames are processed in arrival order. A frame is accepted only if its cumulative `total` is strictly greater than the last accepted `total` for that thread. Equal frames (a duplicate, or the same request reported by both `thread/tokenUsage/updated` and `codex/event/token_count`) and smaller frames (stale or reordered) are ignored for tokens and requests. Totals never regress.
- **Requests.** Codex family: each accepted frame that carries `last` counts one request; there is no request id, so `requests_basis = "partial"` and the UI says "at least N". Claude Code: distinct assistant `message.id` values, `exact`, cross-checked against `num_turns`. Others: null unless a fixture shows an identity.
- **Tool calls.** Distinct stable call ids from tool start events: every Codex `item/started` whose item is a tool (command execution, file change, MCP call, web search and any future tool type with an id), Claude `tool_use` ids, Cursor `call_id`, Muse `call_id`. A start event without an id is not counted and sets the basis to `partial`. Shell-only exec hints keep their existing purpose.
- **Turns.** Codex family: `turn/started` events in the run, `exact` (the existing counter, now persisted). Claude Code: `num_turns` from the result message, `exact`. Muse, Cursor, Grok, OpenClaw: the agent's own turn or step event if the fixture spike finds one, otherwise null. Requests and turns are kept separate because one turn can contain many model requests.
- **Zero-request rule.** For an agent kind whose meter sees turn starts (Codex family, Claude Code), a run that ends before any turn started and before any usage frame arrived made no model request. It records zero tokens with `coverage = "reported"` and `requests = 0` (`exact`). This keeps early platform failures (bundle check, worktree, auth) from making a ticket's cost unknown.
- **Coverage.** `reported` when tokens came from the agent for every attempt of the run with a known baseline, or the zero-request rule applies; `partial` when any attempt reported nothing or the baseline is unknown; `unsupported` when the agent kind reports no tokens (counts may still be present); `unavailable_after_failure` when the run ended after a request may have been made but before any usage arrived (for example a daemon crash).

#### 5.1.3 Per agent kind

- Pi Dash Agent and Codex: the meter above; the retry-exhausted failure path sends the metered `run_usage` instead of `tokens: None` (`supervisor.rs:4127-4135`).
- Claude Code: live `run_usage` from each assistant `message.usage`, dedup by `message.id`; on success and error results, `run_usage` carries final usage, `num_turns`, `total_cost_usd` and `cache_write_tiers` from `usage.cache_creation` when present. The parent payload used in prompts never includes these fields (5.5).
- Muse, Cursor, Grok, OpenClaw: tool calls by call id, turns where an event exists, `coverage = "unsupported"` unless a fixture shows run-scoped tokens. Tokens are never estimated from text length.
- Every agent kind fills `execution` from what the runner itself launched (agent kind and version, model, effort, applied tuning hash, delivery attempt, delivered layout and bundle hash, posture passed to `thread/start` where the agent has one).

#### 5.1.4 Rate model and per-run and per-ticket cost in the product

- **Rate table.** One append-only JSON file, `apps/api/pi_dash/experiments/model_rates.json`, embedded in Rust with `include_str!` and covered by the mirror test (T-12's workflow). It holds named versions; a version is never edited after merge, a change adds a new version. Each version maps a model id to rates per million tokens for four disjoint categories:
  - `input_uncached`, applied to `input - cache_read - cache_write`;
  - `cache_read`, applied to `cache_read`;
  - `cache_write`, applied to `cache_write`, or, where the provider prices write durations differently, `cache_write_5m` and `cache_write_1h`, applied to `cache_write_tiers.ttl_5m` and `ttl_1h`;
  - `output`, applied to `output` (reasoning is inside output for every provider this table covers, so it is not billed again; `reasoning` stays informational).
- **Unknown cost.** Modeled cost is "unknown" with a reason when: the model has no entry in the version; `cache_write > 0` and the entry has no write rate; the entry has tiered write rates and the run has `cache_write > 0` without `cache_write_tiers`, or the tiers do not sum to `cache_write`; any category would be negative; or coverage is not `reported`. A rate is never borrowed from a similar model.
- **Versions.** Product displays use the newest version and show its name. Each experiment records its `rate_table_version` before it starts (7.2); its report, including the closed report, uses that version only, so a later rate change cannot move a closed verdict.
- **Display.** The issue status payload gains `usage_breakdown`: per run (phase, trigger, status, coverage, tokens by category, requests with basis, tool calls, turns, modeled cost or the reason it is unknown, reported cost separately), and per ticket the sum of known modeled cost, the count of runs with unknown cost, and the split between first pass and rework. Rework is every run after the first implementation run (ticks, hand-backs, comment-triggered runs, review and test runs, retries). The label "excludes external review" is always present.
- Both backends serve it (open question 4 decides which one is live for this prefix).

#### 5.1.5 Canary evaluation

Used only by the canary report (section 7), a read-only Django management command. It is not product UI.

- **Experiment record.** Each experiment is an entry in the `RUN_EXPERIMENTS` `InstanceConfiguration` row (3.9): id, workspace, one flag or one tuning key, agent-kind scope, share in basis points, start time, status (`active`, `stopped`, `closed`) and the G1 parameters (7.2). Share, scope and parameters are immutable once the experiment starts; a change means a new experiment. At most one experiment is active per workspace, so a run is never in two treatments.
- **Unit and arm.** The unit is the ticket (issue). Arm = `canary` when `int(sha256(experiment_id + ":" + issue_id).hex()[:8], 16) % 10000 < share_bp`, else `control`. The arm is a pure function of experiment and ticket. It is also written into each run's manifest so an audit does not need to recompute it.
- **Enrollment.** A ticket enrolls at its first attributable run created while the experiment is active, if that run's agent kind is in the experiment's scope. Every later run on the ticket, with any agent kind, belongs to the same arm. Canary runs get the treatment; control runs get baseline settings.
- **Intended and effective treatment.** At creation the manifest records the intended treatment: experiment id, arm, layout version, `flags_sha256`, tuning values and `tuning_sha256`, effort, agent kind. At finalization the server compares it, and the current delivery attempt (5.4.3), with `run_usage.execution` and stores `usage.treatment_match` (true, or false with the differing fields; null when the runner reports no `execution`). This is compliance data only.
- **Counted runs.** Every attributable run on the ticket (machine executor `local_runner` or `managed_runner`, any phase, any status, including retries, failures, cancellations and stop replacements) from the enrollment run up to the cutoff. Cloud-agent runs are excluded. External review is excluded and the report says so. Runs before enrollment are not counted; tickets that had them are reported as a "pre-existing" stratum as well as in the total.
- **Intention-to-treat.** Every metric's denominator is every enrolled ticket of the arm. No ticket is ever dropped for coverage, compliance, mismatch, fallback delivery or failure.
- **Metrics per arm.**
  - Merged rate: enrolled tickets merged at the cutoff over enrolled tickets.
  - Cost per merged ticket: sum of modeled cost of all counted runs of all enrolled tickets, over the number of merged tickets. Unmerged tickets' cost stays in the numerator.
  - Guardrail violations (section 7.2).
  - Reported, not gating: sendbacks per enrolled ticket, where a sendback is an `IssueActivity` state change (3.9) whose previous state is in group `review` or `test` and whose new state is in group `started`, between enrollment and cutoff (when `old_identifier` is null, the previous state is the `new_identifier` of the ticket's prior state activity); blocked-run rate; missing-yield rate; total prompt bytes and guide-read bytes per run.
  - Compliance, reported separately: share of counted runs with `treatment_match = true`, share of canary runs actually delivered slim (for slim experiments), counts of mismatches by field, stop replacements.
- **Unavailable results.** The report returns "unavailable" with a reason, never a number, when: the cutoff is in the future (`immature`); either arm has fewer enrolled tickets than the recorded minimum (`sample`); any enrolled ticket has a merged link with unknown merge time (merged-rate verdict `merge_unknown`); any enrolled ticket has a counted run with unknown modeled cost (economic verdict `cost_unknown`, listing the tickets and reasons); an arm has zero merged tickets (economic verdict `no_merges`). A stopped experiment's verdict is "failed (stopped)" whatever the numbers say.

#### 5.1.6 Merge time and cutoff

- New nullable column `GitCodeReviewLink.merged_at` (Django migration; Rust entity `db/src/entity/git_code_review_links.rs` and model `db/src/integrations/git_models.rs:878-1010`). Every writer of `merged` (the Django webhook `app/views/integration/github.py:966-985`, attach and refresh in `integrations/git/code_reviews.py` through the GitHub and GitLab adapters, and the Rust webhook in `handlers_github_app.rs:2276-2330`) sets `merged_at` from the provider's `merged_at` the first time it sees one, and never overwrites or clears it.
- One-time backfill: for links with `merged = true` and `merged_at` null, read `metadata["remote"]["merged_at"]`, then `metadata["merged_at"]`. Links still without a value keep null and mean "merge time unknown".
- `cutoff = window_end + maturity_days` (maturity default 14, set in G1).
- A ticket is merged at the cutoff if some code review link on it has `merged_at <= cutoff`, `created_at <= cutoff`, and `deleted_at` null or later than the cutoff. A link with `merged = true` and `merged_at` null makes the ticket's outcome unknown, which makes the merged-rate verdict unavailable (5.1.5). A PR merged after the cutoff has `merged_at > cutoff` and does not change a closed cohort.

### 5.2 Correctness fixes that also save tokens

- **Mirror parity.** Port the NEWPIDASH-260 guard into the Django copies of `pidash-cli.md` and `workpad-setup.md`, and run the mirror tests in a PR-triggered workflow.
- **Markdown context, Django.** Description, parent description and comments render through `html_to_markdown(description_html)` and `html_to_markdown(comment_html)` (`utils/markdown_converter.py:174`), falling back to stripped text when the HTML is empty; the manifest records `markdown_source`. Issue text stays a Jinja value, never template source, so `{{ }}` in issue text stays literal.
- **Markdown context, Rust converter seam.** `services` declares `pub fn set_html_to_markdown(f: fn(Option<&str>) -> String)` backed by a `OnceLock`. `rust-api/bin/pidash-api/src/main.rs` installs the `api` crate's existing `html_to_markdown` before dispatching to `serve` or `worker`, so API and background creation use the same converter. If none is installed, context falls back to stripped text and records `markdown_source = "stripped"`.
- **Markdown context, Rust input plumbing.** The issue projection (`creation.rs:174-176`) and the jobs `IssueRow` (`creation_jobs.rs:206-245`) add `description_html`; the parent description queries (`creation.rs:538-539`, `creation_jobs.rs:1624-1642`) add the parent's `description_html`; the comment queries (`creation.rs:498-503`, `creation_jobs.rs:1319-1338`) add `comment_html`. `IssueView`, `ParentView` and the comment row gain the HTML fields; `IssueContextInput` gains `description_markdown`, and context uses the converted text for issue, parent and comments. The API store in `api/src/runner_runs/dispatch.rs` uses the same projections. A contract test drives a live tick through each backend and compares prompts.
- **Retry input.** On a free-quota retry, if the interrupted turn was accepted (`turn/started` seen), send a short "continue the task above" input; otherwise resend as today. It is the first per-run tuning key, `retry_continue_input` (5.6), default off.

### 5.3 Prompt layout v2

Behind `PROMPT_LAYOUT_V2`, activated only through section 7.

1. **Static band first.** The generic sections in their current relative order, with "why this run started", tick counters, the workpad body and every mutable ancestor value (parent title, parent state, lineage titles in `analyze-and-scope.md`) moved out. `{{ run.id }}` (`sections/pidash-cli.md:18`, `:69`, `:167`) becomes `$PIDASH_RUN_ID`, which every runner already sets. `{{ issue.identifier }}` and `{{ parent.identifier }}` stay, since they do not change within a ticket. No timestamps, URLs, machine paths, attempt counts or experiment fields in the static band.
2. **Project band.** `software.context` without linked PRs, and the project state list.
3. **Run band last.** A new `run-context` section with issue fields, Markdown description, relationships, parent and lineage titles, linked PRs, trigger and budget, then the context packet (5.5).
4. **Overrides.** A section with an active workspace or user override is rendered inline exactly as overridden, at its position among the static sections; the manifest records `override_kept_inline`. An override is **volatile** when its Jinja source references any top-level name that layout v2 moves to the run band (`run`, `trigger`, `ticker`, `workpad`, `lineage`, or `parent` other than `parent.identifier`), found with `jinja2.meta.find_undeclared_variables` plus an attribute walk for `parent`. Nothing is rewritten. The manifest records `stable_prefix_end`: the byte offset where the first volatile override begins, or the end of the static band when there is none.

What this promises: for runs of one ticket that share recipe (phase), work type, section versions and overrides, the rendered prompt is byte-identical up to `stable_prefix_end`. Without a volatile override that is the whole static band. Coding and review runs use different recipes and do not share a full prefix. Any cache saving depends on the provider caching across sessions (open question 2). Size cuts (5.4) pay regardless.

### 5.4 Smaller inline prompt with a guardrail floor

#### 5.4.1 Required texts

- A registry file lists each mandatory obligation as `{id, section_key, text}`: the `pidash run yield` contract, the CLI environment and secret rule (`sections/guardrails.md:15`), permission and approval rules, repository guardrails in `software.execute`, wrong-state rules in `state-routing`, blocking rules, workpad and handoff obligations, and the ban on self-extending budgets or bypassing review.
- A test renders every applicable recipe on every render path that exists at that commit and asserts each required text appears verbatim. T-16 covers the default and override paths; T-17 adds layout v2; T-21 adds the slim delivered prompt.
- Save-time validation refuses a new or edited override that removes a required text from a section that owns one.
- Existing overrides: a read-only management command (`prompt_override_audit`) lists every active override that lacks a required text, by workspace and section. A workspace with an unresolved entry cannot start a layout or slim canary; the fix is a human edit of the override. Nothing is migrated or rewritten.
- At every assignment attempt the server checks the delivered text (inline or slim) against the registry and records `required_texts_ok` in that attempt's delivery record (5.4.3). A slim delivery that would fail is replaced by the stored inline prompt and the event is counted as a guardrail violation.

#### 5.4.2 Manifest v3 and creation-time snapshot

`AgentRun.prompt_manifest` for local and managed runs becomes:

```
{ "v": 3, "executor_kind", "kind", "layout_version",
  "flags": {...}, "flags_sha256",
  "treatment": { "experiment_id", "arm", "tuning": {...}, "tuning_sha256",
                 "effort", "agent_kind", "replaces_run_id": null | uuid } | null,
  "prompt_sha256", "prompt_bytes", "instruction_bytes", "override_bytes",
  "stable_prefix_end", "markdown_source",
  "reference_spans": [ { "topic", "start", "end", "sha256", "index_line" } ],
  "sections": [ {section_key, source, version, line_start, line_end, bytes, sha256, inline} ] }
```

A single reader returns sections for a bare list (legacy), the cloud v2 dict, and v3. Prompt, flags, treatment and spans are written in the same save as `AgentRun.prompt`, before the run is queued, and never change afterward. Assignment never re-renders, so an override edited after creation or a flag flipped after creation cannot reach an existing run. What was delivered lives in `AgentRun.delivery` (5.4.3), not in the manifest. The emergency stop (7.4) does not edit a snapshot; it cancels the run and creates a replacement with its own snapshot.

#### 5.4.3 Extraction spans and per-attempt delivery

- **Markers.** Extractable text in default section bodies is wrapped in marker lines `<!-- guide:<topic> -->` and `<!-- /guide:<topic> -->`, each on its own line and never inside a Jinja block that opens or closes within the range. At creation the composer renders as today, then removes the marker lines and records each marked range as a span of the resulting text. `AgentRun.prompt` is that full inline prompt. Markers inside an overridden section are removed and not recorded, so overridden text stays inline. A span that would contain a required text (5.4.1) is not recorded, and the manifest notes it. Slim delivery applies only to the coding-task, review and test kinds; scheduler prompts are always inline, because the scheduler recipe's main task is the scheduler commands.
- **Delivery record.** New nullable JSON column `AgentRun.delivery`:

```
{ "attempt": 2,
  "runner_id", "runner_version", "layout": "inline" | "slim",
  "envelope": { ...the exact assign frame, without the outbox "mid"... },
  "envelope_sha256", "bundle_sha256", "required_texts_ok", "assigned_at",
  "history": [ { "attempt", "runner_id", "runner_version", "layout",
                 "envelope_sha256", "bundle_sha256", "assigned_at", "ended": "requeued" } ] }
```

- **Deciding a delivery (per attempt).** In the assignment transaction, after matching has picked the runner by today's rules (Django `drain_pod` loop `matcher.py:272-282` and `drain_for_runner` `:325-334`; Rust `plan_assignment`, `drain.rs:550-558`, which gains the runner's `runner_version` and the run's manifest in `AssignmentFacts`), the server sets `attempt` to the previous attempt plus one and derives the delivery from stored fields only: slim when the run has spans, `PROMPT_REFERENCE_BUNDLE` was on at creation (it is in `flags`), the experiment is not stopped, the issue's most recent failed run does not have reason `reference_bundle_invalid`, and the chosen runner's `runner_version` is at least `SLIM_LAYOUT_MIN_RUNNER_VERSION`; inline otherwise. Slim means each span replaced by its one-line index entry plus a bundle of the span texts by topic with their hash; slim plus bundle expands back to `AgentRun.prompt` byte for byte. The server checks the delivered text against the required-text registry; a slim delivery that fails is replaced by inline and counted as a guardrail violation. It then builds the complete assign frame, including `delivery_attempt`, `reference_bundle` and `tuning`, and writes it to `delivery.envelope` in the same save that sets `ASSIGNED`.
- **Sending and replay.** The post-commit send transmits `delivery.envelope` exactly. Reconnect redelivery (`build_session_open_redeliver`, `session_service.py:474-525`; Rust `session_service.rs:1427`) sends the stored envelope byte for byte and never rebuilds it, so a repeated send is the same attempt with the same identity. Only runs assigned before this change (with `delivery` null) fall back to today's builder.
- **Requeue and reassignment.** `apply_run_resume_unavailable` (and its Rust twin) appends the current attempt's summary to `history` with `ended = "requeued"` and clears `envelope` in the same save that sets `QUEUED`. The next assignment is a new attempt and derives its delivery again from the same immutable snapshot for its own runner, so a slim run reassigned to an old runner gets the inline prompt with `layout = "inline"` and the new attempt number.
- **Delivery identity in telemetry.** The runner echoes `delivery_attempt`, `delivered_layout` and `bundle_sha256` in `execution`. At finalization the server compares them with the current attempt; a difference sets `treatment_match = false` with the differing field. The canary report reads `delivery` to say which runs actually got slim prompts.
- **Runner side.** The runner writes the bundle under the run's data dir, never the worktree, and never generates an `AGENTS.md`. Before starting the agent it checks the bundle hash; on mismatch it does not start the agent and sends `RunFailed` with the new runner reason `reference_bundle_invalid`. `pidash guide <topic>` prints a topic; each read is logged as a run event with its byte count.
- **Ticker and retry rule for `reference_bundle_invalid`.** Django and Rust add `RunFailureReason.REFERENCE_BUNDLE_INVALID`, map the runner reason to it, and give it `FailurePolicy.RETRY_FREE`. The run ends `FAILED` with that reason; the tick that dispatched it stays spent, like any other platform failure; `_apply_failure_policy` (and its Rust twin) queues one free entry after `FREE_RETRY_BACKOFF_SECONDS`, at most 3 in a row, after which the existing `REPEATED_FAILURE` backstop stops the clock. The delivery rule above sends the retry inline. A human rerun is also delivered inline. Old servers that do not know the reason classify it as `RETRY_NORMAL`, which is the safe default.

#### 5.4.4 Measured baseline, extraction ranges and budgets

**Baseline fixture context.** Rendered with the production Jinja settings, with every optional context value absent: no parent, no lineage, no linked PR, no comments, no prior workpad, no project state list, repository provider unknown. Identifiers render as a 7-character placeholder. This is close to "generic instruction bytes": the issue text and run band add to it in real runs (the observed jarvis prompt was about 101 KB). The numbers below were measured for this design by rendering the section files at `9cc85ccd0` with a copy of that environment; T-18a commits the same context as a fixture and records the composer's own numbers. If those differ from the table by more than 1 KiB, T-18a updates the budgets in the same PR before T-21 starts.

**Step-one extraction ranges** (Django files at `9cc85ccd0`; T-12's mirror fix adds lines only after `pidash-cli.md:159`, so the ranges hold for both trees after T-12):

| Topic | File and lines | Raw bytes removed |
|---|---|---|
| `cli-issues` | `sections/pidash-cli.md:38-48` (issue command reference) | 9,261 |
| `cli-pods` | `sections/pidash-cli.md:50-62` (pods and dedicated runners) | 2,530 |
| `cli-comments` | `sections/pidash-cli.md:66-72` (comment commands and `--fold`) | 1,741 |
| `cli-pages` | `sections/pidash-cli.md:83-90` (page commands) | 2,422 |
| `cli-labels` | `sections/pidash-cli.md:94-114` (label commands and examples) | 3,235 |
| `cli-schedulers` | `sections/pidash-cli.md:122-134` (scheduler commands) | 6,725 |
| `workpad-template` | `sections/workpad-template.md:15-88` (template body; the heading and the instruction to use it stay) | 3,342 |

Kept inline in `pidash-cli.md`: environment and secret rules (`:10-20`), context files and output contract (`:21-33`), workpad commands (`:74-79`), states (`:116-118`), run outcome and the `pidash run yield` contract (`:136-143`), debugging, "Not for you" including the human-only `re-tick` rule (`:148-152`), typical recipes (`:154-200`), available states (`:201-211`) and conventions (`:213-219`). None of the step-one ranges contains an obligation listed in 5.4.1.

**Step-two extraction ranges** (coding-task only, because these sections appear only in that recipe):

| Topic | File and lines | Raw bytes removed |
|---|---|---|
| `split-children` | `sections/analyze-and-scope.md:57-71` (how to create, record, relate and start child issues; the decision of when to split, `:50-55`, stays inline) | 4,679 |
| `writing-to-humans` | `sections/analyze-and-scope.md:75-95` (comment tone and the three examples) | 2,756 |
| `handoff-template` | `sections/implementation.md:38-52` (the hand-off block format; the obligation in `:36` stays) | 1,613 |
| `testing-handoff-software` | `work_types/software/execute.md:74-78` | 877 |

`ending-run.md` is not extracted: its long passages sit inside `{% if %}` branches on shared lines (`:41-48`, `:56-62`) and carry the "never move to Done" and workpad completeness obligations.

**Measured rendered sizes** (bytes, baseline fixture context):

| Recipe and work type | Today | After step one | After step two |
|---|---|---|---|
| coding-task, software | 94,074 (91.9 KiB) | 65,912 (64.4 KiB) | 56,645 (55.3 KiB) |
| coding-task, general | 89,797 | 61,635 | 53,084 |
| review, software | 59,129 (57.7 KiB) | 34,168 (33.4 KiB) | unchanged |
| review, general | 58,252 | 33,291 | unchanged |
| test, software | 65,647 (64.1 KiB) | 40,686 (39.7 KiB) | unchanged |
| test, general | 62,218 | 37,257 | unchanged |
| scheduler | 39,657 | not slimmed | not slimmed |

**Budgets (D4).** Each is the measured value rounded up to the next KiB plus 1 KiB of headroom. After step one: coding-task at most 66 KiB, review at most 35 KiB, test at most 41 KiB. After step two: coding-task at most 57 KiB. These are the acceptance numbers in T-21 and T-22, tested on both work types.

**20 KiB is not reachable by extraction.** After step two the coding-task render still holds about 24 KiB in sections that carry obligations or routing (`task-lifecycle` 4.1 KiB, `blocking` 3.2 KiB, `default-posture` 2.6 KiB, `workpad-setup` 1.8 KiB, `session-framing` 1.7 KiB, `autonomy` 1.6 KiB, `guardrails` 1.4 KiB, `state-routing` 1.4 KiB, the kept `pidash-cli` core and others) plus the kept parts of `analyze-and-scope`, `implementation`, `software.execute` and `ending-run`. Reaching 20 KiB would mean rewriting those texts, which is a content and guardrail decision, not extraction. v4 drops 20 KiB as a target; L-10 holds the rewrite, and O-2 asks Vinh to accept the revised targets.

### 5.5 Context packet and rework

Behind `PROMPT_CONTEXT_PACKET`.

**Comment digest rule.** Deterministic, with no time window and no parent anchor. A comment renders in full if any of these holds:

1. `speaker_type` is `human` or `integration`;
2. `speaker_type` is `agent` and it has no `speaker_agent_run_id` (it may come from an external reviewer or an older client);
3. `speaker_type` is `agent` and its run's `phase_kind` is review or test;
4. it was produced by the most recent run on the issue that posted any comment;
5. it is the last agent comment before a human comment.

Every other comment (system comments and older implementation-run agent comments) renders as one index line: number, author, time, comment UUID, the first 120 characters, and "full text: `pidash comment list <identifier>`". Folded comments stay excluded. Nothing human or review-authored is shortened, so no resolution metadata is needed. Digested bytes are recorded in the manifest.

**Guidance that matches the digest.** Today `sections/analyze-and-scope.md:12` says the full comment thread is shown in the prompt and that the in-prompt snapshot is authoritative. In digest mode that step renders a different sentence (a `{% if %}` branch on the packet flag, in both mirrors): human, integration and review comments are shown in full; older agent and system comments are one-line index entries; when a decision, dependency or requirement may sit in an indexed comment, read it with `pidash comment list {{ issue.identifier }}` before planning. Without the flag the text is unchanged. A test asserts that no digest-mode render contains "full comment thread" or "snapshot is authoritative".

**Hand-back brief.** On a hand-back into In Progress, the run band leads with "what changed since your last run": review verdict comments, PR state, reviewed head SHA, workpad phase, and an instruction to verify each finding against the current diff. Its inputs are the issue's comments, PR links and workpad, so it does not depend on `parent_run`.

**Parent payload.** Review and test prompts get selected fields of the parent `done_payload` (conclusion, result summary, branch, PR), found as today (`parent_run`, then the ticker's `resume_parent_run`), never usage, cost or telemetry.

**Resume** stays Later.

### 5.6 Pi Dash Agent tuning

- **Per-run tuning.** Tuning values are chosen at creation (from the experiment arm, or defaults), stored in the manifest `treatment.tuning`, and sent as an optional `assign.tuning` object. The runner applies them only to that run and echoes their hash in `execution.tuning_sha256`. The machine profile (`api.rs:108-120`) never carries saving keys, so a profile refresh cannot change an enrolled run's treatment. An unknown key is ignored and left out of the echoed hash, which makes the run a treatment mismatch rather than a silent change.
- **Keys.** `retry_continue_input` (runner behavior, 5.2). Engine keys: `tool_output_token_limit`, `model_auto_compact_token_limit`, `project_doc_max_bytes`, and a parallel-tool-call setting if one exists. Each engine key is verified against the pinned engine `rust-v0.153.4` before it can be offered, including how a per-run value reaches the engine: through `thread/start` config overrides or launch-time `-c key=value` overrides of the per-run engine process. A key with no per-run path is not offered and waits for engine-source work (open question 1). No permission key is accepted, and a test checks that the effective `thread/start` sandbox and approval values are unchanged.
- **Parallel tool calls.** If the setting exists and is off by default, it becomes one more key with the hypothesis "median requests per run drop by at least 10% with no change in merged rate or guardrail violations". If it is already on, record that and do nothing. If it is not configurable, defer.
- **Effort.** Baseline for every phase (D3). Recorded as `engine_default` in `execution.effort`. Plumbing for a per-phase effort is Later.
- Third-party agents get no tuning in this epic; personal config is never overwritten.

## 6. Data and API changes

- Wire: new `RunUsage` (`pidash-ipc/src/dto.rs`), optional `run_usage` on live state and on `RunCompleted`, `RunPaused`, `RunFailed`, `RunCancelled` (`runner/src/cloud/protocol.rs:171-212`). New runner failure reason `reference_bundle_invalid`. `TokenUsage` unchanged.
- `Assign` (`protocol.rs:288-303`, `matcher.py:492-517`, Rust `build_assign_msg`): optional `delivery_attempt`, `reference_bundle` and `tuning`. The `prompt` field carries either the stored prompt or the slim prompt.
- Cancel frames gain the reason value `experiment_stopped` (the field already exists).
- `AgentRun.usage`: optional keys from 5.1.1 and `treatment_match`. No migration.
- `AgentRun.prompt_manifest`: v3 envelope with a legacy reader. No migration.
- `AgentRun.delivery`: new nullable JSON column (5.4.3), one migration, Rust entity field. No backfill; null means "assigned before T-18b".
- `GitCodeReviewLink.merged_at`: new nullable column, one migration, one backfill.
- These two columns are the only schema changes.
- `RunFailureReason.REFERENCE_BUNDLE_INVALID` with policy `RETRY_FREE`, both backends.
- Issue agent status payload: `usage_breakdown` (`packages/api-client/src/contracts/issue-agent.ts`), both backends.
- Rate table file: `apps/api/pi_dash/experiments/model_rates.json`, append-only versions, embedded in Rust.
- Runtime configuration: `InstanceConfiguration` row `RUN_EXPERIMENTS` with `flag_defaults` (the default-on state of each prompt flag), `experiments` (each with status and G1 parameters) and `stopped_flags`. Changing it needs no deploy. No migration.
- Management commands (Django): `prompt_override_audit` and `canary_report` (read-only), `canary_stop` (writes; 7.4).
- Prompt flags: `PROMPT_LAYOUT_V2`, `PROMPT_MARKDOWN_CONTEXT`, `PROMPT_REFERENCE_BUNDLE`, `PROMPT_CONTEXT_PACKET`. A run's value is the canary arm's treatment if the run's ticket is in an active experiment's canary arm, else `flag_defaults`, else off. The value used is frozen in the manifest. Setting: `SLIM_LAYOUT_MIN_RUNNER_VERSION`.
- No new endpoint, no new table, no new CLI command beyond `pidash guide`.

## 7. Rollout and rollback

Implementation dependencies (section 10, "Deps") say what must merge first. Activation gates ("Gate") say what must be true before a flag changes model input anywhere. Code merges dark.

**Deviation from the brief, stated again.** The brief asks for "A/B on replayed tickets". At the owner's request, v4 uses live per-ticket canaries with concurrent controls instead, and keeps replay as Later L-01. Vinh confirms this as decision O-1. If he does not, L-01 becomes a prerequisite of G2 and the ticket list grows by the L-01 work.

### 7.1 G0, telemetry live and baseline collected

T-01 to T-08 deployed, then two weeks of baseline by agent kind, model, phase and work type, including each kind's coverage share and the baseline merged rate that sizes G1. G0 is a prerequisite for every saving canary. The only exception is validating telemetry itself, which changes no model input.

### 7.2 G1, decision rule recorded before any canary starts

Richard writes the experiment record (5.1.5) before the canary starts, with these parameters. Proposed defaults are in brackets; he confirms or changes each.

- Flag or tuning key, workspace, agent-kind scope [kinds whose G0 coverage share of `reported` runs is at least 95%, today expected to be the Pi Dash Agent, Codex and Claude Code], share [5,000 basis points].
- `rate_table_version` [the newest version at the start].
- Merged-rate tolerance, a non-inferiority margin [10 points]: the canary passes this rule when the one-sided 90% lower confidence bound (Newcombe hybrid score interval) of canary minus control merged rate is above minus the margin.
- Minimum sample [190 enrolled tickets per arm]. With a baseline merged rate near 70%, a one-sided 10% level and 80% power, the per-arm sample is about 757 for a 5-point margin, 190 for 10 points and 85 for 15 points. Richard replaces 70% with the G0 merged rate.
- Window [enrollment until both arms reach the minimum sample, at most 6 weeks] and maturity [14 days after enrollment ends].
- Cost rule [fixed]: canary cost per merged ticket is lower than control's, and the upper bound of a 90% percentile bootstrap interval of the canary-to-control ratio is below 1.0. The bootstrap resamples tickets within each arm (2,000 resamples, seed recorded in the record); tickets are the randomization unit, so this interval is valid.
- Guardrails [fixed, zero tolerance]: any canary run with `required_texts_ok = false`, a sandbox or approval mismatch in `execution`, or a human-reported incident (issue label `agent-guardrail`) on a canary ticket fails the experiment and triggers the emergency stop with the `cancel` policy.
- Running-agent stop policy [`cancel` for a guardrail stop, `finish` for any other stop].

### 7.3 G2, canary pass

The experiment runs for its window; `canary_report` runs at least weekly and on every guardrail event. After maturity, the closed report is computed once with the recorded `rate_table_version` and stored in the experiment record with the inputs' hash. A pass on the merged-rate, cost and guardrail rules makes the flag default-on for new runs. A fail, a stop, or an unavailable result at the end of the window turns the flag off and is recorded. Sendbacks, blocked runs, missing yields and compliance are in the report for Richard's judgment but are not hard gates.

Flags and keys under G0 to G2, one experiment at a time per workspace: `PROMPT_MARKDOWN_CONTEXT`, `PROMPT_LAYOUT_V2`, `PROMPT_REFERENCE_BUNDLE` (step one, then step two), `PROMPT_CONTEXT_PACKET`, `retry_continue_input`, and each engine tuning key. Rust context-packet and Markdown activation require T-15 deployed so both backends render the same text. No canary starts until T-27a is deployed, and T-18c and T-27b too if production routes runner sessions or orchestration to the Rust API (open question 4).

### 7.4 Emergency stop and rollback

`canary_stop <experiment_id> --reason <text> [--running cancel|finish]`, a Django management command. The default running policy comes from the record (7.2); a guardrail stop always uses `cancel`.

1. **Stop.** One transaction sets the experiment's status to `stopped` with time, reason and policy in `RUN_EXPERIMENTS`, and commits. From then on both backends treat the experiment as inactive at creation (new runs, including runs on enrolled canary tickets, get baseline settings and keep the ticket's arm for attribution), and both assignment paths refuse to assign a `QUEUED` run whose manifest names a stopped experiment and the canary arm. Such a run stays queued until step 2 handles it.
2. **Queued runs.** For each `QUEUED` canary run of the experiment, one transaction per run cancels it with error code `experiment_stopped` and creates a replacement through the creation path of the original's trigger (`orchestration/service.py:593`, `:732`, `:922` and their Rust twins) with baseline settings, `treatment.arm` copied from the original and `treatment.replaces_run_id` set. The replacement spends no tick, as a human's free entry does not. `_on_run_ended` (and the Rust clock) ignore a cancellation with code `experiment_stopped`, so the ticker neither schedules nor spends for it. The sweep runs a second time after `ASSIGN_DELIVERY_GRACE_SECS` to catch runs whose creation committed during the first pass, and the command prints both counts.
3. **Assigned and running runs.** With `cancel`, every non-terminal canary run of the experiment past `QUEUED` is moved to `CANCEL_REQUESTED` and sent a cancel with reason `experiment_stopped` through the existing cancel path (`runner/views/runs.py:41-61`), which is re-sent on poll and reconnect until acknowledged (3.8). The cancel terminal message carries `run_usage`, so cost is kept. A cancelled run keeps the clock ticking (3.8), and the next tick creates a baseline run. With `finish`, those runs complete normally and later runs are baseline.
4. **Attribution.** Nothing is deleted or rewritten. Cancelled runs, replacements and later baseline runs on canary tickets all count in the canary arm under intention-to-treat, and the report marks the stop. The verdict of a stopped experiment is "failed (stopped)".

Rollback of a default-on flag uses the same command with a flag instead of an experiment: `canary_stop --flag <name> --reason <text> [--running cancel|finish]`. Step 1 turns the flag's default off and adds it to `stopped_flags` in `RUN_EXPERIMENTS`; the assignment guard also refuses queued runs whose manifest `flags` has a stopped flag on; steps 2 and 3 apply to every non-terminal run with that flag on in its manifest. A guardrail reason again forces `cancel`. Existing snapshots are never edited; the manifest's treatment and flags and the delivery record trace any regression to its arm and attempt.

## 8. Risks

- **Shorter prompts cause misses or sendbacks.** One sendback is a full fresh run, so a small rise can cancel the saving. Mitigation: required texts checked at every assignment attempt, the merged-rate rule, rework cost inside cost per merged ticket, sendbacks and guide reads in the report.
- **Tickets that share runners or parents interfere.** Ticket randomization assumes one ticket's treatment does not change another's outcome. Child issues of one parent, and tickets queued on the same runner, can interact. Mitigation: the effect is usually small for prompt changes; the report lists parent groups split across arms so Richard can judge.
- **The sample is too large for AI Republic's volume.** A 10-point margin needs about 190 tickets per arm. Mitigation: the window ends at 6 weeks with "unavailable"; Richard can choose a wider margin knowingly, and the margin is written in the record before the start.
- **Missing cost makes verdicts unavailable.** One enrolled ticket with an unknown cost blocks the economic verdict. Mitigation: agent-kind scope at enrollment, the zero-request rule, retry-exhausted and Claude failure paths that keep usage. Muse and Cursor tickets cannot enter a canary until open question 5 is answered.
- **Cross-run caching may not happen** (per-thread cache keys, gateway routing, agent-added prefixes). Mitigation: size cuts pay regardless; first-request `cache_read` shows cache effect directly.
- **Backend drift** already exists. Mitigation: T-12 first, mirror tests in PR CI, contract tests for every prompt and delivery change.
- **Old runners never get slim prompts.** That only limits the saving. The delivered share is in the compliance part of the report, and runners upgrade forward.
- **Delivery envelopes cost storage.** `AgentRun.delivery.envelope` stores a second copy of the prompt (about 100 KB) per run. Mitigation: one copy per run, earlier attempts kept only as summaries; Richard can add a later cleanup for terminal runs older than the longest experiment window.
- **The stop races with creation.** A run created while the stop commits could miss the first sweep. Mitigation: the assignment guard refuses it, and the second sweep replaces it.
- **Request counts are a floor for the Codex family.** Labelled "at least N"; cost does not depend on request counts.
- **Output caps lower quality**, notably on thinking models. Mitigation: default off, one key per canary; truncated output keeps head, tail and exit status.
- **False precision in cost.** Modeled and reported values are separate; missing rates, tiers, coverage or merge times give "unknown" or "unavailable", never a number; closed reports use their pinned rate version.
- **Digest hides context an agent needed.** Only agent and system comments are shortened; the latest run's and pre-human-reply comments stay full; the guidance tells the agent to read indexed comments when they may matter.
- **Unknown merge times on old links.** Only links merged before T-08 and missing `merged_at` in `metadata` are affected; experiments start after G0, which is after T-08.

## 9. Test plan

Every ticket starts with its named failing test (Red), then the minimum change (Green), then refactor. A ticket's test only exercises code in that ticket and its listed dependencies.

- Python: `apps/api/pi_dash/tests/unit/prompting/` (`test_composer.py`, `test_context.py`, `test_validation.py`, `test_manifest.py`), `tests/unit/runner/`, `tests/unit/integrations/`, `tests/unit/experiments/`, `tests/contract/` (runs against both backends where noted with `[rust]`).
- Rust runner: unit tests beside `observability.rs`, `supervisor.rs`, `codex_config.rs`, `claude_code/bridge.rs`, `muse_code/bridge.rs`, `cursor_agent/`, `cli/`; fixtures under `runner/tests/fixtures/`.
- Rust API: unit tests in `crates/services/src/prompting/`, `orchestration/`, `runner_sessions/`, `runner_runs/`, `bin/pidash-api`, and `rust-api/contract-tests/` for layout, manifest, delivery, stop, failure policy, usage ingest and the status payload.
- Web: `AgentStatusPanel.test.tsx`, `agentStatus.test.ts`.
- Prompt fixtures (T-18a): one baseline fixture context, and per recipe and work type a small JSON fixture of rendered bytes, per-section bytes and sha256. Full prompt text is not committed, so fixtures stay a few dozen lines each.
- Cross-cutting cases: fresh-thread first frame with several requests; duplicate, reordered, identical-sized consecutive and retry frames; cache creation charged once at the write rate; tiered write rate without a tier split gives unknown; reasoning not billed twice; zero-request failure counts zero; unsupported run with tool counts and turns; failed Claude run with cost, turns and tiers; terminal turns surviving live-state replacement; PR merged after the cutoff; link deleted after the cutoff; merged link with no merge time; arm independent of pod and profile; failed or mismatched canary ticket still counted; any missing cost gives an unavailable economic verdict; review waiting ticks are not sendbacks; closed report reproducible with its pinned rate version; override edited between creation and assignment; old, private and pinned-only runners receiving inline prompts; slim run reassigned to an old runner; redelivery sends the stored envelope byte for byte; slim plus bundle expanding to the stored prompt; corrupt bundle; stop blocks queued canary dispatch and creates baseline replacements; running canary cancelled under the `cancel` policy keeps its usage; parent title changed between runs; volatile override ends the stable prefix; legacy override missing a required text; digest-mode render never claims the full thread; template syntax in issue text stays literal; reference files never appear in `git status`; effective `thread/start` posture unchanged.
- A golden size test per recipe against the 5.4.4 budgets, so growth is caught in review.

## 10. Tickets

Areas: Python API (`apps/api`), Rust API (`rust-api`), Rust runner (`runner/`, `pidash-ipc/`), web (`apps/web_new`, `packages/api-client`). "Lines" is the estimated changed lines including tests, fixtures and both backends' tests where the ticket touches both; each ticket is one PR under about 400. "Deps" are implementation dependencies. "Gate" is the activation gate from section 7; a dash means the ticket changes no model input and ships live. "G0-G2" means the flag or key it adds is activated only through a canary. A `[rust]` suffix means the same contract test run against the Rust backend.

**Phase 1: measure (all needed for G0)**

| ID | Title | Area | Deps | Gate | Lines | Named failing test |
|---|---|---|---|---|---|---|
| T-01 | `RunUsage` wire envelope with cache-write tiers, turns and execution identity (including `delivery_attempt`) | Rust runner | none | - | 240 | `pidash_ipc::dto::tests::run_usage_without_tokens_round_trips_with_tiers_turns_tool_calls_and_execution` |
| T-02 | Store and merge `RunUsage` and turns in both backends | Python API, Rust API | T-01 | - | 380 | `tests/contract/runner/test_run_usage_ingest.py::test_terminal_turn_count_survives_live_state_replacement` |
| T-03 | Run-scoped meter: zero baseline for fresh threads, ordering, dedup, requests, tool calls, turns, zero-request rule, spans retries | Rust runner | T-01 | - | 390 | `daemon::observability::tests::fresh_thread_first_frame_keeps_all_cumulative_tokens` |
| T-04 | Claude Code usage by `message.id`; usage, cost, turns and cache-write tiers on success and failure | Rust runner | T-03 | - | 280 | `claude_code::bridge::tests::error_result_preserves_usage_cost_turns_and_cache_write_tiers` |
| T-05 | Muse, Cursor, Grok, OpenClaw: tool calls by call id, turns where reported, unsupported coverage | Rust runner | T-03 | - | 250 | `muse_code::bridge::tests::missing_token_fields_report_unsupported_not_zero` |
| T-06a | Rate table file (append-only versions, four disjoint categories, write tiers) and modeled-cost function, both backends, shared fixture | Python API, Rust API | T-02 | - | 300 | `tests/contract/app/test_modeled_cost.py::test_cache_creation_is_charged_once_at_write_rate` |
| T-06b | Per-run and per-ticket `usage_breakdown` in the status payload, both backends | Python API, Rust API, api-client | T-06a | - | 340 | `tests/contract/app/test_issue_usage_breakdown.py::test_incomplete_run_shows_unknown_cost_not_zero_and_missing_rate_is_unknown` |
| T-07 | Usage breakdown UI on the issue | web | T-06b | - | 300 | `AgentStatusPanel.test.tsx > shows per-run cost, rework split, coverage line and unknown instead of zero` |
| T-08 | `GitCodeReviewLink.merged_at`: set once by every writer, backfill from metadata, both backends | Python API, Rust API | none | - | 320 | `tests/contract/integrations/test_code_review_merged_at.py::test_merged_at_set_once_from_provider_and_kept_on_later_updates` |

**Phase 2: experiment plumbing and emergency stop (all needed before any canary)**

| ID | Title | Area | Deps | Gate | Lines | Named failing test |
|---|---|---|---|---|---|---|
| T-09 | Manifest v3 envelope, legacy reader, creation-time snapshot of prompt and flags | Python API, Rust API | none | - | 340 | `test_manifest.py::test_legacy_list_and_v2_dict_read_and_flag_flip_after_creation_keeps_stored_prompt_and_flags` |
| T-10 | `RUN_EXPERIMENTS` row (flag defaults, experiments), per-ticket hash arm, per-run flag resolution, intended treatment, `treatment_match`, both backends | Python API, Rust API | T-02, T-09 | - | 390 | `tests/contract/experiments/test_assignment.py::test_ticket_arm_is_sticky_hash_independent_of_pod_and_profile` |
| T-11a | Cohort selection: enrolled tickets per arm, counted runs, merge outcome at cutoff, hand-back events from `IssueActivity` | Python API | T-08, T-10 | - | 300 | `tests/unit/experiments/test_cohort.py::test_merge_after_cutoff_does_not_change_closed_cohort` (same file: `test_review_waiting_ticks_are_not_sendbacks`) |
| T-11b | Evaluation: intention-to-treat metrics, Newcombe bound, ticket bootstrap, compliance, unavailable reasons | Python API | T-06a, T-11a | - | 330 | `tests/unit/experiments/test_evaluate.py::test_treatment_failure_cannot_improve_canary_by_excluding_ticket` (same file: `test_missing_cost_on_any_enrolled_ticket_makes_economic_verdict_unavailable`) |
| T-11c | `canary_report` command: reads the record, uses the pinned rate version, stores the closed report with its input hash, guardrail counts | Python API | T-11b | - | 230 | `tests/unit/experiments/test_canary_report_command.py::test_closed_report_is_reproducible_with_pinned_rate_table_version` |
| T-27a | Emergency stop, Django: `canary_stop` for an experiment or a flag, baseline creation when stopped, assignment guard, cancel and baseline replacement of queued runs, running-run policy, ticker ignores `experiment_stopped` | Python API | T-10 | - (needed before any canary) | 390 | `tests/contract/experiments/test_canary_stop.py::test_failed_canary_prevents_queued_treatment_dispatch` |
| T-27b | Emergency stop, Rust twin: baseline creation when stopped, assignment guard, replacement creation, clock ignores `experiment_stopped` | Rust API | T-27a | - (needed before any canary if Rust is live, open question 4) | 280 | `tests/contract/experiments/test_canary_stop.py::test_failed_canary_prevents_queued_treatment_dispatch[rust]` |

**Phase 3: correctness and Markdown (T-12 and T-14 may start at once)**

| ID | Title | Area | Deps | Gate | Lines | Named failing test |
|---|---|---|---|---|---|---|
| T-12 | Restore Django and Rust section mirror parity and run mirror tests on PRs | Python API, CI | none | - | 120 | existing `rust-api/contract-tests/prompting/test_data_plane_dual_engine.py::test_mirrors_are_byte_identical_to_sources` (fails today) |
| T-13 | Markdown issue, parent and comment context, Django | Python API | T-09, T-12 | G0-G2 | 260 | `test_context.py::test_description_parent_and_comments_keep_markdown_and_template_syntax_stays_literal` |
| T-14 | Rust converter seam, installed in the `pidash-api` binary for `serve` and `worker` | Rust API | none | - | 150 | `pidash_api_bin::tests::serve_and_worker_install_markdown_converter` |
| T-15 | Rust HTML projections and conversion for issue, parent and comments in `services`, the API store and the jobs store | Rust API | T-13, T-14 | with T-13 | 380 | `rust-api/contract-tests/prompting/test_markdown_context.py::test_live_tick_prompt_preserves_issue_parent_and_comment_markdown` |

**Phase 4: prompt layout and size**

| ID | Title | Area | Deps | Gate | Lines | Named failing test |
|---|---|---|---|---|---|---|
| T-16 | Required-text registry, default and override path test, save-time refusal, `prompt_override_audit` | Python API, Rust API | T-12 | - | 380 | `test_validation.py::test_activation_reports_legacy_override_missing_required_block` |
| T-17 | Layout v2: static-first, run-context tail, `$PIDASH_RUN_ID`, ancestor text in the run band, overrides inline, volatile-override detection and `stable_prefix_end`; adds the v2 path to the required-text test | Python API, Rust API | T-09, T-16 | G0-G2 | 390 | `test_composer.py::test_v2_static_band_identical_for_same_recipe_runs_of_one_ticket_after_parent_title_change` (same file: `test_volatile_override_ends_stable_prefix_before_it`) |
| T-18a | Extraction mechanism: marker parsing, spans at creation, slim composition and exact expansion, baseline fixture context and per-recipe size and hash fixtures, both backends | Python API, Rust API | T-09, T-16 | - | 370 | `tests/contract/prompting/test_slim_composition.py::test_slim_prompt_plus_bundle_expands_to_stored_prompt_for_every_recipe_fixture` |
| T-18b | Per-attempt delivery, Django: `AgentRun.delivery` column, derive in the assignment transaction, persist the exact envelope, replay on redelivery, close the attempt on requeue, delivery fields in `treatment_match` | Python API | T-10, T-18a | - | 380 | `tests/contract/runner/test_assign_delivery.py::test_slim_assignment_reassigned_to_old_runner_gets_inline_with_correct_delivery_identity` |
| T-18c | Per-attempt delivery, Rust: `AssignmentFacts` gains runner version and manifest, `plan_assignment` writes the delivery, `session_service.rs:1427` replays it, lifecycle requeue closes the attempt | Rust API | T-18b | - | 340 | `tests/contract/runner/test_assign_delivery.py::test_slim_assignment_reassigned_to_old_runner_gets_inline_with_correct_delivery_identity[rust]` |
| T-19 | Runner: accept slim assign, echo `delivery_attempt`, write bundle outside the worktree, check hash, `pidash guide` with read events, `reference_bundle_invalid` reason | Rust runner | T-01, T-18b | - | 350 | `cli::guide::tests::corrupt_bundle_fails_before_agent_start_and_bundle_absent_from_worktree` |
| T-20 | `reference_bundle_invalid` failure policy and inline delivery of the retry, both backends | Python API, Rust API | T-18c, T-19 | - | 250 | `tests/contract/orchestration/test_bundle_failure_policy.py::test_invalid_bundle_queues_one_free_retry_delivered_inline` |
| T-21 | Slim step one: markers at the 5.4.4 step-one ranges in both mirrors, index lines, budgets 66, 35 and 41 KiB; adds the slim path to the required-text test | Python API, Rust API | T-17, T-20 | G0-G2 | 200 | `test_composer.py::test_step_one_budgets_66_35_41_kib_and_slim_delivery_keeps_required_texts` |
| T-22 | Slim step two: markers at the four coding-task ranges, budget 57 KiB | Python API, Rust API | T-21 | G0-G2, after T-21 is default-on | 150 | `test_composer.py::test_step_two_coding_task_budget_57_kib_and_slim_delivery_keeps_required_texts` |
| T-23 | Context packet, Django: noise-only comment digest, digest-mode guidance in `analyze-and-scope` (both mirrors), hand-back brief, trimmed parent payload | Python API | T-13, T-17 | G0-G2 | 380 | `test_context.py::test_digest_keeps_human_and_review_comments_full_and_never_claims_full_thread` |
| T-24 | Context packet, Rust twin | Rust API | T-15, T-23 | with T-23 | 350 | `prompting::context::tests::comment_digest_matches_python_fixture` |

**Phase 5: Pi Dash Agent tuning**

| ID | Title | Area | Deps | Gate | Lines | Named failing test |
|---|---|---|---|---|---|---|
| T-25 | Per-run `assign.tuning` from the snapshot, echoed hash, first key `retry_continue_input` | Rust runner, Python API, Rust API | T-03, T-10 | G0-G2 | 330 | `daemon::supervisor::tests::accepted_prompt_is_not_appended_again_on_quota_retry` |
| T-26 | Engine tuning keys verified against `rust-v0.153.4` (including the parallel-tool-call check), applied per run, posture unchanged | Rust runner, Python API, Rust API | T-25 | G0-G2 per key | 300 | `pidash_agent::codex_config::tests::applies_run_tuning_keys_only_when_present_and_never_permissions` |

Core: 33 tickets (v3 had 26; T-06, T-11 and T-18 were split and T-27a and T-27b were added). Two schema changes (`merged_at`, `AgentRun.delivery`), no new table or endpoint, one new CLI command (`pidash guide`).

**Later, not in this epic's core**

| ID | Title | Why later | Entry condition |
|---|---|---|---|
| L-01 | Offline replay harness (capture, isolated execution, comparison), the brief's "A/B on replayed tickets" | Deferred at the owner's request (O-1); live canaries cover activation. When built it must freeze the pre-work repository SHA and project state, isolate each repetition's worktree and external write targets, create fresh PR identities and get fresh review and merge outcomes in a sandbox, and depend on T-10 for treatment identity. First test: `test_repeated_replay_starts_at_original_sha_without_original_pr_outcome`. | Vinh asks for it under O-1, or a saving cannot be canaried safely live. |
| L-02 | Request-count cap with `request_budget_exhausted` | Needs p95 from G0 data. Defined now: new reason in runner wire, Django and Rust taxonomies, policy `NEEDS_HUMAN` (stop the clock with `FAILURE_NEEDS_HUMAN`, never a completion, never an auto-launched run). Test `test_request_budget_exhaustion_stops_future_ticks` on both backends. | G0 p95 known, then G0-G2. |
| L-03 | Hand-back session resume experiment with one fresh fallback | Needs a captured pre-run baseline (5.1.2), session retention data (open question 6) and exclusive worktree ownership. | After T-23 passes. |
| L-04 | Cross-issue static band via `$PIDASH_ISSUE_IDENTIFIER` (includes exporting the variable from `assign`) | Only pays if the provider caches across sessions. | Open question 2 answered. |
| L-05 | Effort plumbing and per-phase opt-in | D3 keeps baseline effort; no phase change is planned. | A phase with data suggesting a lower effort. |
| L-06 | Cohort "cost per merged ticket" analytics card and Rust twin of the report | The canary report is internal tooling; the product shows per-run and per-ticket cost. | After the first canary verdicts. |
| L-07 | External reviewer cost ingestion (D5) | Reviewer lives outside this repo. | D5 reversed. |
| L-08 | Muse output-cap and web-tools flags; observation masking beyond engine config | Needs T-05 data and the engine source (open question 1). | Data and source available. |
| L-09 | Mechanical move of the Rust HTML converter into a shared crate | Optional cleanup; the seam is enough. | Any time. |
| L-10 | Rewrite of obligation-bearing sections toward a 20 KiB coding-task instruction budget | Extraction alone reaches about 55 KiB (5.4.4); the rest needs content and guardrail decisions by the section owners. | Step two default-on and Richard wants a further cut. |
| L-11 | Cleanup of `delivery.envelope` for old terminal runs | Storage only; not needed for correctness. | Storage growth becomes a concern. |

## 11. Open questions

1. Where are the source and release contract of the shipped Pi Dash Agent binary, which config keys does `rust-v0.153.4` accept (`tool_output_token_limit`, `model_auto_compact_token_limit`, `project_doc_max_bytes`, any parallel-tool-call setting), and does it accept per-thread config overrides or `-c` overrides at launch?
2. Does the OpenHub gateway cache prefixes across conversations, pass `prompt_cache_key`, or return per-request cost, quota and request ids? Does Codex set the cache key per thread?
3. Who owns `model_rates.json`, which providers in it price cache writes by duration, and should OpenHub report Pi Dash Agent cost directly instead?
4. Which prefixes does production route to the Rust API today? It decides whether each Rust twin (notably T-18c and T-27b) is on the critical path of the first canary.
5. Do supported Muse and Cursor versions expose run-scoped token usage (export, trace, or a JSON record)?
6. What session retention exists on developer machines, for L-03?

### Decisions

- **D1. Telemetry storage.** Resolved: JSON keys on `AgentRun.usage`, with the rules of 5.1.2 fixed before baseline collection.
- **D2. Reference delivery.** Recommended: server-rendered bundle defined by spans in the creation snapshot, derived per assignment attempt for the chosen runner, the exact frame persisted in `AgentRun.delivery` and replayed verbatim, read locally by `pidash guide`, with the `RETRY_FREE` policy and inline delivery after an invalid bundle. Needs approval (O-4).
- **D3. Effort.** Resolved: baseline effort for every phase; plumbing is Later.
- **D4. Budget definition.** Recommended: generic instruction bytes on the baseline fixture context, excluding the run band and overrides, with the measured budgets of 5.4.4 (66, 35 and 41 KiB after step one; 57 KiB for coding-task after step two). 20 KiB dropped. Needs acceptance (O-2).
- **D5. External reviewer cost.** Recommended: defer; every total says "excludes external review". Needs confirmation (O-5).
- **D6. Decision rule (G1).** Recommended: per-ticket randomization, intention-to-treat over every enrolled ticket, merged-rate non-inferiority, zero canary guardrail violations, cost ratio interval over tickets, unavailable economics when any cost is missing, parameters recorded per experiment before the start. Needs Richard's parameters (O-3).

## Appendix: Pair-wise design record

| Stage | Date | Author | Artifact | Status |
|---|---|---|---|---|
| 1. Brief | 2026-11-16 | Vinh | `.eff/brief.md` | done |
| 2a. Blind draft | 2026-11-16 | Claude | `pidash-claude-draft.md` (29 tickets) | done |
| 2b. Blind draft | 2026-11-16 | Codex | `pidash-codex-draft.md` (31 tickets) | done |
| 3. Merge to v1 | 2026-11-16 | Claude | `.eff/v1.md` (22 core + 2 gated tickets, 6 decisions) | done |
| 4a. Critique round 1 | 2026-11-16 | Codex | `pidash-review-1.md` (9 MUST, 4 SHOULD) | done |
| 4b. Revision to v2 | 2026-11-16 | Claude | `.eff/v2.md` (28 core + 3 later tickets) | done |
| 4c. Critique round 2 | 2026-11-16 | Codex | `pidash-review-2.md` (9 MUST, 4 SHOULD) | done |
| 4d. Revision to v3 | 2026-11-16 | Claude | `.eff/v3.md` (26 core + 9 later) | done |
| 4e. Critique round 3 (last) | 2026-11-16 | Codex | `pidash-review-3.md` (8 MUST, 3 SHOULD) | done |
| 4f. Final fix pass to v4 | 2026-11-16 | Claude | `.eff/v4.md`, copied to `pidash-technical-design.md` (33 core + 11 later) | done |
| 5. Decisions for the owner | | Vinh, Richard | this appendix, O-1 to O-10 | pending |

### Totals across the three rounds

| Round | MUST | SHOULD | Accepted | Accepted by removal or deferral | Rejected |
|---|---|---|---|---|---|
| 1 | 9 | 4 | 13 | 0 | 0 |
| 2 | 9 | 4 | 12 | 1 (replay harness moved to Later) | 1 sub-point (tick credit for an invalid bundle) |
| 3 | 8 | 3 | 11 | 0 | 0 |
| All | 26 | 11 | 36 | 1 | 1 sub-point |

Every code claim in all three rounds was checked against `9cc85ccd0` and held.

### Round 1 disposition summary

Codex raised 9 MUST and 4 SHOULD findings. All 13 were accepted. Two were accepted with a simpler fix than proposed: MUST 4 (no `pidash comment get`; the digest only shortens agent and system noise and points to the existing `pidash comment list`) and MUST 8 (the Rust converter stays in place behind a small injection seam instead of a 900-plus-line move). On decisions, v2 adopted Codex's position on D1 (dedup specified now) and D3 (baseline effort until evidence supports an opt-in), kept D2, D4 and D5 as recommended (D4 made numeric), and turned D6 into an activation requirement owned by Richard. Ticket count went from 22 core to 28 core because every ticket got a line budget under 400.

### Round 2 disposition summary

Codex raised 9 MUST and 4 SHOULD findings. Following the owner's direction to cut machinery rather than add it, v3 accepted 12 findings, accepted MUST 3 by deferring the replay harness to Later (with Codex's requirements written into the Later entry), and rejected one sub-point: crediting back the dispatch tick for an invalid bundle, because existing platform failures already spend that tick and get free retries. The main changes: a live canary with sticky per-ticket arms; a write-once `merged_at` column; assignment-time delivery derived from one stored prompt plus extraction spans instead of capability gating at creation; tuning moved from the machine profile into the run snapshot; persisted turns; Rust Markdown loading in every loader with the converter installed in the single binary. Ticket count went from 28 core to 26 core plus 9 Later items.

### Round 3 disposition summary

Codex raised 8 MUST and 3 SHOULD findings; all 11 were accepted (table in section 0). The measurement findings (MUST 1, MUST 2, SHOULD 11) were closed by randomizing tickets instead of pods or workspaces, computing every outcome over every enrolled ticket, reporting compliance separately, returning an unavailable economic verdict when any cost is missing, and counting sendbacks only from explicit review-to-implementation state changes, outside the hard gate. MUST 3 got four disjoint billable categories with cache-write tiers and a pinned rate version. MUST 4 was closed by measuring every recipe and setting reachable budgets, with 20 KiB dropped. MUST 5 got a per-attempt delivery record with a persisted envelope and verbatim replay. MUST 6 got an emergency stop that blocks, cancels and recreates queued canary runs and stops running ones by policy. MUST 7 split T-06, T-11 and T-18. MUST 8 is stated as a requirements change for Vinh. On decisions, Codex agreed with D1, D3 and D5, agreed with D2 under conditions that v4 meets, agreed with the D4 definition but not its feasibility (now measured), and disagreed with D6 as written (now rebuilt).

### v4 fix pass

No review round follows v4, so this pass also checked v4 for self-consistency: every ticket's dependencies exist and come earlier in the order, every named test exercises only its ticket and dependencies, the measured byte numbers in 5.4.4 come from one rendering of the section files at `9cc85ccd0`, and the extraction ranges were checked not to cross a Jinja block and not to contain an obligation from 5.4.1. Two facts found in this pass changed the design beyond the findings: runs can be re-queued and reassigned in place (`run_lifecycle.py:247-295`), and reconnect redelivery rebuilds the frame (`session_service.py:474-525`, Rust `session_service.rs:1427`); both are handled by the per-attempt delivery record. The emergency stop also needed runtime flag defaults, so the prompt flags' default-on state moved from Django settings into the `RUN_EXPERIMENTS` configuration row.

### Decisions for the owner

Everything that still needs Vinh or Richard. Nothing here blocks Phase 1 (telemetry), Phase 3 (correctness and Markdown) or the dark merge of any ticket; O-1 to O-4 block the first canary.

1. **O-1 (Vinh): live canaries instead of replayed-ticket A/B.** The brief asks for "A/B on replayed tickets". v4 uses live per-ticket canaries with concurrent controls as an owner-requested streamlining and keeps replay as Later L-01. Confirm the replacement, or bring L-01 back as a prerequisite of G2.
2. **O-2 (Vinh): revised size targets.** Accept the measured budgets (coding-task 66 KiB, review 35 KiB, test 41 KiB after step one; coding-task 57 KiB after step two) and dropping 20 KiB to L-10. This includes moving two overridable `analyze-and-scope` passages (the child-issue procedure and the comment-writing guidance with its examples) behind `pidash guide` in step two.
3. **O-3 (Richard): G1 parameters for each experiment**, recorded before it starts: agent-kind scope, share, merged-rate margin, minimum sample, window, maturity, `rate_table_version` and running-agent stop policy. Proposed defaults: Pi Dash Agent, Codex and Claude Code; 50%; 10 points; 190 tickets per arm (resized from the G0 merged rate); up to 6 weeks; 14 days; newest version; `cancel` on a guardrail stop, `finish` otherwise.
4. **O-4 (Richard, with Vinh): D2 delivery and the two schema changes.** Approve the server-rendered bundle with per-attempt delivery, the new `AgentRun.delivery` column (one extra copy of the prompt per run) and the `GitCodeReviewLink.merged_at` column.
5. **O-5 (Richard): D5.** Confirm that external reviewer cost stays out of this epic and that every total is labelled "excludes external review".
6. **O-6 (Richard): rate table.** Name the owner of `model_rates.json`, supply the first version's rates including cache-write tiers where providers price them, and decide whether OpenHub should report Pi Dash Agent cost directly (open question 3).
7. **O-7 (Richard): Rust routing.** Say which prefixes production routes to the Rust API (open question 4), which decides whether T-18c and T-27b must ship before the first canary.
8. **O-8 (Richard): engine source.** Answer open question 1 before T-26 starts; until then no engine tuning key is offered.
9. **O-9 (Richard): emergency stop operators.** Name who may run `canary_stop` and confirm the `agent-guardrail` label as the way humans report a guardrail incident.
10. **O-10 (Richard): remaining facts.** Answers to open questions 2 (gateway caching), 5 (Muse and Cursor usage) and 6 (session retention); they decide L-04, which agent kinds can enter canaries, and L-03.

Note: paths under `.eff/` and the review file names in this record are working files from the design session; they are not committed.
