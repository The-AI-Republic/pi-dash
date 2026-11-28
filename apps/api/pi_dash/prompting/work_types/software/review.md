---
key: software.review
title: Software review guidance
customizable: overridable
---

## Reviewing software deliverables (work type: software)

### Sync the PR branch with its base before reviewing

Before you review anything, bring the {{ repo.code_review_term }} branch up to date with its base. Every review verdict must describe code that can actually merge into today's base — review only the synced branch, never a stale one.

- `git fetch origin`, then check out the PR's head branch. The base is the PR's base branch; when that is not visible, fall back to `{{ repo.base_branch }}` (or the repository's default branch when no base is set). For a stacked PR — the base is another PR's branch, not the project base — sync against that same base branch.
- `git merge origin/<base>` — merge, never rebase, and never force-push. The PR branch is shared and may already carry review history; a merge commit preserves both sides.
- Clean merge: run this repo's own checks (the test guidance names them: README/CONTRIBUTING, `.github/workflows/`, package manifests) on the merged tree. Push the merge commit if it is not a no-op.
- Conflicts: resolve them so the base's newer structure wins and the PR's change is re-applied on top. Run this repo's own checks, then push the merge commit.
- Too large to resolve with confidence — the base rewrote the code the PR changes: do not review stale code. Abort the merge, leave the branch untouched, comment on the issue with what changed on the base and what needs redoing, and move the issue back to **In Progress** so the change can be rebuilt on the current base.
- Record in the workpad the base SHA you were checked against (`git rev-parse origin/<base>`, or the short SHA after the sync), so a later run can tell whether the verdict is still current. For a stacked PR, note in the workpad whether the parent PR has merged.
- If a prior review verdict exists and the base has moved since that verdict, redo the review on the synced branch — a verdict on an old base does not carry over.

Software work products come in these review kinds — prefer them over GENERIC when they match:

- **CODE** — the issue produced a {{ repo.code_review_term }} (look for a `pr_url` in done_payload, or a feature branch ahead of the base branch).
- **DESIGN** — the issue produced a design / planning document (look for paths under `.ai_design/`, paths in `done_payload.design_doc_paths`, or markdown artifacts referenced as outputs).
- **DESIGN_THEN_CODE** — both a design doc AND a {{ repo.code_review_term }} exist. Review the design first, then the code.

Surfaces and fix rules per kind (steps 3–5 of the review cycle):

- **CODE**: existing reviewer comments live on the {{ repo.code_review_term }} — read them there{% if repo.provider == "github" %} (use the `gh` CLI){% endif %}, and comment your validated findings there too. Fixing is permitted: edit, commit, push to the {{ repo.code_review_term }} branch, and resolve the corresponding comment thread.
- **DESIGN**: comment inline on the doc, or post a structured comment on the pidash issue if the doc has no comment surface. Fixing is permitted: edit the doc and resolve / strike the inline comment.
- **DESIGN_THEN_CODE**: design comments first, then {{ repo.code_review_term }} comments; both fix rules above apply.

For software, "waiting on an external process" in the review outcome means waiting on CI checks or on a merge or update of the {{ repo.code_review_term }}: leave the issue In Review and yield `waiting_on_external`.
