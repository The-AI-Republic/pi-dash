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

## FX-ORCH-04 relations (DB + pure)

Generator `/tmp/fx_orch_04.py` against scratch database
`pidash_524_scratch` (workspace `fx4-workspace`, projects `FX4A`/`FX4B`,
`FX4X` in `fx4-other`; `relate`/`unrelate` run under
`impersonate(actor)` since `BaseModel.save` resolves audit fields from
crum; `issue_activity.delay` mocked with the epoch frozen, the
soft-delete cascade task stubbed — both are out-of-scope
D-10/`bgtasks` infra, never recorded).

- `fx04_relations/relation_types.golden.json` —
  `orchestration/relations.py:55-66` (`RELATION_TYPES` order),
  `:70` (`REVERSE_TYPES`), `:74` (`GROUP_LIMIT`),
  `:81-85` (`validate_relation_type`: strip/lower normalisation +
  error string verbatim), `:92-97` (`_stored_edge` matrix over all
  10 types), `:100-114` (`_type_from` matrix: 6 forward-stored ×
  both viewpoints + 4 legacy reverse-stored × both viewpoints).
- `fx04_relations/resolve_refs.golden.json` —
  `orchestration/relations.py:138-163` (`resolve_refs`): UUID +
  `PROJ-123` hits, `iexact` project match, whitespace strip,
  request-order `found`, unresolved passthrough (unknown UUID,
  unknown project, non-ref, empty string, unknown sequence).
- `fx04_relations/relate.before_after.json` —
  `orchestration/relations.py:117-120` (`_pair_rows`), `:123-135`
  (`_log_activity`), `:166-177` (`_check_targets`), `:180-232`
  (`relate`): live pair-table before/after, created-result golden,
  created-row shape (stored type, ends, source project/workspace,
  `created_by=actor`, `updated_by=NULL` — `BaseModel.save` leaves
  `updated_by` empty on insert), unchanged golden, 2-cycle
  conflict golden, different-type conflict golden, self-relation
  and cross-workspace error strings, and the created-activity
  `delay` kwargs (`type`, `requested_data`, `actor_id`,
  `issue_id`, `project_id`, `current_instance=null`, frozen
  `epoch`, `notification=true`).
- `fx04_relations/relate_race.golden.json` —
  `orchestration/relations.py:196-213`: the `IntegrityError`
  re-read branch, exercised live by hiding the pre-created pair
  from the first `_pair_rows` call so the real unique
  constraint raises and the except branch re-reads → `unchanged`.
- `fx04_relations/unrelate.golden.json` —
  `orchestration/relations.py:235-266` (`unrelate`): wrong-type
  `not_related` with the row left live (exact-type-only rule),
  removed golden with `deleted_at` set, repeat `not_related`,
  reverse-name removal (`blocking` from the other side), and the
  two deleted-activity `delay` kwargs (per-target log with
  `current_instance={"relation_type": …}`).
- `fx04_relations/grouped_relations.golden.json` —
  `orchestration/relations.py:72-74` (`GROUP_LIMIT`), `:269-277`
  (`_item`), `:280-313` (`grouped_relations`): all 10 keys always
  present, sort by (project identifier, `sequence_id`), legacy
  stored-reversed row normalised, self-edge and cross-workspace
  rows excluded, visibility narrowing (`FX4B` target dropped for
  the `FX4A`-only member), and the 101-target cap (100 items).

## FX-ORCH-05 clock (DB)

Generator `/tmp/fx_orch_05.py` against scratch database
`pidash_524_scratch` (workspace `fx5-ws`, projects `FX5` pool 10 /
`FX5B` ticking-disabled / `FX5I` infinite pool; project `FX5`
intervals impl/review/test = 10800/3600/7200s so retime cadence
resolution is visible; clock frozen at 2026-06-01T12:00Z,
`random.seed(52405)` before every event call so the jitter draw is
deterministic — verified byte-identical across two runs; states set
via queryset `update` with the orchestration Issue pre/post_save
receivers disconnected, since signals are FX-07 scope).

