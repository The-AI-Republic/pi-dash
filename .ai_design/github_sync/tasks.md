# GitHub Issue Sync — Implementation Tasks

This file turns `design.md` into a concrete MVP implementation checklist.

Related docs:
- `design.md`

## Suggested rollout

### PR 1 — Schema and model scaffolding

Goal:
- land the database changes and model wiring with minimal runtime behavior change

Scope:
- add `GithubRepositorySync.is_sync_enabled`
- add `GithubRepositorySync.last_synced_at`
- add `GithubRepositorySync.last_sync_error`
- add per-project active binding constraint on `GithubRepositorySync`
- add `GithubIssueSync.metadata`
- add `GithubIssueSync.gh_issue_created_at`
- add `GithubIssueSync.gh_issue_updated_at`
- make `Issue` use `ChangeTrackerMixin` with `TRACKED_FIELDS = ["state_id"]`

Why first:
- all later backend and UI work depends on the schema existing

### PR 2 — GitHub backend client and workspace/project endpoints

Goal:
- make credential connection, repo listing, bind, toggle, and unbind real

Scope:
- implement GitHub API client helpers
- implement workspace connect endpoint
- implement workspace repo-list endpoint
- implement workspace soft-disconnect endpoint
- implement project bind endpoint
- implement project enable/disable endpoint
- implement project unbind endpoint
- add shared serializers and permission checks

Why second:
- the web UI and sync worker both need the same GitHub credential and repository plumbing

### PR 3 — Sync worker and completion comment-back

Goal:
- make repository polling, issue/comment upsert, upstream-gone detection, and completion comments work end to end

Scope:
- add `github_sync_task.py`
- add Celery beat entry
- implement issue upsert flow
- implement comment upsert flow
- implement upstream-gone reconciliation
- implement completion comment signal and task
- add helper for absolute Pi Dash issue URL

Why third:
- this is the first end-to-end behavior change and should come after the API/model surface is stable

### PR 4 — Sync locks and web UI

Goal:
- make the feature usable and protect synced content from local edits while bound

Scope:
- replace workspace GitHub integration card behavior
- add project settings GitHub section
- remove dead Plane-era GitHub client code
- add serializer/viewset read-only guards for actively-synced issues/comments
- surface `is_synced` in API payloads
- hide edit/delete affordances in the web UI for actively-synced rows

Why fourth:
- the UI depends on the backend contract and the lock behavior depends on the sync-tracking rows

### PR 5 — Hardening and test coverage

Goal:
- close rollout gaps before shipping

Scope:
- unit tests for client, endpoints, worker, locks, and completion comments
- manual web walkthrough
- docs cleanup
- operational notes for reconnect, unbind, and full-scan behavior

Why last:
- once the feature exists, this PR reduces regression and rollout risk

## Dependency order

1. Schema first:
   `GithubRepositorySync` fields and constraint, `GithubIssueSync` fields, `Issue` mixin
2. Backend surface next:
   GitHub client, workspace endpoints, project endpoints
3. Runtime sync after endpoints:
   Celery task, upserts, upstream-gone reconciliation, completion comment-back
4. Locks and UI after sync exists:
   serializer guards, API `is_synced`, workspace/project settings UI
5. Hardening last:
   tests, docs cleanup, operational notes

## 1. Data model

- [ ] Add migration for `GithubRepositorySync.is_sync_enabled`.
- [ ] Add migration for `GithubRepositorySync.last_synced_at`.
- [ ] Add migration for `GithubRepositorySync.last_sync_error`.
- [ ] Add filtered unique constraint enforcing one active `GithubRepositorySync` per project.
- [ ] Add migration for `GithubIssueSync.metadata`.
- [ ] Add migration for `GithubIssueSync.gh_issue_created_at`.
- [ ] Add migration for `GithubIssueSync.gh_issue_updated_at`.
- [ ] Update `apps/api/pi_dash/db/models/issue.py` so `Issue` extends `ChangeTrackerMixin`.
- [ ] Set `Issue.TRACKED_FIELDS = ["state_id"]`.
- [ ] Add model tests for the new per-project uniqueness constraint.

## 2. GitHub backend client

- [ ] Create or extend a GitHub client module for:
  - `GET /user`
  - `GET /user/repos`
  - `GET /repos/{owner}/{repo}`
  - `GET /repos/{owner}/{repo}/issues`
  - `GET /repos/{owner}/{repo}/issues/comments`
  - `POST /repos/{owner}/{repo}/issues/{number}/comments`
