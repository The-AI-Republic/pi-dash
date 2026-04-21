# Project & Issue Git Fields — Design

**Status:** Draft
**Date:** 2026-04-20
**Scope:** One PR, all layers (model → API → runner protocol → UI)

---

## 1. Problem

The Codex agent needs two things to work on code for an issue:

1. **Which repo to clone** — a URL.
2. **Which branch to operate on** — either an existing feature branch to continue, or a base branch to branch off from.

Today, `Project` has `repo_url` and `base_branch` (migration `0123`), but:

- Neither field is exposed in any UI. They can only be set via Django admin or direct DB writes.
- There is no issue-level branch override, so the agent always branches off `project.base_branch` (or the hardcoded `"main"` fallback baked into the prompt template).
- The `base_branch` fallback hardcodes `"main"` instead of deferring to the repository's actual default branch.

Users need to set these fields through the normal product UI, and they need a way to point a single issue at an existing feature branch without changing project defaults.

## 2. Goals

- Make `repo_url` and `base_branch` visible, settable, and editable from the web UI on project create and project settings.
- Add an issue-level `git_work_branch` field with matching UI on issue create and issue detail/edit.
- Let empty values mean "use the remote's default branch" (auto-detect) rather than the literal string `"main"`.
- Thread the new field through the full orchestration → runner path so the runner daemon can honor it when it checks out code.

## 3. Non-Goals

- No repository connection / OAuth flow. Users paste a URL; we do not verify it.
- No branch-picker autocomplete. Free-form string input.
- No multi-repo projects. One repo per project is still the assumption.
- No change to the matching, dispatching, or supervisor logic beyond passing the extra string.
- No validation that the `git_work_branch` actually exists on the remote. The runner surfaces the error if checkout fails.

## 4. Key Concepts

Two distinct branch roles matter, and naming them clearly avoids downstream confusion:

| Role                | Field             | Level   | Meaning                                                                                                       |
| ------------------- | ----------------- | ------- | ------------------------------------------------------------------------------------------------------------- |
| **Merge target**    | `base_branch`     | Project | Branch the agent rebases onto and targets with PRs. Team's integration branch.                                |
| **Checkout target** | `git_work_branch` | Issue   | Existing branch the agent should check out and commit directly onto. Overrides "create new branch from base". |

Why different names: using `branch` at both levels would force readers (and the agent prompt) to infer intent from context. `base_branch` vs `git_work_branch` are self-documenting.

### 4.1 Naming Inconsistency — Accepted

Project already has `repo_url` / `base_branch` (not `git_repo_url` / `git_base_branch`). We are adding `git_work_branch` on Issue with a `git_` prefix because the issue model has many unprefixed `branch`/`name`/`url`-shaped fields and `git_` disambiguates intent on the issue.

We deliberately do NOT rename the existing project fields. Rationale:

- They ship in migration `0123`, already in the DB.
- A rename means another migration plus serializer/context/runner renames for no behavioral gain.
- The inconsistency is small and local to two models.

## 5. Resolution Order (at run time)

When orchestration builds `run_config` for a dispatch:

```
if issue.git_work_branch:
    runner will: git fetch && git checkout <git_work_branch>
    (runner errors the run if the branch does not exist on remote or locally)
elif project.base_branch:
    runner will: branch off <base_branch> into a fresh feature branch
else:
    runner will: branch off the remote default (git symbolic-ref refs/remotes/origin/HEAD)
```

The empty-string path is what changes the existing hardcoded `"main"` behavior.

## 6. Schema Changes

### Migration `0124_issue_git_work_branch.py`

```python
migrations.AddField(
    model_name="issue",
    name="git_work_branch",
    field=models.CharField(blank=True, default="", max_length=128),
)
```

No changes to `Project`. The existing `repo_url` / `base_branch` fields already permit empty.

## 7. Layer-by-Layer Changes

### 7.1 Django API

**Models** — `apps/api/pi_dash/db/models/issue.py`

- Add `git_work_branch = models.CharField(max_length=128, blank=True, default="")`.

**Serializers**

- `apps/api/pi_dash/app/serializers/project.py` — `ProjectSerializer` already `fields="__all__"`, so `repo_url` / `base_branch` are writable. Verify no `read_only_fields` blocks them.
- `apps/api/pi_dash/api/serializers/project.py` — same verification for the public API surface.
- `apps/api/pi_dash/app/serializers/issue.py` — add `git_work_branch` to whichever field list is used for create/update. If `fields="__all__"`, just verify.
- Same sweep on `apps/api/pi_dash/api/serializers/issue.py`.

**Orchestration** — `apps/api/pi_dash/orchestration/service.py`

- Populate `run_config` with `git_work_branch: issue.git_work_branch or None`.
- Propagate it onto the assign envelope alongside `repo_url` / `repo_ref`.

**Prompt context** — `apps/api/pi_dash/prompting/context.py`

- Add `issue.git_work_branch` under the `issue` key (or a new `git` key — decide during implementation to avoid clutter).

**Prompt template** — `apps/api/pi_dash/prompting/templates/default.j2`

