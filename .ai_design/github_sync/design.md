# GitHub Issue Sync — Design

**Status:** Draft
**Date:** 2026-04-25
**Scope:** One PR, all layers (model → Celery task → API → web UI). MVP only; no GitHub App, no webhooks, no two-way state sync.

---

## 1. Problem

Pi Dash already has the data shape for GitHub mirroring (`GithubRepository`, `GithubRepositorySync`, `GithubIssueSync`, `GithubCommentSync`, plus `Issue.external_source` / `external_id`), but no fetch loop, no working UI to connect a repo, and no per-project toggle. Users currently have no way to mirror GitHub issues into a Pi Dash project.

There is leftover Plane-era client code (see §6.0) that *looks* like a working integration but points at backend routes that were never ported into `apps/api`. The MVP supersedes those stubs and ships the missing backend, a working UI, and the sync loop.

We want a minimum-viable, one-directional sync: GitHub issues become Pi Dash issues on a fixed cadence; when a Pi Dash issue is completed, a single comment is posted back to GitHub to notify the original reporter. No state changes on the GitHub side.

## 2. Goals

- **Connect once per workspace.** A user pastes a GitHub credential (PAT) once on the workspace level. Every project in the workspace can then bind a repo without re-authenticating.
- **Per-project enable/disable.** From project settings, a user picks a repo (from those visible to the credential) and toggles sync on/off.
- **Fixed 4-hour cadence.** A single Celery Beat entry runs every 4 hours; no per-project schedule, no UI knob.
- **One-way content sync.** GitHub issues + comments → Pi Dash issues + comments. No reverse content sync, no state mirroring.
- **Title prefix `[github_<number>]`.** Mirrored issues are unambiguously identifiable in the Pi Dash UI.
- **Completion comment-back.** When a synced Pi Dash issue moves to a state of group `completed`, post one comment on the GitHub issue: "This issue has been completed in Pi Dash." The GitHub issue is not closed.

## 3. Non-Goals (MVP)

- No GitHub App, no manifest flow, no OAuth-for-API. **PAT only**, pasted by the workspace admin.
- No webhooks. Polling only.
- No two-way sync. GitHub side stays open even when Pi Dash side is completed.
- No label, assignee, or milestone mapping. Title + body + comments + author identifier only.
- No backfill UI. Every run is a full scan (see §6.3); the first run is just the steady-state shape with an empty local mirror.
- No conflict resolution. Synced fields (issue title/description, comment bodies) are **read-only in Pi Dash** — the serializer rejects edits and the UI hides the affordances; see §6.8. GitHub is authoritative; the sync task always rewrites those fields from upstream. Workflow fields (state, priority, assignees, labels, cycle, module) remain user-editable.
- No multi-repo per project. One `GithubRepositorySync` per project (already enforced by `unique_together = [project, repository]`).
- **Upstream deletion is *detected* but not propagated as a Pi Dash delete.** Mirrors of upstream-deleted (or upstream-closed) issues are flagged via `GithubIssueSync.metadata["upstream_gone_at"]` and stop syncing; the Pi Dash row is preserved so users don't lose work or comments. UI can render a "no longer on GitHub" badge.
- No upstream→Pi Dash propagation of *state* changes (open/closed). A GitHub closure surfaces in the same diff that detects deletions, but we leave the Pi Dash issue's state alone — the user's workflow lane is theirs.
- No rate-limit handling beyond exponential backoff on 403 secondary limits.

## 4. Key Concepts

| Concept | Where it lives | Lifecycle |
|---|---|---|
| **GitHub credential** | `WorkspaceIntegration.config` (workspace-scoped) | Created once per workspace; reused across all projects in that workspace. |
| **Repo binding** | `GithubRepositorySync` (project-scoped) | Created on `bind`; deleted on `unbind`. Sync is independently turned on/off via the `is_sync_enabled` flag — toggling does **not** create or destroy the binding. |
| **Issue mirror** | `GithubIssueSync` row + `Issue` row (`external_source="github"`, `external_id=<gh issue number>`) | Created on first sight; updated on every poll. |
| **Comment mirror** | `GithubCommentSync` row + `IssueComment` row | Same lifecycle as issue mirror. |
| **Sync cadence** | Single Celery Beat entry, fixed 4h | Iterates every `GithubRepositorySync` with `is_sync_enabled=True`. |

The split between workspace-level credential and project-level binding is what satisfies "link once." The existing `WorkspaceIntegration` model with `unique_together = [workspace, integration]` already enforces one credential per `(workspace, "github")` pair; we lean on it.

## 5. Schema Changes

### Migration `0125_github_sync_mvp.py`

```python
migrations.AddField(
    model_name="githubrepositorysync",
    name="is_sync_enabled",
    field=models.BooleanField(default=False),
)

# Informational only — every run is a full scan (see §6.3), no incremental
# `since=` logic depends on this field. Useful for "last successful sync" UI
# and admin debugging.
migrations.AddField(
    model_name="githubrepositorysync",
    name="last_synced_at",
    field=models.DateTimeField(null=True, blank=True),
)

migrations.AddField(
    model_name="githubrepositorysync",
    name="last_sync_error",
    field=models.TextField(blank=True, default=""),
)

# One sync row per project — the existing unique_together = ["project",
# "repository"] only deduplicates by (project, repo_row), but GithubRepository
# has no constraint on (project, repository_id), so multiple rows for the same
# upstream repo can exist and each could spawn its own sync. The §3 non-goal
# "no multi-repo per project" needs schema enforcement, not just policy.
# Filtered on deleted_at so soft-deletes don't block re-bind.
migrations.AddConstraint(
    model_name="githubrepositorysync",
    constraint=models.UniqueConstraint(
        fields=["project"],
        condition=Q(deleted_at__isnull=True),
        name="github_repository_sync_unique_per_project_when_active",
    ),
)

# Required for:
#   - completion-comment idempotency (metadata["completion_comment_id"]; see §6.5)
#   - upstream-deletion flag (metadata["upstream_gone_at"]; see §6.3.1)
#   - GitHub author identity (metadata["github_user_login"]; see §6.3 field map)
migrations.AddField(
    model_name="githubissuesync",
    name="metadata",
    field=models.JSONField(default=dict),
)

# Preserve original GitHub timestamps without fighting auto_now_add/auto_now
# on Pi Dash's TimeAuditModel (which would clobber values passed at create time).
migrations.AddField(
    model_name="githubissuesync",
    name="gh_issue_created_at",
    field=models.DateTimeField(null=True, blank=True),
)

migrations.AddField(
    model_name="githubissuesync",
    name="gh_issue_updated_at",
    field=models.DateTimeField(null=True, blank=True),
)
```