- [ ] Implement auth header handling for PAT-based requests.
- [ ] Implement pagination helpers for repo listing, issue listing, and repo-wide comment listing.
- [ ] Implement `has_next_page` parsing from GitHub `Link` headers for repo browsing.
- [ ] Normalize GitHub 401, 403, 404, and secondary rate-limit failures into predictable app exceptions.
- [ ] Reuse `encrypt_data` and `decrypt_data` for PAT storage and retrieval.

## 3. Workspace endpoints

- [ ] Add serializer for workspace GitHub connect payload.
- [ ] Implement `POST /api/workspaces/<slug>/integrations/github/connect/`.
- [ ] Validate PAT via `GET /user` before saving.
- [ ] Upsert `WorkspaceIntegration.config` with:
  - `auth_type`
  - encrypted `token`
  - `github_user_login`
  - `verified_at`
- [ ] Implement `GET /api/workspaces/<slug>/integrations/github/repos/?page=<n>`.
- [ ] Return `{ repos, has_next_page }` in the response shape from the design.
- [ ] Implement `POST /api/workspaces/<slug>/integrations/github/disconnect/`.
- [ ] Soft-disconnect by clearing token, setting `disconnected_at`, and disabling dependent syncs.
- [ ] Enforce workspace-admin permissions on connect and disconnect endpoints.

## 4. Project binding endpoints

- [ ] Add serializer for project bind payload:
  - `repository_id`
  - `owner`
  - `name`
  - `url`
- [ ] Implement precondition check returning HTTP 409 when a project already has an active binding.
- [ ] Validate `repository_id` against `GET /repos/{owner}/{name}` before binding.
- [ ] Create or fetch the `GithubRepository` row for the selected repo.
- [ ] Create `GithubRepositorySync` with:
  - `workspace_integration`
  - `actor`
  - `label`
  - `is_sync_enabled=False`
- [ ] Reuse or create the project `github` label.
- [ ] Implement `PATCH /api/workspaces/<slug>/projects/<id>/github/sync/`.
- [ ] Implement `DELETE /api/workspaces/<slug>/projects/<id>/github/`.
- [ ] Ensure unbind relies on existing cascade behavior for `GithubIssueSync` and `GithubCommentSync`.
- [ ] Enforce project membership/authorization checks on bind, toggle, and unbind endpoints.

## 5. Sync worker

- [ ] Add `apps/api/pi_dash/bgtasks/github_sync_task.py`.
- [ ] Register `sync_all_repos` in Celery beat on a 4-hour cadence.
- [ ] Implement repo fan-out from `sync_all_repos()` to `sync_one_repo(sync_id)`.
- [ ] Load and decrypt the workspace PAT inside `sync_one_repo`.
- [ ] Enumerate all open repo issues and skip PRs via the `pull_request` field.
- [ ] Upsert mirrored `Issue` rows keyed by `(project, external_source="github", external_id)`.
- [ ] Persist GitHub timestamps onto `GithubIssueSync`, not `Issue`.
- [ ] Persist GitHub author login onto `GithubIssueSync.metadata["github_user_login"]`.
- [ ] Enumerate repo-wide comments.
- [ ] Parse `issue_url` to resolve each comment's parent issue number.
- [ ] Skip comments whose parent issue is not in the mirrored open non-PR set.
- [ ] Upsert mirrored `IssueComment` rows keyed by `(issue, external_source="github", external_id)`.
- [ ] Set and clear `last_synced_at` / `last_sync_error` per task outcome.
- [ ] Implement retry with exponential backoff for transient failures.

## 6. Upstream-gone handling

- [ ] Implement `reconcile_upstream_gone(sync, remote_issue_numbers)`.
- [ ] Flag absent local mirrors with `GithubIssueSync.metadata["upstream_gone_at"]`.
- [ ] Clear `upstream_gone_at` if a previously absent issue reappears.
- [ ] Ensure upstream-gone issues stop receiving future synced updates until they reappear.
- [ ] Decide whether the issue list MVP surfaces a badge immediately or leaves the field admin-only for the first cut.

## 7. Completion comment-back

- [ ] Add signal registration for GitHub completion comment-back.
- [ ] Gate the receiver on:
  - not `created`
  - `"state_id" in _changes_on_save`
  - new state group is `completed`
  - linked `GithubIssueSync` exists
  - `completion_comment_id` not already set
- [ ] Add `post_completion_comment(issue_sync_id)` task.
- [ ] Build absolute Pi Dash issue URLs using configured web base URL.
- [ ] Post completion comments back to GitHub issues.
- [ ] Persist returned comment id to `GithubIssueSync.metadata["completion_comment_id"]`.
- [ ] Persist permanent failures to `GithubIssueSync.metadata["completion_comment_error"]`.
- [ ] Skip repeat comments on reopen/re-complete cycles unless manually cleared later.

