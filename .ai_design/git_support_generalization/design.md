# Git Support Generalization and GitLab Parity - Design

**Status:** Proposed
**Date:** 2026-06-22
**Repository baseline:** `main` at `b491c41a6ab6e855b912ab10b6e703f09f87326f`, matching `origin/main` after `git fetch origin main --tags`
**Scope:** Generalize repository integrations so GitLab can reach GitHub parity and future Git providers can be added through provider adapters.

## Summary

Pi Dash already has one important provider-neutral primitive: `Project.repo_url`. The runner and orchestration pipeline pass that value through as a generic Git clone URL, and the runner accepts common Git URL schemes.

The rest of the product is still mostly GitHub-shaped. Repository binding, workspace integration status, issue sync, comment sync, pull request attachment, webhook processing, assistant tools, CLI copy, prompts, UI services, and frontend labels all encode GitHub concepts and model names directly. GitLab support exists only for authentication/login. It does not currently support GitLab repository binding, GitLab issue sync, GitLab merge request links, GitLab webhooks, or GitLab completion comments.

The recommended direction is:

1. Keep `Project.repo_url` as the canonical provider-neutral clone URL.
2. Introduce generic Git integration models and services around provider accounts, repository binding, issue sync, comments, code-review links, and webhooks.
3. Put GitHub and GitLab behavior behind provider adapters.
4. Preserve existing GitHub API routes, models, CLI commands, and frontend affordances through compatibility aliases during migration.
5. Add GitLab parity through the same generic services, not by copying the GitHub implementation into parallel GitLab-specific surfaces.

## Goals

- Support GitLab at the same product level as the current GitHub integration:
  - workspace provider account connection and status
  - repository browse and project binding
  - periodic issue and comment import
  - local read-only locks for actively synced remote issues and comments
  - completion comment write-back to the remote issue
  - merge request URL attachment to a Pi Dash issue
  - merge request status snapshots
  - webhook-backed merge request status refresh when configured
  - web, API, CLI, assistant tool, and prompt support
- Generalize repository integrations so adding a third Git provider does not require another end-to-end set of provider-named models, routes, services, and UI components.
- Allow one workspace to connect multiple Git provider accounts or installations at the same time, for example GitHub App plus GitLab token, then bind different projects to different providers.
- Preserve current GitHub behavior for existing projects and agents.
- Make clone authentication explicit. Repository API credentials are not automatically runner clone credentials.

## Non-goals

- Multi-repository projects. The design keeps one active repository binding per project.
- Full two-way issue sync. Remote edits import into Pi Dash, and Pi Dash completion comments can be posted back, but closing or editing upstream issues is not part of this proposal.
- Server-side pull request or merge request creation. Agents can continue creating the code review in the provider and attaching its URL to the issue.
- Immediate removal of existing `Github*` route names, CLI aliases, or model names. Compatibility matters during migration.
- Git provider OAuth login redesign. Current GitLab login and GitHub login are separate from repository integration credentials.

## Current State

### What is already generic

| Area                  | Current state                                                                                                                 |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Project clone URL     | `apps/api/pi_dash/db/models/project.py` has `Project.repo_url` and `base_branch` without provider-specific names.             |
| Prompt context        | `apps/api/pi_dash/prompting/context.py` exposes `repo.url`, `repo.base_branch`, and `repo.work_branch`.                       |
| Run config            | `apps/api/pi_dash/orchestration/service.py` passes `repo_url`, `repo_ref`, and `git_work_branch`.                             |
| Runner assign payload | `runner/src/cloud/protocol.rs` has generic `repo_url`, `repo_ref`, and `git_work_branch`.                                     |
| Runner clone          | `runner/src/workspace/resolve.rs` accepts `https://`, `http://`, `git@`, `ssh://`, and `git://` forms, then delegates to Git. |

This means GitLab clone support may already work for raw repository URLs if the runner has credentials and the URL is usable by Git. The missing product layer is repository integration, sync, and code-review status.

### GitLab support today

| Area                   | Current state                                                                                                                                       | Gap                                                                                           |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| Login/auth             | `apps/api/pi_dash/authentication/provider/oauth/gitlab.py` supports GitLab OAuth login with `read_user`.                                            | Login only. No repo API credential or sync credential.                                        |
| Admin config           | `apps/admin/app/(all)/(dashboard)/authentication/gitlab/form.tsx` exposes GitLab OAuth settings and `ENABLE_GITLAB_SYNC`.                           | The sync flag does not correspond to implemented GitLab repo sync.                            |
| Config registry        | `apps/api/pi_dash/config/registry.py` has `GITLAB_HOST`, `GITLAB_CLIENT_ID`, `GITLAB_CLIENT_SECRET`, `ENABLE_GITLAB_SYNC`, and `IS_GITLAB_ENABLED`. | Repo integration settings are not modeled separately.                                         |
| Account model          | `Account.PROVIDER_CHOICES` and `SocialLoginConnection.medium` include `gitlab`.                                                                     | Social auth provider only.                                                                    |
| Repository integration | None found.                                                                                                                                         | No GitLab repo binding, API client, issue sync, notes sync, MR attach, or webhook processing. |

### GitHub support today

