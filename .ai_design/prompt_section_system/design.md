# Prompt Section System — Centralized, Section-Based Prompt Management

> Directory: `.ai_design/prompt_section_system/`
>
> **Status:** ready for implementation. Decisions resolved through
> design review (§9): user-level overrides apply only to runs a human
> directly triggered — ticks, scheduler beats, and system-bot runs
> resolve workspace + defaults only (§9.1); the admin lock tier is
> deferred but resolution goes through `effective_customizability()`
> from day one (§9.2); broken overrides are prevented at save time via
> dual-sample-context full-prompt render validation, run time stays
> fail-loud with section/author attribution, and context-schema changes
> re-validate all active overrides (§9.3); the project-level override
> layer is deferred but `resolve_section()` accepts `project` from day
> one and ignores it (§9.4).
>
> **Scope:** replace seed-time fragment flattening and the whole-body
> `PromptTemplate` override with compose-time assembly from a
> code-owned **section registry**, per-kind **recipes**, and a
> per-section **override** model (workspace- and user-scoped). Bring
> the project scheduler onto the same composer. Expose the final
> assembled template (and a per-section breakdown) for every prompt
> kind.
>
> **What this changes about today's code**
>
> The prompt system (`.ai_design/prompt_system/`) shipped ordered
> fragments (`prompting/fragments/NN_*.md`) that `assemble()` flattens
> into a single `PromptTemplate` DB row at seed time
> (`prompting/seed.py`), a sandboxed Jinja renderer
> (`prompting/renderer.py`), an issue context builder
> (`prompting/context.py`), and `composer.build_first_turn()` as the
> single entrypoint. The review template is a hand-written monolith in
> `seed.py` that shares nothing with the coding-task fragments (it does
> not even include the Pi Dash CLI section). The project scheduler
> (`.ai_design/project_scheduler/`) bypasses the composer entirely:
> `bgtasks/scheduler.py` string-concatenates `Scheduler.prompt` +
> `SchedulerBinding.extra_context` and `dispatch_scheduler_run()` stores
> it verbatim — no template selection, no context, no Jinja. Workspace
> prompt customization (`prompting/views.py`) is whole-body-only,
> admin-only, `coding-task`-only, and effectively unused.

## 1. Problem

Four goals drive this redesign:

1. **Centralize composition.** The scheduler must share the same
   composer as issue runs. Every prompt that reaches an `AgentRun`
   should be composed by section through one code path.
2. **Manage prompts by section.** In Progress and In Review share
   large amounts of content (CLI usage, session framing, guardrails,
   ending contract), but today the sharing is invisible: fragments are
   flattened at seed time and the review template duplicates (or
   omits) shared content by hand.
3. **Make user customization real.** Each user should be able to
   customize _their own_ prompt sections. Some sections are
   non-customizable (e.g., how to use the `pidash` CLI — the
   orchestration contract), some are fully overridable (e.g., how to
   approach In Progress issues). This requires a section taxonomy,
   customizable/locked labeling, and per-user override storage.
4. **Make the final prompt visible.** For every ready-to-send prompt
   kind (coding-task, review, scheduler), the user must be able to see
   the final assembled template.

### 1.1 Root cause

Fragments already _are_ sections, but `assemble()`
(`prompting/fragments/__init__.py`) destroys the structure at seed
time. Because the DB blob is the unit of storage, everything
downstream — workspace overrides, the review template, scheduler
prompts — is forced to be whole-body. The fix is one structural
change: **the section becomes the unit of storage and resolution, and
assembly moves from seed time to compose time.** Goals 1–4 all fall
out of that change.

## 2. Architecture overview