- `fx05_clock/reconcile.enter_move.before_after.json` —
  `orchestration/scheduling.py:400-448` (`_on_enter_or_move`):
  human enter with no ticker (`dispatch-now`, created + armed),
  human enter while busy (free entry queued with actor +
  `state_transition` trigger memory), human move with
  `want_run=False` (`retimed`), human move into a spent pool
  (`dispatch-now` + `POOL_SPENT` stop), user-disabled
  (`dispatch-now` + `USER_DISABLED` stop), project-disabled
  (`dispatch-now` + stopped with the empty reason), agent move
  with budget (counting entry queued, no actor/trigger memory),
  agent move with pool spent (`pool-spent` park), agent move
  while disabled (`ticking-disabled`), and resume-parent
  capture on a cross-stage move.
- `fx05_clock/reconcile.left_bucket.before_after.json` —
  `orchestration/scheduling.py:451-459` (`_on_left_bucket`):
  dormant with the pool kept (`used`/`granted` survive,
  `next_run_at` cleared, pending cleared,
  `LEFT_TICKING_STATE`), and the no-ticker no-op.
- `fx05_clock/reconcile.run_ended.before_after.json` —
  `orchestration/scheduling.py:462-518` (`_on_run_ended`):
  stopping outcomes × 3 (`done`/`blocked`/`waiting_on_human`
  → `TERMINAL_SIGNAL` stop, `next_run_at` kept), continuing
  outcomes × 2 (re-armed when `next_run_at` was `None`,
  untouched when set), the kind guard (`stage-moved-on`,
  queued next-stage entry survives), no-yield defaults
  (completed coding-task → `progressed`, completed review →
  `done`, `paused_awaiting_input` → `waiting_on_human`,
  failed → `progressed`), pending-entry-kept, prior
  `cap_hit` preserved (`already-stopped`), `not-in-bucket`,
  and no-ticker. (`kind_for` is the identity on the template
  name — `prompting/recipes.py:142-152`, out of scope.)
- `fx05_clock/reconcile.human_retick.before_after.json` —
  `orchestration/scheduling.py:521-534`
  (`_on_human_run_requested`; free → `dispatch-now` + re-arm,
  busy → free entry with actor + explicit-trigger memory,
  `want_run=False` → `retimed`) and `:537-580` (`_on_retick`):
  grant of one fresh pool (`granted` 0 → 10) + dispatch, the
  `no_ticker` / `not_ticking_state` / `budget_not_exhausted`
  guards, the infinite-pool case (returns via the budget
  guard — the `:560-563` sentinel check is unreachable by
  construction), grant + free-entry queue while busy, and
  `granted-from-paused` (grant lands, clock otherwise
  untouched; the move back is `re_tick_ticker`, FX-08).
- `fx05_clock/clock_primitives.golden.json` —
  `orchestration/scheduling.py:231-251` (`_lock_ticker`:
  create-shape is disabled + unarmed with zero budget,
  no-create miss → `None`, existing row rebound),
  `:254-265` (`_TICKER_CLOCK_FIELDS`; also in FX-01),
  `:285-303` (`_queue_entry`: free remembers actor +
  trigger, counting clears both, `next_run_at = now`),
  `:306-310` (`_stop_for_switch`), `:313-330`
  (`_retime_clock`: per-stage cadence resolution,
  spent pool → `POOL_SPENT`, user-disabled →
  `USER_DISABLED`, project-disabled → empty reason),
  `:279-282` (`_stop_clock`: reason stamped, pending
  cleared, `next_run_at` kept), and `:220-222`
  (`_compute_next_run_at` with the seeded jitter draw).
