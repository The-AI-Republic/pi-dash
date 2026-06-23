# Git Support Generalization and GitLab Parity - Tasks

**Design:** [design.md](./design.md)
**Status:** Proposed task breakdown
**Date:** 2026-06-22

## Implementation Assumptions

- Initial private clone support is `runner_managed`.
- Pi Dash does not pass GitHub or GitLab clone credentials to runners in the first implementation.
- `GitProviderAccount` is the source of truth for provider API access.
- `WorkspaceIntegration` remains only as compatibility glue where needed.
- Each project has at most one active `GitRepositoryBinding`.
- GitHub is migrated onto the generic path before GitLab parity is shipped.
- Old GitHub API routes, CLI commands, and assistant tools remain compatibility aliases during rollout.

## Phase 0 - Preflight

- [ ] Confirm the accepted defaults from `design.md` open decisions.
- [ ] Decide the model app/module location for generic Git integration models.
- [ ] Decide whether generic model table names use `git_*` or `vcs_*`.
- [ ] Decide the first canonical CLI command name: `attach-review` or `attach-code-review`.
- [ ] Confirm whether legacy `Github*` models will be retained during dual-read or migrated to generic-only reads immediately.
- [ ] Add a feature flag for generic Git integration rollout.

## Phase 1 - Provider Registry, DTOs, and Parsers

- [ ] Create generic Git integration package, for example `pi_dash/integrations/git/`.
- [ ] Add provider registry with lookup by provider key and URL host.
- [ ] Add normalized DTOs:
  - [ ] `ParsedRepository`
  - [ ] `RemoteRepository`
  - [ ] `GitProviderCapabilities`
  - [ ] `RemoteIssue`
  - [ ] `RemoteComment`
  - [ ] `ParsedCodeReview`
  - [ ] `RemoteCodeReview`
  - [ ] `ProviderWebhookEvent`
- [ ] Define `GitProviderAdapter` protocol.
- [ ] Move GitHub repo URL parsing behind a GitHub adapter.
- [ ] Move GitHub pull request URL parsing behind a GitHub adapter.
- [ ] Add GitLab repo URL parser.
- [ ] Add GitLab merge request URL parser.
- [ ] Add self-hosted GitLab host allowlist support.
- [ ] Add parser tests for GitHub HTTPS repo URLs.
- [ ] Add parser tests for GitHub SSH repo URLs.
- [ ] Add parser tests for GitHub PR URLs.
- [ ] Add parser tests for GitLab HTTPS repo URLs.
- [ ] Add parser tests for GitLab SSH repo URLs.
- [ ] Add parser tests for GitLab nested subgroup URLs.
- [ ] Add parser tests for GitLab MR URLs.
- [ ] Add parser tests for unsupported hosts and unsafe URL shapes.

## Phase 2 - Generic Models and GitHub Migration

- [ ] Add `GitProviderAccount`.
- [ ] Add `GitRepository`.
- [ ] Add `GitRepositoryBinding`.
- [ ] Add `GitIssueSync`.
- [ ] Add `GitCommentSync`.
- [ ] Add `GitCodeReviewLink`.
- [ ] Add `GitWebhookDelivery`.
- [ ] Add `GitWebhookRegistration` if webhook auto-registration is in scope for the first pass.
- [ ] Add indexes and active-row constraints.
- [ ] Ensure `GitCodeReviewLink` uniqueness prefers `(provider, host_url, repo_external_id, external_iid)`.
- [ ] Add fallback uniqueness for unresolved code-review links.
- [ ] Add `clone_auth_mode` on repository bindings.
- [ ] Write migration from GitHub PAT `WorkspaceIntegration` to `GitProviderAccount`.
- [ ] Write migration from `GithubAppInstallation` to `GitProviderAccount`.
- [ ] Write migration from `GithubRepository` to `GitRepository`.
- [ ] Write migration from `GithubRepositorySync` to `GitRepositoryBinding`.
- [ ] Write migration from `GithubIssueSync` to `GitIssueSync`.
- [ ] Write migration from `GithubCommentSync` to `GitCommentSync`.
- [ ] Write migration from `GithubPullRequestLink` to `GitCodeReviewLink`.
- [ ] Write migration from `GithubWebhookDelivery` to `GitWebhookDelivery`.
- [ ] Preserve `completion_comment_id` metadata.
- [ ] Preserve `upstream_gone_at` metadata.
- [ ] Preserve remote created/updated timestamps.
- [ ] Preserve active PR uniqueness.
- [ ] Preserve soft-delete semantics.
- [ ] Add migration tests for all migrated GitHub model families.
- [ ] Add rollback or dual-read safety plan before deleting legacy reads.

## Phase 3 - Generic Repository Binding API

