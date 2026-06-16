# GitHub App Enablement — Design

**Status:** Implementation started — foundation slice
**Date:** 2026-06-16
**Scope:** Foundation only: enable Pi Dash as a **GitHub App**, let a signed-in
user install it from profile settings, safely bind that installation to an
explicit Pi Dash workspace where the user is an admin, and verify the connection
end to end. Targets **both** hosted multi-tenant and self-hosted single-org Pi
Dash deployments. **No automatic issue/comment/PR sync ships in this design**;
sync behavior is deliberately deferred to a separate future design.

---

## 1. Problem

Today Pi Dash connects to GitHub in three shallow ways:

- **OAuth App** — login/SSO only; the user token is used to read identity, then
  discarded. It is never reused for data access.
- **Personal Access Token (PAT)** — the only credential used for data. Pasted
  once per workspace, stored encrypted in `WorkspaceIntegration.config["token"]`
  (`apps/api/pi_dash/app/views/integration/github.py`), and consumed by a single
  REST client (`apps/api/pi_dash/utils/github_client.py`).
- **A 4-hour polling sync** — `sync_all_repos` →​ `sync_one_repo`
  (`apps/api/pi_dash/bgtasks/github_sync_task.py`) does a full scan per binding,
  importing **open issues + comments** into Pi Dash. The only write-back is a
  one-shot "completed in Pi Dash" comment
  (`post_completion_comment`, triggered by `bgtasks/github_signals.py`).

This is enough to mirror issues, but it cannot support the long-term goal:

> Make Pi Dash a layer on top of GitHub that boosts software-development
> efficiency — the developer's whole loop (plan → code → review → ship) lives in
> Pi Dash while GitHub remains the execution substrate.

The current model has four hard ceilings:

1. **No real-time.** Polling every 4 hours means Pi Dash is always stale; there
   is no way to react to a PR opening, a check failing, or a review landing.
2. **No PR awareness.** The sync explicitly skips pull requests
   (`if "pull_request" in gh_issue: continue`,
   `github_sync_task.py:301`). There is no model linking a Pi Dash issue to a PR.
3. **Identity is a person.** Every action is attributed to whoever pasted the
   PAT; when they leave or rotate the token, sync silently breaks.
4. **Read-mostly.** Content flows GitHub → Pi Dash; the only reverse signal is a
   comment. State, PR actions, and checks do not flow at all.

All four are properties of the **PAT auth model**, not of any one feature.
Removing them eventually requires a different foundation: a GitHub App. This
design only builds that foundation and proves that the connection works; it does
not change how issues or comments sync yet.

## 2. Goals

- **Adopt a GitHub App installation as a workspace credential.** Bot identity,
  per-installation scope, admin-approved repository access. PAT remains the
  active data-sync credential until a later sync design replaces it.
- **Install and verify end to end.** A signed-in Pi Dash user can install the App
  from their profile settings, Pi Dash safely binds the GitHub installation to an
  explicit workspace where that user is an admin, mints an installation token,
  verifies repository access, and reports a clear connected/error status in the
  UI.
- **Work in both deployment modes.** Hosted multi-tenant (one App, many
  installations) and self-hosted single-org (each instance registers its own
  App). Same code paths; configuration differs.
- **Reuse existing infrastructure.** `WorkspaceIntegration` for per-tenant
  install state, `InstanceConfiguration` for per-instance App secrets, the
  `encrypt_data`/`decrypt_data` Fernet utilities, and the `GithubClient` shape.
  No new secret store, no new queue.
- **Preserve PAT sync exactly as-is.** Existing PAT bindings and the 4-hour
  issue/comment polling loop keep working. App adoption must not clobber the
  PAT config or silently switch a repo to a new sync path.
- **Lay groundwork without committing sync policy.** Store enough installation
  metadata and webhook-delivery evidence that a later, more conservative sync
  design can build on it without redesigning auth.

## 3. Non-Goals (this design)

- **No automatic issue/comment sync changes.** The existing PAT-backed 4-hour
  poll remains the only issue/comment sync path. GitHub App webhooks for
  `issues` and `issue_comment` are ignored/skipped in this design.
- **No PR/check/review sync.** PR awareness, check status, review state, and
  issue-to-PR linkage are deferred to a new future design.
- **No bidirectional state writes.** Pi Dash does not close GitHub issues/PRs,
  assign GitHub issues, or publish statuses/checks in this design.
- **No reconciler changes.** Do not demote or rename the existing 4-hour
  `github-issue-sync-every-4h` task yet; its cadence remains a PAT-sync concern.
- **No runner/autonomous-PR work.** Runner-created branches, draft PR creation,
  and run-completion integration are deferred until after a sync/PR design
  exists.
