# GitHub Deep Integration — Design

**Status:** Draft (design only — no implementation in this PR)
**Date:** 2026-06-16
**Scope:** The foundation and Layer-1 plan for moving Pi Dash from a one-way,
PAT-polled GitHub mirror to a real-time, bidirectional integration built on a
**GitHub App**. Targets **both** hosted multi-tenant and self-hosted
single-org deployments. The runner/autonomous-coding phase (Layer 2) is
sketched at the end but deliberately deferred.

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

This is enough to mirror issues, but it cannot support the real goal:

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
Removing them requires a different foundation: a GitHub App.

## 2. Goals

- **Adopt a GitHub App as the data-plane credential.** Webhook-driven,
  bot-identity, per-installation, granularly scoped. PAT becomes a legacy
  fallback, not the primary path.
- **Real-time, bidirectional Layer-1 sync.** Issues _and_ PRs, with state
  mapping both ways, driven by webhooks with a polling reconciler as a safety net.
- **Work in both deployment modes.** Hosted multi-tenant (one App, many
  installations) and self-hosted single-org (each instance registers its own
  App). Same code paths; configuration differs.
- **Reuse existing infrastructure.** `WorkspaceIntegration` for per-tenant
  install state, `InstanceConfiguration` for per-instance App secrets, the
  `encrypt_data`/`decrypt_data` Fernet utilities, the Celery/Beat machinery, and
  the `GithubClient` shape. No new secret store, no new queue.
- **Clean migration from PAT.** Existing PAT bindings keep working until their
  workspace adopts the App; no forced cutover, no data loss.
- **Lay groundwork for Layer 2.** The data model (issue ↔ PR linkage, bot
  identity, write scopes) must be the same one the runner will later use to open
  PRs autonomously.

## 3. Non-Goals (this design)

- **No runner/autonomous-PR work.** Layer 2 is sketched (§12) but not designed
  here. It is gated on Layer 1 shipping first.
- **No GitHub Enterprise _Server_ (GHES) support.** First, a distinction this
  doc otherwise invites readers to confuse: self-hosted **Pi Dash** talking to
  `github.com` **is** supported (the "self-hosted single-org" mode in §11). What
  is deferred is self-hosted **GitHub** — a GHES box on a customer host like
  `github.acme-corp.internal`. We support `github.com` and GitHub-hosted
  Enterprise **Cloud** (also `github.com`-hosted), matching the current parser
  (`parse_github_repo_url`) and the `GITHUB_API_BASE` constant in
  `utils/github_client.py`.

  GHES is deferred — not because it's hard, but because it's pure host-URL
  configurability that **changes nothing in the architecture**: the App model,
  token minting, webhooks, and sync logic are identical; only the API base
  (`https://<host>/api/v3`), App-registration endpoints, and webhook origin
  differ. That's exactly what makes it a safe, additive later change. Deferring
  avoids (a) doubling the verification matrix against a license-gated GHES
  instance during the foundational build, (b) the narrower-than-it-looks network
  topology (GHES usually lives inside a corporate network — only the self-hosted
  Pi-Dash-plus-GHES-same-network combination is reachable), and (c) the ongoing
  cost of feature-gating by GHES version, which lags `github.com`.

  **To keep the later add cheap, Layer 1 must centralize the API base URL** —
  resolve every call's host through one place (extend `GITHUB_API_BASE` into a
  per-installation host field) instead of scattering `https://api.github.com`
  literals. Done now, GHES becomes a config change later, not a refactor.

- **No removal of PAT.** PAT stays as a fallback and for the migration window.
- **No GitLab/Bitbucket.** Provider abstraction is noted as a seam but not built.
- **No frontend visual design.** UI surfaces are enumerated (what data appears
  where) but pixel/interaction design is out of scope.
- **No CI-check authoring.** We _read_ check/review status (Phase 2); Pi Dash
  does not _publish_ its own commit statuses or checks in this design.

## 4. Background — what we build on

This design slots into existing systems rather than replacing them. The key
anchors (verified in code):