| Area                          | Current implementation                                                                                                                                                                                                                                   | Generalization needed                                                                                                                                                 |
| ----------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| PAT issue sync                | `apps/api/pi_dash/bgtasks/github_sync_task.py` polls open GitHub issues, imports comments, tracks upstream-gone state, and posts completion comments.                                                                                                    | Move to provider-neutral sync engine with adapter methods for issues, comments, and comment creation.                                                                 |
| Sync schedule                 | `apps/api/pi_dash/celery.py` schedules `github-issue-sync-every-4h`.                                                                                                                                                                                     | Replace with `git-issue-sync-every-4h` while preserving a compatibility task name if needed.                                                                          |
| Sync setting                  | `apps/api/pi_dash/settings/common.py` has `GITHUB_SYNC_ENABLED`.                                                                                                                                                                                         | Add provider-neutral setting or per-provider settings.                                                                                                                |
| Workspace integration APIs    | `apps/api/pi_dash/app/views/integration/github.py` handles GitHub PAT connect, repos, status, disconnect, GitHub App install, callbacks, refresh, and webhooks.                                                                                          | Split common workspace integration flow from GitHub adapter details.                                                                                                  |
| Workspace integration storage | `WorkspaceIntegration` is unique per `(workspace, integration)`, which effectively means one row per provider in a workspace.                                                                                                                            | Add Git provider account/install records so a workspace can connect GitHub and GitLab together, multiple GitLab hosts, and eventually multiple accounts per provider. |
| Project repository bind       | `GithubProjectBindEndpoint` uses `parse_github_repo_url()` and rejects non-GitHub URLs with "Only github.com URLs are supported".                                                                                                                        | Generic project repository bind should route by provider parser and adapter.                                                                                          |
| Project repository status     | `GithubProjectStatusEndpoint` reports/toggles/unbinds a GitHub binding.                                                                                                                                                                                  | Generic project repository status should return provider, host, repo, and sync state.                                                                                 |
| GitHub API client             | `apps/api/pi_dash/utils/github_client.py` has GitHub REST calls and GitHub-only URL parsers.                                                                                                                                                             | Keep as GitHub adapter internals. Add GitLab adapter with the same normalized DTOs.                                                                                   |
| GitHub App auth               | `apps/api/pi_dash/utils/github_app_auth.py` handles GitHub App JWT, install tokens, install sessions, and signature verification.                                                                                                                        | Keep as GitHub-specific adapter support. GitLab webhooks use different setup and verification.                                                                        |
| Integration models            | `apps/api/pi_dash/db/models/integration/github.py` defines `GithubRepository`, `GithubRepositorySync`, `GithubIssueSync`, `GithubCommentSync`, `GithubAppInstallation`, `GithubWebhookDelivery`, `GithubAppInstallSession`, and `GithubPullRequestLink`. | Add generic Git models, then migrate or dual-write GitHub rows.                                                                                                       |
| PR attach service             | `apps/api/pi_dash/utils/github_pr_links.py` attaches only GitHub pull request URLs.                                                                                                                                                                      | Replace with generic code-review attachment service. Keep GitHub PR route as alias.                                                                                   |
| PR APIs                       | `apps/api/pi_dash/api/views/github_pr.py` and `apps/api/pi_dash/app/views/issue/github_pr.py` expose GitHub-specific attach endpoints.                                                                                                                   | Add generic `code-reviews` endpoints.                                                                                                                                 |
| Read-only locks               | `apps/api/pi_dash/app/serializers/issue.py`, `apps/api/pi_dash/app/views/issue/base.py`, and `apps/api/pi_dash/app/views/issue/comment.py` check `external_source == "github"` and GitHub sync rows.                                                     | Lock based on generic sync rows and active repository binding.                                                                                                        |
| Assistant tool                | `apps/api/pi_dash/assistant/tools/github.py` exposes `get_pull_request_status`.                                                                                                                                                                          | Add `get_code_review_status` and keep `get_pull_request_status` as a GitHub-compatible alias.                                                                         |
| Prompts                       | `apps/api/pi_dash/prompting/sections/implementation.md` and `apps/api/pi_dash/prompting/fragments/09_implementation.md` mention `gh`, GitHub PRs, and `pidash issue attach-pr`.                                                                          | Make prompts provider-aware.                                                                                                                                          |
| Web project settings          | `apps/web/core/components/project/form.tsx` displays "Git repository URL" but bind/save behavior is GitHub-only.                                                                                                                                         | Make binding generic while preserving verified repository binding semantics.                                                                                          |
| Project creation UI           | `apps/web/core/components/project/create/common-attributes.tsx` has a generic-looking `repo_url` field with a GitHub placeholder.                                                                                                                        | Change placeholder and validation/copy to provider-neutral.                                                                                                           |
| Project sync UI               | `apps/web/core/components/project/settings/github-sync.tsx` is GitHub-specific.                                                                                                                                                                          | Replace with repository integration panel that renders provider-specific labels.                                                                                      |
| Issue PR widget               | `apps/web/core/components/issues/issue-detail/github-pull-requests/root.tsx` accepts GitHub PR URLs only.                                                                                                                                                | Rename to code-review links and accept GitHub PR or GitLab MR URLs.                                                                                                   |
| Frontend services/types       | `apps/web/core/services/integrations/github.service.ts`, `apps/web/core/services/project/project.service.ts`, `apps/web/core/constants/fetch-keys.ts`, and `packages/types/src/integration.ts` use GitHub names.                                         | Add generic services/types with GitHub wrappers kept temporarily.                                                                                                     |
| CLI                           | `runner/src/cli/issue.rs` has `attach-pr` help text and calls `/github/pull-requests/`.                                                                                                                                                                  | Add canonical generic command and route, keep `attach-pr` as alias.                                                                                                   |
| Design docs                   | `.ai_design/github_deep_integration/design.md` and `.ai_design/github_pr_issue_link/design.md` explicitly excluded GitLab/provider abstraction.                                                                                                          | This design supersedes those constraints for the next integration phase.                                                                                              |

## Product Model

### Repository URL

`Project.repo_url` should remain the canonical clone URL and should not become `github_repo_url` or `gitlab_repo_url`. The field is already used by orchestration and the runner as provider-neutral Git input.

The missing distinction is between:

- **Clone URL:** the string the runner uses to clone, stored on `Project.repo_url`.
- **Repository binding:** the verified provider repository associated with a project, including provider, host, namespace, external repository id, default branch, sync settings, and credential.
- **Repository integration credential:** workspace-level or project-level credential used for provider API calls.

Using one field for both GitHub and GitLab is the right direction, but `repo_url` alone is not enough to support sync, comments, webhooks, and code-review status. Those need a binding row and provider adapter metadata.

### Hybrid workspace topology

Hybrid provider use must be a first-class supported shape:

```text
Workspace "Acme"
  GitProviderAccount 1: provider=github, host_url=https://github.com, auth_type=github_app
  GitProviderAccount 2: provider=gitlab, host_url=https://gitlab.com, auth_type=pat
  GitProviderAccount 3: provider=gitlab, host_url=https://gitlab.company.com, auth_type=pat

Project A
  GitRepositoryBinding -> GitHub account, github.com/acme/api

Project B
  GitRepositoryBinding -> GitLab account, gitlab.com/acme/mobile

Project C
  GitRepositoryBinding -> self-hosted GitLab account, gitlab.company.com/platform/worker
```

This means provider connection state must not be modeled as "one Git integration per workspace". `WorkspaceIntegration` can remain for compatibility with current integration pages and API wrappers, but the source of truth for Git repository access should be a provider-neutral account or installation model.

### Provider terms