- **No GitHub Enterprise _Server_ (GHES) support.** First, a distinction this
  doc otherwise invites readers to confuse: self-hosted **Pi Dash** talking to
  `github.com` **is** supported (the "self-hosted single-org" mode in §11). What
  is deferred is self-hosted **GitHub** — a GHES box on a customer host like
  `github.acme-corp.internal`. We support `github.com` and GitHub-hosted
  Enterprise **Cloud** (also `github.com`-hosted), matching the current parser
  (`parse_github_repo_url`) and the `GITHUB_API_BASE` constant in
  `utils/github_client.py`.

  GHES is deferred — not because it's hard, but because it's pure host-URL
  configurability that **changes nothing in the App architecture**: the App
  model, token minting, and webhook verification are identical; only the API base
  (`https://<host>/api/v3`), App-registration endpoints, and webhook origin
  differ. That's exactly what makes it a safe, additive later change. Deferring
  avoids (a) doubling the verification matrix against a license-gated GHES
  instance during the foundational build, (b) the narrower-than-it-looks network
  topology (GHES usually lives inside a corporate network — only the self-hosted
  Pi-Dash-plus-GHES-same-network combination is reachable), and (c) the ongoing
  cost of feature-gating by GHES version, which lags `github.com`.

  **To keep the later add cheap, this foundation must centralize the API base URL** —
  resolve every call's host through one place (extend `GITHUB_API_BASE` into a
  per-installation host field) instead of scattering `https://api.github.com`
  literals. Done now, GHES becomes a config change later, not a refactor.

- **No removal of PAT.** PAT stays as a fallback and for the migration window.
- **No GitLab/Bitbucket.** Provider abstraction is noted as a seam but not built.
- **No frontend visual design.** UI surfaces are enumerated (what data appears
  where) but pixel/interaction design is out of scope.
- **No CI-check authoring or reading.** CI/check/review handling belongs to the
  deferred PR design.

## 4. Background — what we build on

This design slots into existing systems rather than replacing them. The key
anchors (verified in code):

| System                        | Where                                                                                                                                         | What we reuse                                                                                                                  |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Per-tenant integration row    | `WorkspaceIntegration` (`db/models/integration/base.py`) — `config`/`metadata` JSON, `unique_together = [workspace, integration]`             | Keep the workspace/provider container and legacy PAT config; anchor the App install with a one-to-one `GithubAppInstallation`. |
| Per-instance config + secrets | `InstanceConfiguration` (`license/models/instance.py`), `get_configuration_value()` (`license/utils/instance_value.py`), admin PATCH endpoint | Store the App **id / private key / webhook secret** here, but with a write-only/redacted secret serializer (§6.4).             |
| Encryption                    | `license/utils/encryption.py` — Fernet key derived from `SECRET_KEY` via PBKDF2                                                               | Encrypt the App private key and webhook secret at rest.                                                                        |
| REST client                   | `utils/github_client.py` — PAT-auth, paginated, typed errors                                                                                  | Generalize enough to mint/use an installation token for connection verification.                                               |
| Sync tasks + Beat             | `bgtasks/github_sync_task.py`, `celery.py` beat `github-issue-sync-every-4h`                                                                  | Leave unchanged. PAT polling remains the only issue/comment sync path for now.                                                 |
| State-transition hook         | `bgtasks/github_signals.py` (pre/post-save on `Issue`)                                                                                        | Leave unchanged. No new Pi Dash → GitHub state writes in this design.                                                          |
| Outbound webhooks             | `db/models/webhook.py`, `bgtasks/webhook_task.py` — HMAC-SHA256 signing                                                                       | Reuse the **HMAC verification pattern** (inverted) for _inbound_ GitHub webhooks.                                              |

**Critical gap:** there is **no inbound webhook receiver anywhere** in the
codebase today. The existing webhook system is outbound-only. Building a
verified inbound receiver is still required for GitHub App installation lifecycle
events, but this design only processes install/repository-selection events; it
does not process issue/comment/PR events.

## 5. Why a GitHub App (vs PAT / OAuth)

| Capability            | PAT (today)                | OAuth App        | **GitHub App (proposed)**                   |
| --------------------- | -------------------------- | ---------------- | ------------------------------------------- |
| Acts as               | a user                     | a user           | **its own bot identity** (`pi-dash[bot]`)   |
| Webhooks              | none (manual per-repo)     | none             | **native, per-installation**                |
| Permissions           | all-or-nothing user scopes | user scopes      | **granular, per-resource, admin-approved**  |
| Rate limit            | 5k/hr/user, flat           | 5k/hr/user       | **scales with org size (15k+/hr)**          |
| Token lifecycle       | manual paste + rotate      | refresh tokens   | **auto-minted, 1-hour installation tokens** |
| Survives staff change | no                         | no               | **yes (org-owned install)**                 |
| Install/uninstall UX  | paste a token              | per-user consent | **admin installs on selected repos**        |