**Issue model change** — `apps/api/pi_dash/db/models/issue.py`:

```python
from pi_dash.db.mixins import ChangeTrackerMixin

class Issue(ChangeTrackerMixin, ProjectBaseModel):
    TRACKED_FIELDS = ["state_id"]
    # ... existing fields unchanged
```

Only `state_id` is tracked. `Issue.completed_at` is set inside `Issue.save()` as a downstream effect of state changes (`db/models/issue.py:238-243`), so it changes whenever `state.group` changes — tracking it independently would be redundant. The receiver in §6.5 gates on `"state_id" in _changes_on_save` and reads the new state's group to decide whether to fire.

This is required for the completion-comment-back trigger (§6.5). `IssueComment` already uses this mixin for the same kind of "fire only on transition" pattern. No DB migration needed for the mixin itself — it operates in-memory.

The `Issue.external_source` / `external_id` fields already exist and are reused as-is. No changes to `GithubRepository`, `GithubCommentSync`.

### Credential storage

Stored in `WorkspaceIntegration.config` for `integration.provider == "github"`:

```json
{
  "auth_type": "pat",
  "token": "<encrypted PAT>",
  "github_user_login": "octocat",
  "verified_at": "2026-04-25T12:00:00Z"
}
```

The token field is encrypted at rest using `pi_dash.license.utils.encryption` (`encrypt_data(plaintext)` / `decrypt_data(ciphertext)`). This is the only encryption helper currently in the codebase; we deliberately reuse it rather than introduce a second mechanism. The `verified_at` and `github_user_login` fields are stored in plaintext — they are not secrets.

## 6. Layer-by-Layer Changes

### 6.0 Existing Plane-era stubs to delete

These files exist and look like a GitHub integration, but call backend routes that were never ported into `apps/api`. They are dead UI. The MVP **deletes** them and replaces them with the new components in §6.6.

| File | Disposition |
| --- | --- |
| `apps/web/core/components/project/integration-card.tsx` | Delete. Replaced by `apps/web/core/components/project/settings/github-sync.tsx` (§6.6). |
| `apps/web/core/components/integration/github/select-repository.tsx` | Delete. Replaced by the repo picker inside the new project-settings component. |
| `apps/web/core/services/integrations/github.service.ts` | **Rewrite.** Drop `listAllRepositories`, `getGithubRepoInfo`, `createGithubServiceImport` (all hit unported routes). Replace with `connectWorkspace`, `listRepos`, `disconnectWorkspace` per §6.7. |
| `apps/web/core/services/project/project.service.ts` — `syncGithubRepository`, `getProjectGithubRepository` methods | Drop those two methods (unported routes). Add `bindGithubRepository`, `setGithubSyncEnabled`, `removeGithubBinding` per §6.7 in the same file. |
| `apps/web/core/constants/fetch-keys` — `PROJECT_GITHUB_REPOSITORY` | Delete the constant; nothing else uses it. |

**Why supersede rather than complete the stubs:** the existing client is per-project ("paste a repo into each project") and offers no workspace-level credential model. Our requirement is link-once-per-workspace; that doesn't fit the existing route shape, so a clean replacement is simpler than retrofitting.

### 6.1 Django — connection flow (workspace-level)

**Endpoint** — `POST /api/workspaces/<slug>/integrations/github/connect/`

- Body: `{ "token": "<github PAT>" }`.
- Server validates the PAT by calling `GET https://api.github.com/user`. Stores the token + login in `WorkspaceIntegration.config`. Idempotent: if a row exists for this workspace, update.

**Endpoint** — `GET /api/workspaces/<slug>/integrations/github/repos/?page=<n>`