## 8. Read-only lock behavior

- [ ] Add actively-synced predicate helpers for issues and comments based on `GithubIssueSync` / `GithubCommentSync` existence.
- [ ] Add app-layer serializer validation blocking synced issue edits to:
  - `name`
  - `description_html`
  - `description_json`
  - `description_stripped`
- [ ] Add app-layer serializer validation blocking synced comment edits to:
  - `comment_html`
  - `comment_json`
  - `comment_stripped`
- [ ] Add app-layer `destroy()` guards returning HTTP 409 for actively-synced issues/comments.
- [ ] Mirror the same guards in the public API serializer/viewset layer.
- [ ] Confirm lock release after unbind when sync-tracking rows cascade-delete.

## 9. API payloads and types

- [ ] Extend shared types with:
  - `IGithubRepositorySync`
  - `IGithubBindRequest`
  - `IGithubConnectRequest`
- [ ] Add API response typing for paginated repo list `{ repos, has_next_page }`.
- [ ] Add `is_synced: bool` to issue payloads.
- [ ] Add `is_synced: bool` to issue comment payloads.
- [ ] Ensure the web client can distinguish actively-synced rows from provenance-only post-unbind rows.

## 10. Web services and cleanup

- [ ] Rewrite `apps/web/core/services/integrations/github.service.ts` with:
  - `connectWorkspace`
  - `listRepos`
  - `disconnectWorkspace`
- [ ] Replace old project GitHub methods in `apps/web/core/services/project/project.service.ts` with:
  - `bindGithubRepository`
  - `setGithubSyncEnabled`
  - `removeGithubBinding`
- [ ] Delete dead Plane-era GitHub integration code:
  - `apps/web/core/components/project/integration-card.tsx`
  - `apps/web/core/components/integration/github/select-repository.tsx`
  - obsolete GitHub service methods
  - obsolete fetch key(s)

## 11. Workspace and project UI

- [ ] Update the existing integrations settings page GitHub card to support PAT connect/disconnect.
- [ ] Show connected GitHub login and verification state.
- [ ] Add PAT input validation and submit states.
- [ ] Add project settings GitHub section component.
- [ ] Add repo picker using paginated browse from `/integrations/github/repos/`.
- [ ] Add client-side substring filtering on loaded repos by `full_name`.
- [ ] Add bind action UI.
- [ ] Add enable/disable toggle UI.
- [ ] Add unbind action UI.
- [ ] Surface `last_synced_at` and `last_sync_error`.
- [ ] Show empty state when workspace GitHub integration is missing.

## 12. Issue and comment UI

- [ ] Surface `is_synced` to issue modal and comment components.
- [ ] Hide title and description edit affordances for actively-synced issues.
- [ ] Hide edit/delete affordances for actively-synced comments.
- [ ] Keep native comments on synced issues editable.
- [ ] Preserve visible provenance markers:
  - `[github_<n>]` on issue titles
  - `[Github]` on mirrored comments
- [ ] Decide whether to add a GitHub badge/icon in addition to the stored prefixes.
- [ ] If shipping the upstream-gone UI in MVP, render muted styling or a “no longer on GitHub” badge.

## 13. Tests

- [ ] Migration test for the new `GithubRepositorySync` constraint and `GithubIssueSync` fields.
- [ ] Unit tests for GitHub client pagination and error handling.
- [ ] Unit tests for workspace connect success and invalid token rejection.
- [ ] Unit tests for workspace repo-list pagination and `has_next_page`.
- [ ] Unit tests for soft disconnect and reconnect behavior.
- [ ] Unit tests for bind validation and one-binding-per-project enforcement.
- [ ] Unit tests for `sync_one_repo` first-run import path.
- [ ] Unit tests for idempotent second sync path.
- [ ] Unit tests for comment parent parsing and skipping of PR/closed/unmirrored comments.
- [ ] Unit tests for upstream-gone flag set and clear behavior.
- [ ] Unit tests for completion comment trigger and idempotency.
- [ ] Unit tests for read-only serializer and destroy locks.
- [ ] Unit tests for lock release after unbind.
- [ ] Manual walkthrough:
  - connect workspace GitHub PAT
  - browse repos
  - bind project
  - enable sync
  - run worker manually in dev
  - verify imported issues/comments
  - complete a synced issue
  - verify GitHub completion comment
  - unbind and confirm rows become editable

## 14. Final doc cleanup

- [ ] Remove the stale statement in `design.md` that claims single-repo-per-project is “already enforced by `unique_together = [project, repository]`”.
- [ ] Confirm all endpoint paths in the design match the final implementation.
- [ ] Confirm all serializer/viewset file references in the design match the final implementation.