| System                        | Where                                                                                                                                         | What we reuse                                                                                             |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Per-tenant integration row    | `WorkspaceIntegration` (`db/models/integration/base.py`) — `config`/`metadata` JSON, `unique_together = [workspace, integration]`             | Store the App **installation** state per workspace here, mirroring how PAT is stored today.               |
| Per-instance config + secrets | `InstanceConfiguration` (`license/models/instance.py`), `get_configuration_value()` (`license/utils/instance_value.py`), admin PATCH endpoint | Store the App **id / private key / webhook secret** here (encrypted), exactly like `EMAIL_HOST_PASSWORD`. |
| Encryption                    | `license/utils/encryption.py` — Fernet key derived from `SECRET_KEY` via PBKDF2                                                               | Encrypt the App private key and webhook secret at rest.                                                   |
| REST client                   | `utils/github_client.py` — PAT-auth, paginated, typed errors                                                                                  | Generalize to accept _any_ bearer token (PAT **or** installation token).                                  |
| Sync tasks + Beat             | `bgtasks/github_sync_task.py`, `celery.py` beat `github-issue-sync-every-4h`                                                                  | Keep the full-scan task as the **reconciler**; demote cadence; add webhook-driven incremental path.       |
| State-transition hook         | `bgtasks/github_signals.py` (pre/post-save on `Issue`)                                                                                        | Extend the same hook pattern to push Pi Dash → GitHub state changes.                                      |
| Outbound webhooks             | `db/models/webhook.py`, `bgtasks/webhook_task.py` — HMAC-SHA256 signing                                                                       | Reuse the **HMAC verification pattern** (inverted) for _inbound_ GitHub webhooks.                         |

**Critical gap:** there is **no inbound webhook receiver anywhere** in the
codebase today. The existing webhook system is outbound-only. Building a
verified inbound receiver is the single biggest net-new piece of infrastructure
in this design (§7).

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

The decisive factor is **webhooks**: real-time and bidirectional sync are
impossible on polling. Everything else (identity, scopes, rate limits) is why
this is also the right home for the eventual runner phase.

## 6. Auth architecture

### 6.1 The three-token dance

A GitHub App authenticates in three escalating forms; we need all three:

1. **App JWT** — short-lived (≤10 min), signed with the App's **private key**
   (RS256), `iss = App ID`. Identifies _the App itself_. Used only to call the
   App-level endpoints below.
2. **Installation token** — minted by `POST /app/installations/{id}/access_tokens`
   using the App JWT. Scoped to one installation, expires in **1 hour**, carries
   the granted permissions on the granted repos. **This is the token the data
   plane uses** for all REST calls (issues, PRs, comments, checks).
3. **User-to-server token** (optional) — OAuth-on-the-App, to act _as the
   logged-in user_ for attribution-sensitive writes. Deferred; the bot identity
   is sufficient for Layer 1.

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

- Keep the `token=` constructor for PAT callers (the reconciler, legacy paths).
- Add a classmethod `for_installation(installation_id)` that pulls a fresh
  installation token from §6.2 and constructs the client. Internally identical
  REST surface — the only difference is **where the bearer comes from**.
- New write methods land here regardless of token source:
  `close_pull_request`, `merge_pull_request`, `update_issue_state`,
  `request_review`, `get_pull_request`, `list_check_runs`, etc.
- **Centralize the API base URL.** `GITHUB_API_BASE` is currently a module-level
  constant pointing at `https://api.github.com`. Make the host an instance
  property resolved through one place (defaulting to `github.com`) rather than a
  hard-coded literal scattered across calls. Layer 1 only ever resolves it to
  `github.com`, but doing this now is what makes GHES (§3) a config change later
  instead of a refactor.

This keeps a single REST surface; PAT vs App is just credential provenance, and
host is just configuration.

### 6.4 Where credentials live (both deployment modes)

```
InstanceConfiguration (per deployment, encrypted via Fernet)
  ├─ GITHUB_APP_ID            (plaintext)
  ├─ GITHUB_APP_SLUG          (plaintext; for install URLs)
  ├─ GITHUB_APP_PRIVATE_KEY   (is_encrypted=True)
  ├─ GITHUB_APP_WEBHOOK_SECRET(is_encrypted=True)
  └─ GITHUB_APP_CLIENT_ID/SECRET (is_encrypted; only if user-to-server used)

WorkspaceIntegration.config (per workspace — the *installation*, not the App)
  {
    "auth_type": "github_app",
    "installation_id": 12345678,
    "account_login": "acme-corp",
    "repository_selection": "selected" | "all",
    "installed_at": "...",
    "suspended_at": null
  }
```

The App **secret** is always instance-level; the **installation** is always
workspace-level. This split is what makes both deployment modes the same code.

## 7. Inbound webhook receiver (new infrastructure)

The largest net-new piece. Design mirrors the runner long-poll plane's
discipline: verify at the edge, persist, ack fast, process async.

### 7.1 Endpoint

