# GitHub Actions Self-Hosted Runner — Architecture Reference

Purpose: this document explains how GitHub Actions self-hosted runners work end to end, so Pi Dash can adopt the same pattern for connecting cloud-side orchestration to a local coding agent (Codex) running on a user's dev machine.

Source-of-truth references:

- Runner source: https://github.com/actions/runner (C#/.NET)
- Runner docs: https://docs.github.com/en/actions/hosting-your-own-runners
- Actions Runner Controller (K8s): https://github.com/actions/actions-runner-controller

This doc is a technical summary, not a verbatim spec. Where GHA's internals are undocumented, the behavior is described from observation of the open-source runner and public docs.

Implementation notes for this document:

- Statements about GitHub internals that are not documented should be read as "observed behavior", not as a stable public contract.
- Pi Dash sections below are normative for implementation unless marked as an open question.

---

## 1. The two-role split

Any system that ships work from a cloud service to a user-owned machine has two roles:

| Role                        | Owns                                       | Does                                                                                  |
| --------------------------- | ------------------------------------------ | ------------------------------------------------------------------------------------- |
| **Orchestrator** (cloud)    | The source of truth — jobs, state, routing | Decides _what_ runs and _when_, matches jobs to runners, tracks state, exposes UI/API |
| **Runner / worker** (local) | The execution environment                  | Executes _one_ job at a time, streams results back, holds local credentials           |

In GHA:

- **Orchestrator** = GitHub's Actions service (cloud).
- **Runner** = the `actions/runner` binary a user installs on their own machine.

In Pi Dash's target architecture:

- **Orchestrator** = Django/Celery in `apps/api` (+ a WS service modeled on `apps/live`).
- **Runner** = a new daemon installed on the user's dev PC that drives `codex app-server`.

The rest of this document is how GHA implements the runner side and the cloud ↔ runner contract between them.

---

## 2. The networking trick: outbound-initiated, bidirectional

A runner sits behind NAT / corporate firewall / home router. Cloud → laptop inbound connections are blocked. The runner solves this by **initiating outbound** to the cloud and holding the connection open. Once TCP+TLS is established, the channel is full-duplex; the _server_ can push frames down it at any time.

```
Dev PC (runner)                               GitHub cloud
      │                                             │
      │  TCP+TLS on :443 (outbound, allowed) ─────► │
      │ ◄───────────────────────────  handshake     │
      │                                             │
      │ ═════════ persistent bidirectional ═══════ │
      │                                             │
      │  heartbeat ──►                              │
      │              ◄── "here's job #42"           │   server push, unsolicited
      │  ack ──►                                    │
      │  log line ──►                               │
      │              ◄── "cancel job #42"           │   server push again
```

Transports GHA has used over time:

1. **HTTP long-poll (classic).** Runner sends `GET /message` with a long timeout; server holds it open until a message exists. Simple, firewall-friendly, slightly laggy.
2. **Brokered WebSocket / HTTP2 streams (modern).** Full-duplex, instant push, lower latency. Same NAT-traversal property because the runner initiates.

The exact broker/session internals on GitHub's side are not a public API. The transport summary above is the mental model to copy, not a protocol contract to depend on.

Both look identical to any firewall: one outbound HTTPS connection on 443.

Takeaway: you do not need ngrok, Tailscale, port forwarding, UDP hole-punching, or an A2A push endpoint. An outbound-initiated persistent connection solves reachability.

---

## 3. Registration lifecycle

Runner install is a two-step credential exchange: a **short-lived registration token** is traded for **long-lived runner credentials** that the runner keeps on disk.

### 3.1. Registration token (short-lived)

- Issued by the GitHub UI or API at a scope (repo / org / enterprise). TTL ~1 hour.
- Single-use. Can be revoked.
- Carries the authorization "this token's holder is allowed to register a runner in scope X".

### 3.2. `config.sh` handshake

The user runs:

```bash
./config.sh \
  --url https://github.com/ORG/REPO \
  --token <REG_TOKEN> \
  --labels gpu,staging,arm64 \
  --name my-laptop
```

What the runner does during config:

1. POSTs the registration token to GitHub's API.
2. GitHub validates scope + TTL, assigns the runner a stable **numeric ID**, records labels and name.
3. GitHub returns **long-lived credentials**: an RSA keypair + a token.
4. Runner writes three files to its install dir:
   - `.runner` — JSON config (id, url, labels, poolId, agentName).
   - `.credentials` — credential descriptor (scheme, auth URL).
   - `.credentials_rsaparams` — RSA private key material.
5. Registration token is discarded. From now on the runner authenticates itself with the stored credentials.

### 3.3. Labels

Every runner advertises a label set. Defaults assigned automatically:

- `self-hosted`
- OS: `Linux` | `Windows` | `macOS`
- Arch: `X64` | `ARM64` | `ARM`

Custom labels are user-provided (`--labels`): `gpu`, `staging`, `mac-studio`, `has-docker`, etc. Labels are how jobs target the right runner.

### 3.4. Runner groups (multi-tenant scoping)

Org/enterprise-scoped runners are organized into **runner groups**. A group has a policy:

- Which repos (or orgs/workflows) may use runners in this group.
- Whether public repos are allowed.

At dispatch time, a job is filtered to the groups its repo is authorized for, _then_ label matching happens within that set. This stops Team A's workflows from picking up Team B's GPUs.

### 3.5. Ephemeral vs persistent runners

- **Persistent (default):** stays alive, takes one job, returns to idle, takes the next.
- **Ephemeral (`--ephemeral`):** takes one job then self-deregisters and exits. Used heavily for K8s/Actions Runner Controller autoscaling — each pod is single-use, so no dirty state leaks between jobs.

### 3.6. De-registration

- `./config.sh remove --token <REMOVAL_TOKEN>` with a removal token from the UI.
- A runner that stops heartbeating goes to **offline** state; stays registered (but unusable) for ~14 days, then may be auto-cleaned.

---

## 4. The runtime: waiting for work

Once configured, `./run.sh` (or the systemd / launchd / Windows service wrapper) starts the main loop:

1. **Authenticate.** Exchange the stored credentials for a short-lived session token against GitHub's auth endpoint.
2. **Open session.** Register a "message session" with the Actions service, advertising runner id + current state (`online`, `idle`).
3. **Hold connection.** Long-poll (or brokered WS) on the session, waiting for messages.
4. **Heartbeat.** Send application-level pings every ~30s so dead connections are detected quickly (TCP keepalive alone is too slow and middleboxes often kill it).
5. **Receive a message.** Two kinds:
   - **Job assignment** — full job spec arrives; runner ACKs, locks to busy, begins executing.
   - **Control message** — cancel, settings update, force-upgrade trigger, etc.
6. **Report status.** During job execution, stream logs and step results back up the same session. On completion, post the final job result.
7. **Release.** Return to idle, loop.

If the connection drops:

- Runner reconnects with **jittered exponential backoff** (important — without jitter a deploy on the cloud side causes a thundering-herd reconnect).
- On reconnect, runner presents its id and any in-flight job context. If a job was mid-run, the cloud decides whether to resume or mark failed.

---

## 5. Job dispatch and matching

When a workflow triggers (push, PR, schedule, manual dispatch), the Actions service has to pick a runner. Algorithm in outline:

```
1. Parse `runs-on` in the job definition.
   Example:  runs-on: [self-hosted, linux, gpu]

2. Scope filter:
   - Determine which runner groups this repo/workflow is authorized to use.
   - Candidate set = runners in those groups.

3. Label filter:
   - Keep only candidates whose advertised labels are a SUPERSET of the
     labels in `runs-on`. (All job labels must be present on the runner.)

4. State filter:
   - Keep only runners currently online AND idle (not executing a job).

5. Pick:
   - First-available wins (with internal load-balancing heuristics).
   - If set is empty, job sits in the queue. It will be re-evaluated as
     runners free up or new ones come online.

6. Dispatch:
   - Push the job message down that runner's open session.
   - Include a per-job short-lived credential (GITHUB_TOKEN) scoped to
     this job's permissions.

7. Lock:
   - Mark runner busy in the scheduler. No other job goes to it until it
     reports completion.
```

Label matching is **superset, not equality**. A runner advertising `[self-hosted, linux, X64, gpu, staging]` satisfies a job wanting `[self-hosted, linux, gpu]`.