The decisive factor for this design is **installation identity**: the workspace
connection belongs to a GitHub App installation, not to the human who pasted a
PAT. Webhooks matter here only for App lifecycle events (`ping`,
`installation`, `installation_repositories`). Issue/comment/PR webhooks are
future sync inputs, not part of this implementation.

## 6. Auth architecture

### 6.1 The three-token dance

A GitHub App authenticates in three escalating forms; we need all three:

1. **App JWT** — short-lived (≤10 min), signed with the App's **private key**
   (RS256), `iss = App ID`. Identifies _the App itself_. Used only to call the
   App-level endpoints below.
2. **Installation token** — minted by `POST /app/installations/{id}/access_tokens`
   using the App JWT. Scoped to one installation, expires in **1 hour**, carries
   the granted permissions on the granted repos. In this design it is used only
   for connection verification and repository-access checks; future sync designs
   may use it as the data-plane token.
3. **User-to-server token** — OAuth-on-the-App. In this foundation it is used
   only during setup binding verification, because GitHub explicitly warns that
   the installation callback's `installation_id` can be spoofed. Pi Dash uses this token
   ephemerally to confirm the installing GitHub user can access the returned
   installation, then discards it. Long-lived user tokens and attribution-sensitive
   writes are out of scope.

### 6.2 Token-minting service (new)

A small service module — proposed `utils/github_app_auth.py` — owns:

- `build_app_jwt()` — load the App private key (decrypted from config), sign an
  RS256 JWT. Requires adding `PyJWT[crypto]` (or reusing `cryptography`, already
  a dependency via Fernet) to the API.
- `installation_token(installation_id)` — mint (or return cached) installation
  token. **Cache** in Redis keyed by `installation_id` with TTL = expiry − 60s
  (Valkey/Redis is already provisioned). One mint per installation per ~hour
  keeps us far under rate limits.
- `revoke_cache(installation_id)` — drop on uninstall / permission change.

### 6.3 Generalizing `GithubClient`

`GithubClient` currently takes a raw `token` and hard-codes
`Authorization: Bearer {token}`. The change is minimal and backward-compatible:

- Keep the `token=` constructor for current PAT sync callers.
- Add a classmethod `for_installation(installation_id)` that pulls a fresh
  installation token from §6.2 and constructs the client. Internally identical
  REST surface — the only difference is **where the bearer comes from**.
- Add read-only connection-check methods needed by the install flow, e.g.
  `get_installation`, `list_installation_repositories`, and `get_repo`.
  Write methods (`close_pull_request`, `merge_pull_request`,
  `update_issue_state`, `request_review`, etc.) are deferred.
- **Centralize the API base URL.** `GITHUB_API_BASE` is currently a module-level
  constant pointing at `https://api.github.com`. Make the host an instance
  property resolved through one place (defaulting to `github.com`) rather than a
  hard-coded literal scattered across calls. This design only resolves it to
  `github.com`, but doing this now is what makes GHES (§3) a config change later
  instead of a refactor.

This keeps one REST wrapper; current PAT sync keeps using its existing token
path, while the App path is exercised only by install verification.

### 6.4 Where credentials live (both deployment modes)

```
InstanceConfiguration (per deployment, encrypted via Fernet)
  ├─ GITHUB_APP_ID            (plaintext)
  ├─ GITHUB_APP_SLUG          (plaintext; for install URLs)
  ├─ GITHUB_APP_PRIVATE_KEY   (is_encrypted=True, WRITE-ONLY — see below)
  ├─ GITHUB_APP_WEBHOOK_SECRET(is_encrypted=True, WRITE-ONLY)
  └─ GITHUB_APP_CLIENT_ID/SECRET (is_encrypted, WRITE-ONLY; setup verification only)

WorkspaceIntegration.config (per workspace/provider — legacy PAT credential only)
  {
    "auth_type": "pat",
    "token": "<fernet-ciphertext>", # legacy / fallback — preserved, not clobbered
    "github_user_login": "...",
    "verified_at": "..."
  }

GithubAppInstallation (per workspace — durable App installation state)
  workspace_integration -> OneToOneField(WorkspaceIntegration)
  installation_id       -> BigIntegerField(unique=True, db_index=True)
  account_login         -> CharField
  account_type          -> CharField      # User | Organization
  repository_selection  -> CharField      # all | selected
  repository_count      -> IntegerField
  permissions           -> JSONField
  events                -> JSONField
  installed_at, suspended_at, verified_at, last_checked_at, last_check_error
```

The App **secret** is always instance-level; the **installation** is always
workspace-level. This split is what makes both deployment modes the same code.