Pi Dash should use neutral internal terms and render provider-specific copy at the UI edge:

| Neutral concept    | GitHub                                         | GitLab                                                        |
| ------------------ | ---------------------------------------------- | ------------------------------------------------------------- |
| Repository         | Repository                                     | Project                                                       |
| Code review        | Pull request                                   | Merge request                                                 |
| Issue comment      | Comment                                        | Note                                                          |
| Provider account   | PAT or GitHub App installation                 | PAT, group access token, project access token, or OAuth token |
| Repository webhook | GitHub repository webhook or app webhook event | GitLab project hook                                           |

## Proposed Backend Model

Add generic Git integration models. Exact names can follow existing naming conventions, but the shape should be provider-neutral.

### `GitProviderAccount`

Represents one workspace-level provider account, app installation, or token.

Fields:

- `workspace`
- `provider`: enum, initially `github` and `gitlab`
- `host_url`: normalized host root, for example `https://github.com`, `https://gitlab.com`, or `https://gitlab.company.com`
- `auth_type`: enum such as `github_app`, `pat`, `oauth`, `group_token`, or `project_token`
- `external_account_id`: provider account, org, user, group, or installation id when available
- `external_account_login`: display login or namespace when available
- `display_name`
- `capabilities`: JSON or structured flags, for example `read_repositories`, `read_issues`, `write_comments`, `manage_webhooks`, and `clone`
- `credential_config`: encrypted provider-specific credential material or references
- `workspace_integration`: optional compatibility FK to the existing `WorkspaceIntegration`
- `status`: `connected`, `degraded`, `revoked`, or `error`
- `verified_at`
- `last_check_error`
- `metadata`

Identity rules:

- The table allows multiple active rows per workspace and provider.
- `host_url` is part of identity so `gitlab.com` and self-hosted GitLab are distinct.
- `external_account_id` should be used for uniqueness when the provider returns it.
- Do not rely on `WorkspaceIntegration` uniqueness for Git identity. It is too coarse for hybrid use.

Examples:

```text
workspace=acme, provider=github, host_url=https://github.com, auth_type=github_app, external_account_id=12345
workspace=acme, provider=gitlab, host_url=https://gitlab.com, auth_type=pat, external_account_id=9988
workspace=acme, provider=gitlab, host_url=https://gitlab.company.com, auth_type=pat, external_account_id=42
```

### `GitRepository`

Represents a remote repository/project known to Pi Dash.

Fields:

- `provider`: enum, initially `github` and `gitlab`
- `host_url`: normalized host root, for example `https://github.com` or `https://gitlab.com`
- `external_id`: provider repository id as string
- `namespace`: owner, group, or subgroup path
- `name`: repository/project slug
- `full_name`: `namespace/name`
- `web_url`: canonical browser URL
- `clone_url_http`
- `clone_url_ssh`
- `default_branch`
- `is_private`
- `metadata`: provider-specific JSON

Unique key:

- `(provider, host_url, external_id)`

Fallback unique key before external id is known:

- `(provider, host_url, full_name)`

### `GitRepositoryBinding`

Represents a project binding to one remote repository.

Fields:

- `workspace`
- `project`
- `repository`
- `provider_account`
- `created_by`
- `is_sync_enabled`
- `clone_auth_mode`: `runner_managed`, `public`, or future `managed_ephemeral`
- `last_synced_at`
- `last_sync_error`
- `metadata`
- `is_active`

Constraints:

- one active binding per project
- optional uniqueness on `(workspace, project, repository)` for active rows

This replaces the project-facing role of `GithubRepositorySync`. It also makes the project setting "repository URL" a verified repository binding rather than a GitHub-specific action.

`provider_account` is required for issue sync, comment write-back, merge request or pull request status, and webhook management. A project can store a raw `Project.repo_url` without a binding only if the product intentionally supports clone-only mode, but that mode should be visibly separate from provider integration.

### `GitIssueSync`

Maps a remote provider issue to a Pi Dash issue.

Fields:

- `binding`
- `issue`
- `provider`
- `external_id`: provider-global issue id as string when available
- `external_iid`: provider-local issue number or iid as string
- `web_url`
- `remote_state`
- `remote_created_at`
- `remote_updated_at`
- `metadata`

Unique key:

- `(binding, external_iid)`

For GitHub, `external_iid` is the issue number. For GitLab, `external_iid` is the project-local issue `iid`, while `external_id` can hold the global issue `id`.

### `GitCommentSync`

Maps remote comments/notes to Pi Dash comments.

Fields:

- `issue_sync`
- `comment`
- `provider`
- `external_id`
- `remote_created_at`
- `remote_updated_at`
- `metadata`

Unique key:

- `(issue_sync, external_id)`

### `GitCodeReviewLink`

Represents a pull request, merge request, or equivalent code review attached to a Pi Dash issue.

Fields:

- `issue`
- `provider`
- `host_url`
- `namespace`
- `repo_name`
- `repo_external_id`
- `external_id`: provider-global PR/MR id when available
- `external_iid`: GitHub PR number or GitLab MR iid
- `url`
- `title`
- `state`
- `merged`
- `draft`
- `remote_updated_at`
- `metadata`
- `is_active`

Unique key:

- `(provider, host_url, repo_external_id, external_iid)` for active links when `repo_external_id` is known

Fallback unique key before repository verification:

- `(provider, host_url, namespace, repo_name, external_iid)` for active links

Current `GithubPullRequestLink` is global across projects. The generic model can preserve that behavior by keeping the unique key global. If multi-tenant isolation is preferred later, include `workspace` in the key, but that changes existing semantics.

Use stable repository ids for identity whenever possible. Namespace and repository names are display metadata and lookup hints, not durable identity, because repositories can be renamed or transferred.

### `GitWebhookDelivery`

Tracks webhook processing across providers.

Fields:

- `provider`
- `host_url`
- `delivery_id`: string because providers do not share id formats
- `event`
- `action`
- `repository`
- `raw_headers`
- `payload`
- `status`
- `received_at`
- `processed_at`
- `error`

Unique key:

- `(provider, host_url, delivery_id)` when the provider supplies a stable delivery id

For GitLab, if delivery ids are unavailable or not stable in all deployment modes, the handler can derive an id from timestamp plus payload hash.

### `GitWebhookRegistration`

Optional but useful for GitLab and future providers.

Fields:

- `repository`
- `provider_account`
- `provider_hook_id`
- `events`
- `secret_ref` or encrypted secret
- `last_verified_at`
- `last_check_error`
- `metadata`