- `fx05_clock/thin_senders.golden.json` —
  `orchestration/scheduling.py:598-606` (`arm_ticker`:
  re-timed, `used` untouched), `:609-629` (`disarm_ticker`:
  default stamps `LEFT_TICKING_STATE` and clears
  `next_run_at`, custom reason keeps it, no ticker →
  `None`, `TERMINAL_SIGNAL` refused with the `ValueError`
  string verbatim), `:632-637`
  (`maybe_disarm_on_terminal_signal`: `True` iff the
  reason ends with `:stopped`, `False` for a run with no
  work item), and `:640-647`
  (`reset_ticker_after_comment_and_run`: re-timed with
  budget, `POOL_SPENT` when spent).

## FX-ORCH-06 creation (DB)

Generator `/tmp/fx_orch_06.py` against `pidash_524_scratch` (frozen
clock 2026-06-01T12:00Z, `fx6-` slugs, orchestration Issue signals
disconnected). Real `execution_fields` (local-runner projects) and real
`build_first_turn` on the migrate-seeded templates except where a case
stubs a seam (named per case); real `finalize_agent_run`;
`dispatch_after_commit` + `transaction.on_commit` captured, never
executed (so the celery `.delay` and terminal hooks in
`_publish_effects` never fire — captured callback names are recorded).
Run UUIDs are random per run: the prompt/`prompt_manifest` carry
`<run-id>`, dispatch captures and `replacement_run_id` carry fixture
labels, auto-created pod UUIDs in markers carry pod names; projects /
issues / users use fixed seeded UUIDs. Every service call runs under
`impersonate(<its creator>)` because `BaseModel.save` sources
`created_by` from crum. Regeneration is byte-identical (verified).

- `fx06_creation/create_dispatch.before_after.json` —
  `orchestration/service.py:735-830` (`_create_and_dispatch_run`),
  `:157-167` (`_phase_kind_for_issue`), `:550-566`
  (`_run_config_for_issue`), `:528-547` (`_pinned_runner_for`):
  C1 created, no parent (QUEUED / `state_transition` /
  `coding-task` stamp / prompt+manifest / repo `run_config` / owner
  NULL / `local_runner` execution merge / dispatch captured);
  C2 terminal parent + eligible runner → parent linkage + pin;
  C3 `fresh_session` drops parent + pin; C4 admission error →
  FAILED with `error_code` verbatim and reason = the code
  (`lock_cloud_creation_capacity` stubbed with the real
  `run_quota_exceeded` dict shape); C5 render failure → FAILED
  `prompt_build_failed` (`PromptRenderError` stub); C6 executor
  unavailable → reason string, no run, no dispatch.
- `fx06_creation/continuation.before_after.json` —
  `orchestration/service.py:443-525` (`_create_continuation_run`):
  K1 created with parent linkage + pin
  (`comment_and_run` trigger); K2 admission-error FAILED;
  K3 render-failed; K4 executor-unavailable, no run.
- `fx06_creation/parent_pin.matrix.json` —
  `orchestration/service.py:110-154` (`parent_for_next_run`):
  P1 no-latest → `(None, True)`; P2 non-ticking state →
  `(latest, False)`; P3 same stage → `(latest, False)`;
  P4 cross-stage into `fresh_session_on_entry` review →
  `(None, True)`; P5 hand-back with `resume_parent_run` →
  `(resume, False)`; P6 hand-back without → `(None, True)`;
  P7/P8 explicit `cross_stage` overrides; kinds
  (`coding-task` / `review` / `test`) and `fresh_session_on_entry`
  flags resolved live. `:528-547` (`_pinned_runner_for`):
  M1 no runner → `None`; M2 revoked → `None`; M3 pod
  mismatch → `None`; M4 eligible → pinned; M5 no target pod
  skips the check.
- `fx06_creation/resolvers.golden.json` —
  `orchestration/service.py:303-316`
  (`_resolve_fallback_creator`: creator → lead → default
  assignee → `None`); `:319-338` (`_resolve_pod_for_issue`:
  assigned → default, dangling id → default, no project →
  `None`; the last two use in-memory instances since DB FKs
  forbid them); `:550-566` (`_run_config_for_issue`: handoff
  marker stripped, repo fields refreshed, overrides survive,
  base dict unmutated).