**⚠️ App secrets must NOT be served by the existing config endpoint.** Today
`InstanceConfigurationEndpoint.get` returns _all_ rows
(`license/api/views/configuration.py:37`) and
`InstanceConfigurationSerializer.to_representation` _decrypts_ every
`is_encrypted` value on read (`license/api/serializers/configuration.py:18`).
That path already exposes `EMAIL_HOST_PASSWORD` in plaintext to any instance
admin (and caches it for 2h) — so the §14 "treat it like `EMAIL_HOST_PASSWORD`"
framing is exactly backwards: that _is_ the leak. `GITHUB_APP_PRIVATE_KEY` /
`GITHUB_APP_WEBHOOK_SECRET` must be **write-only**: accept on PATCH, never
return on GET. Required change — exclude these keys from the generic serializer
(or give them a redacted `to_representation` that emits a `"set" / "unset"`
sentinel instead of the value). This is a current-scope prerequisite, not a polish
item.

### 6.4.1 PAT and App credentials must coexist

There is exactly **one** `WorkspaceIntegration` row per `(workspace, "github")`
(`db/models/integration/base.py:56`), and today's sync resolves the credential
**only** from `config["token"]` (`bgtasks/github_sync_task.py:53`). The App
installation state therefore belongs in a sibling `GithubAppInstallation` row,
not as a replacement for the PAT config. Two rules fix this:

1. **Never clobber.** App adoption creates/updates `GithubAppInstallation` and
   **leaves any existing PAT config intact**. Do not migrate or reshape today's
   flat `config["token"]` PAT storage in this foundation.
2. **Do not change sync credential selection yet.** `_resolve_token`
   (`github_sync_task.py:53`) continues to read the PAT path for the existing
   polling sync. The App installation token is used only by connection
   verification in this design.

A future sync design can introduce per-binding credential selection: prefer the
App installation if the repo is covered by it, else fall back to the PAT. That
resolver is deliberately not part of this implementation so installing the App
cannot unexpectedly change how any repo syncs.

### 6.5 Installation handshake and workspace binding

An `installation` webhook by itself is **not enough** to bind a GitHub
installation to a Pi Dash workspace. GitHub's installation callback includes an
`installation_id`, but that query parameter is user-controllable from Pi Dash's
point of view; treating it as proof would let a spoofed callback bind the wrong
installation to a workspace. Hosted multi-tenant therefore needs an explicit
install session:

1. A user opens **Profile Settings -> Integrations**, chooses the target Pi Dash
   workspace, and clicks "Install GitHub App". The target workspace selector only
   lists workspaces where the user has admin permission.
2. Pi Dash creates a `GithubAppInstallSession` row with
   `workspace_id`, `actor_id`, a random `state`/nonce, `expires_at`, and
   `status="started"`.
3. Pi Dash sends the user to
   `https://github.com/apps/{app_slug}/installations/new?state=<nonce>`.
   The GitHub App registration must enable **Request user authorization (OAuth)
   during installation** and use Pi Dash's callback URL:
   `/api/integrations/github/app/callback/`. GitHub then returns `state`,
   `installation_id`, and `code` to that callback.
4. The callback requires the same user's normal Pi Dash session, validates the
   `state` row, and re-checks that the actor is still a workspace admin for the
   stored `workspace_id`.
5. Pi Dash treats the callback `installation_id` as untrusted until GitHub proves
   it belongs to the installing GitHub user:
   - generate an ephemeral GitHub App user-to-server token for the callback user;
   - call the user-installation endpoint and confirm the returned installation set
     contains the callback `installation_id`;
   - discard the user token immediately after this verification.
6. After user-installation verification passes, use the App JWT to fetch the
   installation from GitHub and verify the returned account/repository selection.
7. Only after that verification does Pi Dash create/update the workspace's
   `GithubAppInstallation` row, linked to its `WorkspaceIntegration`, and mark
   the install session `completed`.

Identity rule: Pi Dash and GitHub identities are deliberately not matched by
email address. The Pi Dash user must be signed in and authorized as an admin of
the selected Pi Dash workspace; the GitHub user must be able to access the
returned GitHub App installation. Those are two separate authorization checks.
It is valid for `user.email` in Pi Dash to differ from the installing GitHub
account email/login.

Webhook timing is deliberately decoupled from this binding flow. GitHub may
deliver the `installation.created` webhook before the callback completes. In
that case the receiver persists the delivery and marks it `skipped`, because no
verified `installation_id -> workspace` mapping exists yet. It **does not create
or attach** a `WorkspaceIntegration` row.
After the installation callback completes, subsequent installation events route by the
verified `installation_id -> workspace` mapping in our database.

The existing `WorkspaceIntegration` model requires `actor` and `api_token` FKs.
For App installs, use the installing admin as `actor` and create the same
inactive APIToken shim used by the PAT connect path; if a future background-only
repair path must create the row, it must first resolve a deterministic workspace
system actor instead of leaving those FKs implicit.