GitHub App webhooks may remain installation-level. GitLab project hooks are commonly project-level, but registration should still be owned by `repository + provider_account`, not by a single project binding. Multiple Pi Dash projects can bind the same remote repository, and duplicate provider hooks would create duplicate events. Bindings subscribe to normalized repository events inside Pi Dash.

## Provider Adapter Interface

Add a provider registry and adapter protocol under a generic module such as `apps/api/pi_dash/integrations/git/`.

```python
class GitProviderAdapter(Protocol):
    key: str
    display_name: str
    code_review_term: str

    def parse_repo_url(self, url: str) -> ParsedRepository | None:
        ...

    def parse_code_review_url(self, url: str) -> ParsedCodeReview | None:
        ...

    def verify_provider_account(self, credential: GitCredential) -> ProviderIdentity:
        ...

    def credential_capabilities(self, credential: GitCredential) -> GitProviderCapabilities:
        ...

    def list_repositories(self, credential: GitCredential, page: int = 1) -> RepositoryPage:
        ...

    def get_repository(self, credential: GitCredential, parsed: ParsedRepository) -> RemoteRepository:
        ...

    def list_open_issues(self, credential: GitCredential, repository: RemoteRepository) -> Iterable[RemoteIssue]:
        ...

    def list_issue_comments(
        self,
        credential: GitCredential,
        repository: RemoteRepository,
        issue_iid: str,
    ) -> Iterable[RemoteComment]:
        ...

    def post_issue_comment(
        self,
        credential: GitCredential,
        repository: RemoteRepository,
        issue_iid: str,
        body: str,
    ) -> RemoteComment:
        ...

    def get_code_review(self, credential: GitCredential, parsed: ParsedCodeReview) -> RemoteCodeReview:
        ...

    def normalize_webhook(self, raw_body: bytes, headers: Mapping[str, str]) -> ProviderWebhookEvent:
        ...
```

Normalized DTOs should avoid provider-specific names:

- `ParsedRepository(provider, host_url, namespace, name, full_name, clone_url)`
- `RemoteRepository(provider, external_id, full_name, web_url, clone_url_http, clone_url_ssh, default_branch, is_private, metadata)`
- `GitProviderCapabilities(read_repositories, read_issues, write_comments, manage_webhooks, clone)`
- `RemoteIssue(external_id, external_iid, title, body, state, author, web_url, created_at, updated_at, metadata)`
- `RemoteComment(external_id, body, author, web_url, created_at, updated_at, metadata)`
- `ParsedCodeReview(provider, host_url, namespace, repo_name, external_iid, url)`
- `RemoteCodeReview(external_id, external_iid, title, state, merged, draft, web_url, updated_at, metadata)`
- `ProviderWebhookEvent(provider, event, action, repository_ref, code_review_ref, issue_ref, payload)`

Adapter rules:

- Provider-specific URL parsing stays inside adapters.
- Provider-specific authentication stays inside adapters.
- Sync and API views consume normalized DTOs only.
- New providers register one adapter plus optional frontend labels and credential UI.

## Runner Clone Authentication

Repository API authentication and runner clone authentication are different concerns.

Current runner behavior:

- the server sends only `repo_url`, `repo_ref`, and `git_work_branch` in the assignment payload
- the runner invokes `git clone -- <repo_url> <target>`
- no provider API credential or GitHub App installation token is sent to the runner

Therefore, binding a private GitHub or GitLab repository does not automatically guarantee the runner can clone it. The design must make this explicit instead of treating it as an implementation detail.

### Initial clone-auth policy

Initial Git generalization should use:

```text
clone_auth_mode = runner_managed
```

Meaning:

- Pi Dash stores the canonical repository URL and provider binding.
- Pi Dash uses the provider account credential for API sync, comments, webhooks, and code-review status.
- The runner machine is responsible for Git clone credentials, such as SSH keys, Git credential manager, netrc, or preconfigured HTTPS credentials.
- For private repositories, the UI should show that the connected provider account verifies API access, but runner clone access still depends on runner-side Git credentials.

This preserves current runner protocol and avoids sending long-lived provider tokens to runners.

### Future managed clone-auth option

If the product requires Pi Dash-managed private clone access, add a separate phase and wire-protocol change:

- add a short-lived `clone_credential` or `clone_auth_ref` to runner assignment
- prefer short-lived provider tokens or deploy keys over long-lived PATs
- redact credentials from logs, prompts, status payloads, and run history
- teach the runner to pass credentials through a temporary credential helper instead of embedding secrets in `repo_url`
- expire and revoke credentials after the run when possible

This is intentionally separate from GitLab issue/MR parity. Without it, GitLab parity means "same as current GitHub: Pi Dash can bind/sync/status via provider API, while private clone access is runner-managed."

## GitLab Adapter Design

### Supported hosts

Start with:

- `https://gitlab.com`
- self-managed GitLab hosts from an explicit instance-admin or ops-controlled allowlist

GitLab API base is:

```text
<host_url>/api/v4
```

For self-managed GitLab, avoid arbitrary host SSRF by using an allowlist from instance/admin or deployment configuration. Do not let any workspace user-provided URL become an API base without validation. Pi Dash Cloud should default to `gitlab.com` only; customer/self-managed GitLab hosts should be added by operations or instance administration before workspace users can connect credentials or bind repositories for that host. Provider API calls should require HTTPS and should not follow redirects; a redirect from an allowlisted host to a different target must fail closed.

### URL parsing

Repository URLs:

```text
https://gitlab.com/group/project
https://gitlab.com/group/subgroup/project
https://gitlab.com/group/subgroup/project.git
git@gitlab.com:group/project.git
git@gitlab.com:group/subgroup/project.git
ssh://git@gitlab.example.com/group/subgroup/project.git
```

Merge request URLs:

```text
https://gitlab.com/group/project/-/merge_requests/123
https://gitlab.com/group/subgroup/project/-/merge_requests/123
```

Issue URLs:

```text
https://gitlab.com/group/project/-/issues/123
https://gitlab.com/group/subgroup/project/-/issues/123
```

GitLab namespaces can have variable depth. The parser should keep the full path up to `/-/` or the final project segment. Verification should use the Projects API and store the numeric project id once resolved.

### Credentials

Initial GitLab parity should use token-based provider accounts:

- personal access token
- group access token
- project access token