- `fx06_creation/handoff.before_after.json` —
  `orchestration/service.py:53-57`
  (`PROJECT_MOVE_HANDOFF_CONFIG_KEY`), `:569-641`
  (`_create_project_move_handoff_run`), `:644-732`
  (`complete_project_move_handoff`; markers seeded directly —
  the writer is the out-of-scope issue-move path): H2 happy
  path (replacement inherits trigger, parents on source, pin
  cleared, marker stripped + repo refreshed, `replacement_run_id`
  stamped, dispatched) — note the replacement's `phase_kind` is
  `""`: unlike the other builders the handoff create stamps no
  phase kind; H3 idempotency; H4 moved-again suppression;
  H5 active-run-wins; H7 guards (unknown run / non-terminal /
  no marker → `None`); H8 executor-fallback `LOCAL_RUNNER` row;
  H9 active-race returns existing; H6 no target pod → `None`.

## FX-ORCH-07 entries (DB)

Generator `/tmp/fx_orch_07.py` against `pidash_524_scratch` (frozen
clock 2026-06-01T12:00Z, `random.seed(52407)` before each entry call,
`fx7-` slugs, fixed seeded UUIDs for projects / states / issues /
users / bindings, run.id normalized to `<run-id>`). Real reconcile +
preflight + builders + composer (migrate-seeded templates) except
where a case stubs a seam (named per case); `dispatch_after_commit` +
`on_commit` captured. Transition cases set the issue row to `to_state`
before calling (post-save truth, as the signal would see it) and pass
`actor=None` so the creator falls back to `issue.created_by`.
Regeneration is byte-identical (verified).

- `fx07_entries/transition.matrix.json` —
  `orchestration/service.py:180-300` (`handle_issue_state_transition`):
  T1 leave-bucket (dormant + `not-a-trigger-state`); T2
  non-trigger (no clock event); T3 `dispatch_immediate=False`
  (clock updated, no run); T4 agent move into a spent pool
  (`pool-spent`; cap = pool + granted + waited); T5/T5b
  entry-queued (human free with actor memory / agent counting);
  T6a reconcile reason surfacing (`ticking-disabled`); T6b bare
  `no-dispatch` (stubbed — every real enter/move path sets a
  reason); T7 `active-run-exists` (injected — busy always
  queues through the real reconcile, so the guard is a true
  race net); T8 `no-creator`; T9 `no-pod-available`;
  T10 `no-eligible-runner` (real bounce fired; only the reason
  + landing `Backlog` state + comment head recorded — the bounce
  matrix is FX-ORCH-08); T11a created DB before/after
  (`state_transition` / `coding-task`); T11b cross-stage
  In Progress → In Review (ticker `resume_parent_run` captures
  the impl run, fresh review run with `parent=None`, review
  phase kind).
- `fx07_entries/comment.matrix.json` —
  `orchestration/service.py:341-346`
  (`CONTINUATION_ELIGIBLE_GROUPS`), `:349-440`
  (`handle_issue_comment`): M1 `no-actor`; M2 `bot-comment`;
  M3 `state-not-eligible`; M4 `no-prior-run`; M5 `coalesced`
  (into the QUEUED follow-up); M6 `no-pod-available`;
  M7 `entry-queued` (run in flight, clock queues);
  M8 `prior-run-active` with rollback (custom `Grooming`
  state: eligible group, non-ticking name — ticker byte-identical
  before/after); M9 created DB before/after (`comment_and_run`,
  parent + pin, clock retimed); M10 builder-no-run rollback
  (stubbed executor failure — ticker untouched, no run).
