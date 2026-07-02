# Prompt section customization — governance tiers + admin/member UI

Status: backend tiers landed; frontend page pending.

## Why

PR #235 replaced the legacy single-blob `PromptTemplate` REST surface
(`/api/workspaces/<slug>/prompt-templates`) with the section-based API
(`prompt-sections`, `prompts/<kind>/compiled`, `prompts/<kind>/preview`) but
shipped **backend-only**. The old `/prompts` web page (list + detail) still
calls the deleted `prompt-templates` endpoints, so every action 404s and the
"Customize for this workspace" button shows _"Something went wrong."_

The fix is not to resurrect the legacy endpoints (the `PromptTemplate` model is
retired at render time — agent prompts are composed from sections now). It is to
build the section-customization UI the section design always intended
(`design.md` §8.2: _"the new section UI links 'your previous custom template'"_).

## Governance model (the three tiers)

Each section declares a `customizable` tier in its front-matter. Resolution
precedence is unchanged (personal → workspace → registry default); the tier
controls **who may write at which scope**:

| Tier (`customizable`) | Workspace override (admin) | Personal override (member) | Example                                               |
| --------------------- | -------------------------- | -------------------------- | ----------------------------------------------------- |
| `locked`              | ✗                          | ✗                          | `pidash-cli`, `guardrails`, `intro`                   |
| `workspace`           | ✓                          | ✗                          | (admin-governed policy sections)                      |
| `overridable`         | ✓                          | ✓                          | `implementation`, `review-cycle`, `analyze-and-scope` |

- A **personal** override (`scope=user`) only affects that member's
  human-triggered runs (dual compilation, §9.1). It never changes the workspace
  default or automatic/scheduled runs.
- A **workspace** override (`scope=workspace`) is the shared default for the
  whole workspace and for automatic runs; admin-only.
- `workspace`-tier sections are admin-governed: even a stale personal row left
  over from a tier downgrade does **not** resolve (composer ignores user rows
  unless the tier is `overridable`).

The `workspace` tier is **available but unassigned** — no real section is tagged
`workspace` yet. Promoting specific sections (e.g. a default-posture / autonomy
policy) into `workspace` is a product decision, made by editing that section's
front-matter; no code change needed.

`composer.effective_customizability(section, workspace)` remains the seam for a
future _dynamic_ per-workspace admin-lock (§9.2) — admins pinning an otherwise
`overridable` section — without touching the resolver or the views.

## Backend (landed)

- `registry.py`: third value `CUSTOMIZABLE_WORKSPACE`; section properties
  `allows_workspace_override` / `allows_personal_override`.
- `composer.resolve_section`: only applies a personal override when the
  effective tier is `overridable`.
- `views.py` `PUT prompt-sections/<key>`: per-scope tier gate — `locked` blocks
  all scopes, `workspace` blocks `scope=user`, `overridable` allows both (role
  check still gates workspace writes to admins).
- `_section_breakdown` / `ResolvedSectionSerializer`: every section now returns
  `default_body` (pristine registry default, for an override-vs-default diff)
  and the capability flags `editable_at_workspace` / `editable_at_personal`.
- Tests: `test_registry.py` (tier capability), `test_sections_crud.py`
  (workspace-tier admin-allowed / member-denied, list shape).

## Frontend (pending) — replaces the broken `/prompts` page

Build on the section API; delete the `prompt-template` store/service/types.

1. **Section list per kind.** `GET /prompt-sections?kind=&scope=` renders the
   full ordered recipe. Every section is shown, including `locked` ones:
   - lock badge + read-only body when `editable_at_workspace === false`;
   - source pill (`default` / `workspace` / `your override`);
   - `needs_attention` warning when a code change broke an existing override.
     Kind switcher across `coding-task` / `review` / `scheduler`.
2. **Scope-aware editing.** Show the workspace-override editor when
   `editable_at_workspace && userIsAdmin`; show the personal-override editor when
   `editable_at_personal`. Save → `PUT .../<key>?scope=workspace|user`; revert →
   `DELETE`. Surface 400 validation `detail` inline.
3. **Override-vs-default diff.** Use `default_body` vs `body` so the editor can
   show what changed and a one-click "reset to default" (DELETE).
4. **Receipt / ingredients view.** `GET /prompts/<kind>/compiled` returns the
   assembled `template_body` (the finished prompt) plus the per-section
   breakdown (the ingredients, each tagged with its source). Render the ordered
   ingredient cards → final assembled prompt. When personal overrides exist,
   `automatic_template_body` shows what automatic runs get ("yours vs automatic").
5. **Preview.** `POST /prompts/<kind>/preview` against a picked issue
   (`issue_id`) or scheduler binding (`binding_id`); admin-gated.

## Deferred

- Dynamic per-workspace admin lock (pin an `overridable` section) — the
  `effective_customizability` seam (§9.2).
- Admin governance dashboard listing every member's personal overrides.
