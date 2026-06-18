# GitHub PR ↔ Issue Linking — Design

**Status:** Proposed — foundation slice (builds on the merged GitHub App foundation, PR #261)
**Date:** 2026-06-17
**Scope:** Let a Pi Dash issue carry an optional, possibly-empty set of links to GitHub
**pull requests** (many PRs → one issue). A PR is attached explicitly — primarily by the
coding agent running a new `pidash` CLI command after it opens a PR. The GitHub App keeps
each linked PR's **own status** (open / merged / draft / closed) fresh so the issue can show
an overview of its PRs.

**Hard non-goals:** Attaching or refreshing a PR **never** changes the Pi Dash issue's
workflow state or any other issue field. No write-back to GitHub. Updating *the PR's
displayed status on the issue* is **not** the same as changing *the issue's* state — only
the former is in scope.

---

## 1. Problem

Pi Dash issues are a **superset** of GitHub issues — most have no GitHub issue at all, so
the existing mirror path (`GithubIssueSync` → `GithubRepositorySync`) cannot represent
"this issue is implemented by GitHub PR #42." Today, when an AI agent picks up an issue
(e.g. *"change the home-page button to blue"*), opens a PR, and posts the PR URL as a
free-text comment via `pidash comment add`, there is **no structured link**, **no overview
of an issue's PRs**, and **no live status**. We want a first-class, optional **issue → PR**
association that is cheap to create and whose PR status stays current — without inheriting
any issue-state-mutation semantics.

## 2. Goals

1. **Link PRs to an issue.** A standalone model, independent of `GithubIssueSync`, so
   superset issues (no GitHub issue) are supported. Cardinality: **one issue → many PRs;
   one PR → one issue.**
2. **Attach via the pidash CLI.** A new command the agent runs after opening a PR; the link
   is created from authenticated, issue-scoped input — no GitHub-side marker, no parsing.
3. **Keep each linked PR's status fresh.** The GitHub App `pull_request` webhook updates the
   link's status snapshot when a link exists; if no link exists, it does nothing. This
   powers a **PR overview** on the issue.

## 3. Non-Goals

- **No PR → issue state mapping.** Merging/closing a PR does not move the issue. (Updating
  the PR's badge on the issue is display only.)
- **No write-back to GitHub.** The agent continues to author its own PR; Pi Dash never
  opens/edits/merges PRs.
- **No runner protocol change.** Binding rides the existing authenticated CLI path, not a
  new `AgentRunEvent` kind. (The coding agent can't speak the runner protocol; it can call
  the CLI.)
- **No GitHub-issue / PAT-sync changes.**
- **No provider abstraction** (GitHub only; the CLI verb is named generically).
- **No `agent_run` linkage.** Provenance is covered by `linked_by`/`linked_at`; a run FK
  would only add a `db`→`runner` cross-app dependency for no real benefit.

## 4. Background — what we build on (verified in code)

| System | Where | Reuse |
| --- | --- | --- |
| Inbound webhook receiver | `GithubAppWebhookEndpoint` (`app/views/integration/github.py`, #261) | Add a `pull_request` branch (today it stores-and-skips it). |
| Installation tokens (if needed) | `utils/github_app_auth.py`, `GithubClient.for_installation` (#261) | Optional best-effort snapshot at attach time when the App is connected. |
| Issue conversation CLI | `pidash comment add <PROJECT-123> --agent-run-id "$PIDASH_AGENT_RUN_ID"` (`pi-dash-skill/shared/pidash-workflows.md`) | Add a sibling `pidash issue attach-pr` in the same style. |
| External API surface | `pi_dash.api` (`/api/v1/`, `X-Api-Key`); `api/views/issue.py`, `api/urls/work_item.py` | New attach/detach/list endpoints. |
| Issue identifier | `Issue.sequence_id` → `PROJECT-123` | Command accepts the human identifier. |
| Soft-delete + partial unique pattern | `github_repository_sync_unique_per_project_when_active` (`db/models/integration/github.py`) | Same shape for detach/re-attach. |

**Why not `GithubIssueSync`:** it requires a GitHub *issue* and a PAT-backed
`GithubRepositorySync`; superset issues have neither. The PR link must be standalone.

## 5. Data model

```
GithubPullRequestLink(BaseModel):
  issue       FK(db.Issue, related_name="github_pull_requests")   # required
  repo_owner  CharField
  repo_name   CharField
  pr_number   IntegerField
  url         URLField
  # PR status snapshot — display only; refreshed by the webhook (§7):
  title       CharField(blank, default="")
  state       CharField(choices=open|closed, default=open)
  merged      BooleanField(default=False)
  draft       BooleanField(default=False)
  pr_updated_at DateTimeField(null=True)        # GitHub updated_at; out-of-order guard
  linked_by   FK(User, null=True, on_delete=SET_NULL)   # CLI audit actor
  # created_at / updated_at / deleted_at from BaseModel

  constraints:
    UniqueConstraint(fields=[repo_owner, repo_name, pr_number],
                     condition=Q(deleted_at__isnull=True),
                     name="github_pr_link_unique_per_pr_when_active")
  indexes: (repo_owner, repo_name, pr_number), (issue)
```

- **Issue → many PRs:** `issue.github_pull_requests` (0..N); empty = zero rows.
- **PR → one issue:** the partial-unique key. (Global across workspaces, matching the
  stated "one PR → one issue" cardinality.)
- **No `repo_id`/`installation_id`:** identity is `(owner, repo, number)`, parsed from the
  URL and present in the webhook payload — so **attach needs no GitHub App / API call**, and
  the webhook matches without a join. Repo renames are an accepted edge (see §9).
- Normalize `repo_owner`/`repo_name` to lowercase on write (GitHub is case-insensitive) so
  attach and webhook match consistently.

## 6. Attach via the pidash CLI

### 6.1 Command
```bash
pidash issue attach-pr <PROJECT-123> --url <pr-url>
```
The agent runs this after `gh pr create`. It supplies only the URL — the one datum it
reliably has. The skill prompt (`pi-dash-skill/shared/pidash-workflows.md`) is updated to
instruct the agent accordingly. A `detach`/`list` variant backs the UI.

### 6.2 Endpoint (external API, `pi_dash.api`)
`POST /api/v1/workspaces/.../issues/<id>/github/pull-requests/`:
1. Resolve the issue from the identifier **within the API key's workspace**.
2. **Authorize:** caller must have issue edit/comment permission (not just any workspace
   key). Detach requires the same.
3. Validate it is a GitHub PR URL; parse `owner / repo / number` (github.com only, matching
   #261's host scope).
4. Upsert the link on `(repo_owner, repo_name, pr_number)`:
   - new → create, linked to the issue;
   - same issue → no-op (idempotent re-report);
   - **different issue → `409`** ("PR already linked to <ISSUE>").
5. Record `linked_by`.
6. **Optional best-effort snapshot:** if the issue's workspace has a connected App
   installation covering the repo, fetch the PR once via `GithubClient.for_installation`
   to pre-fill `title/state/merged/draft`. If not, leave them blank — they fill on the first
   webhook (§7). Attach never *fails* for lack of an App.

**Why CLI, not a PR-body marker or runner event:** GitHub has no per-PR metadata field, so a
marker means embedding an id in the PR body/commit and parsing it back — editable
(spoofable) and fragile. The authenticated, issue-scoped CLI call has no GitHub round-trip
and no spoof surface. And the coding agent can't emit runner-protocol events — the CLI is
its real capability.

## 7. Keep PR status fresh — `pull_request` webhook

Extend `GithubAppWebhookEndpoint` (#261), which currently persists `pull_request`
deliveries and marks them `skipped`:

- On a `pull_request` event, look up a link by `(owner, repo, number)` from
  `payload.pull_request` / `payload.repository`.
- **If a link exists:** copy `title`, `state`, `merged`, `draft`, `pr_updated_at` from the
  payload onto the link. Skip if the payload `updated_at` is older than stored
  `pr_updated_at` (out-of-order / replay; deliveries already dedupe on `X-GitHub-Delivery`).
- **If no link exists:** do nothing (today's persist + `skipped`). We never auto-create
  links from webhooks.
- The handler touches **only the link row** — never the issue.

GitHub App registration (one-time): add **Pull requests: Read** and subscribe to
**`pull_request`**. This is the only part that needs the new scope; attach works without it.

## 8. UI — PR overview on the issue

On the Pi Dash issue detail, a **"Pull requests"** section lists
`issue.github_pull_requests`: title, number, a status badge (open / draft / merged /
closed), and an out-link to GitHub. Plus a manual **Attach PR** (paste a URL) and **Detach**
action. Display only — no issue-state controls, no sync toggles.

## 9. Security & edge cases

- **Tenant scope:** attach resolves the issue within the caller's workspace; the unique key
  is global so a PR can't be double-linked.
- **No spoof surface:** the link comes from authenticated CLI input, nothing carried inside
  the PR.
- **Least privilege:** only Pull requests: Read added; no write scopes. Webhook stores only
  public PR metadata the attacher already had access to.
- **No issue side effects:** by construction, neither attach nor webhook mutates issue
  fields.
- **Repo rename:** keying on `(owner, repo, number)` means a rename orphans the snapshot
  refresh until re-attached. Accepted for v1 (rare; webhook `repository.id` could be used to
  self-heal later).
- **Attach before App connected:** snapshot starts blank, fills on the first `pull_request`
  event after the App is installed.

## 10. Implementation slices

1. **Slice A — link + attach + overview.** Model + migration, the `attach-pr`/`detach`/`list`
   endpoints + CLI command + skill-prompt update, and the issue "Pull requests" section.
   Snapshot is blank or best-effort at attach. No new GitHub scope.
2. **Slice B — live status.** Add Pull requests: Read + `pull_request` subscription; extend
   the webhook to refresh snapshots. Turns the overview's badges live.

Slice A is independently useful; Slice B only makes the badges current.

## 11. File-level impact map

| Area | File(s) | Change |
| --- | --- | --- |
| Model + migration | `apps/api/pi_dash/db/models/integration/github.py`, `db/migrations/` | Add `GithubPullRequestLink` + migration. |
| Attach/detach/list | `apps/api/pi_dash/api/views/issue.py` (or new `api/views/github_pr.py`), `api/urls/work_item.py` | Endpoints; parse + authorize + upsert. |
| Installation client | `apps/api/pi_dash/utils/github_client.py` | `get_pull_request(owner, name, number)` (used by §6.2 optional snapshot + §7). |
| Webhook refresh | `apps/api/pi_dash/app/views/integration/github.py` | `pull_request` branch: refresh link snapshot only (Slice B). |
| GitHub App config | App registration | Pull requests: Read; subscribe `pull_request` (Slice B). |
| CLI | pidash CLI (where `issue create` / `comment add` live; confirm `deployments/cli`) | New `issue attach-pr` / `detach` / `list-pr` commands. |
| Skill prompt | `pi-dash-skill/shared/pidash-workflows.md` | Instruct the agent to run `attach-pr` after opening a PR. |
| Types/UI | `packages/types/src/integration.ts`, issue-detail components | PR-link types + the "Pull requests" issue section. |

## 12. To confirm at implementation (not re-design)

1. **API surface/auth** the `pidash` CLI uses for `comment add`, so `attach-pr` matches it.
2. **Permission level** for attach/detach — issue edit/comment.

## 13. Deferred

- PR → **issue** state automation (hard non-goal here; would be its own design).
- Write-back / runner-authored PRs.
- Provider abstraction (GitLab/Bitbucket) behind the generic `attach-pr` verb.
- Repo-rename self-healing via `repository.id`.