- Replace `{{ repo.base_branch or "main" }}` with logic that renders "the repository's default branch" when empty, not the literal `"main"`. Same for the `git pull --rebase` instruction.
- Add a work-branch section: if `git_work_branch` is set, instruct the agent to `git checkout <branch>` and commit on it directly; otherwise, branch off base.

### 7.2 Runner Protocol & Daemon

**Protocol** — `runner/src/cloud/protocol.rs`

- Extend the `Assign` envelope with `git_work_branch: Option<String>`.
- Fallback in `protocol_roundtrip` test: `None`.

**Supervisor / checkout logic** — `runner/src/daemon/supervisor.rs` (and whatever `workspace/git.rs` does today)

- When `git_work_branch` is `Some(name)` → `fetch` + `checkout name`.
- Else when `repo_ref` is `Some(name)` → branch new feature branch from `name`.
- Else → branch from `origin/HEAD` (auto-detect via `git symbolic-ref`).

### 7.3 Types Package

`packages/types/src/project/projects.ts`

- Add `repo_url: string` and `base_branch: string` to `IProject` (or `IPartialProject`, whichever covers create payloads).

`packages/types/src/issues/issue.ts`

- Add `git_work_branch: string` to the issue interface used by create/update.

Rebuild `packages/types/dist/*` via the package's existing build script.

### 7.4 Web UI

**Project create** — `apps/web/core/components/project/create/common-attributes.tsx`

- Two new inputs under the existing attributes:
  - "Git repository URL" (free text, placeholder `git@github.com:org/repo.git` or `https://github.com/org/repo.git`).
  - "Base branch (optional)" — placeholder `leave empty to use repository default`.
- Both wired through the existing react-hook-form instance on the create flow.

**Project settings** — `apps/web/core/components/project/settings/` + `project/form.tsx`

- Add a new "Repository" section (or append to general settings, decide during implementation).
- Same two fields, persisted via the existing project update API call.

**Issue create/edit** — `apps/web/core/components/issues/issue-modal/form.tsx`

- Single optional "Work branch" input.
- Placed in the advanced/optional area (not required above the name/description fields).
- Empty submits as `""` so the server uses the existing default.

**Issue detail** — wherever issue fields are surfaced for inline edit; add `git_work_branch` as an editable property.

### 7.5 Client-Side Validation (Soft)

- `repo_url`: soft-check for `^(git@|https?://|ssh://)` prefix; show a hint, not a blocking error.
- Branch names: disallow spaces and control chars (`^[A-Za-z0-9._/-]+$`), with a clear inline error. Server re-validates.

### 7.6 Server-Side Validation

- `repo_url`: `max_length=512`, accept any non-control-char string. Connection testing is out of scope.
- `base_branch` / `git_work_branch`: `max_length=128`, regex `^[A-Za-z0-9._/-]*$` (empty allowed).

## 8. Prompt Template Behavior Matrix

| `base_branch` | `git_work_branch` | What the template tells the agent                                                                 |
| ------------- | ----------------- | ------------------------------------------------------------------------------------------------- |
| empty         | empty             | "Find the repo's default branch (`git symbolic-ref refs/remotes/origin/HEAD`) and branch off it." |
| `develop`     | empty             | "Branch off `develop`; PR back to `develop`."                                                     |
| `main`        | `feat/abc`        | "Check out `feat/abc`; commit on it; PR targets `main` when opening."                             |
| empty         | `feat/abc`        | "Check out `feat/abc`; commit on it; PR targets the repo's default branch when opening."          |

## 9. Testing

**Backend**

- Migration dry-run + reverse test.
- Unit test extending `tests/unit/prompting/test_context.py`: cover the four rows of the matrix above.
- Serializer tests ensuring round-trip on project create and issue create.
- Orchestration test ensuring `run_config` receives `git_work_branch` and the assign envelope includes it.

**Runner**

- Extend `tests/protocol_roundtrip.rs` to cover `git_work_branch: Some(...)` and `None`.
- Supervisor unit test around the new checkout branch resolution (may need mocking of git commands).

**Web**

- Visual / interaction smoke: project create with / without repo fields, settings update, issue create with / without work branch.
- No full E2E in this PR; manual browser walkthrough is acceptable given we're already adding plenty of surface area.

## 10. Risk & Rollout

- Low risk: additive fields, empty-string defaults preserve current behavior for all existing projects and issues.
- The prompt template change (removing hardcoded `"main"`) is behavior-changing for every active run. Mitigation: ensure empty-branch runs on repos with a non-`main` default now work, and that repos whose default IS `main` still behave identically (the auto-detect path returns `main` for them).
- Runner protocol is versioned (`v: 1`); we're adding an optional field, which is backward-compatible for older runners that ignore unknown keys. Verify the deserializer is lenient (it looks like it is — `Option<String>` is already the pattern used for other fields).

## 11. Out-of-Scope / Follow-ups

- Repository provider OAuth (GitHub App installation, token storage).
- Branch listing / autocomplete from the remote.
- Monorepo path selectors (which subdirectory is this project?).
- Per-workspace default repo URL for quick-start project creation.
- Showing the resolved branch in the run detail view once the runner reports it.