A single public endpoint on the **external API** surface
(`pi_dash.api`, mounted `/api/v1/`), e.g.
`POST /api/v1/integrations/github/webhook/`. It must be **unauthenticated by
`X-Api-Key`** (GitHub won't send one) and instead authenticated by **signature**.

### 7.2 Verification (reuse the HMAC pattern, inverted)

GitHub signs each delivery with `X-Hub-Signature-256: sha256=<hmac>` over the
raw body, keyed by the App's webhook secret. The outbound webhook code
(`bgtasks/webhook_task.py:313`) already does the _forward_ of this exact
construction; we invert it:

```python
expected = hmac.new(webhook_secret.encode(), raw_body, hashlib.sha256).hexdigest()
hmac.compare_digest(expected, received_sig)   # constant-time
```

Reject on mismatch with `401` before any parsing. Resolve `webhook_secret` from
`InstanceConfiguration` (self-hosted) or the single hosted App secret.

### 7.3 Persist-then-process

1. Verify signature on the **raw** body (must read body before DRF parses it).
2. Persist a `GithubWebhookDelivery` row (dedupe on `X-GitHub-Delivery` UUID —
   GitHub retries deliveries; we must be idempotent).
3. Enqueue `process_github_webhook.delay(delivery_id)` and return `202`
   immediately. GitHub enforces a ~10s delivery timeout; never process inline.
4. The Celery task routes by `X-GitHub-Event` (`pull_request`, `issues`,
   `issue_comment`, `check_suite`, `pull_request_review`,
   `installation`, `installation_repositories`) to a handler.

### 7.4 Handler routing

| Event                                    | Action                                                                                 |
| ---------------------------------------- | -------------------------------------------------------------------------------------- |
| `installation` (created/deleted/suspend) | Create/disable the workspace `WorkspaceIntegration` install record; mint/evict tokens. |
| `installation_repositories`              | Update which repos a binding may cover.                                                |
| `issues`                                 | Upsert the issue mirror **incrementally** (replaces a full poll for that issue).       |
| `issue_comment`                          | Upsert/delete the mirrored comment.                                                    |
| `pull_request`                           | Upsert the PR mirror; map PR state → linked issue state (§9).                          |
| `pull_request_review`                    | Update review status on the linked issue.                                              |
| `check_suite` / `check_run`              | Update CI status on the linked PR/issue.                                               |

Handlers are the **incremental** counterpart to today's full scan. The full scan
survives as the reconciler (§10).

## 8. Data model changes

New rows; existing GitHub models (`GithubRepository`, `GithubRepositorySync`,
`GithubIssueSync`, `GithubCommentSync`) are unchanged in shape, only joined to.

```
GithubWebhookDelivery (new)
  delivery_id (UUID, unique)   # X-GitHub-Delivery — idempotency key
  event        (str)           # X-GitHub-Event
  action       (str)
  installation_id (bigint, null)
  payload      (JSON)
  status       (received | processed | failed | skipped)
  received_at, processed_at, error

GithubPullRequestSync (new — the missing issue↔PR link)
  repository_sync (FK GithubRepositorySync)
  issue        (FK Issue, null)         # the linked Pi Dash issue, if any
  repo_pr_number (int)
  github_pr_id (bigint)
  pr_url       (url)
  head_ref, base_ref (str)
  state        (open | draft | merged | closed)
  review_state (str, null)              # approved / changes_requested / ...
  check_state  (str, null)              # success / failure / pending
  metadata     (JSON)                   # last_synced_sha, linkage_source, ...
  unique_together = [repository_sync, repo_pr_number]
```

**Linkage discovery** (how an issue finds its PR), in priority order:

1. PR body / title references (`closes #N`, `fixes #N`) → map `#N` to the
   mirrored `GithubIssueSync.repo_issue_id`.
2. Branch naming convention (e.g. `pi-dash/<issue-identifier>`), which is also
   exactly what the runner will produce in Layer 2 (`Assign.git_work_branch`
   already exists in the runner protocol).
3. Manual link (paste a PR URL on the issue) — always-available fallback.

## 9. Bidirectional state mapping (Layer 1)

State sync is **policy**, and surprising automation is worse than none — so
every reverse (Pi Dash → GitHub) mapping is **opt-in per project**.

**GitHub → Pi Dash** (inbound webhook → issue state, via the existing
`State.group` vocabulary `backlog/unstarted/started/review/completed/cancelled`):

| GitHub event                       | Pi Dash effect (default)                              |
| ---------------------------------- | ----------------------------------------------------- |
| PR opened linked to issue          | issue → `started`                                     |
| PR marked ready / review requested | issue → `review`                                      |
| PR merged                          | issue → `completed`                                   |
| PR closed unmerged                 | no change (surface a badge)                           |
| issue closed upstream              | flag `upstream_gone_at` (today's behavior, unchanged) |

**Pi Dash → GitHub** (extend the `github_signals.py` post-save hook; today it
only fires the completion _comment_ on `group == completed`):

| Pi Dash transition     | GitHub effect (opt-in)                                   |
| ---------------------- | -------------------------------------------------------- |
| issue → `completed`    | comment (today) **+ optionally** close linked PR / issue |
| issue → `cancelled`    | optionally close the linked PR (the original example)    |
| issue assigned to user | optionally assign the GitHub issue                       |

Reverse writes require the App installation to have been granted
**Issues: write** and **Pull requests: write**; the handler no-ops with a
surfaced warning if the grant is missing (mirrors today's
`GithubPermissionError` handling).

**Loop prevention:** a write Pi Dash makes to GitHub comes back as a webhook.
Tag Pi-Dash-originated changes (bot actor / a marker in `metadata`) and short-
circuit in the handler when the incoming change matches one we just made — the
same idempotency discipline as `completion_comment_id` today.

## 10. Reconciliation — webhooks are not reliable

Webhooks get dropped, mis-delivered, or missed during downtime. The full-scan
task (`sync_one_repo`) is **not deleted** — it is repurposed:

- Demote `github-issue-sync-every-4h` cadence (e.g. daily) and rename its intent
  to "reconcile," not "sync."
- It becomes the **drift-repair** pass: re-scan issues + PRs, compare to local
  mirrors, fix anything webhooks missed.
- Webhook handlers carry steady-state; the reconciler guarantees eventual
  consistency. This is the standard belt-and-suspenders for webhook systems.

## 11. Deployment modes — one design, two configs

### Hosted multi-tenant

- **One** GitHub App, registered by Pi Dash, owned by the vendor org.
- App id / private key / webhook secret live in the hosted instance's
  `InstanceConfiguration` (or env) — set once.
- Each customer org **installs** the App; the `installation` webhook creates a
  `WorkspaceIntegration` install record for their workspace.
- One public webhook URL for all tenants; the handler routes by
  `installation_id` → workspace.

### Self-hosted single-org

- Each instance **registers its own** GitHub App (via GitHub's **App manifest
  flow** — a one-click create that returns id + private key + webhook secret to a
  callback, far better self-host UX than manual field entry).
- Those secrets are written to _that instance's_ `InstanceConfiguration`
  (encrypted). Admin UI lives next to the existing GitHub OAuth config screen in
  `apps/admin`.
- Webhook URL is the instance's own public origin. Self-host docs must note the
  instance has to be reachable by GitHub (the one genuinely new operational
  requirement vs. PAT polling, which needed no inbound connectivity).

The **only** differences are _who registers the App_ and _which origin receives
webhooks_. Storage, token minting, handlers, and sync logic are identical —
both read App secrets from `InstanceConfiguration` and installs from
`WorkspaceIntegration`.

## 12. Layer 2 — runner / autonomous PRs (deferred, sketch only)

Not designed here; captured so Layer 1's data model doesn't paint us into a
corner. The runner plane already has the needed seams:

- `AgentRun.run_config` already carries `repo_url`, `repo_ref`,
  `git_work_branch`; the `Assign` protocol message
  (`runner/src/cloud/protocol.rs`) already ships a branch to check out, and
  `runner/src/workspace/git.rs` already clones, branches, and manages worktrees.
- The runner already pushes branches; it does **not** open PRs today.
- **Future flow:** issue → `AgentRun` (existing dispatch via
  `matcher.drain_pod`) → runner writes code + pushes branch → on
  `RunCompleted`, the cloud (not the runner) opens a **draft PR** via the App
  installation token and records a `GithubPullRequestSync` linked to the issue.
  Because the PR is opened by the App, it is attributed to `pi-dash[bot]`, and
  every Layer-1 mechanism (state mapping, checks, review surfacing) applies to it
  automatically.

The point of doing Layer 1 on a GitHub App first is precisely that Layer 2 then
needs _no new auth, no new client, no new linkage model_ — only a "create PR on
run completion" step.

## 13. Phased roadmap

| Phase                                     | Deliverable                                                                                                                                                                                                                                                                                                 | Gates unlocked                                               |
| ----------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| **0 — Foundation**                        | App registration (manifest flow for self-host; manual for hosted), secret storage in `InstanceConfiguration`, token-minting service (§6.2), `GithubClient.for_installation`, **inbound webhook receiver + verification + delivery persistence** (§7), `installation` event → `WorkspaceIntegration` record. | The App can be installed and we receive verified events.     |
| **1 — Real-time issue sync**              | Webhook handlers for `issues` / `issue_comment` doing incremental upsert; reconciler demotion (§10). PAT path still works in parallel.                                                                                                                                                                      | Issues sync in real time instead of every 4h.                |
| **2 — PR + checks + bidirectional state** | `GithubPullRequestSync` model + linkage discovery (§8); `pull_request` / `review` / `check_suite` handlers; PR/CI/review status surfaced on issues; opt-in reverse state mapping incl. close-PR-on-cancel (§9); new write methods on `GithubClient`.                                                        | The full Layer-1 vision: bidirectional, real-time, PR-aware. |
| **3 — Runner autonomy**                   | (Separate design) issue → runner → draft PR via App token.                                                                                                                                                                                                                                                  | Layer 2 — the differentiator.                                |

Phases 0–2 deliver "catch up to Linear, in real time, both deployment modes."
Phase 3 is the part competitors can't easily copy because Pi Dash owns the
runner.

## 14. Security considerations

- **Private key** is the crown jewel — encrypted at rest (Fernet), never logged,
  never returned by any API (the config serializer must treat it like
  `EMAIL_HOST_PASSWORD`).
- **Webhook signature** verified constant-time on the raw body before parsing;
  unsigned/mismatched → `401`, no side effects.
- **Delivery idempotency** via `X-GitHub-Delivery` to survive GitHub's retries.
- **Installation token scope** is the least-privilege boundary — request only the
  permissions each phase needs (Phase 1: Issues read; Phase 2: + Pull requests
  read/write, Checks read).
- **Tenant isolation** (hosted): always resolve `installation_id` → workspace
  from our own records; never trust a workspace id from the payload.
- **Loop/echo prevention** (§9) so our own writes don't ping-pong.

## 15. Open questions

1. **User attribution** — is bot-identity (`pi-dash[bot]`) acceptable for all
   writes in Layer 1, or do some actions need user-to-server tokens (§6.1.3) for
   correct authorship? (Affects whether we build the OAuth-on-App flow now.)
2. **Reconciler cadence** — daily? hourly? Tunable per instance?
3. **PAT deprecation timeline** — keep indefinitely as a fallback, or sunset
   once App adoption crosses a threshold?
4. **Multi-repo per project** — the current schema enforces one repo per project
   (`github_repository_sync_unique_per_project_when_active`). Does deep
   integration need many repos per project (monorepo vs polyrepo teams)?
5. **Self-host reachability** — for instances not publicly reachable, do we offer
   a polling-only degraded mode, or require a tunnel/relay?

---

## Appendix A — File-level impact map

| Area               | File(s)                                                                                             | Change                                                        |
| ------------------ | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| App auth           | `apps/api/pi_dash/utils/github_app_auth.py` _(new)_                                                 | JWT + installation-token minting, Redis cache.                |
| REST client        | `apps/api/pi_dash/utils/github_client.py`                                                           | `for_installation()` classmethod; new write methods.          |
| Webhook receiver   | `apps/api/pi_dash/api/views/integration/github_webhook.py` _(new)_, `apps/api/pi_dash/api/urls/...` | Signed inbound endpoint + URL registration.                   |
| Webhook processing | `apps/api/pi_dash/bgtasks/github_webhook_task.py` _(new)_                                           | Event router + handlers (incremental upsert).                 |
| Models             | `apps/api/pi_dash/db/models/integration/github.py`                                                  | `GithubWebhookDelivery`, `GithubPullRequestSync` + migration. |
| State hook         | `apps/api/pi_dash/bgtasks/github_signals.py`                                                        | Extend to reverse state mapping (opt-in).                     |
| Reconciler         | `apps/api/pi_dash/bgtasks/github_sync_task.py`, `apps/api/pi_dash/celery.py`                        | Demote cadence; reframe as drift-repair.                      |
| Instance config    | `apps/admin/...` GitHub config screens; `license/...` config keys                                   | App id/key/secret entry; manifest-flow callback (self-host).  |
| Install state      | `apps/api/pi_dash/app/views/integration/github.py`                                                  | `installation` handling alongside existing PAT connect.       |
| Types/UI           | `packages/types/src/integration.ts`, web settings components                                        | PR/check/review status surfaces; install vs PAT status.       |

## Appendix B — Why not just add write methods to the PAT client?

We could ship close-PR-on-cancel today purely on the PAT path (add
`close_pull_request`, extend `github_signals.py`, bump the PAT scope). That is a
legitimate **quick win** and nothing here blocks it. But every _other_ part of
the vision (real-time, PR awareness, checks, durable identity, the runner loop)
requires the App. Building the close-PR feature only on PAT means rebuilding it
on the App later. The roadmap therefore treats the App as Phase 0 and folds
close-PR into Phase 2, where it costs almost nothing on top of the PR model that
already has to exist.