- Single mode: `GET /user/repos?affiliation=owner,collaborator,organization_member&per_page=100&sort=updated&page=<n>`. The `affiliation` filter is **required** — without it, GitHub omits org repos the user can read but doesn't directly own/collaborate on, surprising users with "I have access to this repo, why isn't it listed?". Returns the most recently updated 100 repos per page; the picker pages on demand.
- Returns `{ repos: [{ id, owner, name, full_name, default_branch, private }, ...], has_next_page: bool }` (the `has_next_page` flag is derived from GitHub's `Link` response header).
- The picker filters loaded results client-side by substring match on `full_name` for typeahead. If the user can't find their repo within the first few pages of `sort=updated`, they keep paging.
- **Why no `/search/repositories` mode:** the search endpoint's `user:<login>` qualifier scopes to repos *owned* by the named account (a common misconception is that it scopes to "repos visible to the authenticated token"). For users who primarily work in org repos, `user:<login>` would silently hide most of their repos. A correct search mode would have to enumerate the user's orgs (`GET /user/orgs`) and emit one `org:<name>` qualifier per org, plus `user:<login>` — feasible but complex enough that paginated browse is the simpler MVP choice. Adding a proper search mode is a follow-up.
- Used by the project-settings UI when the user picks which repo to bind.

**Endpoint** — `POST /api/workspaces/<slug>/integrations/github/disconnect/` (soft disconnect)

- Does **not** delete the `WorkspaceIntegration` row. Reason: `GithubRepositorySync.workspace_integration` is `on_delete=models.CASCADE` (`db/models/integration/github.py:37`), so a hard `DELETE` would cascade-delete every project's sync row in the same transaction — we'd lose `GithubIssueSync` mirror links and any disconnect annotations would be unreachable.
- Soft-disconnect semantics:
  - Clears `WorkspaceIntegration.config["token"]` (set to `""`).
  - Sets `WorkspaceIntegration.config["disconnected_at"] = now()`.
  - Iterates dependent `GithubRepositorySync` rows in the workspace and sets `is_sync_enabled=False`, `last_sync_error="Workspace GitHub integration disconnected"`.
  - Existing mirrored issues stay; their `GithubIssueSync` links survive.
- Reconnect = `POST /connect/` with a new token; the same `WorkspaceIntegration` row is reused (idempotent path), the `disconnected_at` key is removed, and project admins can re-enable each project's sync individually (intentional — silent auto-resume on reconnect would surprise users whose sync was disabled long ago).
- A separate hard-`DELETE` admin endpoint is **out of scope for MVP**. If we add one later, it must first delete dependent `GithubRepositorySync` rows in an explicit pre-step or change the FK to `on_delete=SET_NULL`.

### 6.2 Django — repo binding & toggle (project-level)

**Endpoint** — `POST /api/workspaces/<slug>/projects/<id>/github/bind/`

- **Precondition** (returns HTTP 409 if violated): `GithubRepositorySync.objects.filter(project=<id>).exists()` must be `False`. The schema constraint added in §5 also enforces this at the DB layer; the explicit check is for a human-readable error. To re-bind a project to a different repo, the user first calls `DELETE /github/` to remove the existing binding.
- Body: `{ "repository_id": <int>, "owner": "<owner>", "name": "<name>", "url": "<html_url>" }`. All four fields come from the picker's `/repos/` response (§6.1) — passing them through avoids an extra `GET /repos/{owner}/{repo}` to resolve `repository_id` (a `BigIntegerField` on `GithubRepository`). The server validates `(owner, name, repository_id)` is consistent by calling `GET /repos/{owner}/{name}` once and comparing the returned `id` — guards against a malicious client passing a mismatched `repository_id`.
- Creates or fetches the `GithubRepository` row with `repository_id` as the natural key (per existing model shape, the row is project-scoped — this duplicates `GithubRepository` rows across projects pointing at the same upstream repo; acceptable for MVP).
- Creates `GithubRepositorySync` with `is_sync_enabled=False`, `workspace_integration` = the workspace's GitHub integration row, `actor` = the requesting user, `label` = `Label.objects.get_or_create(project=<project>, name="github")[0]` (existing label with the same name in the project is reused; color is whatever exists).

**Endpoint** — `PATCH /api/workspaces/<slug>/projects/<id>/github/sync/`

- Body: `{ "enabled": true | false }`.
- Flips `GithubRepositorySync.is_sync_enabled`. When flipping `false → true`, **does not** trigger an immediate sync; it waits for the next 4-hour tick. (We can add an immediate-trigger option in a follow-up; intentionally out of scope.)

**Endpoint** — `DELETE /api/workspaces/<slug>/projects/<id>/github/`

- Removes the `GithubRepositorySync`. Cascade behavior (per existing FK definitions in `db/models/integration/github.py:54-87`):
  - `GithubIssueSync` rows for the project: deleted (`repository_sync` FK is `CASCADE`).
  - `GithubCommentSync` rows: deleted (`issue_sync` FK is `CASCADE`).
  - `Issue` rows: **survive** with `external_source="github"` / `external_id` intact. Title and body content remain; `[github_<n>]` prefix stays as a provenance marker.
  - `IssueComment` rows: same — survive with `external_source="github"` and `[Github]` prefix intact.
- The §6.8 read-only lock is keyed on the existence of `GithubIssueSync` / `GithubCommentSync` rows, **not** on `external_source`. Once those rows cascade-delete, the lock releases automatically — users can edit, soft-delete, or hard-delete the surviving `Issue` / `IssueComment` rows like any native row.
- A re-bind to the same upstream repo will rediscover the existing rows via the `(project, external_source="github", external_id)` upsert key, recreate `GithubIssueSync` rows, and re-engage the lock.

### 6.3 Django — sync task (Celery Beat)

**File** — `apps/api/pi_dash/bgtasks/github_sync_task.py` (new).

**Beat schedule** — added to `apps/api/pi_dash/celery.py`:

```python
"github_issue_sync": {
    "task": "pi_dash.bgtasks.github_sync_task.sync_all_repos",
    "schedule": crontab(minute=0, hour="*/4"),
},
```

**Task structure** — every run is a full scan. No `since=`, no watermark logic, no first-run-vs-incremental branching.

```python
@shared_task
def sync_all_repos():
    for sync in GithubRepositorySync.objects.filter(is_sync_enabled=True).select_related(...):
        sync_one_repo.delay(sync.id)

@shared_task(bind=True, max_retries=3)
def sync_one_repo(self, sync_id):
    sync = GithubRepositorySync.objects.get(id=sync_id)
    client = GithubClient(token=decrypt_data(sync.workspace_integration.config["token"]))

    try:
        # 1. Full enumeration of open issues (paginated; PRs filtered out via the
        #    `pull_request` field on each item).
        remote_issue_numbers: set[int] = set()
        for issue in client.list_all_open_issues(sync.repository.owner, sync.repository.name):
            if "pull_request" in issue:
                continue  # GitHub returns PRs from /issues; skip them in MVP
            upsert_issue(sync, issue)
            remote_issue_numbers.add(issue["number"])

        # 2. Full enumeration of comments (single repo-wide endpoint, paginated).
        # GitHub returns comments for every issue including PRs and closed issues.
        # We mirror only comments whose parent is in `remote_issue_numbers` (the
        # open non-PR set from step 1). Skipping is critical: an `IssueComment`
        # FK without a local parent `Issue` is a data-integrity bug.
        for comment in client.list_all_repo_comments(sync.repository.owner, sync.repository.name):
            parent_number = parse_issue_number_from_url(comment["issue_url"])
            if parent_number not in remote_issue_numbers:
                continue  # PR comment, closed-issue comment, or otherwise unmirrored
            upsert_comment(sync, comment, parent_number)

        # 3. Diff: anything we have a GithubIssueSync for, but that didn't appear
        #    in step 1, is gone-or-closed upstream. Flag and stop syncing it.
        reconcile_upstream_gone(sync, remote_issue_numbers)

        sync.last_synced_at = timezone.now()
        sync.last_sync_error = ""
        sync.save(update_fields=["last_synced_at", "last_sync_error"])

    except Exception as e:
        sync.last_sync_error = str(e)[:1000]
        sync.save(update_fields=["last_sync_error"])
        raise self.retry(exc=e, countdown=60 * (2 ** self.request.retries))
```

**Why full-scan over `since=` incremental:** see the rate-limit estimation below. For 95%+ of repos, full scan costs <100 requests per run against a 5,000/hr PAT budget. The `since=` filter cannot surface deletions or closures (a deleted issue has no `updated_at` to compare; it simply stops appearing) — full enumeration is the only poll-based way to detect upstream-gone items short of webhooks. The cost is negligible; the architectural simplification is large (no watermark race, no first-run branching).

#### 6.3.1 Upstream-gone detection (the diff step)

After step 1 of `sync_one_repo` builds `remote_issue_numbers` (the set of open issues GitHub currently has), `reconcile_upstream_gone` flags anything we know about but didn't see:

```python
def reconcile_upstream_gone(sync, remote_issue_numbers):
    locals_ = GithubIssueSync.objects.filter(
        repository_sync=sync,
    ).only("id", "repo_issue_id", "metadata")
    now = timezone.now()
    for ghi in locals_:
        is_present_upstream = ghi.repo_issue_id in remote_issue_numbers
        was_flagged_gone = bool(ghi.metadata.get("upstream_gone_at"))

        if not is_present_upstream and not was_flagged_gone:
            ghi.metadata["upstream_gone_at"] = now.isoformat()
            ghi.save(update_fields=["metadata"])
        elif is_present_upstream and was_flagged_gone:
            # Issue reappeared (rare — reopen, or a transferred-back). Clear the flag.
            ghi.metadata.pop("upstream_gone_at", None)
            ghi.save(update_fields=["metadata"])
```

| Aspect | Behavior |
| --- | --- |
| **Why a set diff is sufficient** | Deletions don't appear in any list endpoint (the issue's row is gone — no `updated_at` to filter on). Closures don't appear in `state=open`. Both surface as "absent from this run's listing." We don't try to distinguish them. |
| **What "gone" means in Pi Dash** | The `Issue` row stays. Future sync runs skip mirroring updates onto it (the upsert path is keyed on the GitHub-side issue still being in the listing). The issue becomes effectively a Pi Dash-native record with frozen synced fields. |
| **UI** | Render a "no longer on GitHub" badge / muted styling on issues with `metadata["upstream_gone_at"]` set, sourced via the `GithubIssueSync` join. Optional in MVP — at minimum the field is queryable for admins. |
| **Reopen / transfer-back** | If an upstream-gone issue reappears in a later run (e.g. an admin restored it, or a transfer was reverted), the flag is cleared and normal sync resumes. |
| **Hard delete on Pi Dash side** | Out of scope. Users who want the mirror gone can soft-delete the `Issue` manually. |
| **PR filtering** | GitHub returns PRs alongside issues from the `/issues` endpoint; we filter via the `pull_request` field. `remote_issue_numbers` therefore only contains real issues — a PR's number won't accidentally cause a flag clear. |
| **Closed-on-GitHub items** | Treated identically to deleted items (both absent from the `state=open` listing). MVP intentionally does not mirror state changes back to Pi Dash (see §3) — the user keeps their workflow lane. |
| **Failure resume** | On exception, no flags are written; the next run computes the diff from scratch. Idempotent. |