Operational details from GitHub's documented behavior that matter when copying this model:

- If an assigned runner does not accept/pick up a job within about **60 seconds**, GitHub re-queues the job.
- If no matching runner becomes available, a queued self-hosted runner job can remain queued for up to **24 hours** before timing out.

---

## 6. Per-job lifecycle on the runner

Once a job message arrives:

1. **Job envelope arrives** — workflow + job id, commit sha, repo, secrets, per-job `GITHUB_TOKEN`, steps, container/service specs.
2. **Workspace setup** — fresh checkout (or clean of previous), set `GITHUB_WORKSPACE`.
3. **Services up** — any `services:` containers started.
4. **Steps execute** sequentially:
   - Each step is a process the runner spawns (bash/pwsh/node) with env vars and secret masking.
   - stdout/stderr captured line-by-line, timestamped, and streamed back to the service.
   - Commands like `::set-output::` / `$GITHUB_OUTPUT` are parsed from the stream and reflected into job state.
5. **Post-step cleanup** — tear down containers, clean caches per config, optionally delete the workspace.
6. **Report final status** — `completed` + conclusion (`success` | `failure` | `cancelled` | `skipped`).
7. **Return to idle** (persistent) or **exit** (ephemeral).

Cancellation: if a cancel message arrives mid-step, the runner sends SIGINT / SIGTERM (then SIGKILL after a grace period) to the step process, aborts remaining steps, runs post-step cleanups, reports `cancelled`.

---

## 7. Security model

Layers:

1. **Registration token.** Short TTL, single-use, scoped. Cannot do anything but register once.
2. **Runner credentials.** Long-lived but per-runner; can be revoked from the UI at any time. Stored on disk with file-mode 0600; compromise = that runner is compromised, not the org.
3. **Per-job `GITHUB_TOKEN`.** Minted by the service at dispatch time, TTL ≈ job duration + a grace window, scope = just this job's repo with permissions declared in the workflow. This is what actually authenticates the job's git operations and API calls. It is **never** the runner's own credentials.
4. **Secrets.** Sent in the job envelope, masked in log output, never persisted on the runner beyond the job's lifetime (unless a step writes them to disk itself, which is a workflow author error).
5. **Network.** All runner ↔ service traffic is TLS on 443. The runner never accepts inbound connections.
6. **Trust boundary.** The service trusts the runner to execute code faithfully. The runner trusts the service to send well-formed jobs. Neither trusts the _workflow author_ past the permissions granted by `GITHUB_TOKEN` + supplied secrets.

---

## 8. Reconnection, offline handling, and reliability

Operational realities of long-lived connections:

- **Middlebox idle timeouts.** Corporate proxies drop idle TCP at 60s. Runner sends application pings every ~20–30s.
- **Reconnect storms.** If the service redeploys, thousands of runners disconnect at once. Without jitter, they all reconnect simultaneously and melt the new instance. Runner uses exponential backoff with randomized jitter (sleep random 1–10s, double on repeat, cap at a minute).
- **In-flight job on disconnect.** Runner continues executing the job locally. On reconnect, it presents in-flight status. Service decides: resume streaming logs if within a resume window, or mark the job as failed if the runner was gone too long.
- **Crash recovery.** If the runner process crashes mid-job, the job is marked failed on next reconnect; no partial-state merge. Workspace may be left dirty — cleaned on next job start.
- **Offline detection.** No heartbeat for N seconds → runner marked `offline` in the UI. Job queue will skip it during matching.

---

## 9. Scalability

Per-connection cost is dominated by memory for socket buffers + session state. Typical orders of magnitude:

| Stack                  | Memory/connection | Practical max per server |
| ---------------------- | ----------------- | ------------------------ |
| Python/Django Channels | ~10–20 KB         | 10K–50K                  |
| Node.js (`ws`)         | ~6–10 KB          | 100K–500K                |
| Go                     | ~4–8 KB           | 100K–500K                |
| Elixir/Phoenix         | ~3 KB             | 1M+                      |
| Rust (tokio)           | ~1–2 KB           | 1M+                      |

Idle CPU is effectively zero (just heartbeats). Real cost comes from **message volume during active jobs**, not from idle connection count.

Horizontal scaling pattern:

- WS servers are **stateless** — session state (which runner is connected where, which job is running) lives in a shared store (Redis / DB).
- Each WS server subscribes to a pub/sub channel keyed by the runner-ids currently connected to it.
- When the orchestrator wants to push to runner R, it publishes to channel `runner:R`; the WS server holding R's connection receives and forwards down the socket.
- Reconnects can land on any WS server (behind an L4 LB); the pub/sub mapping updates automatically.

---

## 10. Mapping to Pi Dash

What transfers directly:

| GitHub concept                     | Pi Dash equivalent                                                          | Notes                                                                     |
| ---------------------------------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| Actions service (orchestrator)     | Django (`apps/api`) + a WS service (new, modeled on `apps/live`)            | Django owns state; WS service owns the socket                             |
| Self-hosted runner binary          | `pidash` daemon (new, to build)                                             | Single binary: Go or Rust recommended                                     |
| Registration token                 | One-time code generated in Pi Dash settings UI                              | Short TTL, single-use                                                     |
| `.credentials` on disk             | Runner token + machine id, stored under `~/.config/pidash/`                 | File mode 0600                                                            |
| Labels (`linux`, `gpu`, `staging`) | Capabilities: `codex`, `macos`, `arm64`, `docker`, `git`                    | Same matching rules; do not encode an exact local checkout path in labels |
| Runner groups                      | Per-user / per-workspace pools                                              | "Only this user's machines pick up this user's tasks"                     |
| `runs-on: [self-hosted, gpu]`      | Pi Dash work item metadata declaring required capabilities                  | Triage step sets these                                                    |
| Job envelope                       | `AgentRun` payload: work item id, repo path, prompt, run config             | Matches Codex `thread/start` + `turn/start` params                        |
| Per-job `GITHUB_TOKEN`             | Per-run short-lived credential scoped to the AgentRun's allowed API surface | E.g. scoped webhook back                                                  |
| Step execution (bash)              | Drive `codex app-server` via JSON-RPC on stdio or local WS                  | `turn/start` → stream `item/*` deltas → `turn/completed`                  |
| Log streaming                      | Stream Codex `item/*` events up the WS                                      | 1:1 shape, just wrapped                                                   |
| Cancel message                     | `turn/interrupt` on Codex + runner cleanup                                  | Server → runner → app-server                                              |
| Ephemeral runner                   | Probably not — dev PCs are persistent                                       | But consider ephemeral "workspace runs" inside containers                 |

Features GHA has that Pi Dash specifically needs _beyond_ the GHA model:

- **Mid-run approvals.** Codex pauses and asks "can I run `rm`?" → runner forwards an approval request up the WS → Django persists it and pushes to the user's browser → user clicks in Pi Dash UI → answer flows back. GHA has no analog; this is unique to interactive agent work and is the reason you can't repurpose the GHA runner binary directly.
- **Thread resume.** Codex's `thread/resume` lets a run reattach after reconnect. GHA jobs are stateless shell runs and have no equivalent. Pi Dash must persist `thread_id` per `AgentRun` and pass it through on reconnect.
- **Local credentials stay local.** Codex login (`account/login/start`) happens on the laptop. Pi Dash never sees the user's OpenAI/ChatGPT credentials. The runner mediates everything.

---

## 11. Pi Dash implementation constraints

These are the extra rules Pi Dash needs so this explainer is implementable rather than just informative.

### 11.1. Runner auth is a separate protocol from `apps/live`

The new runner WS service may reuse the deployment/runtime pattern of `apps/live`, but **must not** reuse its auth model.

- `apps/live` authenticates browser/editor traffic with cookies and shared secrets.
- The runner service must use:
  - short-lived registration tokens for initial pairing
  - long-lived per-runner credentials stored locally
  - short-lived session auth for each active connection
  - explicit revocation and credential rotation

Do not authenticate a machine runner with browser cookies or a static shared header secret.

### 11.2. Capabilities are for eligibility, not for exact repo selection

Scheduler matching and local execution binding are separate concerns.

- **Capabilities** answer: "is this machine eligible?"
  - Examples: `codex`, `git`, `docker`, `macos`, `arm64`