The GitLab REST API accepts access tokens with the `PRIVATE-TOKEN` header. OAuth bearer tokens are also supported, but they should be a later enhancement unless the product wants a GitLab App-style install flow from day one.

GitLab credentials should be stored as `GitProviderAccount` rows, not as one coarse workspace-level `gitlab` integration row. This allows the same workspace to connect `gitlab.com`, self-hosted GitLab, and eventually more than one GitLab account.

Required scopes:

- `api` for full parity, because Pi Dash needs to read issues and merge requests, write completion notes, and potentially create or update project webhooks
- `read_api` only supports read-only browsing and sync
- `read_repository` and `write_repository` are for Git-over-HTTP repository access and do not replace API access

The connection UI should make the required scope explicit. If a token only has read-only capability, Pi Dash can allow browse/sync but must disable completion comment write-back and webhook auto-registration with a clear status message.

These API scopes do not imply runner clone access. For private GitLab repositories, the binding should record `clone_auth_mode = runner_managed` unless a future managed clone-auth feature is implemented.

### Repository browse and bind

Use GitLab Projects API to list visible projects and verify a parsed repository path.

Binding flow:

1. Parse URL with the GitLab adapter.
2. Select or require a matching `GitProviderAccount` by provider and host.
3. URL-encode the full namespace/project path for the Projects API.
4. Fetch project metadata.
5. Store `GitRepository.external_id` as the numeric GitLab project id.
6. Store canonical `web_url`, `http_url_to_repo`, `ssh_url_to_repo`, and `default_branch`.
7. Save `Project.repo_url` to the chosen canonical clone URL.
8. Create or update `GitRepositoryBinding` pointing to the selected `GitProviderAccount`.
9. Optionally create or verify GitLab project hook registration at the repository/account level.

### Issue sync

GitLab Issues API exposes project issues. The sync adapter should request open issues for the bound project.

Mapping:

| Pi Dash                     | GitLab                                                                         |
| --------------------------- | ------------------------------------------------------------------------------ |
| `GitIssueSync.external_iid` | issue `iid`                                                                    |
| `GitIssueSync.external_id`  | issue `id`                                                                     |
| `Issue.title`               | issue `title` with Pi Dash prefix policy applied                               |
| `Issue.description`         | issue `description`                                                            |
| `Issue.external_source`     | keep legacy field as `gitlab` if needed, but generic sync row is authoritative |
| `Issue.external_id`         | issue `iid` or provider-specific compatibility value                           |

Import comments from GitLab Notes API:

- Use issue notes endpoints.
- Skip system notes initially to avoid importing status-transition noise.
- Store note `id` as `GitCommentSync.external_id`.
- Prefix imported local comments with `[GitLab]` to match the existing GitHub pattern.

Completion comment:

- Use GitLab Notes API to create an issue note.
- Preserve the existing one-shot completion comment semantics.

### Merge request support

GitLab merge request URLs use project-local `iid` values. The adapter should parse `/-/merge_requests/<iid>`, fetch the merge request, and normalize:

| Generic field            | GitLab source                                                         |
| ------------------------ | --------------------------------------------------------------------- |
| `external_iid`           | merge request `iid`                                                   |
| `external_id`            | merge request `id`                                                    |
| `title`                  | `title`                                                               |
| `state`                  | `state`, usually `opened`, `closed`, `locked`, or `merged`            |
| `merged`                 | `state == "merged"` or merge metadata                                 |
| `draft`                  | `draft` when available, or title/work-in-progress markers as fallback |
| `remote_updated_at`      | `updated_at`                                                          |
| `metadata.source_branch` | `source_branch`                                                       |
| `metadata.target_branch` | `target_branch`                                                       |

The UI can display "Merge request" for GitLab while the API stores it as a generic code-review link.

### Webhooks

GitLab project hooks should be configured for:

- issue events, if Pi Dash wants push-based issue updates later
- merge request events, for attached MR status refresh
- note events, if comment sync becomes webhook-based later

Auto-registration requires sufficient token permissions. If hook creation fails, binding should still succeed with a degraded status:

- polling sync still works
- attach-time MR snapshot still works
- MR status may become stale until the next poll or manual refresh

GitLab webhook verification differs from GitHub. The adapter should normalize verification and event extraction so generic webhook views can dispatch on provider.

Webhook verification flow:

1. Receive the raw body and headers without mutating them.
2. Identify provider from the route and host/account hints in the payload or hook URL.
3. Find candidate `GitWebhookRegistration` rows by provider, repository, and provider account.
4. Verify provider-specific signature or token against the stored secret.
5. Reject the event before JSON-side effects if verification fails.
6. Compute an idempotency key from provider delivery id when available, otherwise from provider, event type, repository id, object id, action, timestamp, and payload hash.
7. Store `GitWebhookDelivery` before processing.
8. Normalize the event and fan it out to active `GitRepositoryBinding` rows for the repository.

Secret handling:

- Store webhook secrets encrypted or by reference to the existing secret store.
- Rotate by creating a new secret, updating the provider hook, accepting both old and new secrets during a short grace window, then retiring the old secret.
- Never expose webhook secrets in API responses or frontend payloads.

## Generic Services

### Repository binding service

Create a provider-neutral service that backs both API and web app views.

Responsibilities:

- detect provider from URL and configured host allowlist
- select a matching `GitProviderAccount`, or require `provider_account_id` when more than one account can access the same host
- verify the selected provider account for that provider and host
- fetch repository metadata
- upsert `GitRepository`
- create/update `GitRepositoryBinding`
- set `Project.repo_url` to the canonical clone URL
- optionally set `Project.base_branch` from provider default branch if empty
- return a generic binding DTO

The current GitHub project bind route should call this service with provider detection and the workspace's GitHub account selection. The new generic route should be the preferred entrypoint.

### Sync service

Replace `github_sync_task.py` with a provider-neutral sync task:

- `sync_all_bindings()`
- `sync_one_binding(binding_id)`
- `post_completion_comment(issue_id)`

Behavior to preserve:

- scheduled polling every 4 hours
- import open remote issues
- skip GitHub pull requests that appear in the GitHub issues API
- import remote comments
- mark upstream-gone state for previously synced issues no longer present in open results
- prevent local edits/deletes on actively synced issues and comments
- post completion comment once

Generalized behavior:

- issue prefix becomes `[<provider>_<iid>]`, for example `[github_123]` or `[gitlab_123]`
- comment prefix uses adapter display name, for example `[GitHub]` or `[GitLab]`
- sync keys are provider plus binding plus external iid/id
- read-only locks check generic sync rows instead of GitHub-only rows

### Code-review link service

Replace GitHub-only PR attach logic with a generic code-review service.

Responsibilities:

- parse URL across all registered adapters
- verify the remote code review exists
- ensure one remote code review is attached to only one Pi Dash issue, preserving current GitHub semantics unless product changes that intentionally
- upsert `GitCodeReviewLink`
- provide status DTOs for web and assistant tools
- refresh links from webhook events

Keep:

- old GitHub PR attach endpoint as compatibility alias
- `pidash issue attach-pr` as compatibility alias
- old `get_pull_request_status` assistant tool as alias

Add:

- generic API route and service name
- `pidash issue attach-review` or `pidash issue attach-code-review`
- assistant tool `get_code_review_status`

## Backend API Shape

Add generic routes while keeping old GitHub routes available.

Workspace integration:

```text
GET    /api/workspaces/<workspace_slug>/integrations/git/providers/
GET    /api/workspaces/<workspace_slug>/integrations/git/accounts/
POST   /api/workspaces/<workspace_slug>/integrations/git/accounts/
GET    /api/workspaces/<workspace_slug>/integrations/git/accounts/<account_id>/
DELETE /api/workspaces/<workspace_slug>/integrations/git/accounts/<account_id>/
GET    /api/workspaces/<workspace_slug>/integrations/git/accounts/<account_id>/repos/
```

Provider-scoped compatibility routes can remain:

```text
GET    /api/workspaces/<workspace_slug>/integrations/git/<provider>/status/
POST   /api/workspaces/<workspace_slug>/integrations/git/<provider>/connect/
POST   /api/workspaces/<workspace_slug>/integrations/git/<provider>/disconnect/
GET    /api/workspaces/<workspace_slug>/integrations/git/<provider>/repos/
```

Those routes should resolve to one default account only when unambiguous. If multiple accounts exist for the provider, they should return an explicit account-selection error rather than choosing silently.

Account disconnect semantics:

- Disconnecting or revoking a `GitProviderAccount` should mark dependent bindings as degraded, not silently unbind projects.
- Issue sync, comment write-back, webhook management, and status refresh pause while the account is degraded.
- Runner clone behavior is unchanged for `runner_managed` clone auth because clone credentials live on the runner.
- A workspace admin can either reconnect the account, rebind affected projects to another account, or explicitly unbind the repository.

Project repository:

```text
GET    /api/workspaces/<workspace_slug>/projects/<project_id>/repository/
POST   /api/workspaces/<workspace_slug>/projects/<project_id>/repository/bind/
PATCH  /api/workspaces/<workspace_slug>/projects/<project_id>/repository/
DELETE /api/workspaces/<workspace_slug>/projects/<project_id>/repository/
```

Bind request body:

```json
{
  "repo_url": "https://gitlab.com/acme/mobile",
  "provider_account_id": "optional-uuid"
}
```

If `provider_account_id` is omitted, the server may auto-select only when exactly one active account matches the parsed provider and host.

Bind response should include:

- selected `provider_account_id`
- provider and host
- repository stable id and full name
- canonical clone URL saved to `Project.repo_url`
- `clone_auth_mode`
- sync status and degraded reasons, if any

Issue code reviews:

```text
GET    /api/workspaces/<workspace_slug>/issues/<issue_id>/code-reviews/
POST   /api/workspaces/<workspace_slug>/issues/<issue_id>/code-reviews/
DELETE /api/workspaces/<workspace_slug>/issues/<issue_id>/code-reviews/<link_id>/
```

External API:

```text
GET    /api/v1/work-items/<issue_id>/code-reviews/
POST   /api/v1/work-items/<issue_id>/code-reviews/
```

Webhooks:

```text
POST   /api/integrations/git/<provider>/webhook/
```

Compatibility aliases:

```text
POST   /api/v1/work-items/<issue_id>/github/pull-requests/
POST   /api/workspaces/<workspace_slug>/issues/<issue_id>/github-pull-requests/
```

Compatibility aliases should return the same response shape as today where possible, with generic fields added rather than breaking existing clients.

## Frontend Design

### Workspace settings

Replace provider-specific GitHub-only settings with a Git provider integrations list:

- GitHub
- GitLab
- future providers

Each provider card should show:

- connection status
- credential type
- token capability status, for example full access vs read-only
- repository count if available
- disconnect action
- setup instructions for required scopes

Provider cards should support more than one connected account or installation per provider. For example, the GitLab area can show `gitlab.com` and `gitlab.company.com` as separate account rows.

GitLab authentication/login settings should remain separate from GitLab repository integration settings. A user being able to log in with GitLab does not mean Pi Dash can sync GitLab repositories.

### Project settings

Rename the GitHub sync section to a generic repository section.

Recommended UI behavior:

- Field label: `Repository URL`
- Placeholder examples should include GitHub and GitLab, or avoid provider-specific placeholder text.
- Primary action: `Bind repository`
- Binding detects GitHub/GitLab from URL.
- If multiple matching provider accounts exist, the user chooses which account to bind through.
- After binding, show provider badge, host, full name, default branch, sync enabled state, last sync time, and last error.
- For private repositories, show clone access as `Runner-managed` unless managed clone credentials are implemented.
- Sync toggle operates on `GitRepositoryBinding.is_sync_enabled`.
- Unbind removes the binding and releases generic sync locks.

Do not silently save an unverified changed `repo_url` through the normal project settings save path unless the product intentionally supports "raw clone URL only" mode. Today, the project settings form looks generic but the real verified save path is GitHub bind. Keeping verification explicit avoids stale sync metadata.

### Issue detail

Rename the GitHub pull request widget to code reviews or pull requests / merge requests.

Behavior:

- Accept GitHub PR URLs and GitLab MR URLs.
- Render provider-specific labels:
  - GitHub: Pull request
  - GitLab: Merge request
- Show status, merged state, draft state, and last refreshed time.
- Preserve existing GitHub list behavior for current users.

### Frontend services and types

Add generic services and types:

- `gitIntegrationService`
- `projectRepositoryService`
- `codeReviewService`
- `IGitProvider`
- `IGitProviderAccount`
- `IGitRepositoryBinding`
- `IGitCodeReviewLink`

Keep old GitHub service methods as wrappers until all call sites move.

## CLI, Agent, and Prompting