**Upsert keying**:

- Issue: `Issue.objects.update_or_create(project=sync.project, external_source="github", external_id=str(gh_issue["number"]), defaults={...})`.
- Comment: `IssueComment.objects.update_or_create(issue=mirrored_issue, external_source="github", external_id=str(gh_comment["id"]), defaults={...})`. The defaults always include the `[Github]` prefix on `comment_html` / `comment_stripped` (see §6.4) so the marker is regenerated on every sync.

**Comment parent resolution** — `parse_issue_number_from_url` and parent lookup:

```python
import re
ISSUE_URL_RE = re.compile(r"/repos/[^/]+/[^/]+/issues/(\d+)$")

def parse_issue_number_from_url(issue_url: str) -> int | None:
    """Extract the issue number from a GitHub comment's `issue_url` field."""
    m = ISSUE_URL_RE.search(issue_url)
    return int(m.group(1)) if m else None

def upsert_comment(sync, gh_comment, parent_number):
    parent = Issue.objects.get(
        project=sync.project,
        external_source="github",
        external_id=str(parent_number),
    )  # safe — caller guaranteed parent_number is in remote_issue_numbers
    IssueComment.objects.update_or_create(
        issue=parent,
        external_source="github",
        external_id=str(gh_comment["id"]),
        defaults={
            "comment_html": f"<p>[Github] </p>{render_markdown(gh_comment['body'])}",
            "comment_stripped": f"[Github] {strip_tags(...)}",
            ...
        },
    )
```

This mirrors only comments on open non-PR issues (the only ones with local parents). PR comments, closed-issue comments, and comments on issues filtered out for any other reason are skipped.