- [ ] Add generic provider account list/create/detail/delete endpoints.
- [ ] Add generic provider account repository listing endpoint.
- [ ] Add generic project repository status endpoint.
- [ ] Add generic project repository bind endpoint.
- [ ] Add generic project repository update endpoint for sync toggle.
- [ ] Add generic project repository unbind endpoint.
- [ ] Implement provider account selection by provider and host.
- [ ] Return account-selection error when multiple matching accounts exist and no account id is provided.
- [ ] Implement account disconnect semantics: dependent bindings become degraded, not silently unbound.
- [ ] Implement generic repository binding service with GitHub adapter first.
- [ ] Point current GitHub bind route to generic binding service.
- [ ] Point current GitHub status/toggle/unbind routes to generic binding service.
- [ ] Ensure bind response includes provider account id, provider, host, repository id, canonical clone URL, clone auth mode, sync status, and degraded reasons.
- [ ] Add API contract tests for GitHub through old routes.
- [ ] Add API contract tests for GitHub through generic routes.
- [ ] Add API contract tests for ambiguous account selection.

## Phase 4 - Generic Sync Engine for GitHub

- [ ] Create generic sync task module.
- [ ] Implement `sync_all_bindings()`.
- [ ] Implement `sync_one_binding(binding_id)`.
- [ ] Implement generic completion comment write-back.
- [ ] Use GitHub adapter for issue listing.
- [ ] Use GitHub adapter for comment listing.
- [ ] Use GitHub adapter for comment creation.
- [ ] Preserve current GitHub behavior for skipping PRs from issue import.
- [ ] Preserve current upstream-gone behavior.
- [ ] Preserve current read-only issue locks through generic sync rows.
- [ ] Preserve current read-only comment locks through generic sync rows.
- [ ] Replace Celery beat schedule with generic task.
- [ ] Keep old GitHub task alias if operationally needed.
- [ ] Add sync tests for GitHub import.
- [ ] Add sync tests for GitHub comments.
- [ ] Add sync tests for completion comment idempotency.
- [ ] Add sync tests for read-only locks.

## Phase 5 - GitLab Provider Account, Repository, Issue, and Notes Parity

- [ ] Add GitLab provider account connect endpoint.
- [ ] Add GitLab provider account status endpoint.
- [ ] Add GitLab provider account disconnect endpoint.
- [ ] Add GitLab repository listing through Projects API.
- [ ] Add GitLab repository verification by URL-encoded full path.
- [ ] Store GitLab project numeric id as `GitRepository.external_id`.
- [ ] Store GitLab canonical web URL.
- [ ] Store GitLab HTTP clone URL.
- [ ] Store GitLab SSH clone URL.
- [ ] Store GitLab default branch.
- [ ] Bind GitLab repositories through generic project repository endpoint.
- [ ] Set `clone_auth_mode = runner_managed` for GitLab private repos.
- [ ] Implement GitLab Issues API listing for open issues.
- [ ] Map GitLab issue `iid` to `GitIssueSync.external_iid`.
- [ ] Map GitLab issue `id` to `GitIssueSync.external_id`.
- [ ] Implement GitLab Notes API import for issue notes.
- [ ] Skip GitLab system notes initially.
- [ ] Implement GitLab completion note write-back.
- [ ] Implement GitLab sync errors and degraded capability reporting for read-only tokens.
- [ ] Add GitLab adapter HTTP tests with mocked Projects API.
- [ ] Add GitLab adapter HTTP tests with mocked Issues API.
- [ ] Add GitLab adapter HTTP tests with mocked Notes API.
- [ ] Add GitLab sync service tests.
- [ ] Add GitLab read-only lock tests.

## Phase 6 - Generic Code-Review Links and GitLab MR Parity

- [ ] Add generic code-review list endpoint.
- [ ] Add generic code-review attach endpoint.
- [ ] Add generic code-review delete endpoint if needed by UI.
- [ ] Point old GitHub PR attach endpoint to generic service.
- [ ] Migrate web app GitHub PR endpoint usage to generic code-review endpoint.
- [ ] Implement generic code-review attach service.
- [ ] Preserve one remote code review to one Pi Dash issue behavior.
- [ ] Use stable repository id for uniqueness when available.
- [ ] Add GitLab MR parser integration.
- [ ] Add GitLab Merge Requests API snapshot fetch.
- [ ] Normalize GitLab MR state, merged, draft, source branch, and target branch.
- [ ] Add GitHub PR regression tests through generic service.
- [ ] Add GitLab MR attach tests.
- [ ] Add code-review uniqueness tests.
- [ ] Add old GitHub PR route compatibility tests.

## Phase 7 - Webhooks

