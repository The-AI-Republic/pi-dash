# Pi Dash Prompt System — MVP Design

This document specifies the MVP prompt system that turns a Pi Dash **Issue** into a fully-rendered instruction string sent to the coding agent (Codex) via a runner.

The design borrows from Symphony's `WORKFLOW.md` approach: the prompt is not a task description, it is a **workflow handbook** that teaches the agent how to move an issue through Pi Dash's state machine and record its progress in a structured comment on the issue.

Companion docs in this directory:

- `workflow-handbook.md` — the actual default prompt body (workflow rules, per-state routing, workpad template, done-signal schema). Data, not code. Evolves independently of the system design.

---

## 1. Scope

### In scope

- A Django module (`apps/api/pi_dash/prompting/`) that renders a prompt string for a given `AgentRun` at creation time.
- A DB-backed `PromptTemplate` model with workspace-scoped overrides and a seeded global default.
- Jinja2 (sandboxed) as the template engine.
- A narrow, stable **template context** of issue + workspace + project + run fields.
- A first-turn renderer. (Continuation-turn rendering is stubbed but deferred — runner is single-turn in MVP.)
- A **preview endpoint** for rendering a template against a sample issue without creating a run.
- A default `workflow-handbook` seed that encodes Pi Dash's state machine, instructs the agent to maintain an **Agent Workpad** comment on the issue, and defines the **done-signal** the agent must emit at the end.
- Milestone-style progress tracking inside the Agent Workpad (`investigation_complete`, `design_choice_recorded`, `implementation_complete`, `validation_complete`, `pr_opened`, `review_feedback_addressed`) rather than an arbitrary percentage, with optional checkpoints allowed to be marked `n/a`.
- A structured escalation model in the prompt contract so the agent can report when it can continue autonomously vs when it should pause for human input.
- An internal orchestration module at `apps/api/pi_dash/orchestration/service.py` that reacts to issue-state transitions, creates follow-up `AgentRun`s when appropriate, renders prompts, and enqueues work to the runner.
- Storage of the rendered prompt on `AgentRun.prompt` at creation time (immutable after).

### Out of scope for MVP