### 6.5.1 Install-session expiry and purge

`GithubAppInstallSession` is a short-lived CSRF/workspace-binding proof, not a
durable integration record. The durable record is `GithubAppInstallation`, linked
to the workspace's `WorkspaceIntegration`.

Rules:

- Set `expires_at = created_at + 15 minutes` when the install session is created.
- The installation callback must reject an expired session, mark it `expired`, and never
  create/update `GithubAppInstallation`.
- Use **lazy cleanup only** for the foundation; do not add a Celery Beat fallback
  yet. Run a best-effort, bounded purge from the install-session start endpoint
  and the installation callback:
  - mark any `started` session with `expires_at < now()` as `expired`;
  - hard-delete terminal sessions (`completed`, `expired`, `failed`) older than
    7 days;
  - log updated/deleted counts by status;
  - catch and log cleanup failures without failing the user-facing install flow.
- The lazy purge must be idempotent. If one request fails midway, the remaining
  rows still match the same status/timestamp predicates, so the next install
  start or callback can pick them up.
- Add indexes for `state` (unique), `status`, and `expires_at` so callback lookup
  and cleanup do not scan the table.

Do **not** purge `GithubAppInstallation` or `WorkspaceIntegration` as part of this
task. Uninstall/suspend lifecycle events update the durable install state; they
are not install-session cleanup.

## 7. Inbound webhook receiver (new infrastructure)

The largest net-new piece. Design mirrors the runner long-poll plane's
discipline: verify at the edge, persist, ack fast, process async.

### 7.1 Endpoint

A single public endpoint on the authenticated app API mount:
`POST /api/integrations/github/app/webhook/`. The endpoint itself must be
unauthenticated by the normal Pi Dash session/API-key mechanisms (GitHub won't
send either) and instead authenticated by **signature**.

### 7.2 Verification (reuse the HMAC pattern, inverted)

GitHub signs each delivery with `X-Hub-Signature-256: sha256=<hmac>` over the
raw body, keyed by the App's webhook secret. The outbound webhook code
(`bgtasks/webhook_task.py:313`) already does the _forward_ of this exact
construction; we invert it:

```python
# GitHub sends the header as "sha256=<hexdigest>" and signs the RAW body bytes.
# Build our digest with the same prefix and compare the full strings — do NOT
# compare a bare hexdigest against the prefixed header.
received = request.headers.get("X-Hub-Signature-256", "")
expected = "sha256=" + hmac.new(webhook_secret.encode(), raw_body, hashlib.sha256).hexdigest()
if not received or not hmac.compare_digest(expected, received):  # constant-time
    reject_401()
```

Two correctness traps to call out for the implementer: (a) the header carries
the `sha256=` prefix — compare prefixed-to-prefixed (or strip it from both); a
missing/empty header must reject, not pass. (b) `raw_body` must be the exact
bytes GitHub sent — capture it _before_ DRF parses/re-serializes the request
(any re-encode breaks the digest). Reject on mismatch with `401` before any
parsing. Resolve `webhook_secret` from `InstanceConfiguration` (self-hosted) or
the single hosted App secret.

### 7.3 Persist-then-process bounded lifecycle events

1. Verify signature on the **raw** body (must read body before DRF parses it).
2. Persist a `GithubWebhookDelivery` row (dedupe on `X-GitHub-Delivery` UUID —
   GitHub retries deliveries; we must be idempotent).
3. Process only the bounded lifecycle events inline in the request:
   `ping`, `installation`, and `installation_repositories`; return `202` after
   status is recorded. Do **not** add a Celery task for this foundation.
4. Any other delivery is persisted and marked `skipped` with no side effects.

### 7.4 Handler routing

| Event                                    | Action                                                                                                                              |
| ---------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `ping`                                   | Mark the App/webhook connection as reachable; useful for setup diagnostics.                                                         |
| `installation` (created/deleted/suspend) | If a verified binding exists, enable/disable install state and evict tokens. If no binding exists yet, mark the delivery `skipped`. |
| `installation_repositories`              | Update install metadata/repository count for connection status only.                                                                |
| anything else                            | Mark `skipped`; issue/comment/PR/check sync is out of scope.                                                                        |

The GitHub App should subscribe only to the lifecycle events above for this
implementation. If GitHub sends another event anyway, Pi Dash stores enough
delivery evidence for debugging and returns `202`, but it does not mutate issues,
comments, PR records, or project state.

## 8. Data model changes

Current-scope rows; existing GitHub sync models (`GithubRepository`,
`GithubRepositorySync`, `GithubIssueSync`, `GithubCommentSync`) are unchanged
and continue to serve only the PAT-backed polling sync.

