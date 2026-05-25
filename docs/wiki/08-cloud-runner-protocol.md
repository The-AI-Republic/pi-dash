# 08 — Cloud ↔ Runner Protocol

This is the one **external, versioned contract** in Pi Dash. The web frontend ↔ Django REST surface ships together and can change freely. The runner does not — it lives on user machines, may be running an older release than the cloud, and must keep working long enough to auto-update.

The schema lives in `runner/src/cloud/protocol.rs` (Rust side) and is consumed by `apps/api/pi_dash/runner/views/sessions.py` and friends (Django side). Both sides ship together when version bumps happen — but they tolerate version skew for the auto-update grace period.

## Wire version

**Current wire version: `4`.**

History:

- **v1 – v3** — WebSocket transport. Long-lived connection from runner to cloud carrying session frames and run dispatches.
- **v4** — moved to **per-runner HTTPS long-poll**. Runner authenticates per request with a short-lived access token; long-poll holds open for new work. No persistent WS connection from the runner side.

The Channels WebSocket routing in `apps/api/pi_dash/runner/routing.py` still exists for backward compatibility and for editor channels — but new runners ride on the HTTP path.

**Bump the wire version on incompatible shape changes only.** Additive optional fields don't need a bump. Removing or repurposing a required field does.

## The handshake — `welcome` frame

When the runner opens a session, the cloud returns a `welcome` payload. Key fields:

| Field                   | Source                      | Meaning                                                                                                                                                                                                            |
| ----------------------- | --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `protocol_version`      | echoed by server            | Confirms the version the cloud is willing to speak. Runner refuses if mismatched beyond compat range.                                                                                                              |
| `latest_runner_version` | env `LATEST_RUNNER_VERSION` | Newest runner version that exists. Drives the yellow "update available" advisory and, with auto-update on, triggers in-place binary swap.                                                                          |
| `min_runner_version`    | env `MIN_RUNNER_VERSION`    | Cloud-set floor. Below this version, runner shows a red "update required" banner. Advisory in the current implementation — does **not** refuse new tasks yet, but it's the signal that the floor is about to bite. |

Both version envs are optional. Leave them unset to skip the announcement.

## Auto-update advisory states

What the runner shows in TUI / `pidash status` based on the welcome contents:

| Running version vs. cloud advisory                 | Banner                                                 |
| -------------------------------------------------- | ------------------------------------------------------ |
| `>= latest_announced` and `>= min_required`        | (nothing)                                              |
| `< latest_announced`, swap already on disk         | yellow `⚠ Restart to apply vX.Y.Z`                     |
| `< latest_announced`, auto-update on, swap pending | yellow `⚠ Update vX.Y.Z pending swap`                  |
| `< latest_announced`, auto-update off              | yellow `⚠ Update vX.Y.Z available — run pidash update` |
| `< min_required`                                   | red `⛔ Update required: cloud floor vX.Y.Z`           |

The toggle lives in TUI → General → Daemon settings → `auto_update`. Default **on**.

## Authentication

The runner does **not** carry a long-lived password. Authentication is a two-token model:

1. **Refresh token** — issued during enrollment (device-code or legacy enrollment-token), stored on disk at `0600`.
2. **Access token** — short-lived, derived from the refresh token, used per request to the cloud.

`pidash runner add` mints the runner-side credentials cloud-side using the CLI token from `pidash auth login`. The legacy `pidash connect --token <ONE_TIME_TOKEN>` flow is still supported for headless / scripted hosts where the browser flow is awkward.

See [11 — Authentication](./11-authentication.md) for the full flow.

## Endpoints (Django side, `pi_dash/runner/`)

| URL path                                 | Purpose                                                     |
| ---------------------------------------- | ----------------------------------------------------------- |
| `POST /api/v1/runner/sessions/`          | Open a session — returns the welcome frame                  |
| `GET  /api/v1/runner/runs/` (long-poll)  | Wait for the next assigned run                              |
| `POST /api/v1/runner/runs/<id>/...`      | Stream run events back to the cloud                         |
| `POST /api/v1/runner/approvals/<id>/...` | Submit an approval decision                                 |
| `POST /api/v1/runner/enroll/...`         | Device-code enrollment endpoints                            |
| `GET/POST /api/runners/...`              | Web-UI side of runner admin (CRUD, "add connection" tokens) |

Schemas are typed in `pi_dash/runner/serializers.py` and mirrored in `runner/src/cloud/protocol.rs`. Treat the two as the contract.

## Versioning discipline

- **Additive** (new optional fields) → no bump. The other side just ignores them.
- **Renamed / repurposed / removed required field** → bump.
- When you bump, **ship both sides in the same release window** and update `LATEST_RUNNER_VERSION` / `MIN_RUNNER_VERSION` envs on the cloud so old runners get nudged forward.
- The Rust `protocol.rs` test (`tests/protocol_roundtrip.rs`) round-trips every variant — keep it green.

## Where to read next

- `runner/src/cloud/protocol.rs` — the authoritative schema
- [07 — Runner architecture](./07-runner-architecture.md) — what consumes this protocol on the runner side
- [06 — Backend architecture](./06-backend-architecture.md) — `pi_dash/runner/` is the cloud side
- [15 — Releasing](./15-releasing.md) — how the version envs get set on cloud after a runner tag