- Continuation-turn prompting (runner is single-turn; multi-turn is a later runner capability).
- Web UI template editor (edit via Django admin for now).
- Template versioning beyond an integer `version` field.
- Multiple template **names** per workspace (just `"coding-task"` in MVP).
- Non-coding task modes (decision #4 of scoping: all tasks are coding tasks).
- Agent-authored prompt rewrites / meta-prompting.
- Extracting linked-issue / comment / PR context into variables — the agent fetches those via MCP tools at runtime.
- Per-project (sub-workspace) template overrides — workspace-scoped is the only override level in MVP.
- Localization / i18n of the handbook.

---

## 2. Committed decisions

| #   | Topic                    | Decision                                                                                                                                                                                    |
| --- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Where prompts are built  | Pi Dash cloud (Django). Runner receives a ready-to-use string. (Confirms runner-design decision #4.)                                                                                         |
| 2   | Template engine          | Jinja2 with `SandboxedEnvironment`. Closest Python analog to Symphony's Liquid; templates are workspace-admin-editable so unsandboxed execution is unacceptable.                            |
| 3   | Template storage         | DB model `PromptTemplate`, scoped per-workspace, with a global default row seeded at migrate time from `prompting/templates/default.j2` (the rendered form of `workflow-handbook.md`).      |
| 4   | Template selection       | Lookup order: `(workspace, name="coding-task")` → global `(workspace=NULL, name="coding-task")`. No per-project override in MVP.                                                            |
| 5   | Task mode                | All `AgentRun`s are coding tasks in MVP. No mode detection / no branching templates by intent.                                                                                              |
| 6   | Render timing            | At `AgentRun` creation. The rendered string is stored on `AgentRun.prompt` and is **immutable** for the lifetime of that run. Edits to the template after do not affect in-flight runs.    |
| 7   | First turn vs continuation | MVP renders first turn only. The `composer.build_continuation()` entry point is stubbed to raise `NotImplementedError` so the shape is reserved.                                          |
| 8   | Dynamic variables        | Narrow, explicit set (see §5). No arbitrary ORM access from templates. Adding variables is a deliberate code change, not a template-author concern.                                        |
| 9   | Workflow encoding        | Static handbook content carries the workflow rules (per-state routing, escape hatches, workpad template, done-signal). Dynamic fields only identify *which* issue this run is for.         |
| 10  | Progress model           | The handbook uses milestone completion and current phase in the workpad to express progress. The system does **not** ask the agent for a scalar percent-complete estimate.                   |
| 11  | Escalation model         | The handbook instructs Codex to emit a structured autonomy/escalation payload (`score`, `type`, `reason`, `question_for_human`, `safe_to_continue`) as metadata. Cloud-side branching keys off `status`; autonomy fields explain why that outcome was chosen. |
| 12  | Workpad pattern          | The handbook instructs the agent to maintain exactly one `IssueComment` with a `## Agent Workpad` marker header per issue, edited in place across turns. See `workflow-handbook.md`.        |
| 13  | Done signal              | The handbook instructs Codex to emit a structured JSON block as its final turn output. Runner forwards it verbatim (per runner-design decision #5); cloud parses and acts.                  |
| 14  | Orchestration boundary   | Run-trigger logic lives in `apps/api/pi_dash/orchestration/service.py`. Prompting renders strings; orchestration decides when runs are created and queued.                                   |
| 15  | Preview surface          | `POST /workspaces/{slug}/prompt-templates/{id}/preview` with `{"issue_id": ...}` returns the rendered string. Admin-only. Critical for iterating on the handbook.                          |

---

## 3. Architecture

```
┌──────────────────────────── Pi Dash (Django) ────────────────────────────┐
│                                                                         │
│  Issue enters a delegated execution state                               │
│  (default: `In Progress`; generally any configured `started` state)     │
│                      │                                                  │
│                      ▼                                                  │
│          orchestration.service handles transition                       │
│          ├─ active run exists?           -> no-op                       │
│          ├─ prior run can continue?       -> create follow-up run        │
│          └─ otherwise                    -> AgentRun.objects.create_run  │
│                                              │                          │
│                                              ▼                          │
│                                      prompting.composer                 │
│                                      ├─ load_template(workspace)        │
│                                      ├─ build_context(issue, run)       │
│                                      ├─ render (Jinja2 sandbox)         │
│                                      └─ return prompt string            │
│                                              │                          │
│                                              ▼                          │
│                                   AgentRun.prompt = <rendered>          │
│                                   AgentRun.status = queued              │
│                                              │                          │
│                                              ▼                          │
│                              runner.services.matcher ─► Runner chosen   │
│                                              │                          │
│                                              ▼                          │
│                              Channels consumer sends `Assign`           │
│                              envelope carrying AgentRun.prompt          │
│                                                                         │
└─────────────────────────────────────┬───────────────────────────────────┘
                                      │ WebSocket (existing)
                                      ▼
                              pi-dash-runner
                              (thin — receives string verbatim,
                               passes to `codex app-server`)
```

The runner is unchanged. The new surface area is the Django-side `prompting/` module plus `apps/api/pi_dash/orchestration/service.py`, which owns run-trigger and enqueue logic when an issue enters a delegated execution state.

---

## 4. Data model

One new model for prompting, plus small extensions to `AgentRun` to make run lineage and lifecycle explicit.

```python
# existing model, with MVP clarifications
class AgentRun(TimeAuditModel):
    work_item      = FK(Issue, null=True, on_delete=CASCADE)
    prompt         = TextField(default="")  # immutable for this run once created
    parent_run     = FK("self", null=True, on_delete=SET_NULL)  # null for initial run; points to prior run for follow-up attempts
    done_payload   = JSONField(null=True, blank=True)  # normalized parsed final done-signal payload
    status         = CharField(
        max_length=32,
        choices=[
            ("queued", "Queued"),
            ("running", "Running"),
            ("blocked", "Blocked"),
            ("failed", "Failed"),
            ("completed", "Completed"),
            ("cancelled", "Cancelled"),
        ],
    )
```

```python
# existing model, with MVP clarifications
class AgentRunEvent(TimeAuditModel):
    agent_run      = FK(AgentRun, on_delete=CASCADE)
    kind           = CharField(max_length=64)
    payload        = JSONField()
```

```python
# existing model, with MVP clarifications
class Project(TimeAuditModel):
    # ... existing fields ...
    repo_url       = CharField(max_length=512, blank=True, default="")
    base_branch    = CharField(max_length=128, blank=True, default="main")
```

```python
# apps/api/pi_dash/prompting/models.py

class PromptTemplate(TimeAuditModel):
    workspace    = FK(Workspace, null=True, on_delete=CASCADE)   # NULL = global default
    name         = CharField(max_length=64)                       # MVP: always "coding-task"
    body         = TextField()                                    # Jinja2 source
    is_active    = BooleanField(default=True)
    version      = PositiveIntegerField(default=1)                # bumped on edit
    updated_by   = FK(User, null=True, on_delete=SET_NULL)

    class Meta:
        constraints = [
            UniqueConstraint(
                fields=["workspace", "name"],
                condition=Q(is_active=True),
                name="prompt_template_one_active_per_ws_name",
            ),
        ]
        indexes = [Index(fields=["workspace", "name", "is_active"])]
```

**Seeding**: a post-migrate signal (or data migration) inserts the global default row from `prompting/templates/default.j2`. The file lives in-repo; the row is the source of truth at runtime. The file is only read during the seed step and after schema migrations that ship an updated default (operator opts in to re-seed via a management command — we do not silently overwrite edited workspace rows).

**Why DB, not files at runtime**: Pi Dash is multi-tenant. Filesystem-backed templates don't give per-workspace isolation, can't be edited from the web UI later, and require SSH for tweaks. DB storage keeps the "edit your prompt" story sane as soon as we add a UI.

**AgentRun semantics**:
- An `Issue` is the task.
- An `AgentRun` is one concrete execution attempt of the coding agent on that task.
- An issue may have many runs over time, but at most one active run (`queued` or `running`) at once.
- A follow-up run is always a new row with a new immutable prompt. It links back to the prior attempt via `parent_run_id`.
- `AgentRun.done_payload` is the normalized final parsed outcome for the run and is the backend source of truth for structured completion metadata.
- `AgentRunEvent` stores raw runtime events, streaming messages, and audit trail data. It is not the source of truth for final run outcome.

---

## 5. Template context

The **only** fields the template sees. Building this dict is the serializer layer's job — ORM objects never enter Jinja.

```python
{
    "issue": {
        "id":            str,    # UUID (for agent to call back via API if needed)
        "identifier":    str,    # "PROJ-42" style, composed from project + sequence_id
        "title":         str,    # from Issue.name
        "description":   str,    # raw markdown from Issue.description; preserve headings/code fences for agent parsing
        "state":         str,    # State.name (human-readable, e.g. "In Progress")
        "state_group":   str,    # StateGroup value: backlog|unstarted|started|completed|cancelled
        "priority":      str,    # urgent|high|medium|low|none
        "labels":        list[str],  # label names only
        "assignees":     list[str],  # display names only
        "url":           str,    # deep link to the issue in the web app
        "target_date":   str|None,   # ISO date
    },
    "workspace": {
        "slug": str,
        "name": str,
    },
    "project": {
        "id":         str,
        "identifier": str,    # Project.identifier (the prefix in issue identifier)
        "name":       str,
    },
    "repo": {
        "url":         str|None,   # from Project.repo_url
        "base_branch": str|None,   # from Project.base_branch
    },
    "run": {
        "id":            str,
        "attempt":       int,   # incremented across retries on the same issue (MVP: always 1)
        "turn_number":   int,   # MVP: always 1
    },
}
```

**Deliberately absent** (deferred):

- `issue.comments` — agent fetches via API/MCP tool at runtime.
- `issue.parent`, sub-issues, linked issues.
- `cycles`, `modules` — filterable dimensions; not load-bearing for a single-issue run.
- Prior-run outcome / previous workpad content — the workpad comment itself carries this; agent reads it.

Adding fields is a deliberate, reviewed change to `prompting/context.py` — not a surprise because someone edited a template.

---

## 6. Render lifecycle

```
AgentRun.create()
   │
   ▼
composer.build_first_turn(issue, run) -> str
   ├─ load_template(workspace, name="coding-task")
   ├─ build_context(issue, run) -> dict
   ├─ renderer.render(body, context) -> str
   └─ return string
   │
   ▼
run.prompt = rendered_string
run.save(update_fields=["prompt"])
```

Issue-state transition lifecycle:

```
Issue state changes: Todo -> In Progress
   │
   ▼
orchestration.service.handle_issue_state_transition(issue, from_state, to_state, actor)
   ├─ check whether `to_state.name == "In Progress"` (MVP trigger rule)
   ├─ if active run exists (`queued` or `running`), no-op
   ├─ else determine parent_run (most recent prior run, if any)
   ├─ create new AgentRun(status="queued", parent_run=<prior or null>)
   ├─ call composer.build_first_turn(issue, run)
   ├─ persist run.prompt
   └─ enqueue run for the runner
```

Module layout:

```
apps/api/pi_dash/prompting/
  ├─ __init__.py
  ├─ apps.py
  ├─ models.py               # PromptTemplate
  ├─ context.py              # build_context(issue, run) -> dict
  ├─ renderer.py             # Jinja2 SandboxedEnvironment; render(body, ctx) -> str
  ├─ composer.py             # build_first_turn(issue, run); build_continuation (raises)
  ├─ seed.py                 # seed_default_template(); called from migration
  ├─ views.py                # preview endpoint
  ├─ urls.py
  ├─ templates/default.j2    # in-repo seed source — mirrors workflow-handbook.md
  └─ tests/
```

**Invariants**:

- `composer` is the only caller-facing function; views/services never render templates directly.
- `renderer.render()` wraps `SandboxedEnvironment.from_string(body).render(**ctx)`. Any `TemplateError` bubbles as a domain exception (`PromptRenderError`) so call sites can fail the `AgentRun` cleanly instead of 500-ing the request.
- `context.build_context()` never raises on missing optional fields — it returns `None` / `""` / `[]` and lets the template's `{% if %}` blocks handle absence.
- `orchestration.service` is the only module that decides whether an issue-state transition should create a run. Views and model hooks may call into it, but they must not duplicate its decision logic.

---

## 7. Workpad, progress checkpoints, and done-signal (summary)

Full content lives in `workflow-handbook.md`. Summary here so this design doc is self-contained:

- **Agent Workpad** — the handbook instructs the agent to find or create **exactly one** `IssueComment` on the issue whose body begins with `## Agent Workpad`. The agent edits this comment in place across turns and uses it as its persistent scratchpad (Phase / Progress Checkpoints / Plan / Acceptance Criteria / Validation / Notes / Confusions sections). It is stored as a normal `IssueComment` row in the primary application database and should be authored by a dedicated agent service user.

- **Progress checkpoints** — progress is represented as explicit milestone completion, not a guessed percentage. The default checkpoint set is:
  - `investigation_complete`
  - `design_choice_recorded`
  - `implementation_complete`
  - `validation_complete`
  - `pr_opened`
  - `review_feedback_addressed`

Checkpoint values are tri-state:
  - `true` — completed in this run or a prior run
  - `false` — still incomplete
  - `"n/a"` — not applicable for this task or this run (for example, a non-PR investigative task)

- **Escalation / human-involvement signal** — the handbook requires the agent to maintain a structured autonomy payload in the workpad and final done signal:

```json
{
  "score": 0,
  "type": "none" | "assumption" | "decision" | "blocker",
  "reason": "why this score/type was chosen",
  "question_for_human": "specific question or null",
  "safe_to_continue": true
}
```

The score is a policy hint, not the sole control signal. `type` and `safe_to_continue` carry the real semantics, but `status` remains the parser's source of truth for run outcome:
  - `none` / `assumption` generally means the agent may continue and should record its rationale.
  - `decision` means a human-visible choice is needed; cloud policy may pause based on workspace threshold.
  - `blocker` means the task cannot responsibly continue without an external dependency, missing access, or explicit human action.

- **Done signal** — the agent's final message must include a fenced JSON block tagged `pi-dash-done` carrying `{status, summary, state_transition, changes, validation, progress, autonomy, blockers}`. The runner forwards this verbatim (runner-design decision #5); the cloud parses it and decides what to do (move the issue to the requested state, post a summary, etc.). `status` is the branch point; `autonomy` is explanatory metadata. Schema is pinned in `workflow-handbook.md`.

- **Done-payload ingestion** — in MVP, the cloud parses the terminal `pi-dash-done` block from the final `AgentRunEvent`, normalizes it, and persists the normalized JSON to `AgentRun.done_payload`. `AgentRunEvent` remains the raw audit trail; `done_payload` is the structured backend field used for orchestration follow-up and UI summaries.

The cloud-side parser for the done signal is a separate piece of work not scoped here — this design only guarantees that the prompt produces a parseable signal.

---

## 8. Integration points

- **Run trigger policy** — Pi Dash is a pure agent-orchestration system, so issue state is operational rather than descriptive. An issue entering a delegated execution state means the task has been handed to the coding agent for autonomous execution. In the default workflow this state is named `In Progress`; in custom workflows it should be interpreted as a state in the `started` group that carries delegation semantics.
  - `Todo` means queued and ready, but not yet delegated.
  - `In Progress` is the only delegation-trigger state in MVP.
  - other states in the `started` group do not auto-trigger runs unless explicitly wired in later
  - `Blocked`, `Done`, and `Cancelled` are not runnable.
  - `In Review` is a first-class issue state in MVP. If review feedback later sends the task back to `In Progress`, that transition triggers a new follow-up run.
- **Orchestration service** — `apps/api/pi_dash/orchestration/service.py` is an internal application module, not a separate product or external service. Its job is to react to issue-state transitions and own the delegation workflow:
  - detect whether the transition should trigger an agent run
  - enforce the single-active-run guardrail
  - create a new `AgentRun` or follow-up `AgentRun`
  - call the prompting composer to render `run.prompt`
  - enqueue the run to the runner
  - centralize this logic so it is not duplicated in views, signals, model `save()`, or prompting code
- **AgentRun creation / follow-up** — when an issue enters `In Progress`, orchestration evaluates:
  - if an active run already exists for the issue, do nothing
  - if the issue re-enters `In Progress` after any prior non-active run, always create a new follow-up run linked to the prior attempt; never reopen or mutate the old run's prompt
  - otherwise create a new `AgentRun`, call `composer.build_first_turn(issue, run)` before saving, and assign the result to `run.prompt`
  - if composition raises, the run is marked `failed` with a clear error and nothing is sent to any runner
- **Single-active-run guardrail** — at most one active `AgentRun` may exist per issue at a time. Active means `queued` or `running`. An issue may accumulate multiple runs across its lifecycle (initial implementation, retry, post-blocker resume, review-feedback follow-up), but only one may be running or queued concurrently.
- **Runner protocol** — unchanged. The `Assign` envelope already carries `AgentRun.prompt` as a string. Runner passes it to `codex app-server` unmodified.
- **Runner dispatch boundary** — `orchestration.service` does not talk to the runner process directly. It calls the existing runner dispatch/enqueue interface responsible for delivering the `Assign` envelope.
- **Django admin** — `PromptTemplate` registered with a large `body` textarea. Read-only for global default; workspace admins can create/edit their workspace's row.
- **Preview endpoint** — `POST /api/v1/workspaces/{slug}/prompt-templates/{id}/preview` with `{"issue_id": "<uuid>"}` → `{"prompt": "<rendered string>"}`. Requires workspace admin. Does **not** create an `AgentRun`.

---

## 9. Open questions

1. **Prompt size budget** — Symphony's rendered prompt is ~290 lines. Codex's context window is generous but not infinite. Do we set a hard cap (`len(prompt) <= N`) enforced at render time? Probably yes; noisy templates shouldn't silently push the model past a useful budget.
2. **Template editor auth** — MVP uses Django admin, but workspace admins aren't Django admins. Does the preview endpoint require `is_staff` or workspace-admin role? Decide before shipping.
3. **Pause / continue policy owner** — where does the threshold live for escalation scores and `safe_to_continue` handling? Workspace-level setting, runner policy, or cloud-side orchestration config? Note that `status` remains authoritative for outcome parsing; any threshold policy affects future orchestration and notification, not how a finished run is interpreted.
4. **Checkpoint persistence** — in MVP the checkpoints live only in the workpad and final done signal. Do we also want a first-class run-level checkpoint table or event payload for easier UI rendering beyond what `AgentRunEvent` and the workpad already provide?

---

## 10. MVP acceptance

The MVP is done when:

- Creating an `AgentRun` for an issue produces a non-empty `AgentRun.prompt` rendered from the seeded default handbook.
- Moving an issue from `Todo` to `In Progress` calls `orchestration.service`, which creates exactly one delegated `AgentRun`, unless an active run already exists.
- Re-entering `In Progress` after a prior non-active run always creates a new follow-up `AgentRun` linked by `parent_run_id`; no old run is reopened in place.
- The runner receives the rendered prompt unchanged and invokes `codex app-server` with it.
- The agent, following the handbook, creates a `## Agent Workpad` `IssueComment` on the issue under a dedicated agent/system user and updates it as work progresses.
- The workpad expresses progress via explicit phase + milestone completion, not a free-form percentage guess.
- The agent's final turn emits a parseable done signal including the escalation payload, and the backend normalizes that payload into `AgentRun.done_payload`.
- A workspace admin can preview the rendered prompt for any issue without creating a run.
- Editing the workspace's `PromptTemplate.body` changes the prompt rendered for **new** runs; in-flight runs keep their stored prompt.