```
GithubWebhookDelivery (new)
  delivery_id (UUID, unique)   # X-GitHub-Delivery — idempotency key
  event        (str)           # X-GitHub-Event
  action       (str)
  installation_id (bigint, null)
  payload      (JSON)
  status       (received | processed | failed | skipped)
  received_at, processed_at, error

GithubAppInstallation (new)
  workspace_integration -> OneToOneField(WorkspaceIntegration)
  installation_id       -> BigIntegerField(unique=True, db_index=True)
  account_login         -> CharField
  account_type          -> CharField      # User | Organization
  repository_selection  -> CharField      # all | selected
  repository_count      -> PositiveIntegerField(default=0)
  permissions           -> JSONField(default=dict)
  events                -> JSONField(default=list)
  installed_at          -> DateTimeField(null=True)
  suspended_at          -> DateTimeField(null=True)
  verified_at           -> DateTimeField(null=True)
  last_checked_at       -> DateTimeField(null=True)
  last_check_error      -> TextField(blank=True)
  indexes               (installation_id unique, workspace_integration unique)

GithubAppInstallSession (new)
  state       (str, unique)      # random setup nonce; never trust installation_id alone
  workspace   (FK Workspace)
  actor       (FK User)          # installing admin
  installation_id (bigint, null)
  account_login   (str, blank)
  status      (started | completed | expired | failed)
  expires_at, completed_at, error
  indexes     (state unique, status, expires_at)
```

`GithubAppInstallation` carries the current connection status:

```json
{
  "installation_id": 12345678,
  "account_login": "acme-corp",
  "repository_selection": "selected",
  "repository_count": 12,
  "installed_at": "...",
  "verified_at": "...",
  "last_checked_at": "...",
  "last_check_error": "",
  "suspended_at": null
}
```

No `GithubPullRequestSync`, `GithubOutboundOperation`, or issue-linkage tables
ship in this design. Those belong to the future sync/PR design.

## 9. End-to-end connection verification

The success criterion for this design is not "issues sync in real time." It is:
a user can install the App from profile settings, Pi Dash can prove it has a
valid installation credential for the selected workspace, and the UI can show
whether the connection is healthy.

Connection flow:

1. Instance admin configures or creates the GitHub App.
2. User starts an install session from Profile Settings -> Integrations (§6.5),
   selects the target Pi Dash workspace, and installs the App on all or selected
   GitHub repositories/accounts.
3. Callback validates `state`, exchanges the OAuth `code`, confirms the GitHub
   user can access the returned `installation_id`, fetches the installation
   using the App JWT, creates/updates `GithubAppInstallation`, and marks the
   install session complete.
4. Pi Dash immediately runs a connection check:
   - mint installation token;
   - fetch the installation/account;
   - list installation repositories or fetch the selected repository metadata;
   - write `verified_at`, `last_checked_at`, `repository_count`, and
     `last_check_error`.
5. The settings UI shows one of: not configured, install started, connected,
   connected but suspended, missing permissions, token-mint failed, or webhook
   unreachable.

UI placement:

- The in-app GitHub App install entry point lives in **Profile Settings ->
  Integrations** (`/settings/profile/integrations`). This profile tab does not
  exist today; add it to the profile settings tab constants, route map, sidebar,
  and content map.
- The Profile Settings GitHub card owns "Install GitHub App", "Reconnect", and
  "Refresh connection". It includes a required target-workspace selector before
  starting a new install session, then displays the connected GitHub
  account/installation, target Pi Dash workspace, selected-repository summary or
  count, `verified_at`, `last_checked_at`, and the latest connection error.
- **Workspace Settings -> Integrations** can retain the legacy PAT connection UI
  and may show a read-only pointer to Profile Settings -> Integrations for the
  App install flow, but it is not the primary App install surface.
- Project-level GitHub settings remain consumers of the workspace connection.
  They may show a read-only "GitHub App not connected" state with a link back to
  Profile Settings -> Integrations, but this foundation must not add new
  project-level App sync toggles or automatic issue/comment sync controls.
- The existing PAT-backed project binding UI remains unchanged until a separate
  sync design decides how App-backed repository binding should coexist with
  `GithubRepositorySync`.

The same connection check backs a manual "Refresh connection" action. It is a
diagnostic and permission probe only; it does not create, update, close, or
comment on Pi Dash issues or GitHub issues.

## 10. Deferred sync design

Issue/comment/PR synchronization is intentionally out of scope and should get a
new design before implementation. That future design must decide the conservative
sync policy explicitly instead of inheriting "webhook event means mutate local
state" by default.

Future design questions:

- Which projects/repos opt into App-backed sync, and how does that coexist with
  existing PAT-backed `GithubRepositorySync` rows?
- What content is authoritative on each side: title/body/comments/state/labels/
  assignees/milestones?
- Are GitHub issue/comment webhooks used at all, or do we keep a polling-first
  model with a manual "sync now" action and webhooks only as invalidation hints?