**Field mapping** (issue):

| GitHub | Pi Dash |
|---|---|
| `title` | `name` (with `[github_<number>] ` prefix prepended — see §6.4) |
| `body` (markdown) | `description_html` (rendered), `description_stripped` (plain) — reuse existing markdown→HTML helper |
| `user.login` | record in `GithubIssueSync.metadata["github_user_login"]`; **created_by** is set to the workspace integration's `actor` (the bot user), not a real Pi Dash user |
| `state` (open/closed) | We list `state=open` only, so closed issues never enter the local set. On first import, state = project default. Subsequent updates do not change Pi Dash state — see §3 non-goal on state propagation. If an issue closes upstream, it disappears from the listing and is treated identically to a deletion (see §6.3.1). |
| `labels` (MVP) | not synced |
| `assignees` (MVP) | not synced |
| `created_at` | `GithubIssueSync.gh_issue_created_at` (NOT `Issue.created_at`) |
| `updated_at` | `GithubIssueSync.gh_issue_updated_at` (NOT `Issue.updated_at`) |

**Why GitHub timestamps don't go on `Issue`:** `Issue` inherits `TimeAuditModel`, where `created_at` uses `auto_now_add=True` and `updated_at` uses `auto_now=True` (`db/mixins.py:19-20`). Both fields are *forced* by Django on every INSERT / UPDATE — passing values is silently ignored. Working around that with a follow-up `Issue.objects.filter(pk=...).update(created_at=...)` is possible but pollutes the audit semantics: `Issue.created_at` should mean "when this Pi Dash record was created," not "when the upstream GitHub issue was filed." Storing the GitHub timestamps on the dedicated `GithubIssueSync.gh_issue_created_at` / `gh_issue_updated_at` fields keeps both meanings clean. The UI can render whichever is appropriate per surface.

**State on first import**: assign the project's default state (the same one new manual issues get). This avoids importing 5000 closed issues into a "Backlog" lane.

**Closed issues**: skip on import (`state=open` only via `state=open` query param). If an open issue later becomes closed upstream, we leave the Pi Dash mirror untouched in MVP.

### 6.4 Title and comment prefixes

Both mirrored issues and mirrored comments carry a stored prefix so the GitHub origin is visible everywhere they render — list views, search results, activity feeds, notifications, exports — without each surface having to consult `external_source`.

**Issue title** — `[github_<number>]`:

- On insert and on every subsequent sync: `name = f"[github_{gh_issue['number']}] {gh_issue['title']}"`. No dirty-check, no `last_synced_title` bookkeeping — synced titles are read-only in Pi Dash (see §6.8), so there is never a local edit to preserve. The sync task always rewrites the field from upstream.
- The prefix uses the **GitHub issue number** (e.g. `#123` → `[github_123]`), not the global GraphQL `id`. Rationale: numbers are visible to users and match the GitHub UI; IDs are opaque.

**Comment body** — `[Github]`:

- On insert and every subsequent sync: prepend a literal `[Github] ` marker as a leading paragraph to `comment_html`, and as a leading token to `comment_stripped`:
  - `comment_html = f"<p>[Github] </p>{rendered_html}"` — separate leading paragraph so multi-paragraph upstream bodies aren't broken (an inline `"<p>[Github] " + inner_html + "</p>"` would close `</p>` too early when `inner_html` already contains block elements).
  - `comment_stripped = f"[Github] {stripped}"`.
- No dirty-check on comments — synced comment bodies are read-only in Pi Dash (see §6.8). The sync task always rewrites synced rows in full from upstream; the prefix is regenerated each run.
- No comment ID in the marker (unlike the issue title). Comment IDs are opaque numeric values; rendering them adds noise without giving users an actionable handle. The plain `[Github]` marker is the visible signal; `IssueComment.external_id` keeps the precise mapping in the database.
- Native (Pi Dash-authored) comments are **not** prefixed — `external_source IS NULL` → no marker → easy at-a-glance distinction in the comment thread.

