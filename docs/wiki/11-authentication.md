# 11 — Authentication

Three distinct identity flows live in Pi Dash. They all terminate at the Django `authentication/` module, but they have very different user experiences and security models.

| Flow                                               | Who uses it                                            | Where it lives                                            |
| -------------------------------------------------- | ------------------------------------------------------ | --------------------------------------------------------- |
| **Instance admin** ("god mode")                    | The person who first stood up the self-hosted instance | `apps/admin` UI + `pi_dash.authentication`                |
| **User login** (password, magic link, OIDC)        | Day-to-day product users                               | `apps/web` + `apps/space` + `pi_dash.authentication`      |
| **Runner enrollment** (device-code + legacy token) | The Rust `pidash` daemon                               | `runner/src/cli/auth` + `pi_dash.runner.views.enrollment` |

## 1. Instance admin (one-time bootstrap)

The very first thing you do on a fresh self-hosted instance:

```
http://<your-instance>/god-mode/   ← register the instance admin
```

This is served by `apps/admin/` (port `3001` in dev). Once registered, the admin's credentials are also valid user credentials on the main app — you log into `apps/web` with the same identity.

There is no "create admin" CLI command — it's deliberately a one-shot web flow tied to "the first request to god-mode wins".

## 2. User login (web + space)

Lives in `pi_dash/authentication/` (mounted at `/auth/`):

```
authentication/
├── views/         ← sign-in, sign-up, password reset, OIDC callbacks
├── serializers/
├── permissions/
├── middleware/
└── urls/
```

Mechanisms supported (configurable per instance):

- **Email + password** (always available)
- **Magic-link / email OTP**
- **OIDC / SSO** — when configured. Cloud Pi Dash uses the **home-page OIDC** as the plan source-of-truth: the JWT `plan` claim is cached on `Account` and gates Cloud-specific features. There are **no plan webhooks** — plan updates flow through subsequent OIDC logins. Upgrade UI in the app deep-links back to the home-page user center.

The `space` app (public/guest views) reuses the same auth module but can serve content without a session.

## 3. Runner enrollment

The runner (`pidash` CLI) needs to authenticate to the cloud without a human at the keyboard for every request. There are two paths:

### A. Device-code flow (recommended)

```bash
pidash auth login --url https://your-pidash-instance.com
```

1. CLI requests a device code from the cloud (`/api/v1/runner/enroll/...`).
2. CLI shows the user a short code + URL to open in a browser.
3. User approves in the browser (signed in via flow #2 above).
4. CLI polls until the code is approved → cloud mints a **CLI token**.
5. CLI persists token at `~/.config/pidash/config.toml` (`0600`).
6. CLI prompts to add a runner inline if none exists on this host.

This is the same UX as `gh auth login` or `stripe login`. Bare `pidash` (no subcommand) drops into this flow when no config exists — useful after MSI install on Windows.

### B. Headless login

For headless hosts where opening a browser automatically is awkward:

```bash
pidash auth login --no-browser --url https://pidash.example.com
pidash runner add --project <PROJECT_ID>
```

The CLI prints the verification URL and user code; approve it from any browser signed in to Pi Dash.

## Runner credential model

After enrollment, the runner holds a **refresh / access token pair**:

| Token                          | Lifetime              | Use                                                                                                        |
| ------------------------------ | --------------------- | ---------------------------------------------------------------------------------------------------------- |
| **CLI token**                  | Long-lived            | Identifies the _user_ on this host. Authorizes `runner add`. Stored once at login.                         |
| **Refresh token** (per-runner) | Long-lived, rotatable | Identifies the _runner registration_. Stored at `0600`. Minted by `pidash runner add` using the CLI token. |
| **Access token** (per-runner)  | Short-lived           | Derived from refresh token. Sent on each cloud request.                                                    |

Cloud-side, refresh/access lifecycle is handled by `pi_dash/runner/services/tokens.py`.

`pidash auth logout` revokes the CLI token cloud-side. `pidash runner remove` revokes the runner credentials.

## On-disk security

- Config + credential files: `0600` on Unix.
- IPC socket between daemon and TUI: `0600`.
- On Windows: files under the user profile, IPC over a local named pipe with appropriate ACLs.

## Where to read next

- [08 — Cloud ↔ runner protocol](./08-cloud-runner-protocol.md) — how the access token is presented per request
- [07 — Runner architecture](./07-runner-architecture.md) — `runner/src/cli/auth/` and `runner/src/config/`
- [06 — Backend architecture](./06-backend-architecture.md) — `pi_dash/authentication/` module structure