- **Execution binding** answers: "where exactly should this run happen on disk?"
  - Examples: `/Users/alice/src/acme/foo`, worktree `feature/runner`, checkout strategy `clone_if_missing`

The orchestrator must not assume that a capability like `repo:acme/foo` is enough to identify a unique local workspace. One machine may have:

- multiple clones of the same repo
- multiple worktrees
- no checkout yet
- a stale checkout on the wrong branch

`AgentRun.run_config` therefore needs an explicit workspace binding policy, not just labels.

### 11.3. Approval states require leases and timeouts

`awaiting_approval` is not just a UI state; it is a resource lock on a real machine. The system must define:

- approval request timeout
- what happens if the web UI disconnects
- whether the runner keeps the Codex process warm while waiting
- when a run is auto-cancelled vs resumed
- whether timed-out approvals release the runner immediately

Default implementation rule:

- while a run is in `assigned`, `running`, or `awaiting_approval`, that runner slot is reserved
- `awaiting_approval` expires after a server-defined timeout
- on expiry, the server sends cancel to the runner and marks the run `cancelled` unless a future resume protocol is explicitly implemented

### 11.4. Event retention must be bounded

Persisting every raw Codex delta forever in Postgres will become expensive quickly.

Default implementation rule:

- persist all events needed for live UI replay during the active run
- persist a durable final transcript or compacted event stream at completion
- allow raw high-volume deltas to be compacted or moved to object storage later

---

## 12. Django-side data model (proposed)

Minimum tables to support this architecture:

- **`Runner`**
  - `id`, `user_id` (owner), `workspace_id`
  - `name` (user-chosen, e.g. "my-laptop")
  - `labels` / `capabilities` (JSON or m2m)
  - `status` (`online` | `offline` | `busy`)
  - `last_heartbeat_at`
  - `credential_fingerprint`
  - `protocol_version`
  - `os`, `arch`, `runner_version`
  - `created_at`, `revoked_at`

- **`RunnerRegistrationToken`**
  - `id`, `workspace_id`, `created_by_user_id`
  - `token_hash` (store hash, not plaintext)
  - `expires_at` (≤ 1h)
  - `consumed_at` (nullable; single-use)

- **`AgentRun`**
  - `id`, `work_item_id`, `runner_id` (nullable until assigned)
  - `status` (`queued` | `assigned` | `running` | `awaiting_approval` | `completed` | `failed` | `cancelled`)
  - `thread_id` (Codex thread, for resume)
  - `prompt`, `run_config` (model, repo binding, branch, checkout strategy)
  - `required_capabilities` (JSON)
  - `lease_expires_at`
  - `created_at`, `assigned_at`, `started_at`, `ended_at`
  - `error` (nullable)

- **`AgentRunEvent`** (append-only log of `item/*` events for UI replay + audit)
  - `id`, `agent_run_id`, `seq`, `type`, `payload`, `created_at`

- **`ApprovalRequest`**
  - `id`, `agent_run_id`, `kind` (`commandExecution` | `fileChange`), `payload`
  - `status` (`pending` | `accepted` | `declined`)
  - `decision_payload`
  - `expires_at`
  - `requested_at`, `decided_at`, `decided_by_user_id`

Matching query (pseudo-SQL):

```sql
SELECT r.*
FROM runner r
WHERE r.workspace_id = :ws
  AND r.status = 'online'
  AND r.labels @> :required_capabilities  -- superset
  AND NOT EXISTS (
    SELECT 1 FROM agent_run ar
    WHERE ar.runner_id = r.id AND ar.status IN ('assigned','running','awaiting_approval')
  )
ORDER BY r.last_heartbeat_at DESC
LIMIT 1;
```

---

## 13. Protocol and lifecycle defaults

The following defaults close the main implementation gaps.

### 13.1. Runner connection protocol

Minimum message families:

- `hello`
  - sent by runner on connect
  - includes `runner_id`, `credential`, `protocol_version`, `runner_version`, `os`, `arch`, advertised `capabilities`
- `hello_ack`
  - sent by server
  - includes accepted protocol version, heartbeat interval, reconnect backoff policy
- `heartbeat`
  - sent by runner on interval
  - includes current run state and optional in-flight `agent_run_id`
- `assign_run`
  - sent by server
  - includes `agent_run_id`, `run_config`, prompt payload, approval policy, scoped per-run credential
