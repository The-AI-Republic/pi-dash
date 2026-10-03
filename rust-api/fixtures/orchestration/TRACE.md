# TRACE — D-12 orchestration fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(no drift: the 10 source files are byte-identical at the fixture base
`refs/remotes/origin/rust-dev @ e27b6a17`).
How values were produced: FX-ORCH-01 ran the real units under
`pi_dash.settings.test` with no database (throwaway generator
`/tmp/fx_orch_01.py`); state/run inputs are duck-typed stand-ins
(`SimpleNamespace` with the `group`/`name` / `done_payload`/`phase_kind`
attributes the functions read) — every recorded output is the real
function's return value. DB-backed groups (FX-ORCH-02..08) run against
scratch database `pidash_524_scratch` (local postgres, migrated from this
tree) with the clock frozen and seam mocks noted per fixture; their
generators are throwaway `/tmp/fx_orch_0N.py` scripts.

OUT OF SCOPE (never re-recorded; mocked at seams where callers touch them):
`core/user_settings.py` (F-10), `core/permissions.py` decisions (F-06),
`IssueAgentTicker` model helpers (D-10), relation-mapper pure fns
(`services/src/assistant/tools_issues.rs`), `core/agent_execution.py`
(D-11), `runner/` models+services (D-13…D-15), `bgtasks/` (D-07…D-10),
`cloud_agent/`+`managed_runner/` (D-11), `prompting/` (D-04).
Reading material only: `tests/unit/orchestration/`.

## FX-ORCH-01 types (no DB)

- `fx01_types/phases.golden.json` —
  `orchestration/agent_phases.py:32-46` (`CadenceFields`),
  `:53-66` (`CADENCE_FIELDS` verbatim), `:71` (`DEFAULT_CADENCE_KEY`),
  `:74-114` (`PhaseConfig`), `:117-146` (`PHASES` verbatim),
  `:149-162` (`is_ticking_state`), `:165-176` (`phase_config_for`),
  `:179-188` (`template_name_for`), `:191-200` (`cadence_fields_for`),
  `:203-212` (`auto_pauses_on_cap`), `:215-219`
  (`cadence_fields_by_group`), `:222-224`
  (`ticking_state_names_by_group`); `KIND_CODING_TASK` context from
  `prompting/recipes.py` (fallback value only, not ported here).
  Vectors cover all 8 `StateGroup` members (3 registered, custom name in
  each ticking group, every non-ticking group), an unknown group, and the
  `None` state.
- `fx01_types/done_signal.extract_fence.golden.json` —
  `orchestration/done_signal.py:27-30` (`FENCE_RE`), `:45-53`
  (`extract_fence`): empty/None/no-fence → `None`, last-fence-wins,
  space/tab-tolerant fences, unclosed fence → `None`, mid-line triple
  backtick inside a JSON string does not terminate, blank body → `""`.
- `fx01_types/done_signal.parse.golden.json` —
  `orchestration/done_signal.py:32` (`VALID_STATUSES`), `:56-87`
  (`parse`): all 4 statuses ok; error strings verbatim for missing fence,
  invalid JSON, non-object payload, bad/missing status, and
  paused-without-question (incl. empty-string question).
- `fx01_types/done_signal.normalize.golden.json` —
  `orchestration/done_signal.py:90-138` (`_normalize`): minimal-payload
  defaults golden + rich-payload golden (partial sections defaulted,
  unknown keys dropped).
- `fx01_types/ticker_primitives.golden.json` —
  `orchestration/scheduling.py:100-106` (`TickerEventKind`),
  `:109-183` (`TickerEvent` convenience constructors incl. stamped
  trigger values), `:186-202` (`TickerDecision` defaults), `:46`
  (`PAUSED_STATE_NAME`), `:54` (deprecated `DELEGATION_STATE_NAME`);
  `orchestration/service.py:64` (deprecated `DELEGATION_STATE_NAME`),
  `:67-72` (`TransitionOutcome` defaults), `:75-81`
  (`ContinuationOutcome` defaults).
