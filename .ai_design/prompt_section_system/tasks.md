# Prompt Section System — Implementation Tasks

This file turns the design into a concrete implementation checklist.

Related docs:

- `design.md` — decisions resolved through design review (§9).
- `.ai_design/prompt_system/prompt-system-design.md` — the upstream
  prompt system this design replaces the storage/override layer of
  (renderer and context builder carry forward).
- `.ai_design/project_scheduler/design.md` — the scheduler whose
  dispatch path joins the composer in PR 2.
- `.ai_design/create_review_state/design.md` — the phase registry
  (`agent_phases.py`) that keeps owning state → kind mapping.

## Suggested rollout

Four PRs, each shippable. PR 1 is a pure refactor (one intended
behavior change: review gains the shared CLI/framing sections);
PR 2 unifies the scheduler; PR 3 introduces overrides; PR 4 is
visibility/UI.

### PR 1 — Section registry + recipes + compose-time assembly

Goal:

- make the section the unit of storage; move assembly from seed time
  (`fragments/assemble()` → `PromptTemplate` blob) to compose time
- fix the review-prompt defect: it instructs the agent to use
  `pidash` but never includes the CLI documentation

Scope:

- create `apps/api/pi_dash/prompting/sections/` — markdown files with
  YAML front-matter (`key`, `title`, `customizable`); port the 13
  fragments 1:1 (drop `NN_` prefixes; order moves to recipes)
- split `seed.py::REVIEW_TEMPLATE_BODY` into `review-intro` +
  `review-cycle` sections; review recipe reuses `session-framing`,
  `pidash-cli`, `guardrails`, `ending-run`
- registry loader (`PromptSection` dataclass, front-matter parse at
  import, `REGISTRY` dict) + CI checks: recipe keys ⊆ registry, valid
  front-matter, `validate_syntax()` on every default body, no
  orphan files in `sections/`
- create `apps/api/pi_dash/prompting/recipes.py` (`RECIPES`,
  kind names = `PhaseConfig.template_name` values); kind lookup
  written as `kind_for(phase, work_kind)` with `work_kind` hardcoded
  to `"coding"` (design §9.5 seam)
- rewrite `composer.py`: `compose(kind, *, workspace, project, user,
context) -> ComposedPrompt(text, manifest)`; `build_first_turn`
  keeps its signature, passes `user=None` for now (the §9.1 user rule
  activates in PR 3); per-section line ranges recorded in the manifest
- add `run.kind` to `build_context` (§5.2 base-context contract)
- transition guards (§8.1): `build_first_turn` renders an active
  workspace-scoped `PromptTemplate` row whole-body when one exists
  (legacy fallback, deleted in PR 3), composes from sections
  otherwise; `seed.py::read_default_body()` and
  `views.py::_get_global_default_body()` re-pointed at the registry
  so seeding keeps working until PR 3; keep `PromptRenderError` →
  failed-run behavior in `orchestration/service.py`
- golden-file snapshot tests: assembled defaults-only template for
  `coding-task` and `review`
- update `tests/unit/prompting/test_fragments.py` /
  `test_composer.py` to the registry/recipe surface

### PR 2 — Scheduler onto the composer

Goal: one composer for every prompt that reaches an `AgentRun`
(design §5).

Scope:

- new sections: `scheduler-intro`, `scheduler-task` (thin frame
  rendering `{{ scheduler_task_body }}`), `scheduler-ending`;
  `scheduler` recipe
- `build_scheduler_context(binding)` in `prompting/context.py`
  (workspace / project / scheduler / run keys +
  `scheduler_task_body` = `Scheduler.prompt` + `binding.extra_context`
  injected as a context variable, §5.1 — operator text is never
  parsed as Jinja, so **no data migration and no save-path
  validation needed**)
- call-site change (§5.3): composition moves inside
  `dispatch_scheduler_run(binding)` (prompt param dropped), which
  mirrors the issue path — create run with `prompt=""`, compose
  (context needs `run.id`), save; `PromptRenderError` → run marked
  FAILED with attributed error, `binding.last_run` points at it;
  `binding.last_error` keeps its no-run-short-circuit-only semantic;
  `(run, fail_reason)` contract preserved for short-circuits
- shared-section guard pass (§5.2): `session-framing`, `pidash-cli`,
  `guardrails` currently reference `issue.*` (e.g.
  `{{ issue.identifier }}`, `{{ issue.project_states }}` in
  `03_pidash_cli.md`) — guard issue-specific lines with
  `{% if run.kind != "scheduler" %}` branches; StrictUndefined means
  unguarded references crash, so the §3.2 CI check renders every
  kind's full default prompt
- golden-file snapshot for the scheduler kind; parity test that a
  legacy prompt containing literal `{{` renders verbatim

### PR 3 — Per-section overrides + PromptTemplate retirement

Goal: workspace- and user-scoped section overrides with save-time
validation (design §6, decisions §9.1/§9.3).