- `run_event`
  - sent by runner
  - includes `agent_run_id`, `seq`, `type`, `payload`
- `approval_requested`
  - sent by runner
  - includes `approval_request_id`, `agent_run_id`, `kind`, `payload`, `expires_at`
- `approval_decision`
  - sent by server
  - includes `approval_request_id`, decision, actor, decision payload
- `cancel_run`
  - sent by server
  - includes `agent_run_id`, reason
- `run_completed`
  - sent by runner
  - includes `agent_run_id`, final status, summary payload

Protocol rules:

- all messages carry a stable message id for dedupe
- `run_event.seq` is monotonic per `agent_run_id`
- server operations must be idempotent on retries
- runner reconnect always starts with `hello`, never with an implicit resume

### 13.2. Execution binding defaults

Default local workspace rule:

- one runner per machine
- one active run per runner in v1
- `run_config` must contain either:
  - an explicit absolute local path chosen earlier, or
  - a repo URL plus a checkout policy (`require_existing` or `clone_if_missing`)

Recommended v1 default:

- support `require_existing` first
- do not implement automatic clone/worktree creation until the runner protocol is stable

### 13.3. Cancellation and reconnect defaults

Default behavior:

- if runner disconnects during a run, local execution may continue
- server marks runner `offline` after heartbeat expiry
- run enters a reconnect grace window
- if the same runner reconnects within the grace window and presents the in-flight `agent_run_id`, the server resumes streaming
- otherwise the server cancels the run and marks it `failed` or `cancelled` based on cause

Suggested starting values:

- heartbeat every 20 seconds
- runner offline after 60 seconds without heartbeat
- reconnect grace window 5 minutes
- approval timeout 10 minutes

### 13.4. Recommended defaults to keep

1. **Runner language.** Go or Rust give the best binary-distribution story. Rust is a good fit for the local daemon.
2. **Runner scope.** One runner per machine.
3. **Approval defaults.** Auto-approve reads; require approval for writes, destructive shell commands, and network access.
4. **WS service stack.** Dedicated Node service from day one; reuse `apps/live` deployment patterns, not its auth model.
5. **Pub/sub store.** Valkey/Redis for runner-id to WS-server routing.
6. **Protocol versioning.** Required in `hello`.
7. **Upgrade mechanism.** Start manual or prompt-and-confirm; add auto-update later.
8. **Telemetry.** Minimal by default: version, OS, arch, health counters.

---

## 14. What to build first (suggested phasing)

The runner ships with a built-in **TUI** from v1 — see `tui-design.md` in this directory. TUI and daemon share a local Unix-socket JSON-RPC so the TUI is a thin client that can attach/detach without restarting the daemon.

**Phase 0 — proof of concept, one user, happy path.**

- Django model for `Runner`, `RunnerRegistrationToken`, `AgentRun`.
- A tiny Node WS service that accepts runner connections and forwards messages to/from Django via Redis.
- Minimal runner daemon: config via token, WS connect, execute a hard-coded shell command, stream stdout back. No Codex yet.
- Minimal TUI: Status view only, read-only, attaches over local socket. Proves the daemon/client split.

**Phase 1 — Codex integration + TUI MVP.**

- Runner spawns `codex app-server` and bridges its JSON-RPC to the cloud WS.
- Django UI shows live `item/*` events per `AgentRun`.
- TUI adds: Config view (read/write, hot-apply), Approvals view, first-run onboarding wizard.

**Phase 2 — approvals end-to-end.**

- `ApprovalRequest` model, web UI round-trip, TUI round-trip, Codex `requestApproval` integration.
- Either surface can answer; daemon records decision source.

**Phase 3 — multi-runner, label matching, runner groups, thread resume.**

- TUI adds: Runs history view with detail, doctor subcommand, search/filter.

**Phase 4 — production hardening.**

- Jittered reconnect, upgrade flow, capability advertisement, telemetry, admin UI for runner management.
- TUI adds: diff preview, log follower, help overlay.

**Post-v1 — optional native tray icon.**

- Reuses the same local-socket IPC as the TUI. Tray is an alternate client, not a rewrite.

Each phase is independently demonstrable and testable.
