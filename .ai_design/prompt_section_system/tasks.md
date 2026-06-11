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
  kind names = `PhaseConfig.template_name` values)
- rewrite `composer.py`: `compose(kind, *, workspace, project, user,
context) -> ComposedPrompt(text, manifest)`; `build_first_turn`
  keeps its signature, passes `user=None` for now (the §9.1 user rule
  activates in PR 3); per-section line ranges recorded in the manifest
- bypass `PromptTemplate` lookup in `build_first_turn` (model/table
  untouched until PR 3); keep `PromptRenderError` → failed-run
  behavior in `orchestration/service.py`
- golden-file snapshot tests: assembled defaults-only template for
  `coding-task` and `review`
- update `tests/unit/prompting/test_fragments.py` /
  `test_composer.py` to the registry/recipe surface

### PR 2 — Scheduler onto the composer

Goal: one composer for every prompt that reaches an `AgentRun`
(design §5).

Scope:

- new sections: `scheduler-intro`, `scheduler-task` (dynamic body:
  `Scheduler.prompt` + `binding.extra_context`), `scheduler-ending`;
  `scheduler` recipe
- `build_scheduler_context(binding)` in `prompting/context.py`
  (workspace / project / scheduler / run keys only)
- call-site change: `bgtasks/scheduler.py` fire path composes via
  `compose("scheduler", ...)`; `dispatch_scheduler_run` keeps the
  `(run, fail_reason)` contract; `PromptRenderError` → recorded on
  `binding.last_error` (respect `LAST_ERROR_MAX_LEN`)
- data migration: `validate_syntax()` over every `Scheduler.prompt`
  and `SchedulerBinding.extra_context`; escape Jinja delimiters on
  rows that fail so rendered output stays byte-identical
- forward-path validation: scheduler prompt/extra-context save paths
  run `validate_syntax()`
- shared-section audit: every section in the `scheduler` recipe
  references only keys present in the scheduler context (CI check via
  the dual-sample render from §6.3 machinery, landed here in
  minimal form for defaults)
- golden-file snapshot for the scheduler kind; parity test that a
  legacy escaped prompt renders verbatim

### PR 3 — Per-section overrides + PromptTemplate retirement

Goal: workspace- and user-scoped section overrides with save-time
validation (design §6, decisions §9.1/§9.3).

Scope:

- `PromptSectionOverride` model + partial-unique constraint + index;
  migration
- `resolve_section(key, *, workspace, project, user)` — single
  precedence function; `effective_customizability(section, workspace)`
  indirection (returns registry flag in v1, §9.2); `project` accepted
  and ignored (§9.4)
- §9.1 user rule at call sites: Run AI / Comment & Run / state
  transition pass `run.created_by`; tick, scheduler beat, system-bot
  runs pass `user=None`; explicit trigger classification at run
  creation (`run_is_human_triggered`), derived from existing trigger
  plumbing in `orchestration/scheduling.py`
- save-time validation: syntax + dual-sample-context (populated +
  minimal) full-prompt render for every kind containing the section;
  sample-context fixtures owned next to context builders with a
  key-set-sync test
- run-time failure attribution: map render error position through
  manifest line ranges → error names section, source, version
- composition manifest persisted per run (JSON field on `AgentRun` or
  `run_config`)
- endpoints (replace the four `PromptTemplate` routes in
  `prompting/urls.py` / `views.py`):
  - `GET /workspaces/<slug>/prompt-sections?kind=&scope=`
  - `PUT /workspaces/<slug>/prompt-sections/<key>?scope=`
  - `DELETE /workspaces/<slug>/prompt-sections/<key>?scope=`
  - permission matrix: workspace scope admin-write, user scope
    member-write own-row-only, all member-read
- `PromptTemplate` retirement (§8): delete `load_template`, seed
  machinery, `reseed_default_template` command, `post_migrate`
  receiver, `PI_DASH_SKIP_PROMPT_SEED`; archive active
  workspace-scoped rows (body preserved for copy-out); table drop
  deferred one release
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
  today's preview endpoint: real issue for coding-task/review, real
  binding for scheduler; no run created; admin-gated
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
- User-editable recipes (section order/membership) — explicitly not
  planned; recipes encode cross-section step numbering