### CLI

Current:

```text
pidash issue attach-pr --url <github-pr-url>
```

Add canonical command:

```text
pidash issue attach-review --url <provider-code-review-url>
```

or:

```text
pidash issue attach-code-review --url <provider-code-review-url>
```

Keep `attach-pr` as an alias and update help text to say it accepts GitHub pull request URLs and GitLab merge request URLs.

### Assistant tools

Current tool:

- `get_pull_request_status`

Add:

- `get_code_review_status`

The new tool should:

- parse URL through the provider registry
- find matching repository binding and provider account
- fetch status through the adapter
- return provider, term, state, merged, draft, title, URL, and updated time

Keep `get_pull_request_status` as an alias that calls the generic tool.

### Prompt context

Add provider-aware fields to prompt context:

```json
{
  "repo": {
    "url": "...",
    "provider": "gitlab",
    "provider_display_name": "GitLab",
    "code_review_term": "merge request",
    "host_url": "https://gitlab.com",
    "clone_auth_mode": "runner_managed",
    "base_branch": "main",
    "work_branch": "..."
  }
}
```

Prompt guidance should branch by provider:

- GitHub: use `gh` when available, create or find a pull request, then attach the PR URL.
- GitLab: use `glab` when available, create or find a merge request, then attach the MR URL.
- Unknown provider: create the provider's code review through the available provider workflow, then attach the URL.

The completion payload can keep `pr_url` for compatibility, but docs should define it as a code-review URL. Prefer adding `code_review_url` and treating `pr_url` as an alias.

## Migration Plan

### Phase 1 - Registry and parsers

- Add provider registry and normalized DTOs.
- Move GitHub URL parsers behind a GitHub adapter.
- Add GitLab URL parsers.
- Add parser tests for:
  - GitHub HTTPS and SSH repo URLs
  - GitHub PR URLs
  - GitLab HTTPS and SSH repo URLs
  - GitLab subgroup repo URLs
  - GitLab MR URLs
  - unsupported hosts

No production behavior changes in this phase.

### Phase 2 - Generic models

- Add `GitProviderAccount`, `GitRepository`, `GitRepositoryBinding`, `GitIssueSync`, `GitCommentSync`, `GitCodeReviewLink`, `GitWebhookDelivery`, and optional `GitWebhookRegistration`.
- Write data migration from existing `Github*` rows to generic rows with `provider = "github"` and `host_url = "https://github.com"`.
- Decide whether to dual-read/dual-write temporarily or switch reads to generic rows with old models retained for rollback.

Migration field mapping:

| Current model/field                                             | Generic target                                                                                                                                       |
| --------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `WorkspaceIntegration` for GitHub PAT                           | `GitProviderAccount(provider=github, host_url=https://github.com, auth_type=pat, credential_config.token)`                                           |
| `GithubAppInstallation`                                         | `GitProviderAccount(provider=github, host_url=https://github.com, auth_type=github_app, external_account_id=installation_id, metadata.installation)` |
| `GithubRepository.repository_id`                                | `GitRepository.external_id`                                                                                                                          |
| `GithubRepository.owner` + `GithubRepository.name`              | `GitRepository.namespace`, `GitRepository.name`, `GitRepository.full_name`                                                                           |
| `GithubRepository.url`                                          | `GitRepository.web_url` and candidate canonical clone URL source                                                                                     |
| `GithubRepository.config`                                       | `GitRepository.metadata`                                                                                                                             |
| `GithubRepositorySync`                                          | `GitRepositoryBinding(provider_account, repository, project, actor, is_sync_enabled, last_synced_at, last_sync_error)`                               |
| `GithubIssueSync.repo_issue_id`                                 | `GitIssueSync.external_iid`                                                                                                                          |
| `GithubIssueSync.github_issue_id`                               | `GitIssueSync.external_id`                                                                                                                           |
| `GithubIssueSync.issue_url`                                     | `GitIssueSync.web_url`                                                                                                                               |
| `GithubIssueSync.metadata.completion_comment_id`                | `GitIssueSync.metadata.completion_comment_id`                                                                                                        |
| `GithubIssueSync.metadata.upstream_gone_at`                     | `GitIssueSync.metadata.upstream_gone_at`                                                                                                             |
| `GithubIssueSync.gh_issue_created_at`                           | `GitIssueSync.remote_created_at`                                                                                                                     |
| `GithubIssueSync.gh_issue_updated_at`                           | `GitIssueSync.remote_updated_at`                                                                                                                     |
| `GithubCommentSync.repo_comment_id`                             | `GitCommentSync.external_id`                                                                                                                         |
| `GithubPullRequestLink.repo_owner/repo_name/pr_number`          | `GitCodeReviewLink(namespace, repo_name, external_iid)`                                                                                              |
| `GithubPullRequestLink.title/state/merged/draft/pr_updated_at`  | `GitCodeReviewLink.title/state/merged/draft/remote_updated_at`                                                                                       |
| `GithubWebhookDelivery.delivery_id/event/action/payload/status` | `GitWebhookDelivery(provider=github, delivery_id, event, action, payload, status)`                                                                   |

Migration invariants:

- Do not repost completion comments after migration.
- Do not release read-only locks for actively synced GitHub issues or comments.
- Preserve upstream-gone metadata.
- Preserve active PR link uniqueness.
- Preserve soft-delete semantics for unbound repositories and links.
- Ensure all compatibility routes can read migrated rows before deleting legacy rows.
- Keep rollback possible until generic GitHub sync has run successfully in production for at least one full sync interval.

### Phase 3 - Generic repository binding API

- Add generic project repository endpoints.
- Implement generic binding service using the GitHub adapter first.
- Point existing GitHub bind/status/toggle/unbind endpoints at the generic service.
- Update frontend project settings to consume generic binding DTOs.
- Keep old GitHub frontend service wrappers.

### Phase 4 - Generic sync engine for GitHub

- Implement provider-neutral sync task.
- Use GitHub adapter and generic sync rows.
- Preserve current behavior for issue import, comment import, upstream-gone marking, local read-only locks, and completion comment write-back.
- Replace the Celery beat route with a generic task.
- Keep a GitHub task alias if external operations reference the task name.

### Phase 5 - GitLab repository and issue parity

- Add GitLab provider account connect/status/repos/disconnect.
- Add GitLab repository bind.
- Add GitLab issue import.
- Add GitLab notes import.
- Add GitLab completion comment write-back.
- Add read-only lock coverage for GitLab-synced issues and comments.

