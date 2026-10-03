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