Scope:

- `PromptSectionOverride` model + **two** partial-unique constraints
  (workspace-level `user__isnull=True` and user-level
  `user__isnull=False` — single-constraint version is broken by
  Postgres NULL-distinctness on Django 4.2, §6.1) + index; migration;
  upsert handles `IntegrityError` race as update
- `resolve_section(key, *, workspace, project, user)` — single
  precedence function; `effective_customizability(section, workspace)`
  indirection (returns registry flag in v1, §9.2); `project` accepted
  and ignored (§9.4)
- `AgentRun.trigger` CharField (choices: `state_transition | run_ai |
comment_and_run | tick | scheduler | direct`; promotes the
  `TRIGGER_*` constants in `orchestration/scheduling.py` to a shared
  enum) — required because human-vs-automatic is **not derivable**
  from `created_by` (ticks resolve a human creator via
  `_resolve_creator_for_trigger`); threaded as required kwarg through
  `_create_and_dispatch_run` / `_create_continuation_run` /
  `dispatch_scheduler_run`, set before `build_first_turn` runs (§7.1)
- §9.1 user rule via `run_is_human_triggered(run)` =
  `run.trigger ∈ {state_transition, run_ai, comment_and_run, direct}`:
  human triggers pass `run.created_by` to `compose`; tick, scheduler
  beat, system-bot runs pass `user=None`
- save-time validation: syntax + dual-sample-context (populated +
  minimal) full-prompt render for every kind containing the section;
  sample-context fixtures owned next to context builders with a
  key-set-sync test
- run-time failure attribution: map render error position through
  manifest line ranges → error names section, source, version
- composition manifest persisted as `AgentRun.prompt_manifest`
  JSONField (dedicated field, not `run_config` — that is the
  runner-facing dispatch payload)
- endpoints (replace the four `PromptTemplate` routes in
  `prompting/urls.py` / `views.py`):
  - `GET /workspaces/<slug>/prompt-sections?kind=&scope=`
  - `PUT /workspaces/<slug>/prompt-sections/<key>?scope=`
  - `DELETE /workspaces/<slug>/prompt-sections/<key>?scope=`
  - permission matrix: workspace scope admin-write, user scope
    member-write own-row-only, all member-read
- `PromptTemplate` retirement (§8.2): delete the §8.1 legacy
  whole-body fallback, `load_template`, seed machinery,
  `reseed_default_template` command, `post_migrate` receiver,
  `PI_DASH_SKIP_PROMPT_SEED`; archive active workspace-scoped rows
  (body preserved for copy-out); table drop deferred one release
- contract tests for section CRUD replacing
  `tests/contract/prompting/test_crud.py`; resolution-matrix unit
  tests; manifest tests (tick runs never carry `user:*` sources)

### PR 4 — Final-prompt visibility + section UI

Goal: goal 4 — users see exactly what will be sent (design §7.2).

Scope:

- `GET /workspaces/<slug>/prompts/<kind>/compiled?scope=` — assembled
  template + per-section breakdown (key, title, customizable, source,
  needs_attention, override metadata); dual compilation when user
  overrides exist ("what you get when you trigger" vs "what automatic
  runs get", §9.1)
- `POST /workspaces/<slug>/prompts/<kind>/preview` — generalized from
  today's preview endpoint: body `{"issue_id"}` for
  coding-task/review, `{"binding_id"}` for scheduler (400 on
  kind/parameter mismatch), optional `{"scope": "user"}`; `_FakeRun`
  generalized to carry `kind`; no run created; admin-gated
- web UI (apps/web): section list per kind with lock badges,
  source/modified indicators, per-section edit with available-variable
  hints (from the kind's context schema), revert, `needs_attention`
  badge, final-template view; admin sees user-override sources
- re-validation management command (§6.4): re-run save-time checks
  over all active overrides, set `needs_attention`; documented as a
  required step in the same PR as any `build_context` /
  `build_scheduler_context` key removal
- E2E: admin edits a workspace section → compiled view reflects it →
  Run AI run's manifest records it; member user-override applies to
  their manual run and not to a tick

## Out of scope (deferred, with re-entry points)

- Admin lock tier (open / workspace-only / locked) —
  `effective_customizability()` is the seam (§9.2)
- Project-level override rung — `resolve_section(project=...)` is the
  seam (§9.4)
- Sticky per-engagement user resolution for ticks — manifest data is
  the evidence source (§9.1)
- Work-kind axis (non-coding prompts): `project.default_work_kind` +
  nullable `issue.work_kind` (dedicated enum fields, not free-form
  labels) + `effective_work_kind()` resolver; new kind + recipe per
  work kind; `(phase × work_kind)` matrix filled deliberately —
  `kind_for(phase, work_kind)` is the seam (§9.5)
- User-editable recipes (section order/membership) — explicitly not
  planned; recipes encode cross-section step numbering