### Phase 6 - GitLab merge request parity

- Add generic code-review endpoints.
- Migrate GitHub PR UI and APIs to generic code-review service.
- Add GitLab MR attach and snapshot fetch.
- Add GitLab project webhook registration at `repository + provider_account` scope when token permissions allow it.
- Add GitLab MR webhook event handling and status refresh.

### Phase 7 - Agent, CLI, and prompt updates

- Add generic CLI attach command.
- Keep `attach-pr` as alias.
- Add `get_code_review_status` assistant tool.
- Keep `get_pull_request_status` as alias.
- Update prompts and Pi Dash skill docs to describe PR/MR provider behavior.

### Phase 8 - Cleanup

- Remove or deprecate old GitHub-only frontend services and fetch keys after all call sites move.
- Decide whether legacy `Github*` models remain as compatibility views, unmanaged models, or are removed after migration confidence.
- Remove stale copy such as "Only github.com URLs are supported".

## Testing Plan

Backend unit tests:

- provider registry dispatch
- provider account selection, including multiple accounts for one workspace
- GitHub URL parsing through adapter
- GitLab URL parsing, including nested subgroups
- unsupported host rejection
- credential capability interpretation
- normalized DTO mapping

Backend service tests:

- generic repository bind with GitHub adapter
- generic repository bind with GitLab adapter
- repository bind fails with account-selection error when multiple matching accounts exist and no account id is provided
- sync upsert by provider and external iid
- upstream-gone marking
- completion comment write-back
- read-only lock enforcement for GitHub and GitLab sync rows
- code-review attach uniqueness
- old GitHub PR attach route compatibility

HTTP adapter tests:

- GitLab Projects API list and get project
- GitLab Issues API pagination
- GitLab Notes API import and create note
- GitLab Merge Requests API fetch
- GitLab project hook create or degraded fallback
- GitHub adapter regression coverage around current client behavior

Webhook tests:

- GitHub App pull request event still updates generic link
- GitLab merge request event updates generic link
- invalid signatures or tokens are rejected
- duplicate deliveries are idempotent
- one provider hook registration can fan out to multiple active Pi Dash project bindings for the same remote repository

Migration tests:

- GitHub PAT workspace integration migrates to `GitProviderAccount`
- GitHub App installation migrates to `GitProviderAccount`
- GitHub repository sync rows preserve sync enabled, last synced, and errors
- completion comment idempotency metadata survives
- upstream-gone metadata survives
- existing PR links keep uniqueness and snapshots

Runner/clone tests:

- private repo binding can report `clone_auth_mode = runner_managed`
- runner assignment does not leak provider API credentials
- future managed clone-auth payloads are redacted if that phase is implemented

Frontend tests:

- workspace provider cards for GitHub and GitLab
- project repository bind accepts GitHub URL
- project repository bind accepts GitLab URL
- project sync toggle uses generic binding API
- issue code-review widget accepts GitHub PR URL
- issue code-review widget accepts GitLab MR URL

CLI tests:

- `attach-pr` still works for GitHub PR URL
- `attach-pr` accepts GitLab MR URL if kept as broad alias
- `attach-review` posts to generic endpoint

Prompt tests:

- GitHub project prompt mentions pull request flow
- GitLab project prompt mentions merge request flow
- provider unknown prompt avoids GitHub-only commands

## Open Decisions

1. **Credential strategy:** Start with GitLab PAT/group/project tokens or implement OAuth repo integration first. Recommendation: start with token-based credentials to match current GitHub PAT sync and reduce initial scope.
2. **Webhook registration:** Auto-create GitLab project hooks when permissions allow or require manual setup. Recommendation: attempt auto-create, then fall back to polling with a clear warning.
3. **Managed clone authentication timing:** Keep clone auth runner-managed for initial parity or add Pi Dash-managed short-lived clone credentials in the same project. Recommendation: keep it runner-managed initially and make the UI explicit.
4. **Raw clone URL mode:** Allow `Project.repo_url` to save without a provider binding or require verified bind for settings edits. Recommendation: keep verified bind as the default. Add explicit raw clone-only mode only if needed.
5. **Global link uniqueness:** Preserve current GitHub behavior where one remote PR maps to one Pi Dash issue globally, or scope by workspace. Recommendation: preserve current behavior during migration.
6. **GitLab confidential issues:** Sync all visible open issues by default or skip confidential issues unless opted in. Recommendation: start with visible issues returned by the token and add an explicit setting later if needed.
7. **GitLab system notes:** Import system notes or skip them. Recommendation: skip system notes initially to avoid noisy local comments.

## Acceptance Criteria

GitLab reaches parity when:

- A workspace admin can connect a GitLab API token.
- A workspace can have GitHub and GitLab provider accounts connected at the same time.
- Different projects in the same workspace can bind repositories from different providers.
- Pi Dash can list GitLab projects visible to that token.
- A project can bind to a GitLab repository URL using the same repository URL field used for GitHub.
- A GitLab-bound project stores a canonical `Project.repo_url` usable by the runner.
- Private repository clone access is either verified as runner-managed in the UI or supported by an explicit managed clone-auth feature.
- Open GitLab issues sync into Pi Dash.
- GitLab issue notes sync into Pi Dash comments.
- Actively synced GitLab issues and comments are protected from local edits/deletes.
- Completing a synced Pi Dash issue can post a completion note back to GitLab.
- A GitLab merge request URL can be attached to a Pi Dash issue from web, API, and CLI.
- Merge request status can be fetched and displayed.
- GitLab merge request webhooks refresh attached link status when configured.
- Existing GitHub bind, sync, PR attach, webhook, CLI, and assistant behavior remains compatible.
- A third provider can be added by implementing a provider adapter, provider settings UI, and tests without cloning the full GitHub-specific stack.

## Official GitLab References

- GitLab REST API authentication: https://docs.gitlab.com/api/rest/authentication/
- GitLab access token scopes: https://docs.gitlab.com/security/tokens/access_token_scopes/
- GitLab Projects API: https://docs.gitlab.com/api/projects/
- GitLab Issues API: https://docs.gitlab.com/api/issues/
- GitLab Notes API: https://docs.gitlab.com/api/notes/
- GitLab Merge Requests API: https://docs.gitlab.com/api/merge_requests/
- GitLab Project Webhooks API: https://docs.gitlab.com/api/project_webhooks/