**Casing note:** the title prefix is lowercase (`[github_123]`) while the comment marker is title-case (`[Github]`). This is intentional — the title prefix functions as a quasi-identifier (mirrors GitHub's lowercase `gh issue` notation), while the comment marker is a natural-language label. If a future cleanup wants to normalize to one style, the migration is trivial (one regex sweep over `Issue.name` and `IssueComment.comment_html` / `comment_stripped`); both choices are easy to revisit.

### 6.5 Completion comment-back

**Why a plain `post_save` is insufficient:** the receiver only sees the new row state. It can't tell whether `state` *transitioned into* `completed` versus the issue having always been completed (e.g. a save that touched some unrelated field on an already-completed issue). To trigger exactly once per transition, we need previous-value awareness.

**Mechanism:** use the `ChangeTrackerMixin` already on `Issue` (added in §5). The mixin captures field values at `__init__` time, exposes `instance.has_changed("state_id")`, and stashes the change set on `instance._changes_on_save` after `save()` so a `post_save` receiver can read it.

**Receiver** (registered in `pi_dash/db/signals/issue_signals.py`, wired through `apps.py`):

```python
@receiver(post_save, sender=Issue)
def trigger_github_completion_comment(sender, instance, created, **kwargs):
    if created:
        return
    if "state_id" not in getattr(instance, "_changes_on_save", []):
        return
    if instance.state is None or instance.state.group != StateGroup.COMPLETED.value:
        return
    sync = GithubIssueSync.objects.filter(issue=instance).first()
    if sync is None or sync.metadata.get("completion_comment_id"):
        return  # not a synced issue, or already commented
    post_completion_comment.delay(sync.id)
```

**Action task**:

```python
@shared_task
def post_completion_comment(issue_sync_id):
    sync = GithubIssueSync.objects.select_related(
        "repository_sync__repository",
        "repository_sync__workspace_integration",
        "issue",
    ).get(id=issue_sync_id)
    if sync.metadata.get("completion_comment_id"):
        return  # second-line idempotency in case of task duplication
    token = decrypt_data(sync.repository_sync.workspace_integration.config["token"])
    client = GithubClient(token=token)
    body = f"This issue has been completed in Pi Dash: {pi_dash_issue_url(sync.issue)}"
    comment = client.post_comment(
        sync.repository_sync.repository.owner,
        sync.repository_sync.repository.name,
        sync.repo_issue_id,
        body,
    )
    sync.metadata["completion_comment_id"] = comment["id"]
    sync.save(update_fields=["metadata"])
```

**Idempotency**: stored at `GithubIssueSync.metadata["completion_comment_id"]` (the metadata field added in §5). Both the signal receiver and the task itself short-circuit if already set. This survives reopen → re-complete cycles: only the first completion comments back.

**Re-arm policy**: out of scope for MVP. If a user wants the comment to fire again after a reopen, they manually clear `metadata["completion_comment_id"]` via Django admin. Documented as a follow-up.

**Helper — `pi_dash_issue_url(issue)`**: returns the absolute URL of the issue in the Pi Dash UI:

```python
def pi_dash_issue_url(issue: "Issue") -> str:
    base = settings.WEB_URL or settings.APP_BASE_URL
    if not base:
        raise ImproperlyConfigured("WEB_URL or APP_BASE_URL must be set for GitHub completion comments")
    return f"{base.rstrip('/')}/{issue.workspace.slug}/projects/{issue.project_id}/issues/{issue.id}"
```

Add this in `apps/api/pi_dash/bgtasks/github_sync_task.py` (or a small helpers module if reused). The existing `prompting.context._absolute_issue_url` returns only a relative path — fine for templates rendered inside Pi Dash, not enough for a comment that lives on github.com.

**Failure mode**: best-effort. Celery default retry (3 attempts with exponential backoff) for transient errors. The Pi Dash state change is not rolled back. Permanent failure (403/404) writes the error to `sync.metadata["completion_comment_error"]` so it surfaces in admin. A 404 specifically means the upstream issue was deleted between the `reconcile_upstream_gone` flag-write and the comment task running; the task does not retry into oblivion (Celery's `MaxRetriesExceededError` after 3 attempts is the natural backstop, plus the 4xx classification skips retries entirely).

### 6.6 Web UI

**Workspace settings — Integrations tab**:

- Reuse the existing integrations settings page at `apps/web/app/(all)/[workspaceSlug]/(settings)/settings/(workspace)/integrations/page.tsx`.
- Replace the GitHub branch of `apps/web/core/components/integration/single-integration-card.tsx`:
  - **Disconnected**: PAT input + "Connect" button. Help text links to GitHub's fine-grained PAT creation page with the required permissions (`issues:read`, `metadata:read`, and `issues:write` only if completion-comment is enabled).
  - **Connected**: shows `github_user_login`, verified timestamp, and a "Disconnect" button that calls the soft-disconnect endpoint from §6.1.
- Keep Slack on its current OAuth install path. Only GitHub changes behavior in this MVP.

**Project settings — new "GitHub" section**:

- If the workspace has no GitHub integration: shows an empty state with a link to the workspace integration page.
- If connected and not yet bound: a searchable repo picker (calls the `repos/` endpoint), "Bind repository" button.
- If bound: shows the bound repo, a toggle "Sync issues every 4 hours", and a disconnect button. Surfaces `last_synced_at` and `last_sync_error` as a small status block.
- File: new component under `apps/web/core/components/project/settings/github-sync.tsx`. Wire it into the existing project-settings router and remove the old dead integration-card path from §6.0 rather than rendering both.

**Issue list**:

- No special UI in MVP. The `[github_123]` prefix is enough to disambiguate. Optionally render a small GitHub icon next to issues with `external_source == "github"` — defer if not trivial.

### 6.7 Types & services

- Extend the shared types package with `IGithubRepositorySync`, `IGithubBindRequest`, and `IGithubConnectRequest`. If we keep integration types in a single file, update `packages/types/src/integration.ts`; if we split by provider, add `packages/types/src/integration/github.ts` and re-export it from the package index.
- Add the web client methods in the existing app service layer, not a new package service:
  - `apps/web/core/services/integrations/github.service.ts`: `connectWorkspace`, `listRepos`, `disconnectWorkspace`.
  - `apps/web/core/services/project/project.service.ts`: `bindGithubRepository`, `setGithubSyncEnabled`, `removeGithubBinding`.
- Delete the legacy methods from §6.0 that target unported Plane-era routes.

### 6.8 Edit and delete permissions on synced rows

GitHub is authoritative for the fields it syncs. To prevent edits-then-overwrite churn (and to make "what's upstream" / "what's local" unambiguous to users), **actively-synced** rows lock the synced fields at both the API and UI layers. Workflow fields stay editable. After unbind, the lock releases.

**Lock predicate — actively synced:**

A row is "actively synced" iff a corresponding sync-tracking row exists:
- `Issue` is actively synced iff `GithubIssueSync.objects.filter(issue=instance).exists()`.
- `IssueComment` is actively synced iff `GithubCommentSync.objects.filter(comment=instance).exists()`.

**Why predicate-on-sync-row, not on `external_source`:** `Issue.external_source` is set permanently on import — it's a provenance marker, not a state flag. Using it as the lock predicate would mean unbind has to either clear `external_source` (which would break re-bind: the upsert key in §6.3 wouldn't find the row, and a duplicate would be created) or leave the lock dangling forever (no edit path post-unbind). Keying on the sync-tracking row instead gives clean cascade semantics: unbind → `GithubRepositorySync` deleted → `GithubIssueSync` / `GithubCommentSync` rows cascade-delete → lock releases atomically. Re-bind reverses it. Provenance markers (`external_source`, the title/comment prefixes) are unaffected.

**Lock matrix** (where "synced" = "actively-synced" per the predicate above):

| Object | Field / action | Editable when... | Why |
| --- | --- | --- | --- |
| Synced `Issue` | `name`, `description_html`, `description_json`, `description_stripped` | **Locked while synced** | Mirrored from GitHub each sync; let upstream win. |
| Synced `Issue` | `state`, `priority`, `assignees`, `labels`, `cycle`, `module`, `start_date`, `target_date`, `point`, `estimate_point`, `parent`, `sort_order` | Editable | Pi Dash workflow fields, never mirrored from GitHub. State change is the trigger for §6.5 completion comment-back. |
| Synced `Issue` | delete (soft or hard) | **Locked while synced** | Prevents the resurrect-on-next-sync race. To delete a mirrored issue, the user first unbinds the project via §6.2 `DELETE /github/`, which cascade-deletes `GithubIssueSync` and releases the lock. |
| Unsynced `Issue` (post-unbind, still has `external_source="github"`) | all fields, delete | Editable | Lock predicate no longer matches; row behaves like a native issue with a `[github_<n>]` prefix as a provenance marker. |
| Synced `IssueComment` | `comment_html`, `comment_json`, `comment_stripped` | **Locked while synced** | Mirrored from GitHub. |
| Synced `IssueComment` | delete | **Locked while synced** | Same reason. |
| `IssueComment` with `external_source IS NULL` on any issue | body, delete | Editable | Native Pi Dash discussion, never mirrored, never pushed back to GitHub. |
| Unsynced `IssueComment` (post-unbind, still has `external_source="github"`) | body, delete | Editable | Same release-on-unbind logic. |

**Implementation — defense in depth:**

1. **Serializer** — `apps/api/pi_dash/app/serializers/issue.py` `IssueSerializer.validate()`:
   ```python
   def validate(self, attrs):
       if self.instance and self._is_actively_synced(self.instance):
           locked = {"name", "description_html", "description_json", "description_stripped"}
           bad = locked & set(attrs.keys())
           if bad:
               raise serializers.ValidationError(
                   {f: "This field is synced from GitHub and is read-only." for f in bad}
               )
       return super().validate(attrs)

   @staticmethod
   def _is_actively_synced(issue):
       # One DB hit per validate(); GithubIssueSync.issue has an index via the FK.
       return GithubIssueSync.objects.filter(issue=issue).exists()
   ```
   Same shape for `IssueCommentSerializer.validate()`, keyed on `GithubCommentSync.objects.filter(comment=instance).exists()`.
2. **ViewSet** — both `IssueViewSet` and `IssueCommentViewSet` override `destroy()` to reject when the instance is actively synced, returning HTTP 409 with a body explaining the unbind step.
3. **Public API** — same guards on `apps/api/pi_dash/api/serializers/issue.py` and the corresponding viewsets, since the public REST API hits a different serializer set.
4. **UI** — `apps/web/core/components/issues/issue-modal/form.tsx` and the comment list component check whether the row is actively synced (the API serializer surfaces this as `is_synced: bool` on the issue / comment payload):
   - Hide the title and description edit affordances on synced issues (render plain `<h1>` / `<div>` instead of editable inputs).
   - Hide the edit/delete menu on synced comments; render a small "GitHub" badge in their place.
   - Native (`external_source IS NULL`) comments on a synced issue render with the normal edit/delete menu.
5. **Sync task** — `update_or_create` bypasses serializers, so the Celery task in §6.3 mutates synced fields freely. This is the only legitimate write path.

**Why the lock applies to deletes too:** if a synced row is soft-deleted while still synced, the next sync's `update_or_create` (which runs through `IssueComment.objects` — the soft-delete-filtering manager) won't see the row and will insert a fresh duplicate keyed on the same `(issue, external_source, external_id)`. That breaks the upsert invariant. Locking deletes while synced is the simpler fix; users who genuinely want a synced row gone unbind first (cascade releases the lock), then delete the surviving native row.

## 7. Sync Flow Summary

```
[Workspace admin] paste PAT
      │
      ▼
WorkspaceIntegration row (provider=github, config.token=encrypted PAT)
      │
      ▼
[Project lead] in project settings → pick repo → toggle "Sync ON"
      │
      ▼
GithubRepositorySync row (is_sync_enabled=True)
      │
      ▼
[Celery beat] every 4h → sync_all_repos → fan out to sync_one_repo per row
      │
      ▼
Full scan: list all open issues + all repo comments (paginated):
  - upsert Issue (external_source=github, external_id=<number>)
  - prefix title with [github_<number>]
  - upsert IssueComment for each comment, prefix body with [Github]
  - diff local GithubIssueSync rows vs. remote_issue_numbers
    → flag any locally-known but remote-absent issues with
      metadata.upstream_gone_at (deletion or closure detection)
      │
      ▼
[User] completes issue in Pi Dash (state.group transitions to completed)
      │
      ▼
ChangeTrackerMixin marks state_id as changed → post_save signal
gates on (a) state_id in _changes_on_save, (b) new state.group == completed,
(c) GithubIssueSync exists, (d) metadata.completion_comment_id not set
      │
      ▼
post_completion_comment task
      │
      ▼
GitHub API: POST /repos/.../issues/<n>/comments  (idempotent via metadata)
```

## 8. Testing

**Backend**

- Migration dry-run + reverse.
- Unit tests for `GithubClient` (mock `requests` — the codebase already standardizes on it, no `httpx` imports): pagination, PR-vs-issue filtering on the `pull_request` field, comment listing, comment posting, 401/403/404 surfacing.
- Unit test `sync_one_repo` end-to-end with a fake client: empty-local first-run path (everything upserts), idempotent path on second run (no DB writes for unchanged rows), upstream-gone path (`reconcile_upstream_gone` sets the flag once, not on every run), reappearance path (flag clears), error path setting `last_sync_error`.
- Unit test the title prefix is regenerated unconditionally from upstream (no dirty-check; §6.4) and the `[Github]` comment prefix is regenerated on each sync.
- Unit test the read-only lock (§6.8): `IssueSerializer.validate()` rejects edits to `name`/`description_*` on `external_source="github"` issues; `IssueCommentSerializer.validate()` rejects edits to body fields on synced comments; `destroy()` returns 409 on synced rows; native comments on synced issues remain editable and deletable.
- Unit test the completion-comment signal path: triggers exactly once per state transition (`state_id` change → completed group), idempotent against double-fire (two saves in one transaction → one task), short-circuits when `metadata["completion_comment_id"]` is already set.
- Unit test the completion-comment 404 path: upstream issue deleted between flag-write and task-run → writes `metadata["completion_comment_error"]`, does not retry into oblivion.
- Unit test the soft-disconnect / reconnect roundtrip: disconnect clears `config["token"]`, marks dependent `GithubRepositorySync` rows `is_sync_enabled=False`, leaves rows in place (no cascade); reconnect with a new token clears `disconnected_at` and reuses the same `WorkspaceIntegration` row; project syncs do **not** auto-resume on reconnect.
- Unit test connect/bind/toggle endpoints + permission checks (only workspace admins can connect; only project members can bind/toggle); bind validates `(owner, name, repository_id)` consistency against `GET /repos/{owner}/{name}`.

**Web**

- Manual walkthrough: connect → list repos → bind → toggle on → wait for sync (or trigger manually in dev via `manage.py shell`) → see prefixed issues → complete one → check the GitHub side for the comment.

No protocol changes → no runner test impact.

## 9. Risk & Rollout

**Risk: PAT scope creep.** A PAT with `repo` scope grants more than read-on-issues. Mitigation: docs link to fine-grained PAT page with minimal scopes. We do not enforce scope server-side; if `issues:write` is missing, the completion-comment task will 403 and surface in `last_sync_error`.

**Risk: rate limits.** A workspace with many synced repos all firing on the 4-hour mark will burst-call GitHub. PAT limit is 5000/hr. With per-page=100, 50 repos × 50 pages worst case = 2500 requests; comfortably under. Mitigation if hit: `Retry-After` honored, exponential backoff via Celery retry.

**Risk: user confusion about read-only synced fields.** Some users will expect to be able to edit a synced issue's title or description in Pi Dash. The lock in §6.8 will reject the change at the API layer and hide the affordance in the UI, but the workflow fields (state, priority, etc.) *are* editable, and the inconsistency may surprise users. Mitigation: a small "synced from GitHub" badge with a tooltip explaining which fields are upstream-managed; the bind flow shows the same explanation before the user enables sync.

**Risk: signal storm on bulk state changes.** A user bulk-completing 100 issues at once would enqueue 100 GitHub POST tasks. Acceptable: GitHub's per-issue comment endpoint is well-behaved; tasks are async and rate-limited by Celery worker count. Watch in production.

**Risk: cascade on disconnect.** Disconnecting the workspace integration silently breaks every project's sync. Mitigation: the disconnect endpoint in §6.1 marks all dependent `GithubRepositorySync` rows with `is_sync_enabled=False` and writes an explanatory `last_sync_error`, so project settings UI can display the cause.

**Rollout**: feature-flag the entire integration behind a Django setting `GITHUB_SYNC_ENABLED` (default `True` in CE, read once at process start). Self-hosters who don't want the integration set the env var to `False` and the integration disappears: the workspace integrations card hides the GitHub option, the project-settings GitHub section short-circuits to an empty state, the Celery beat entry no-ops, and the connect/bind endpoints return HTTP 404. No per-workspace migration is needed; an instance-level toggle matches how other features in `pi_dash/settings/common.py` are gated.

## 10. Out-of-Scope / Follow-ups

- **GitHub App + manifest flow** for self-hosted instances (replaces PAT, gives per-install rate limits, enables webhooks).
- **Webhook receiver** for near-real-time sync; falls back to the 4-hour poll.
- **Two-way state sync**: closing a GitHub issue when the Pi Dash issue is completed (today: comment only).
- **Label / assignee / milestone mapping.**
- **Per-project sync cadence** (UI knob).
- **"Sync now" button** that triggers `sync_one_repo.delay()` immediately.
- **Backfill of closed issues** with a per-bind option (today MVP only mirrors `state=open`; closed issues never enter the local set).
- **`since=` incremental fallback** for very large repos (>5k open issues × multiple repos in one workspace), where full-scan starts to eat noticeable rate-limit budget. Would coexist with the existing diff-based deletion detection by running incremental on most ticks and a full scan periodically (e.g. once per day).
- **Hard delete propagation** — convert `metadata["upstream_gone_at"]` flags into actual Pi Dash soft/hard deletes after a configurable grace period. Today the mirrored row is preserved indefinitely.
- **Closed-state mirroring** — optional reverse of the §3 non-goal: when an upstream issue is closed (visible in the same diff that detects deletions), move the Pi Dash mirror to the project's "closed/done" state.
- **Lift the read-only lock on synced fields** with a conflict-merge UI (today: serializer rejects edits to title/description on synced issues and bodies on synced comments; see §6.8).
- **Multiple repos per project.**
- **Hard disconnect** (full delete of `WorkspaceIntegration` + cascading sync rows). Requires either an explicit pre-delete cleanup pass over `GithubRepositorySync` or changing the FK to `on_delete=SET_NULL`.
- **Auto-resume sync on reconnect.** Today, reconnecting requires re-enabling each project's sync individually; an "auto-resume previously enabled syncs" toggle could be added.
- **Re-arm completion comments** after a reopen-then-re-complete cycle (requires a UI to clear `GithubIssueSync.metadata["completion_comment_id"]`).