- `fx07_entries/scheduler_dispatch.before_after.json` —
  `orchestration/service.py:846-1001`
  (`dispatch_scheduler_run`): S1 created DB before/after
  (`work_item=None`, `parent=None`, binding linked, `scheduler`
  trigger, `run_config={}`, dispatch captured); S1b live pod
  override honored; S2a soft-deleted override → default;
  S2b cross-project override → default; S3 no-pod string
  verbatim (`no default pod for project <uuid>`);
  S4a–d cloud creator chain (chain real, D-11/F-06 seams
  scripted, chosen actor captured via a spy `execution_fields`:
  actor wins / bot skipped / no-LLM skipped / no-role skipped);
  S5 cloud `no current human execution principal`; S5b local
  `None` actor → real agent system user; S6 outcome-mode
  refusal string verbatim; S7 admission-error FAILED run
  returned with `error=None`; S8 render-failure FAILED run
  returned with `error=None`; S9 executor-unavailable
  `(None, reason)`.
- `fx07_entries/signals.golden.json` —
  `orchestration/signals.py:37-52` (flag constants verbatim),
  `:61-71` (`capture_prior_state`: new → `None`, existing →
  DB `state_id`, missing row → `None`), `:74-107`
  (`fire_state_transition`: G4 no-transition no-op, G5 exact
  handler args with flag defaults, G5b per-instance flag
  overrides, G5c `to_state=None` passthrough, G6 raise →
  swallowed with counter `0→1` and the `ERROR` log record
  verbatim incl. exception), `:110-116` (`_lookup_state`:
  `None`/existing/missing → `None`).

## FX-ORCH-08 dispatch (DB)

Generator `/tmp/fx_orch_08.py` against `pidash_524_scratch` (frozen
clock 2026-06-01T12:00Z, `random.seed(52408)` before
reconcile-touching calls, `time.time` frozen at 1780000000.0 for
wait-activity epochs, `fx8-` slugs, fixed seeded UUIDs, run.id
normalized to `<run-id>`). Orchestration Issue signals stay CONNECTED
(production truth for the `issue.save()` calls inside bounce /
retick-paused / deferred-pause). Real matcher / builders / composer
(migrate-seeded templates) except where a case stubs an out-of-scope
seam (named per case); `dispatch_after_commit` + `on_commit` captured;
creations under `impersonate(creator)`; guard/bounce reasons captured
from the scheduling logger. Regeneration is byte-identical (verified).

- `fx08_dispatch/preflight.matrix.json` —
  `orchestration/scheduling.py:955-1026`
  (`preflight_eligibility_or_bounce`): F1 cloud + LLM → `True`;
  F2 cloud no-LLM → `False` + `no-llm-config` bounce; F3a–d
  managed gates (`None` creator → `no-managed-runner`;
  disabled → `managed_runner_disabled`; no profile →
  `llm_config_missing`; not enrolled → `no-managed-runner`);
  F3e all-pass → `True`; F4a local matcher `True`; F4b empty
  pod → `False` + `no-eligible-runner` bounce. Reason codes
  from the log records.
- `fx08_dispatch/bounce.matrix.json` —
  `orchestration/scheduling.py:1029-1125`
  (`_bounce_issue_no_eligible_runner`): B1 `no-llm-config` body
  verbatim; B2 default body verbatim (B2b: managed codes reuse
  it); B3a `-default` beats `sequence`; B3b lowest `sequence`
  wins; B4 `project.default_state` fallback (non-ticking);
  B5 ticking fallback → stays + explicit disarm; B6 no target
  → stays + explicit disarm; B7 already-Backlog → comment
  only; B8 comment failure rolls the move back (exception
  propagates, state + ticker + 0 comments); B9 comment-row
  shape (system-user actor, `agent` speaker, linkage).
- `fx08_dispatch/pod_resolve.matrix.json` —
  `orchestration/scheduling.py:871-884`
  (`_resolve_pod_for_issue`, a duplicate of the service copy):
  assigned → pinned, dangling → default, unset → default, no
  project → `None`.
- `fx08_dispatch/creator.matrix.json` —
  `orchestration/scheduling.py:860-868` (`TRIGGER_*`,
  `_MACHINE_TRIGGERS = {tick}`), `:887-952`
  (`_resolve_creator_for_trigger`): L1 local actor wins (even
  on `tick`); L2 local tick → system user; L3/L4 local chain
  (creator → `None`); C1 cloud `[actor]` fast path; C2 `tick`
  ignores the explicit actor; C3a/b order + dedup + live-only
  proven by the llm spy (`[lead]`, `[lead, live]` — bot
  skipped silently, dup offered once, soft-deleted never);
  C4 inactive skipped; C5 managed enrollment gate; C6 `None`.