```
                 ┌─ Section Registry (code) ─────────────────────┐
                 │ prompting/sections/*.md with front-matter:     │
                 │   key, title, customizable: locked|overridable │
                 └───────────────┬───────────────────────────────┘
                                 │ default bodies
Recipes (code)                   ▼
 coding-task: [intro, framing, pidash-cli*, ..., ending]     ┌─ Overrides (DB) ─┐
 review:      [intro, framing, pidash-cli*, review-cycle…]   │ user-scoped      │
 scheduler:   [sched-intro, pidash-cli*, task-body, …]       │ workspace-scoped │
                                 │                            └────────┬─────────┘
                                 ▼                                     │
                  compose(kind, workspace, project, user, context) ◄───┘
                  1. load recipe for kind
                  2. per section: resolve_section() → user override →
                     workspace override → registry default
                     (locked sections skip the override chain)
                  3. concatenate → single template body
                  4. render once via sandboxed Jinja (unchanged env)
                                 │
                                 ▼
                       AgentRun.prompt  (+ composition manifest)
```

`*` = shared section appearing in multiple recipes.

Properties preserved from the current system:

- **Sandboxed rendering** is unchanged (`renderer.py`
  `SandboxedEnvironment` + `StrictUndefined`, `from_string` only).
  Assembly is plain Python string concatenation before a single
  `from_string` render, so `{% include %}` stays out of the attack
  surface even though assembly now happens at compose time.
- **Sections evolve through code review.** Default bodies live in the
  repo, exactly like fragments today.
- **A render failure fails the run cleanly** (no 500), now with
  section-level attribution (§6.3).

## 3. Section registry

### 3.1 Format

`prompting/sections/` replaces `prompting/fragments/`. Each section is
a markdown file with YAML front-matter:

```markdown
---
key: pidash-cli
title: Pi Dash CLI usage
customizable: locked # locked | overridable
---

## Pi Dash CLI (`pidash`)

...body, markdown + Jinja, same as fragments today...
```

- `key` — stable identifier; referenced by recipes and override rows.
  Renaming a key is a migration (it orphans overrides; see §6.4).
- `customizable: locked` — the platform-level ceiling. Locked sections
  never consult the override chain. Locked v1 set: everything that
  orchestration _parses or depends on_ — `pidash-cli`,
  `session-framing`, `state-routing`, `blocking`, `guardrails`,
  `ending-run` (done-signal vocabulary), `workpad-template`.
- `customizable: overridable` — opinion/style sections:
  `default-posture`, `autonomy`, `analyze-and-scope`, `workpad-setup`,
  `implementation`, `review-cycle`.
- Ordering is **not** encoded in filenames anymore (no `NN_` prefix) —
  order belongs to recipes (§4), because the same section can appear
  at different positions in different kinds.

### 3.2 Loading

A small registry module parses front-matter once at import time and
exposes:

```python
@dataclass(frozen=True)
class PromptSection:
    key: str
    title: str
    customizable: str          # "locked" | "overridable"
    default_body: str

REGISTRY: dict[str, PromptSection]
```

A startup/CI check asserts: every recipe key exists in the registry,
no unknown front-matter values, every section body passes
`validate_syntax()`. (Replaces the `FRAGMENT_GLOB` stray-file guard:
unknown files without valid front-matter fail the check loudly instead
of being silently skipped.)

### 3.3 Splitting the existing content

- The 13 coding-task fragments map 1:1 to sections (front-matter
  added, `NN_` prefix dropped).
- The review monolith (`seed.py::REVIEW_TEMPLATE_BODY`) splits into:
  a `review-intro` section (issue context header for review),
  `review-cycle` (steps 1–2: decide kind, run the cycle), and reuses
  shared `session-framing`, `pidash-cli`, `guardrails`, and a
  review-aware `ending-run`. **This fixes a live defect: today's
  review prompt instructs the agent to use `pidash` commands but never
  includes the CLI documentation.**
- New scheduler sections: `scheduler-intro` (framing: scheduled agent
  for project X, no issue), `scheduler-task` (dynamic body, §5),
  `scheduler-ending` (reporting contract for project-scoped runs).

## 4. Recipes

`prompting/recipes.py` — code-owned ordered lists per prompt kind:

```python
RECIPES: dict[str, tuple[str, ...]] = {
    "coding-task": ("intro", "session-framing", "pidash-cli", "default-posture",
                    "autonomy", "state-routing", "analyze-and-scope",
                    "workpad-setup", "implementation", "blocking", "guardrails",
                    "workpad-template", "ending-run"),
    "review":      ("review-intro", "session-framing", "pidash-cli",
                    "review-cycle", "guardrails", "ending-run"),
    "scheduler":   ("scheduler-intro", "session-framing", "pidash-cli",
                    "scheduler-task", "guardrails", "scheduler-ending"),
}
```

- Kind names align with `PhaseConfig.template_name`
  (`orchestration/agent_phases.py`): the phase registry keeps mapping
  state → kind; only the lookup target changes from a `PromptTemplate`
  row to a recipe.
- Recipes are not user-editable in v1. Section _content_ is the
  customization surface; section _order and membership_ stays
  code-owned (it encodes step numbering and cross-references between
  sections).
- Where a shared section needs minor per-kind variation (e.g.
  `ending-run` describing review-specific done-signals), prefer Jinja
  conditionals on a context variable (`run.kind`) inside the shared
  section over forking the section — fork only when the conditional
  becomes the majority of the body.

## 5. Scheduler integration (goal 1)

### 5.1 The `scheduler-task` section

The scheduler kind has one **dynamic-body section**: `scheduler-task`.
Its body comes from data, not the registry:

```
{Scheduler.prompt}

{SchedulerBinding.extra_context}   # when non-empty
```

Structurally, a `Scheduler` row _is_ a per-scheduler section override
— `Scheduler.prompt` and `binding.extra_context` keep their existing
storage and editing surfaces; they simply slot into the recipe instead
of being the whole prompt.

### 5.2 Scheduler context builder

New `build_scheduler_context(binding)` (in `prompting/context.py`,
alongside `build_context`):

```python
{
    "workspace": {"slug": ..., "name": ...},
    "project":   {"id": ..., "identifier": ..., "name": ..., "description": ...},
    "scheduler": {"slug": ..., "name": ..., "description": ...},
    "run":       {"id": ..., "kind": "scheduler"},
}
```

Issue-centric keys (`issue`, `comments_section`, `parent_done_payload`,
`workpad_body`, …) do not exist in this context. Shared sections used
by the scheduler recipe must only reference keys present in _all_ their
kinds' contexts — enforced by save-time and CI validation (§6.3, §3.2).

### 5.3 Call-site change

`bgtasks/scheduler.py` stops concatenating; the fire path calls
`compose("scheduler", workspace, project, user=None, context=...)`
and `orchestration/service.py::dispatch_scheduler_run` receives the
composed prompt (or composes internally — keep the existing
`(run, fail_reason)` contract and `binding.last_error` reporting; a
`PromptRenderError` becomes a recorded fail reason exactly like
`no default pod`).

### 5.4 Jinja migration of existing `Scheduler.prompt` rows

Scheduler prompts become Jinja-rendered (sandboxed). Existing rows may
contain literal `{{` / `{%`. Migration: run `validate_syntax()` over
every `Scheduler.prompt` and `binding.extra_context`; rows that fail
get their delimiters escaped (`{{` → `{{ '{{' }}`) so behavior is
byte-identical to today. Save paths for scheduler prompts add the same
syntax validation going forward.

## 6. Overrides (goal 3)

### 6.1 Model

```python
class PromptSectionOverride(models.Model):
    id          = UUIDField(primary_key=True, default=uuid4)
    workspace   = FK("db.Workspace", CASCADE)
    user        = FK(AUTH_USER_MODEL, CASCADE, null=True)  # NULL = workspace-level
    section_key = CharField(max_length=64)
    body        = TextField()
    is_active   = BooleanField(default=True)
    version     = PositiveIntegerField(default=1)
    needs_attention = BooleanField(default=False)   # set by re-validation, §6.4
    updated_by  = FK(AUTH_USER_MODEL, SET_NULL, null=True, related_name="+")
    created_at / updated_at

    class Meta:
        constraints = [UniqueConstraint(
            fields=["workspace", "user", "section_key"],
            condition=Q(is_active=True),
            name="prompt_section_override_one_active",
        )]
        indexes = [Index(fields=["workspace", "user", "section_key", "is_active"])]
```