- What is the backfill story for existing mirrored issues and comments?
- How do we prevent duplicate mirrors when a workspace has both PAT and App
  credentials?
- If Pi Dash ever writes back, what is the transaction boundary, idempotency key,
  and user/bot attribution model?

Until that design exists, the current PAT sync stays exactly as it is:
`sync_all_repos` runs every 4 hours, `sync_one_repo` imports open issues and
comments, and `github_signals.py` only posts the existing one-shot completion
comment.

## 11. Deployment modes — one design, two configs

### Hosted multi-tenant

- **One** GitHub App, registered by Pi Dash, owned by the vendor org.
- App id / private key / webhook secret live in the hosted instance's
  `InstanceConfiguration` (or env) — set once.
- Each customer org **installs** the App from a Pi-Dash-created install session
  (§6.5). The verified installation callback creates/updates the workspace's
  `GithubAppInstallation`; the raw `installation` webhook alone never creates the
  binding.
- One public webhook URL for all tenants; the handler routes by
  verified `installation_id` → workspace mapping.

### Self-hosted single-org

- Each instance **registers its own** GitHub App (via GitHub's **App manifest
  flow** — a one-click create that returns id + private key + webhook secret to a
  callback, far better self-host UX than manual field entry).
- Those secrets are written to _that instance's_ `InstanceConfiguration`
  (encrypted and write-only/redacted on read). Admin UI lives next to the
  existing GitHub OAuth config screen in `apps/admin`.
- Webhook URL is the instance's own public origin. Self-host docs must note the
  instance has to be reachable by GitHub (the one genuinely new operational
  requirement vs. PAT polling, which needed no inbound connectivity).

The **only** differences are _who registers the App_ and _which origin receives
webhooks_. Storage, token minting, verified install binding, lifecycle handlers,
and connection checks are identical — both read App secrets from
`InstanceConfiguration` and installs from `WorkspaceIntegration`.

## 12. Future runner / autonomous PRs (deferred)

Not designed here. The only foundation this design intentionally provides for
runner autonomy is durable GitHub App auth: the cloud can later mint an
installation token for a verified workspace installation. Everything else needs
the future sync/PR design first.

- `AgentRun.run_config` already carries `repo_url`, `repo_ref`,
  `git_work_branch`; the `Assign` protocol message
  (`runner/src/cloud/protocol.rs`) already ships a branch to check out, and
  `runner/src/workspace/git.rs` already clones, branches, and manages worktrees.
- The runner already pushes branches; it does **not** open PRs today.
- A future runner design may use the App installation token to open draft PRs,
  but it must first define issue/PR linkage, state ownership, and write-back
  idempotency in the separate sync/PR design.

Do not implement issue → runner → draft PR as part of this foundation.

## 13. Current and future roadmap

| Stage                                    | Deliverable                                                                                                                                                                                                                                                                                                       | Gates unlocked                                                             |
| ---------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| **Current — App enablement**             | App registration (manifest flow for self-host; manual for hosted), write-only/redacted secret storage in `InstanceConfiguration`, token-minting service (§6.2), `GithubClient.for_installation`, verified install-session flow (§6.5), inbound lifecycle webhook receiver (§7), and connection verification (§9). | The App can be installed, safely bound to a workspace, and proven healthy. |
| **Future design — conservative sync**    | Separate design for issue/comment sync policy: opt-in surface, authority rules, webhook-vs-polling posture, PAT/App coexistence, backfill, duplicate prevention, and failure handling (§10).                                                                                                                      | App-backed sync can be implemented without surprising users.               |
| **Future design — PR/check/state layer** | Separate design for PR linkage, checks/reviews, bidirectional state writes, outbound operation idempotency, and attribution.                                                                                                                                                                                      | PR-aware workflows can be added safely.                                    |
| **Future design — runner autonomy**      | Separate design for issue → runner → branch/PR workflows using the verified App installation.                                                                                                                                                                                                                     | Autonomous PR workflows.                                                   |

Only the **Current — App enablement** stage is in scope for this document.

## 14. Security considerations

- **Private key** is the crown jewel — encrypted at rest (Fernet), never logged,
  and never returned by any API. The existing generic configuration serializer
  decrypts encrypted values on read, so App secret keys must be excluded from
  that serializer or represented only as redacted `set` / `unset` sentinels.
- **Webhook signature** verified constant-time on the raw body before parsing;
  unsigned/mismatched → `401`, no side effects.
- **Delivery idempotency** via `X-GitHub-Delivery` to survive GitHub's retries.
- **Installation token scope** is the least-privilege boundary — request only the
  permissions this foundation needs. Current scope should not request Issues,
  Pull requests, or Checks write permissions; future sync designs can justify
  additional grants explicitly.
- **Tenant isolation** (hosted): always resolve `installation_id` → workspace
  from our own verified install-session records; never trust a workspace id from
  the payload or a bare callback query parameter.