- `fx01_types/outcomes.golden.json` —
  `orchestration/scheduling.py:74-78` (`OUTCOME_*`), `:79-87`
  (`RUN_OUTCOMES`), `:89` (`STOPPING_OUTCOMES`), `:94-97`
  (`_LEGACY_OUTCOME_ALIASES`), `:333-350` (`normalize_outcome` matrix:
  verbatim + case/whitespace variants, `noop` × 5 phase kinds, legacy
  aliases + variants, unknown/empty strings → `None`, 7 non-string
  inputs → `None`), `:353-357` (`outcome_for_run`: statuses,
  `noop` × kinds, missing/None/non-dict payloads), `:360-371`
  (`default_outcome_for_kind`: `""`/`coding-task` → `progressed`, else
  `done`).
- `fx01_types/dispatch_constants.golden.json` —
  `orchestration/scheduling.py:863-865` (`TRIGGER_*`, sourced from
  `AgentRunTrigger` — enum map recorded alongside), `:868`
  (`_MACHINE_TRIGGERS`), `:1223-1225` (`RUN_AI_*` refusal reasons),
  `:742-751` (`WAIT_*` results), `:754` (`WAIT_ACTIVITY_FIELD`),
  `:758-763` (`_WAIT_REARMABLE_DISARMS`), `:254-265`
  (`_TICKER_CLOCK_FIELDS`).

## FX-ORCH-02 reads (DB)

Generator `/tmp/fx_orch_02.py` against scratch database
`pidash_524_scratch` (fresh `migrate`, clock frozen at
`2026-10-03T12:00:00Z` where timestamps are recorded, seeded PKs fixed
for byte-stable SQL). Seed layout: workspace `fx2-workspace`,
projects `FX0` (sections A–D issues; the queryset user is not a
member, so they stay out of section E sets), `FX2` (member project),
`FX2B` (non-member project); all issues in `Todo`/`unstarted` so the
post_save transition handler takes the no-dispatch path.

- `fx02_reads/active_run.sql` —
  `orchestration/service.py:84-103` (`_active_run_for`): executed
  `SELECT` via `CaptureQueriesContext` — `work_item_id = … AND
  status IN (queued, assigned, waiting_for_worktree, running,
  cancel_requested, awaiting_approval, awaiting_reauth) ORDER BY
  created_at DESC LIMIT 1`.
- `fx02_reads/latest_prior_run.sql` —
  `orchestration/service.py:106-107` (`_latest_prior_run`): executed
  `SELECT` — `work_item_id = … ORDER BY created_at DESC LIMIT 1`.
- `fx02_reads/active_run.rows.json` — one issue per active status
  (all 7 returned by `_active_run_for`); active+newer-terminal issue
  (`_active` skips the terminal, `_latest` returns it);
  paused-only issue (`PAUSED_AWAITING_INPUT` is non-terminal but NOT
  in the active set → `_active` is `None`); empty issue (both
  `None`). The DB partial unique index
  `agent_run_one_active_per_work_item` (`runner/models.py:1085-1100`,
  context only — D-13…D-15 own that file) forbids two actives on one
  issue, so multi-row newest-first ordering is unreachable; the
  `ORDER BY` is recorded in SQL all the same.
- `fx02_reads/ingest.before_after.json` —
  `orchestration/done_signal.py:141-181` (`ingest_into_run`): 6 live
  cases — completed/blocked/noop → terminal + `ended_at` + prior
  error cleared, `noop` maps to `COMPLETED`; paused →
  `PAUSED_AWAITING_INPUT` with `ended_at` NULL; parse-error on a
  non-terminal run → `FAILED`; parse-error on a terminal run keeps
  its status (error + `ended_at` still stamped). Return value
  (`DoneSignal` vs `None`) and the single `UPDATE agent_run …`
  per case recorded.
- `fx02_reads/workpad.golden.json` —
  `orchestration/workpad.py:74-76` (`get_workpad`: fresh issue →
  `""`, `None` attribute → `""`), `:79-84` (`set_workpad`:
  round-trip, overwrite, clear-with-`""`, `None` → `""`, plus the
  `UPDATE issues SET updated_at, workpad …` with the frozen clock).
- `fx02_reads/agent_system_user.golden.json` —
  `orchestration/workpad.py:29-32` (`AGENT_*` constants), `:35-41`
  (`AgentUserCollisionError`), `:44-71` (`get_agent_system_user`):
  created-row shape (`is_bot`, unusable password), exists-path
  same-PK idempotency, collision message verbatim.