Replaces `PromptTemplate` (retirement: §8).

### 6.2 Resolution

One central function — the only place precedence lives:

```python
def resolve_section(key, *, workspace, project, user) -> ResolvedSection:
    """ResolvedSection = (key, body, source, override_version)
    source ∈ {"default", "workspace", f"user:{id}"}"""
    section = REGISTRY[key]
    if effective_customizability(section, workspace) == "locked":
        return default(section)
    # NOTE: `project` accepted and ignored in v1 (§9.4).
    if user is not None:
        row = active_override(workspace, user, key)
        if row: return from_override(row, source=f"user:{user.id}")
    row = active_override(workspace, None, key)
    if row: return from_override(row, source="workspace")
    return default(section)
```

- `effective_customizability(section, workspace)` returns the registry
  flag in v1; it exists so the deferred admin-lock tier (§9.2) lands
  as a change to one function.
- **Which `user` is passed** is the §9.1 rule, enforced at call sites:
  - State transition into a ticking phase, **Run AI** button,
    **Comment & Run** → the triggering human (`run.created_by`).
  - **Tick** (`bgtasks/agent_ticker.py` path), **scheduler beat**,
    any system-bot-created run → `user=None` (workspace + defaults).

### 6.3 Save-time validation (the §9.3 gate)

An override cannot be saved unless **all** pass:

1. `validate_syntax(body)` — Jinja parse (exists today).
2. For **every kind whose recipe contains the section**: compose the
   full prompt with this override slotted in and render it against
   **two synthetic sample contexts** — one fully populated, one
   minimal (all optionals `None`/empty: no parent, no comments, no
   labels) — using the kind's context shape (`build_context` vs
   `build_scheduler_context`). Catches typo'd variables,
   kind-mismatched variables (a shared section referencing `issue.*`
   while also used by `scheduler`), missing-`{% if %}` traps, and
   cross-section interactions.

Sample contexts are fixtures owned next to the context builders, with
a test asserting their key-sets match what the builders emit — so
schema drift breaks CI, not users.