- **No issue side effects from webhooks.** Non-lifecycle webhook events are
  persisted/skipped only; they cannot mutate Pi Dash issues or comments.

## 15. Implementation defaults

1. **Minimal permission set** — ship with the mandatory GitHub App metadata access
   plus lifecycle webhooks only (`ping`, `installation`,
   `installation_repositories`). Do not request Issues, Pull requests, Checks, or
   Contents permissions in this foundation.
2. **Install visibility** — Profile Settings shows GitHub account/login,
   repository selection mode, repository count, verification timestamps, and
   errors. Do not show the full selected-repository list until a later sync design
   needs it.
3. **PAT deprecation timeline** — keep PAT support indefinitely for now. Revisit
   only after a separate App-backed sync design ships and has a migration plan.
4. **Self-host reachability** — require a publicly reachable callback/webhook URL
   or an operator-provided tunnel/relay for GitHub App setup. Do not add a
   polling-only degraded mode in this foundation.

---

## Appendix A — File-level impact map

| Area               | File(s)                                                                                                                                                                                                                                                                                                                                                                                                                                          | Change                                                                                                                                                              |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| App auth           | `apps/api/pi_dash/utils/github_app_auth.py` _(new)_                                                                                                                                                                                                                                                                                                                                                                                              | JWT + installation-token minting, Redis cache.                                                                                                                      |
| REST client        | `apps/api/pi_dash/utils/github_client.py`                                                                                                                                                                                                                                                                                                                                                                                                        | `for_installation()` classmethod; read-only connection-check methods.                                                                                               |
| Webhook receiver   | `apps/api/pi_dash/app/views/integration/github.py`, `apps/api/pi_dash/app/urls/integration.py`                                                                                                                                                                                                                                                                                                                                                   | Signed inbound endpoint + URL registration; unauthenticated-by-session view method.                                                                                 |
| Webhook processing | `apps/api/pi_dash/app/views/integration/github.py`                                                                                                                                                                                                                                                                                                                                                                                               | Bounded inline event router for `ping`, `installation`, `installation_repositories`; skip everything else.                                                          |
| Models             | `apps/api/pi_dash/db/models/integration/github.py`                                                                                                                                                                                                                                                                                                                                                                                               | `GithubAppInstallation`, `GithubWebhookDelivery`, `GithubAppInstallSession` + migration, including install-session expiry indexes.                                  |
| Lazy cleanup       | `apps/api/pi_dash/app/views/integration/github.py`                                                                                                                                                                                                                                                                                                                                                                                               | Best-effort request-triggered cleanup for stale/terminal `GithubAppInstallSession` rows; no Celery Beat fallback in the foundation.                                 |
| Existing PAT sync  | `apps/api/pi_dash/bgtasks/github_sync_task.py`, `apps/api/pi_dash/celery.py`, `apps/api/pi_dash/bgtasks/github_signals.py`                                                                                                                                                                                                                                                                                                                       | No behavior change. Keep polling cadence and completion comment as-is.                                                                                              |
| Instance config    | `apps/api/pi_dash/utils/instance_config_variables/core.py`, `apps/api/pi_dash/license/api/serializers/configuration.py`, future `apps/admin/...` GitHub config screen                                                                                                                                                                                                                                                                            | App id/key/secret config variables; redacted secret readback; manifest-flow callback/admin UI can follow.                                                           |
| Install state      | `apps/api/pi_dash/app/views/integration/github.py`                                                                                                                                                                                                                                                                                                                                                                                               | Install-session start/callback, PAT/App coexistence, verified binding.                                                                                              |
| Types/UI           | `packages/types/src/integration.ts`, `packages/types/src/settings.ts`, `packages/constants/src/settings/profile.ts`, `apps/web/core/components/settings/profile/content/pages/index.ts`, `apps/web/core/components/settings/profile/content/pages/integrations.tsx` _(new)_, optional legacy pointer in `apps/web/app/(all)/[workspaceSlug]/(settings)/settings/(workspace)/integrations/page.tsx`, project GitHub settings read-only link state | Add Profile Settings -> Integrations and put App install state, connection health, repository count, refresh/reconnect actions there; no new project sync controls. |

## Appendix B — Why not sync issues/comments now?

Real-time webhook sync is powerful but easy to make surprising. If every GitHub
issue/comment event immediately mutates Pi Dash, users inherit implicit field
ownership, duplicate-prevention, backfill, delete/close semantics, and failure
recovery rules before we have designed them. That is too aggressive for this
step.

The safer split is:

1. First ship the GitHub App foundation and prove install/token/webhook health.
2. Then design App-backed sync separately, with explicit opt-in and conservative
   rules for what can change automatically.
3. Only after that consider PR/check/state write-back and runner-created PRs.