- `fx02_reads/querysets.sql` —
  `core/querysets.py:19-30` (`member_project_issues`),
  `:33-52` (`user_issues_queryset` × `all`/`assigned`/`created`):
  executed `SELECT DISTINCT …` per scope (incl. the `IssueManager`
  triage/archived/draft exclusions and the member-project joins).
- `fx02_reads/querysets.rows.json` — seeds (assigned-only,
  created-only, subscribed-only, uninvolved, non-member-project)
  plus the per-scope result sets: member → the 4 `FX2` issues;
  `all` → assigned+created+subscribed; `assigned`/`created` →
  their single issue each.
- `fx02_reads/role_facts.sql` —
  `core/permissions.py:28-34` (`is_workspace_member`),
  `:37-45` (`workspace_role`), `:48-58`
  (`workspace_role_by_slug`), `:61-70` (admin/member decisions via
  one `workspace_role` `SELECT` each), `:73-119`
  (`check_project_role`: allowed-role `EXISTS` plus the two bypass
  `EXISTS` per case): executed `SELECT`s for every matrix cell
  (9 `check_project_role` cases incl. the 3-statement bypass
  path). Role-fact SQL only — the F-06 kernel owns the decisions.
- `fx02_reads/role_facts.golden.json` — `ROLE_*` constants plus the
  result matrix over admin/member/guest/inactive/outsider/
  anonymous/`None` (member + guest fail admin; guest fails
  at-least-member; inactive/outsider/anonymous/`None` are all
  false/`None`) and the 9 `check_project_role` verdicts (allowed
  hit, bypass grant/deny, no-membership, anonymous/`None`).

## FX-ORCH-03 blockers (DB)

Generator `/tmp/fx_orch_03.py` against scratch database
`pidash_524_scratch` (shared with FX-ORCH-02; separate workspace
`fx3-workspace`, projects `FX3A`/`FX3B`, `FX3X` in `fx3-other`).
Issues are created in `Todo` and moved by queryset `update` (bypasses
signals, so no orchestration side effects); state moves that must hit
the no-dispatch path use the same trick. Dependent `D` is `FX3A-1`.

- `fx03_blockers/edges.sql` —
  `orchestration/blockers.py:60-61` (`_live_relations`), `:64-84`
  (`_blocked_by_edges`), `:87-106` (`_blocking_edges`), `:113-126`
  (row querysets), `:129-130` (`_ordered`): executed `SELECT`s for
  `blockers` / `open_blockers` / `has_open_blockers` /
  `dependents` on `D`.
- `fx03_blockers/edges.rows.json` — the 14 seeded relation rows
  around `D` with included/excluded + why: forward + stored-reversed
  (open and resolved), self-edge exclusion, soft-deleted exclusion,
  cross-workspace exclusion, cross-project inclusion, archived /
  draft / triage / soft-deleted target exclusion (live-work-item
  filter), no-state blocker.
- `fx03_blockers/blockers.golden.json` —
  `orchestration/blockers.py:47-48` (`BLOCKED_BY`/`BLOCKING`),
  `:50-51` (`CLOSED_STATE_GROUPS`), `:109-110` (`_open`),
  `:133-149` (row form): `blockers` (7, ordered by project
  identifier then `sequence_id`), `open_blockers` (5),
  `has_open_blockers`, `dependents` (any state, incl. a completed
  one); all-resolved `D2` (`has` false, `open` empty, `blockers`
  lists the resolved row); relation-less `D3`; open-rule matrix
  (review/test/no-state open, completed/cancelled closed).
- `fx03_blockers/open_blockers_q.sql` —
  `orchestration/blockers.py:152-160` (`open_blockers_q`): bulk-scan
  `SELECT` with `EXISTS` × 2 (forward + stored-reversed); scan over
  `D`/`D2`/`D3`/plain-blocker flags only `D`.
- `fx03_blockers/relations_summary.golden.json` —
  `orchestration/blockers.py:57` (`SUMMARY_LIMIT`), `:163-181`
  (`_summary_item`/`_summary_list`), `:184-198`
  (`relations_summary`): full summary for `D` (open-first in both
  lists) plus the cap case — 101 blockers (1 open + 100 completed)
  yield a 100-item list headed by the open one while
  `has_open_blockers` stays true over the full set.