Run-time backstop stays **fail-the-run** (no silent fallback — a
fallback would mean the user's customization quietly stops applying,
which is the credibility failure mode of the current feature). The
error message names the failing section and its source:
`section 'implementation' (override by alice@…, v3) failed: …` —
derivable because compose renders the final body but maps error
positions back through per-section offsets (compose records each
section's line range in the concatenated body).

### 6.4 Schema/registry drift

When _we_ change things, existing overrides must not break silently:

- **Context variable removed/renamed** (`build_context` change): a
  management command re-validates every active override (same checks
  as §6.3) and sets `needs_attention=True` on failures — never
  deletes, never deactivates. Surfaced as a badge in the section UI;
  affected runs would fail loudly per §6.3 until fixed. Removals of
  context keys require running this command in the same PR (checklist
  item in the doc-block of `build_context`).
- **Section key removed/renamed**: migration must deactivate or re-key
  orphaned overrides explicitly.
- **Section flipped overridable → locked**: existing overrides stay in
  the DB but stop resolving (locked short-circuits); UI shows them as
  "inactive — section is now locked".

## 7. Composer API and visibility (goal 4)

### 7.1 Composer

```python
# prompting/composer.py — replaces load_template/build_first_turn internals
def compose(kind, *, workspace, project, user, context) -> ComposedPrompt:
    """ComposedPrompt = (text, manifest)
    manifest = [{section_key, source, version, line_start, line_end}, ...]"""

def build_first_turn(issue, run) -> str:   # signature unchanged for callers
    kind = template_name_for(issue.state)          # phase registry, unchanged
    user = run.created_by if run_is_human_triggered(run) else None
    composed = compose(kind, workspace=issue.workspace, project=issue.project,
                       user=user, context=build_context(issue, run))
    persist_manifest(run, composed.manifest)
    return composed.text
```

- `AgentRun.prompt` keeps storing the final rendered text (audit
  record, unchanged).
- The **composition manifest** is persisted per run (new JSON field on
  `AgentRun` or inside `run_config`) so "why did this run behave
  differently" is answerable by diffing manifests: which sections,
  whose overrides, which versions.
- Trigger classification (`run_is_human_triggered`) derives from the
  existing trigger plumbing in `orchestration/scheduling.py`
  (`TRIGGER_RUN_AI`, comment-continuation, state-transition actor vs.
  tick/system creator resolution) — it must be explicit at run
  creation, not inferred after the fact.

### 7.2 Endpoints

Replace the four `PromptTemplate` endpoints (`prompting/urls.py`) with:

```
GET    /workspaces/<slug>/prompt-sections?kind=<kind>
       → ordered section list for the kind, each: key, title,
         customizable, resolved body, source (default|workspace|user),
         needs_attention, override metadata. ?scope=user resolves with
         the caller as user; default scope resolves workspace-level.
       Member-readable.

PUT    /workspaces/<slug>/prompt-sections/<key>?scope=workspace|user
       → upsert override body (runs §6.3 validation; 400 with
         section/line attribution on failure).
       scope=workspace: admin-only. scope=user: any member, own row only.

DELETE /workspaces/<slug>/prompt-sections/<key>?scope=...
       → deactivate override (revert to next rung). Same permissions.

GET    /workspaces/<slug>/prompts/<kind>/compiled?scope=user|workspace
       → the assembled final template (Jinja markers intact) + the
         per-section breakdown. This is goal 4's "see the final
         template". Member-readable. When user overrides exist, the
         response distinguishes "what you get when you trigger" from
         "what automatic runs get" (workspace-only resolution) — two
         compilations, per §9.1.

POST   /workspaces/<slug>/prompts/<kind>/preview
       → render the compiled template against a real issue
         (coding-task/review: body of today's preview endpoint,
         generalized) or a scheduler binding (scheduler kind), without
         creating a run. Admin-gated like today's preview.
```

The section breakdown with `source` per section is the v1
admin-governance surface (§9.2): admins can _see_ every user override
in effect even though they cannot yet lock sections.

## 8. Retirement of `PromptTemplate`

- `composer.load_template()` and the seed machinery
  (`seed_default_template*`, `reseed_default_template` command, the
  `post_migrate` receiver, `PI_DASH_SKIP_PROMPT_SEED`) are deleted —
  defaults are code, so there is no DB sync and the
  `PromptTemplateNotFound` / "did the seed migration run?" failure
  mode disappears.
- Migration: any active **workspace-scoped** `coding-task` row is
  archived (`is_active=False`) with its body preserved on the row;
  the new section UI links "your previous custom template" so an admin
  can copy content into section overrides by hand. No automatic
  blob→section diffing — the feature is effectively unused, a
  tombstone is honest and cheap.
- The `PromptTemplate` model/table is dropped in a follow-up migration
  one release later (grace period for the copy-out path).

## 9. Resolved decisions

### 9.1 Whose user overrides apply to automatic runs

**Human-triggered runs use the triggering user's overrides; ticks,
scheduler beats, and system-bot runs resolve workspace + defaults
only.** Rationale: per-user prompts on unattended runs make the same
issue's successive runs personality-shift with reassignment and make
automatic behavior non-reproducible per project; conversely a
scheduler is team infrastructure, not a personal delegate
(`binding.actor` is merely "whoever installed it"). The seam this
creates (manual run customized, follow-up tick stock) is documented in
the compiled view (§7.2 shows both compilations). Upgrading later to
sticky-per-engagement resolution (pin the resolution at first run,
reuse on continuations — precedent: `pinned_runner_id`) is additive
because resolution is centralized; revisit when manifest data shows
the seam hurting in practice.

### 9.2 Admin lock on overridable sections

**Deferred.** Two-state customizability (platform-set) in v1. The
resolution path goes through `effective_customizability(section,
workspace)` from day one so a per-workspace three-state policy
(open / workspace-only / locked) lands as one function change + one
policy row later. Compensating control in v1: override sources are
visible to admins in the section breakdown and in run manifests.
Revisit trigger: a workspace asks for it with evidence, or rollout to
compliance-sensitive workspaces.

### 9.3 Broken override at run time

**Prevent at save time; stay fail-loud at run time.** Save gate =
syntax + dual-sample-context full-prompt render for every kind the
section appears in (§6.3). Run-time `PromptRenderError` still fails
the run (per current `service.py` behavior) — no silent fallback to
defaults — but the error attributes the failing section and override
author/version. Our own schema changes re-validate all active
overrides and flag `needs_attention` (§6.4).

### 9.4 Project-level override layer

**Deferred.** `resolve_section()` accepts `project` from day one and
ignores it, so adding the rung (`user → project → workspace →
default`) later touches only the resolver internals — every call site
already has a project. Interim escape valve: section bodies are Jinja
and `project.*` is in context, so a workspace override can branch per
project. The strongest per-project need (scheduler task body) is
already per-binding by construction (§5.1). Revisit trigger: demand
evidenced by per-project `{% if %}` branching appearing in real
workspace overrides.

## 10. Implementation phasing

Four PRs, each shippable:

- **PR 1 — Registry + recipes + compose-time assembly.**
  `prompting/sections/` with front-matter, `recipes.py`, new
  `compose()`; port `coding-task` and `review` onto it; split the
  review monolith into sections (review gains `pidash-cli` /
  `session-framing` — the one intended behavior change); golden-file
  snapshot tests of assembled output per kind; CI registry checks
  (§3.2). `PromptTemplate` lookup bypassed but model untouched.
- **PR 2 — Scheduler onto the composer.** `scheduler` recipe +
  sections, `build_scheduler_context`, `scheduler-task` dynamic body,
  call-site change in `bgtasks/scheduler.py` /
  `dispatch_scheduler_run`, Jinja-escape migration for existing
  scheduler prompt rows (§5.4), render-failure → `binding.last_error`.
- **PR 3 — Overrides.** `PromptSectionOverride` model, `resolve_section`
  with the §9.1 user rule, save-time validation, section CRUD
  endpoints (workspace scope, then user scope), composition manifest
  on `AgentRun`, `PromptTemplate` retirement migration (§8).
- **PR 4 — Visibility.** `compiled` endpoint (dual compilation per
  §9.1), generalized `preview`, the section-management UI (section
  list, lock badges, source/modified indicators, `needs_attention`,
  final-template view), re-validation management command (§6.4).

## 11. Testing strategy

- **Golden files**: assembled template per kind (defaults only)
  snapshot-tested; any section/recipe edit shows up as a readable diff
  in review.
- **Resolution matrix**: parametrized tests over
  (locked/overridable) × (no override / workspace / user / both) ×
  (user passed / user=None) asserting body + source.
- **Validation**: each §6.3 failure class has a test (syntax, unknown
  variable, kind-mismatch on shared section, minimal-context trap);
  sample-context fixtures asserted in sync with context builders.
- **Scheduler parity**: composed scheduler prompt contains the binding
  task body verbatim post-migration for escaped legacy rows;
  render-failure lands on `binding.last_error`.
- **Manifest**: every created run carries a manifest consistent with
  the resolution inputs; tick-created runs never carry `user:*`
  sources.
- Existing contract tests for the retired endpoints are replaced, not
  deleted, by section-CRUD contract tests with the same
  permission-matrix rigor (`tests/contract/prompting/`).