- [ ] Implement generic webhook delivery model writes.
- [ ] Implement provider webhook route dispatch.
- [ ] Implement raw-body verification path.
- [ ] Implement GitHub webhook compatibility through generic delivery rows.
- [ ] Implement GitLab webhook token/signature verification.
- [ ] Add `GitWebhookRegistration` creation at `repository + provider_account` scope.
- [ ] Add GitLab project hook auto-registration when permissions allow.
- [ ] Add degraded fallback when GitLab hook registration fails.
- [ ] Implement duplicate-event idempotency.
- [ ] Implement fanout from one repository event to all active project bindings for that repository.
- [ ] Implement webhook secret storage.
- [ ] Implement webhook secret rotation plan.
- [ ] Add GitHub webhook regression tests.
- [ ] Add GitLab MR webhook tests.
- [ ] Add invalid signature/token tests.
- [ ] Add duplicate delivery tests.
- [ ] Add webhook fanout tests.

## Phase 8 - Frontend

- [ ] Add generic Git provider account types to `packages/types`.
- [ ] Add generic repository binding types to `packages/types`.
- [ ] Add generic code-review link types to `packages/types`.
- [ ] Add `gitIntegrationService`.
- [ ] Add `projectRepositoryService`.
- [ ] Add `codeReviewService`.
- [ ] Keep GitHub service wrappers for compatibility.
- [ ] Replace GitHub-only workspace integration card with provider account list.
- [ ] Support multiple accounts/installations under one provider.
- [ ] Separate GitLab login/auth settings from GitLab repository integration settings.
- [ ] Replace project GitHub sync panel with generic repository panel.
- [ ] Add provider account picker when more than one account matches.
- [ ] Show clone auth mode and runner-managed private clone status.
- [ ] Replace issue GitHub PR widget with generic code-review widget.
- [ ] Render GitHub labels as pull request.
- [ ] Render GitLab labels as merge request.
- [ ] Update project creation placeholder/copy to avoid GitHub-only examples.
- [ ] Remove stale UI copy saying only github.com URLs are supported.
- [ ] Add frontend tests for GitHub bind.
- [ ] Add frontend tests for GitLab bind.
- [ ] Add frontend tests for account picker.
- [ ] Add frontend tests for code-review widget with PR and MR URLs.

## Phase 9 - CLI, Assistant, and Prompting

- [ ] Add canonical CLI command `pidash issue attach-review` or accepted final name.
- [ ] Keep `pidash issue attach-pr` as alias.
- [ ] Update `attach-pr` help text to accept GitHub PR and GitLab MR URLs.
- [ ] Point CLI attach command to generic code-review endpoint.
- [ ] Add CLI contract test for old GitHub PR behavior.
- [ ] Add CLI contract test for GitLab MR behavior.
- [ ] Add assistant tool `get_code_review_status`.
- [ ] Keep `get_pull_request_status` as alias.
- [ ] Update assistant tool implementation to use provider registry.
- [ ] Add prompt context fields for provider, provider display name, code-review term, host URL, and clone auth mode.
- [ ] Update implementation prompts for GitHub PR flow.
- [ ] Update implementation prompts for GitLab MR flow.
- [ ] Update prompts for unknown provider fallback.
- [ ] Update Pi Dash skill docs for PR/MR attach workflow.
- [ ] Add prompt snapshot tests for GitHub.
- [ ] Add prompt snapshot tests for GitLab.
- [ ] Add prompt snapshot tests for unknown provider.

## Phase 10 - Rollout and Cleanup

- [ ] Ship generic GitHub path behind feature flag.
- [ ] Run one full GitHub sync interval on generic path before retiring legacy reads.
- [ ] Monitor GitHub sync errors, completion comments, webhook processing, and PR link updates.
- [ ] Enable GitLab provider account connect for internal workspace.
- [ ] Validate GitLab repo bind, issue sync, notes sync, completion note, and MR attach in staging.
- [ ] Enable GitLab parity behind workspace or instance flag.
- [ ] Deprecate GitHub-only frontend fetch keys after all call sites move.
- [ ] Deprecate GitHub-only service names after wrappers are no longer used.
- [ ] Decide final retirement plan for legacy `Github*` models.
- [ ] Remove stale GitHub-only copy from docs and UI.
- [ ] Update release notes and admin/operator docs.

## Cross-Cutting Acceptance Checks

- [ ] A workspace can connect GitHub and GitLab provider accounts at the same time.
- [ ] A workspace can connect `gitlab.com` and self-hosted GitLab at the same time.
- [ ] Project A can bind a GitHub repo while Project B binds a GitLab repo in the same workspace.
- [ ] Existing GitHub bind, sync, PR attach, webhook, CLI, assistant, and prompt behavior remains compatible.
- [ ] GitLab open issues sync into Pi Dash.
- [ ] GitLab issue notes sync into Pi Dash comments.
- [ ] GitLab completion notes post back once.
- [ ] GitLab MR URLs attach from web, API, and CLI.
- [ ] GitLab MR status snapshots display correctly.
- [ ] GitLab MR webhook refresh works when configured.
- [ ] Private clone support is clearly reported as runner-managed unless managed clone auth is implemented.
- [ ] No provider API credential is leaked to runner assignment, logs, prompts, or run history.
- [ ] A third provider can be added by implementing an adapter, provider account UI, and tests without duplicating the GitHub-specific stack.