- `fx08_dispatch/continuation.matrix.json` —
  `orchestration/scheduling.py:1128-1216`
  (`dispatch_continuation_run`): A1 active → `None`; A2 no
  prior → `None`; A3 same-stage continuation (parent + pin,
  `tick`); A4 cross-stage fresh (no parent); A5 hand-back
  parents off `resume_parent_run`; A6 local no-creator →
  `None` with no bounce; A7 cloud no-creator → `None` +
  `no-llm-config` bounce; A8 no-pod → `None`; A9 preflight
  bounce → `None`.
- `fx08_dispatch/run_ai.matrix.json` —
  `orchestration/scheduling.py:1219-1225` (`RUN_AI_*` refusal
  codes verbatim), `:1240-1322`
  (`dispatch_run_ai_run_with_reason`): H1 active, H2 no-pod,
  H3 no-creator (no bounce), H4 preflight bounce; H5 prior →
  continuation; H6 no prior → fresh templated run (prompt
  parity); `:1228-1237` thin wrapper (H7a refused → `None`,
  H7b created → run); `:1325-1340` `run_ai_for_human` (H8
  created + retimed clock, H9 refused + byte-identical
  ticker rollback).
- `fx08_dispatch/retick.matrix.json` —
  `orchestration/scheduling.py:650-706` (`re_tick_ticker`):
  R1 `no_issue`; R2 roomy pool → `budget_not_exhausted`, no
  grant; R3 spent pool → `granted` + `run_ai` run (grant +10
  but used still hits the new cap, so the re-time stops
  `POOL_SPENT` — the run fires anyway); R3b 20-pool grants
  +20; R4 paused → `granted-from-paused`, moved to
  In Progress, run carries the retick actor; R5 no-pod →
  `dispatch-failed` rollback (ticker untouched); R6 no
  In Progress state → rollback; R7 infinite pool → guard.
  `:709-726` (`_in_progress_state_for`): Q1 SQL + found, Q2
  SQL + `None`, Q3 soft-deleted skipped.
- `fx08_dispatch/wait.matrix.json` —
  `orchestration/scheduling.py:741-763` (`WAIT_*` verbatim),
  `:766-827` (`wait_ticker`): W1 applied (`waited` 0→1 +
  activity `Waited on a blocker (1 of 10); run <label>`);
  W10 second wait (→2); W2 cap; W3 infinite; W4 no ticker;
  W5 `CAP_HIT` + room → re-armed (enabled, reason cleared,
  `next_run_at` set); W6/W7/W8 no re-arm (user-disabled /
  still-capped / `LEFT_TICKING_STATE` — `waited` still +1,
  activity still written); W9 no run/actor (no suffix,
  system-user actor). `:830-852` activity row golden
  (`agent_wait`, pool/waited, frozen epoch).
- `fx08_dispatch/deferred_pause.matrix.json` —
  `orchestration/scheduling.py:1349-1469`
  (`maybe_apply_deferred_pause`): U1 no work item, U2 no
  ticker, U3 enabled, U4 pending entry, U5 `POOL_SPENT`, U6
  no state, U7 non-ticking, U8 review (no auto-pause), U9
  other active run, U10 no Paused state → all `False`; U11
  happy path → `True` (moved to Paused; ticker flips to
  `LEFT_TICKING_STATE` via the real leave-bucket signal) —
  note `updated_by` stays `NULL`: the `= bot` assignment is
  wiped by `BaseModel.save` (crum) and `update_fields`
  excludes it; U12 self-active allowed. The three lock
  re-checks (`:1442-1459`) are concurrency-only — U11 passes
  them single-threaded; their `False` branches are noted in
  the fixture, not injected.
