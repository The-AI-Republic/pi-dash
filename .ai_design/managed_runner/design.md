# Pi Dash Managed Runner (Desktop built-in agent engine) — Design

- **Status:** Implementation under end-to-end validation — local Linux native enrollment and lifecycle are verified; live model execution and the remaining §23/§24 acceptance/release gates are still open. See `private-pi-dash/desktop/tests/README.md` for current evidence.
- **Date:** 2026-09-06
- **Scope:** A zero-setup agent execution path for the Pi Dash desktop app: a Pi Dash-provisioned local runner that ships inside the desktop bundle with a pinned upstream Codex binary, a Pi Dash-managed Codex configuration, Pi Dash tooling, and model access through the user's existing Pi Dash login. Introduces a third `AgentExecutorKind`, `managed_runner`.

This document records the decisions reached in the 2026-09-05/06 design
discussion and maps them onto the existing Pi Dash code. It deliberately
does **not** embed, fork, or modify Codex for the MVP (§20 explains why and
§21 records the path to do so later).

> **Naming.** The product-facing name is **Pi Dash Agent** (in the desktop
> app: "Runs on: this computer"). The executor kind is `managed_runner`. It
> **is** a `Runner` in the existing data model — unlike the Cloud Agent
> (`.ai_design/inhouse_runner/design.md`), which is not. "Managed" means Pi
> Dash controls its lifecycle: install, version, configuration, and
> registration. "Local runner" continues to mean a `pidash` daemon the user
> installed and enrolled themselves.

---

## 1. Problem

Pi Dash already has two ways to execute an `AgentRun`
(`core/agent_execution.py:7`):

|                               | `local_runner`                      | `cloud_agent`                               |
| ----------------------------- | ----------------------------------- | ------------------------------------------- |
| Runs where                    | user's machine, `pidash` daemon     | Pi Dash servers, Celery                     |
| Filesystem / shell / worktree | yes                                 | **no** (by design)                          |
| Tools                         | the driven agent CLI + `pidash` CLI | Pi Dash issue tools + GitHub read, over MCP |
| Model credentials             | the agent CLI's own login           | creator's `UserLLMConfig` (cloud: OpenHub)  |

The Cloud Agent gives web users an out-of-box path, but it cannot edit
code. The local runner can, but user research says its setup is the reason
people bounce. Today a desktop user who wants an agent working on their
repository must:

1. install the desktop app
2. discover they also need `pidash`, and install it
3. run device-code login in a terminal
4. register the machine as a runner, per project
5. install Codex (or another agent CLI) separately
6. log into a second, unrelated account (ChatGPT / OpenAI)
7. pick an agent kind for the project
8. configure a model credential

Eight steps and three logins. The desktop app itself performs no agent work
— `desktop/src-tauri/src/pidash_cli.rs` only detects and installs the OSS
`pidash` CLI.

**None of these steps is a capability gap.** Users are not asking for a
different agent; they are asking not to assemble one. The goal of this
design is therefore _zero-config setup_, not a new engine.

## 2. Decisions already settled

1. **Do not build an agent engine.** The engine is upstream Codex, consumed
   as a released binary. No fork, no source modification, no in-process
   embedding in the MVP (§20).
2. **Bundle, don't install.** The desktop installer ships both `pidash` and
   `codex`. Neither is added to the user's `PATH`.
3. **Managed configuration.** The desktop app writes and owns a private
   Codex configuration directory (`CODEX_HOME`). The user's `~/.codex`, if
   any, is never read or written.
4. **Pi Dash login is the only login, and Pi Dash settings are the only
   LLM settings.** Model calls go to the OpenHub gateway with a token
   derived from the user's Pi Dash session. Which model, and whose key
   (platform or BYOK), is whatever the user configured in Pi Dash
   settings, resolved through the same seam Pi Dash AI and the Cloud Agent
   use (`ee/assistant/model_provider.py`). **MVP: OpenHub lane only** —
   BYOK users see the managed runner as unavailable with a reason, because
   no stored key is ever returned to a client (§12.2). No API key of any
   kind lives on the laptop.
5. **Third executor kind, shared mechanism.** `AgentExecutorKind` gains
   `managed_runner`. It exists to carry _policy_ (availability, scheduling,
   metering, trust, UX). Execution goes through the existing local-runner
   machinery unchanged — daemon, worktrees, approvals, WebSocket protocol.
6. **Prompts stay outside the binary.** Task prompts continue to come from
   `prompting/` over the wire; managed-runner recipes alias the local-runner
   recipes (§10). Base instructions, when needed, go in the managed config.
7. **Tools are the bundled `pidash` CLI**, exactly as for a local runner
   today (`"pidash-cli"` prompt section). MCP is a v2 option, not an MVP
   dependency (§11).
8. **The Cloud Agent is not replaced.** It remains the executor for web
   users, scheduled work, and anyone without the desktop app.

## 3. Goals

- Execute general-purpose tasks, including coding and non-coding work, in a
  managed working folder. Git and a project repository URL are optional. This
  applies equally to built-in and user-connected direct-mode local runners.

- A new desktop user goes from "installed" to "an agent is editing my repo"
  with exactly one login and no terminal.
- Every desktop user runs a Codex version Pi Dash chose and can reproduce
  in support.
- A user who already has Codex, or later installs it, is unaffected in
  both directions (§13).
- Server-side changes are additive and small; nothing about local-runner
  or Cloud Agent behaviour changes.
- The path to deeper Codex integration later (fork, extension API) is
  preserved and does not require redoing any of this work (§21).

## 4. Non-goals

- Modifying, forking, embedding, or re-implementing Codex.
- Replacing the Cloud Agent or the user-installed local runner.
- Supporting agent kinds other than Codex in the managed runner (v1).
- Reusing a user's personal ChatGPT subscription or `~/.codex` login.
- A Pi Dash MCP tool server (possible v2; §11).
- The BYOK lane on the desktop engine. Deliberately deferred (§12.2, §21).
- Headless / server-hosted managed runners. The kind is named to allow it;
  nothing here designs it.

**Pre-existing constraints, unchanged by this design and not solved here.**
Both apply identically to a hand-installed local runner and to the Cloud
Agent today; this design only makes more users reach them by removing the
setup steps in front of them. It surfaces the existing error (§9.4, §17)
and nothing more:

- _Git and git credentials for repository-backed work only._ Clone and push use the
  machine's own git, per `clone_auth_mode = runner_managed`
  (`git_support_generalization/design.md`).
- _OpenHub wallet balance._ Every OpenHub-lane run bills the wallet; an
  empty wallet is the existing `OpenHubWalletEmpty` 402
  (`pi_dash_cloud/openhub/llm.py:classify_exception`).

## 5. Conceptual model

Two orthogonal choices exist today and both stay:

```
Executor (server policy)         Agent kind (runner config)
────────────────────────         ──────────────────────────
cloud_agent      → Celery, no sub-choice
local_runner     → pidash daemon → codex | claude_code | cursor_agent | grok | openclaw
managed_runner   → pidash daemon → codex (fixed, bundled)      ← NEW
```

A managed runner is a `Runner` row whose `DevMachine` was enrolled by the
desktop app rather than by `pidash auth login`. From the daemon's point of
view it is an ordinary `[[runner]]` entry with `agent.kind = codex` and an
absolute `codex.binary`. From the server's point of view it is a runner
whose `provisioning = desktop_bundled`, dispatched only for
`executor_kind = managed_runner` runs.

The three user-facing rules that make the rest work:

- **the bundled Codex is an implementation detail of the app, not a
  program the user owns**;
- **executor choice is visible at the point of action**, never inferred
  from which client the user happens to be in;
- **the managed runner is online exactly while the desktop app is open.**

## 6. Architecture

```
┌──────────────── Pi Dash Desktop (Tauri, proprietary) ────────────────┐
│  webview (cloud SPA + desktop overlay)                                │
│     │ invoke()                                                        │
│  Rust host                                                            │
│     ├── session (OIDC deep-link → /api/auth/desktop-exchange/)        │
│     ├── managed-runner controller  ── writes ──►  <app-data>/         │
│     │       • enroll DevMachine / Runner per project     managed/     │
│     │       • write pidash config + CODEX_HOME             ├─ pidash/ │
│     │       • start/stop bundled pidash daemon             └─ codex/  │
│     │       • mint gateway token, refresh                             │
│     └── IPC client (existing protocol) ◄─────┐                        │
│                                              │                        │
│  bundle resources (not on PATH)              │                        │
│     ├── pidash   (OSS runner, pinned)  ──────┘  daemon                │
│     └── codex    (upstream release, pinned)  ◄── spawned by daemon    │
└───────────────────────────────────────────────────────────────────────┘
          │ WebSocket / REST (existing runner protocol)      │ HTTPS
          ▼                                                  ▼
   Pi Dash Cloud API  ──────────────────────────────►  OpenHub gateway
   (executor policy, dispatch, approvals, events)      POST /v1/responses
```

The install brings three components onto the machine, and the way Pi Dash
works does not change:

1. **the Tauri app** — the Pi Dash UI plus a small host that provisions
   and supervises the other two;
2. **the `pidash` daemon** — the same CLI/daemon a user would install by
   hand, bundled and pre-enrolled;
3. **the built-in agent engine** — upstream Codex, bundled, driven by the
   daemon through the existing `AgentBridge::Codex` path.

A run flows exactly as it does for any local runner:

1. the user creates or triggers an issue in Pi Dash — the data lives in
   the cloud;
2. the cloud creates the `AgentRun`, resolves its executor, and assigns it
   to a runner over the existing runner protocol;
3. the bundled daemon picks the assignment up as a **native runner**,
   prepares a task folder or clones/reuses a configured repository (§9.4), spawns the built-in
   engine with the composed prompt, relays approvals and events, and
   reports completion.

Nothing in the run-time data path is new. The desktop app automates what
`pidash auth login` + `pidash runner add` + "install Codex" + "configure a
provider" do by hand, and the server learns one new executor kind so it can
route to, and reason about, runners that were provisioned this way. The
runner concept is not bypassed for the built-in engine; the built-in
engine _is_ a runner with a Pi Dash-controlled lifecycle.

## 7. Data model

### 7.1 `AgentExecutorKind` (`core/agent_execution.py`)

```python
class AgentExecutorKind(models.TextChoices):
    LOCAL_RUNNER   = "local_runner",   "Local Runner"
    CLOUD_AGENT    = "cloud_agent",    "Pi Dash Cloud Agent"
    MANAGED_RUNNER = "managed_runner", "Pi Dash Agent"
```

`AgentRun.executor_kind` is `max_length=24`; `managed_runner` is 14 chars.

### 7.2 `Runner.provisioning` (new field)

```python
class RunnerProvisioning(models.TextChoices):
    MANUAL          = "manual",          "Enrolled by the user"
    DESKTOP_BUNDLED = "desktop_bundled", "Provisioned by Pi Dash Desktop"

provisioning = models.CharField(max_length=24, choices=RunnerProvisioning.choices,
                                default=RunnerProvisioning.MANUAL, db_index=True)
```

Set at enrollment and immutable thereafter. This is the field every
"special arrangement" hangs off (§8.4, §15). It is deliberately a column,
not a `dev_metadata` key: dispatch and availability queries filter on it.

`DevMachine` gets the same field, set by the desktop enrollment path, so a
machine and all runners created on it agree.

### 7.3 `AgentRun` check constraint (migration required)

`agent_run_cloud_has_no_local_assignment` (`runner/models.py:995`) currently
admits only `executor_kind = local_runner` **or** `cloud_agent` with the
local-assignment columns null. A `managed_runner` row satisfies **neither
branch** and would be rejected at insert. The migration replaces the
constraint with:

```python
Q(executor_kind__in=[LOCAL_RUNNER, MANAGED_RUNNER])
| Q(executor_kind=CLOUD_AGENT, runner__isnull=True, pinned_runner__isnull=True,
    owner__isnull=True, assigned_at__isnull=True, queue_position__isnull=True)
```

This is the one change that fails loudly at runtime if forgotten; it is
first in the implementation order (§22). The migration ships with a test
that applies it to a database holding existing `local_runner` and
`cloud_agent` rows, inserts one `managed_runner` row with a pinned runner,
and asserts both the insert and the reverse migration succeed (§23.9).

### 7.4 Unchanged

`Project.default_agent_executor`, `Issue.agent_executor` (per-issue
override), `AgentRun.pinned_runner`, `Runner.pod`, `DevMachine`,
`MachineToken` — all reused as-is.

## 8. Executor policy: creation, availability, dispatch

### 8.1 Where the codebase branches today

Every executor branch in the server tests `== CLOUD_AGENT` with an implicit
`else` meaning "local": `cloud_agent/creation.py:27,96`,
`cloud_agent/dispatch.py:28,65`, `cloud_agent/tasks.py` (5 sites),
`orchestration/scheduling.py:515,674,742`,
`runner/services/agent_run_finalization.py:44,156`, `runner/models.py:999`.
Nothing tests `== LOCAL_RUNNER`.

**Consequence:** a `managed_runner` run falls into the local path at every
one of those sites with no edit. Only the sites below change, and each
change is a deliberate policy decision.

Two string-literal sites must be corrected regardless:
`orchestration/service.py:549` hard-codes the fallback
`{"executor_kind": "local_runner", ...}` and `:844`/`:890` compare to
`"cloud_agent"`. Replace with the enum; the fallback stays `LOCAL_RUNNER`.

### 8.2 Resolution (`cloud_agent/policy.py:resolve_executor_kind`)

Unchanged signature. Add the availability gate for the new kind:

```python
if value == AgentExecutorKind.MANAGED_RUNNER and not managed_runner_is_enabled():
    raise ManagedRunnerUnavailable("Pi Dash Agent is not enabled on this instance")
```

`managed_runner_is_enabled()` is the `MANAGED_RUNNER_ENABLED` kill switch
(§16), mirroring `cloud_agent_is_configured()`.

### 8.3 Availability (`core/agent_execution.py:agent_executor_options`)

Availability for a managed runner is **viewer-and-device scoped**, which is
the property that justifies a separate kind: "is _this user's_ desktop app
open right now?", not "does any runner exist on this project?".

```python
def managed_runner_availability(project, user) -> tuple[bool, str]:
    """Single source of truth for the picker, the profile endpoint (§12.2.1)
    and creation (§8.4). Reason precedence is fixed; the first failing gate
    wins so the UI copy is deterministic."""
    if not managed_runner_is_enabled():
        return False, "managed_runner_disabled"
    if user is None:
        return False, "desktop_not_connected"
    profile = managed_llm_profile(user)          # §12.2.1 — same seam as Pi Dash AI
    if not profile.available:
        return False, profile.reason_code        # llm_config_missing | gateway_scopes_missing | byok_not_supported_on_desktop
    enrolled = Runner.objects.filter(
        owner=user,
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
        pod__project_id=project.id,
        workspace_id=project.workspace_id,
        revoked_at__isnull=True,
    )
    if not enrolled.exists():
        return False, "no_managed_runner_for_project"
    online = enrolled.filter(
        status=RunnerStatus.ONLINE,
        last_heartbeat_at__gte=timezone.now() - HEARTBEAT_GRACE,
    ).exists()
    return (True, "") if online else (False, "desktop_not_connected")
```

Lives in a new `pi_dash/managed_runner/policy.py` (the package mirrors
`cloud_agent/`: `policy.py`, `checks.py`, `errors.py`). `HEARTBEAT_GRACE`
is the matcher's existing constant. `no_managed_runner_for_project` is
the one reason the desktop fixes silently (§9.3 step 5).

The options list returns three entries. Local availability must now
**exclude** `provisioning=desktop_bundled` runners so the picker never
reports "a local runner is available" on the strength of the bundled one.
The same exclusion applies to `count_active` / `can_register_another`
(`Runner.MAX_PER_USER`), and the managed cap is enforced separately in the
enrollment endpoint (§9.3, §16).

### 8.4 Dispatch and matching

Managed runs are always **pinned**. `execution_fields()`
(`cloud_agent/creation.py:11`) gains a `MANAGED_RUNNER` branch that calls
`managed_runner_availability(project, actor)`, refuses on any failing
reason (`ManagedRunnerUnavailable(code)`; §8.5 relaxes this for automatic
runs), and returns the creator's bundled runner on the project's pod in a
new key:

```python
return {"executor_kind": executor, "tool_plan": {}, "pinned_runner_id": runner.id}
```

The three creation sites that unpack this dict
(`orchestration/service.py:428,725,900`) pass `pinned_runner_id` into
`AgentRun(...)`; today they only forward `executor_kind` and `tool_plan`.

**Delivery path.** `next_queued_run_for_pod()` deliberately excludes
pinned runs (`matcher.py:136`), so a pinned managed run is never handed
out by `drain_pod()`. It is delivered only by `drain_for_runner()` →
`next_for_runner(runner)` (`matcher.py:171`), which fires on the pinned
runner's heartbeat and reconnect. This is exactly the behaviour wanted: a
run created for _this_ desktop executes on _this_ desktop when it is
online, and on no other runner ever — including a teammate's bundled
runner on the same pod.

`select_runner_in_pod()` and `drain_pod()` must exclude
`provisioning=desktop_bundled` runners so unpinned local-runner work is
never assigned to a managed runner. Managed runners serve pinned managed
runs only.

`pod_has_runner_for_issue_principal()` (the scheduler preflight) is
executor-aware: for `MANAGED_RUNNER` it asks only the _structural_
question its docstring already frames — "is a bundled runner owned by the
run creator **enrolled** on this pod (not revoked)?" — and ignores
transient status, exactly as it does for local runners. Online-ness is
handled by §8.5, not by the preflight.

### 8.5 Scheduled and automatic work

The scheduler and ticker create runs with no client present. A managed
runner is online only while an app window is open. The policy separates
the **structural** case (nobody could ever serve this) from the
**transient** one (the laptop is closed right now), matching how the
codebase already treats local runners (`matcher.py:387`: OFFLINE runners
"still count as registered … `drain_pod` re-fires on heartbeat").

| Situation                                               | User-triggered run (a person clicked Run)                                                                                       | Automatic run (ticker / scheduler)                                                                                                                                                                                                                |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| No bundled runner enrolled for the creator on this pod  | refuse at creation: `no_managed_runner_for_project`                                                                             | **bounce** via `_bounce_issue_no_eligible_runner(reason="no-managed-runner")` — the existing Backlog-move + comment                                                                                                                               |
| Enrolled but offline                                    | refuse at creation: `desktop_not_connected` (immediate feedback; the desktop is by definition open when the click came from it) | **create the pinned run and queue it visibly**: `AgentRun.status = QUEUED`, `error_code = "desktop_not_connected"` surfaced in the run list and on the issue ("Waiting for your desktop"). Delivered by `drain_for_runner` on the next heartbeat. |
| Queued longer than `MANAGED_RUNNER_QUEUED_MAX_AGE_SECS` | —                                                                                                                               | fail the run with `desktop_not_connected` and post the notice, via the same periodic sweep pattern the Cloud Agent uses for stale queued runs (`cloud_agent/tasks.py:216`)                                                                        |

Never a silent fallback to `cloud_agent`: the Cloud Agent cannot do the
same job, and a downgrade the user did not choose is worse than a visible
wait. A future `MANAGED_RUNNER_SCHEDULED_FALLBACK` may allow opt-in
fallback per project; not in v1.

The three scheduler sites that branch on `CLOUD_AGENT` today
(`orchestration/scheduling.py:515,674,742`) gain the `MANAGED_RUNNER`
branch above; everything else falls through to the local path unchanged.

### 8.6 Choosing the executor from the desktop

The executor remains a property of the work (`Project.default_agent_executor`
/ `Issue.agent_executor`), **not** of the client, and **the desktop never
mutates a project's default**: a project is shared, and flipping its
default to `managed_runner` would make every teammate's web-triggered run
wait on one person's laptop.

Instead the desktop pins **the issue**:

1. The Run action in the desktop overlay shows "Runs on: this computer"
   when `managed_runner_availability` is true for the viewer.
2. On click it sets `Issue.agent_executor = managed_runner` through the
   existing issue API (`app/serializers/issue.py:256`, which already
   refuses the change while a run is active), then triggers the run
   through the existing path. `execution_fields(requested=issue.agent_executor)`
   does the rest.
3. The pin is visible on the issue ("Pinned to Pi Dash Agent on
   `<hostname>`") and clearable through the same field. Scheduled
   re-runs of a pinned issue follow §8.5.

A project owner who _wants_ every run on desktops can still set
`default_agent_executor = managed_runner` in project settings; that is a
deliberate project-level choice, not something onboarding does.

Explicit beats implicit here because the executors have different
capabilities; a silent switch would produce "why did it only comment?"
tickets.

## 9. Managed runner provisioning (desktop side)

### 9.1 Bundle layout

Tauri `bundle.resources` (not `externalBin`; the project deliberately does
not ship `tauri-plugin-shell`, `pidash_cli.rs` header) carries:

```
<app-resources>/
  bin/pidash[.exe]                OSS runner, pinned tag (AGPL-3.0-only, unchanged)
  bin/pidash-agent-engine[.exe]   upstream codex release asset, pinned tag (Apache-2.0,
                                  unchanged bytes; file name per §13.3, verified §24.8)
  licenses/               LICENSE + NOTICE for both, surfaced in About
```

Neither binary is added to `PATH`. The Rust host spawns `pidash` by
absolute path; `pidash` spawns `codex` by the absolute `codex.binary` in
its config. On macOS both binaries are re-signed with the Pi Dash
certificate during notarization (§24.2).

### 9.2 Per-user data directory

```
<app-data>/managed/
  pidash/
    config.toml             Config { runners: [...], workdirs: [...], cli: {token} }
    credentials             MachineToken
    data/ , runtime/        passed as data_override / runtime dir
  codex-home/
    config.toml             §12 — model provider, instructions, policy
    (no auth.json)          model auth is env-injected per spawn, never persisted
  workdirs/
    <workspace-slug>/<project-slug>/   working copy per project (direct mode, §9.4)
  runtime/
    model.token             gateway token, 0600, rewritten by the host (§14.3)
```

`Paths::resolve(config_override, data_override)` (`util/paths.rs:28`)
already supports relocating the runner's directories; the desktop passes
both so a user-installed `pidash` and the bundled one never share state.

### 9.3 Enrollment sequence (first launch, and per new project)

**Who talks to whom.** The Pi Dash session lives in the webview's cookie
jar, not in the Rust host. All session-authenticated HTTP calls are made
by the **desktop overlay (JS)** using the SPA's existing CSRF-aware client;
results are handed to the host through `invoke()`. The host never replays
cookies. (Tauri 2.11's `cookies_for_url` exists as a fallback; not used in
v1.) All enrollment _logic_ stays in the OSS runner: the host drives it
through two hidden subcommands added to `pidash` so that `runner_ops`
(`write_cli_token`, `apply_enroll_response`, config writes) is never
re-implemented in a second binary.

```
pidash __managed bootstrap --cloud-url <url> --workspace <slug> --machine-token-stdin
pidash __managed enroll    --workspace <slug> --project <slug> \
                           --engine <abs path> --codex-home <abs path> \
                           --working-dir <abs path> --path-prepend <abs bin dir> \
                           --model-token-file <abs path>
pidash __managed remove    --workspace <slug> --project <slug>
```

Both honour `PIDASH_CONFIG_DIR` / `PIDASH_DATA_DIR` like every other
subcommand, which is how the managed tree (§9.2) is selected.

Sequence:

1. **Desktop session marker.** The exchange view
   (`AIRepublicDesktopExchangeView`) already validates
   `expected_kind="desktop"` on the OIDC state; it now also mints the
   session pair with a `client: "desktop"` claim (`tokens.mint_pair`
   gains the kwarg). A DRF permission `IsDesktopSession` gates every
   endpoint below (§14.5).
2. **Machine token — per workspace.** `DevMachine` and `MachineToken` are
   workspace-scoped, so enrollment is per workspace the user opens.
   `POST /api/v1/runner/dev-machines/desktop-enroll/` body
   `{workspace_slug, host_label}` creates or reuses a `DevMachine` with
   `provisioning = desktop_bundled` and mints a `MachineToken`
   (`is_service=True`, label `desktop`), returned once. This is the
   session-cookie equivalent of `/api/v1/auth/device/token/`. The overlay
   invokes the host, which runs `__managed bootstrap` — the same
   `write_cli_token` the CLI login performs (`cli/auth/login.rs:57`), so
   `[cli].token` **is** the machine token, exactly as for a hand-installed
   runner. There is no separate user token.
3. **Runner — per (workspace, project).** When the user opens a project,
   the host runs `__managed enroll`, which calls the daemon's existing
   machine-token create path (`cloud::runners::register_runner` →
   `RunnerCreateEndpoint`, `POST /api/v1/runner/runners/`) with the
   project's default pod and applies the response with
   `apply_enroll_response`, then writes the `[[runner]]` block:

   ```toml
   [[runner]]
   name = "desktop-<hostname>"          # per-pod unique; "-2", "-3" on collision
   project_slug = "<project>"
   pod_id = "<uuid from response>"
   workspace = { slug = "<workspace>" }
   working_dir = "<app-data>/managed/workdirs/<workspace>/<project>"   # direct mode, §9.4
   [runner.agent]   kind = "codex"
   [runner.codex]
   binary = "<app-resources>/bin/pidash-agent-engine"
   codex_home = "<app-data>/managed/codex-home"
   path_prepend = "<app-resources>/bin"
   model_token_file = "<app-data>/managed/runtime/model.token"
   ```

   The server sets `provisioning = desktop_bundled` from the enrolling
   `DevMachine`; the body cannot. It enforces
   `MANAGED_RUNNER_MAX_PER_USER_PROJECT` and excludes bundled runners
   from `Runner.MAX_PER_USER`.

4. **Managed Codex config.** The host writes `codex-home/config.toml`
   (§12.1) from the profile (§12.2.1) and creates the directory before
   the daemon starts (Codex refuses a missing `CODEX_HOME`).
5. **Start the daemon.** `pidash start` delegates to systemd/launchd
   (`cli/start.rs`); the desktop instead spawns the hidden foreground
   entry point **`pidash __run`** (`cli/mod.rs:141`) as a child process
   with `PIDASH_CONFIG_DIR` / `PIDASH_DATA_DIR` set, no console window on
   Windows. It stops when the app exits; the runner goes `OFFLINE`
   through the normal heartbeat lapse. Opening a project that has no
   runner yet runs step 3 and the daemon picks the new `[[runner]]` up
   through its existing config reload — this is the silent fix for
   `no_managed_runner_for_project`.
6. **IPC.** The host connects to `Paths::ipc_socket_path` with the
   existing protocol (`runner/src/ipc/protocol.rs:34`) for status,
   approvals, and doctor — the same surface the TUI uses.

`MachineMsg::CreateRunner` (`cloud/protocol.rs:404`) — the cloud-pushed
creation the web "Add runner" modal drives — remains available but is not
used here: the desktop already holds a session, and the REST + `__managed
enroll` path keeps the cloud out of the loop for a purely local decision.

### 9.4 Work directory — Git optional, provisioned by the runner

**General-purpose task contract (2026-09-07 clarification).** Neither built-in
nor user-connected direct-mode runners require a Git repository. Without a
repository URL, the runner creates or reuses the configured task directory,
preserves existing files, and starts the selected agent. It does not run
`git init`, invent a remote, or require commits/PRs. Branch checkout is skipped
for ordinary folders. Task prompts must support non-coding results and files.
With a repository URL, existing clone/auth safeguards remain: clone into an
empty folder, reuse an existing repository, and refuse to overwrite a
non-empty non-repository folder. Explicit worktree pools remain Git-specific;
they are not used for repo-free tasks. These rules supersede the former
clone-first assumption in this section.

The managed runner is a runner. It obtains its working copy the way a
cloud-generated runner already does, with no user step:

1. Enrollment (§9.3) creates the runner with `working_dir` under the
   managed data directory — `<app-data>/managed/workdirs/<project-slug>`
   — instead of the `$TMPDIR/.pidash` sandbox `Paths::default_working_dir`
   gives a `MachineMsg::CreateRunner` with an empty `working_dir`
   (`daemon/machine_control.rs:229`). Same mechanism, a durable location.
2. When a repository is configured, the first assignment carries `repo_url`, `repo_ref`, and
   `git_work_branch` (`cloud/protocol.rs:279`) from the project's
   repository binding. The supervisor calls
   `workspace::resolve(&wd, repo_url)` (`daemon/supervisor.rs:2857`),
   which clones into the empty directory and thereafter reuses it
   (`Resolution::Cloned` / `Resolution::ExistingRepo`).
3. Clone and push use the machine's own git credentials — the
   `clone_auth_mode = runner_managed` policy in
   `git_support_generalization/design.md` §"Runner Clone Authentication".
   The desktop's users are developers on their own machines; nothing is
   minted or sent. A clone failure surfaces git's message verbatim
   ("Permission denied (publickey)") with a "set up git access" link.
4. Git is needed only for repository operations, not for task-folder runs.
   Git is not bundled.

**Worktrees.** v1 uses the direct-in-`working_dir` mode the cloud-generated
runner uses today. Promoting the clone to a `[[workdir]]` pool
(`.ai_design/worktree_pooling`) for parallel issues is a follow-up: the
canonical clone already exists at that point, so it is a config change,
not a re-clone. Either way the agent works on `git_work_branch` in a
Pi Dash-owned directory, never in a checkout the user has open in an
editor; the first-run screen says so.

Letting a user point the runner at an existing local clone instead is an
optional later convenience (`pidash workdir add` semantics), not part of
the MVP path.

Pi Dash-managed clone credentials (short-lived installation tokens per
assignment) are only needed for a headless managed runner on a machine
with no developer on it — the git design's deferred phase, and out of
scope here.

### 9.5 Lifecycle

- **Upgrade.** Codex and `pidash` versions are pinned per app release and
  update with the app through the existing updater
  (`api/desktop/updates/…`). On upgrade the host rewrites `codex.binary`
  and restarts the daemon.
- **Sign-out.** Stop the daemon, delete `runtime/model.token`, call
  `DELETE /api/v1/runner/dev-machines/desktop-enroll/` (revokes the
  `MachineToken`), remove `[cli].token` from the managed config. Runner
  rows remain and go `OFFLINE`; they are reused on the next sign-in on
  this machine.
- **Mid-run app close or upgrade.** The host waits for the daemon's
  current run to reach a terminal state (IPC `StatusGet`) for up to
  `MANAGED_RUNNER_GRACEFUL_STOP_SECS`, then stops it; the existing reaper
  handles anything that survives.
- **Uninstall.** Runner rows go `OFFLINE` and are cleaned by the existing
  reaper; no cloud call is required.

## 10. Prompts

Three layers, none inside the Codex binary:

| Layer                            | Source                                                                                                                     | Change latency  |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | --------------- |
| Task prompt (per run, per phase) | `prompting/` composer, sent in `turn/start`                                                                                | next run        |
| Repo conventions                 | `AGENTS.override.md` / `AGENTS.md` in the worktree (`core/src/agents_md.rs`, walks cwd → project root; override name wins) | next run        |
| Base instructions                | `instructions` / `model_instructions_file` in managed `CODEX_HOME/config.toml`                                             | next app launch |

### 10.1 Recipes

`prompting/recipes.py` gains:

```python
MANAGED_RECIPES: dict[str, tuple[str, ...]] = dict(RECIPES)

def managed_recipe_for(kind: str) -> tuple[str, ...]: ...
```

An alias, not a copy: the managed runner has the same capabilities as a
local runner (filesystem, shell, `pidash` CLI), so the same sections apply.
Divergence, when wanted, is a per-key override in `MANAGED_RECIPES`.

`prompting/apps.py:35` gains the third completeness loop so a phase missing
a managed recipe fails at boot, not at run creation. `validation.py:231`
(section-usage lookup) walks the third map.

**Composer seam.** `composer.py` has three entry points today:
`compose()` (calls `recipe_for` at `:304`), `compose_cloud()` (`:323`,
`cloud_recipe_for` at `:326`), and `compile_template()` (`:351`). No new entry
point: `recipes.recipe_for(kind, executor_kind=LOCAL_RUNNER)` gains the
kwarg and returns `MANAGED_RECIPES[kind]` for `MANAGED_RUNNER`;
`compose()` gains the same kwarg and the local creation path passes
`run.executor_kind`. Per-user prompt overrides (`triggered_by`, design
§9.1) apply to managed runs exactly as to local ones because the
sections are the same customizable sections.

### 10.2 `AGENTS.md` placement

The runner already owns the worktree. v1 writes nothing into the repo;
`instructions` in the managed config carries Pi Dash's base guidance. If a
per-project layer is wanted later, prefer `project_doc_fallback_filenames`
(`config_toml.rs:314`) with a Pi Dash-specific filename that the workdir
pool gitignores, over touching the user's `AGENTS.md`.

## 11. Tools

The local-runner prompt recipes already instruct the agent to use the
`pidash` CLI (`"pidash-cli"` section; subcommands `issue`, `comment`,
`state`, `workpad`, `context`, `ai`, `run`, … — `runner/src/cli/mod.rs`).
The managed runner reuses this unchanged: the bundled `pidash` is on the
**agent's** `PATH` (injected at spawn, §12.3), not the user's.

**v2 option — MCP.** Codex's `mcp_servers` supports `streamable_http` with
`bearer_token_env_var` and `stdio` with `command`/`env`
(`config/src/mcp_types.rs:533`). OpenHub already exposes an MCP endpoint
that Pi Dash AI consumes (`pi_dash_cloud/openhub/mcp.py:build_toolset`).
Registering it in the managed config is a configuration change with no
runner work, and can ship independently of this design.

## 12. Managed Codex configuration

### 12.1 `CODEX_HOME/config.toml` (written by the desktop, never by the user)

```toml
# Managed by Pi Dash Desktop. Edits are overwritten on launch.
model_provider = "pidash"
model          = "<OPENHUB_DEFAULT_MODEL or user selection>"
approval_policy = "on-request"          # approvals surface via the runner → cloud UI
sandbox_mode    = "workspace-write"
instructions    = "<Pi Dash base guidance>"

[model_providers.pidash]
name      = "Pi Dash"
base_url  = "<OPENHUB_GATEWAY_BASE_URL>/v1"
wire_api  = "responses"                 # the only wire API Codex supports
env_key   = "PIDASH_GATEWAY_TOKEN"      # injected per spawn; never on disk
requires_openai_auth = false            # no login screen, no auth.json

[mcp_servers]                            # empty in v1 — §11
```

Exact `model_providers` fields per `model-provider-info/src/lib.rs:97`
(`base_url`, `env_key`, `experimental_bearer_token`, `auth` command-backed
token, `env_http_headers`, `requires_openai_auth`).

### 12.2 The engine honours the user's Pi Dash LLM settings

**Principle:** one LLM setting powers Pi Dash AI, the Cloud Agent, and the
desktop engine. The user configures it once in Pi Dash settings
(`users/me/ai-assistant/config/`, `assistant/views/llm_config.py`); the
engine never has a model setting of its own.

Pi Dash Cloud resolves that setting in one place —
`ee-overlay/…/ee/assistant/model_provider.py:resolve_model_for_user` —
into two user-facing lanes (a third, `UserCodexCredential`, exists in code;
§13.1):

| Lane                                                      | Setting                                                                                                    | How Pi Dash AI / Cloud Agent call the model today                                |
| --------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| **OpenHub** (default for cloud users with gateway scopes) | selected in settings; model via `openhub_model_for(user)`                                                  | OpenHub gateway, OIDC-derived gateway token (`pi_dash_cloud/openhub/llm.py`)     |
| **BYOK**                                                  | `UserLLMConfig`: `provider_kind ∈ {openai_compatible, anthropic}`, `base_url`, `model_name`, encrypted key | pydantic-ai directly against the vendor (`assistant/runtime/llm.py:build_model`) |

**Decision (MVP): the desktop engine supports the OpenHub lane only.**

| Pi Dash setting                 | Desktop engine                                                          | Why                                                                                           |
| ------------------------------- | ----------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| **OpenHub lane**                | available: `base_url = OpenHub /v1`, gateway token, user's chosen model | the credential is a short-lived token derived from the session — nothing stored is handed out |
| **BYOK** (either provider kind) | unavailable: `byok_not_supported_on_desktop`                            | see below                                                                                     |

BYOK works well for Pi Dash AI and the Cloud Agent because the stored key
is decrypted only inside a server process at the moment of use
(`assistant/runtime/llm.py:get_decrypted_api_key`; KMS-backed in the
cloud, `assistant/crypto.py`) and the API has **no read path** for it —
`users/me/ai-assistant/config/` returns `has_api_key`, never the key.
Making BYOK work on the desktop would mean adding the first endpoint that
returns a stored key to a client. That is a new credential-handling
surface, not an extension of the existing one, and it deserves its own
decision with usage data behind it (§21). The MVP does not open it.

Two further facts make OpenHub-only the natural MVP scope:

- Codex speaks exactly one wire protocol, the OpenAI Responses API
  (`WireApi::Responses`; `chat` is removed with an error). OpenHub serves
  `POST /v1/responses` natively where the provider supports it and
  translated otherwise (`openhub/gateway/README.md:153`), so every model
  in the OpenHub catalog is reachable, with fidelity reported per request
  in `X-OpenHub-Responses-Mode` (§24.9).
- OpenHub is the default lane for every cloud user whose session carries
  gateway scopes (`has_gateway_scopes`), so "OpenHub-only" covers the
  out-of-box user this design exists for.

Consequences:

- **One credential on the laptop**: the gateway token (§14.2). Wallet,
  metering, spend caps, and key policy are OpenHub's and apply unchanged.
- **Model name** flows from the setting (`openhub_model_for(user)`) into
  the managed `config.toml` `model` field; the resolved name and the
  observed Responses mode are recorded on the run (`model_started` event,
  `dev_metadata`) as the Cloud Agent already records its model.
- **BYOK users are told, not stranded.** The picker shows the managed
  runner as unavailable with "Pi Dash Agent on desktop uses OpenHub;
  switch your AI provider to OpenHub to use it here. Pi Dash AI and the
  Cloud Agent keep using your own key." Switching is the existing
  settings flow.
- **Nothing changes in OpenHub**, and no BYOK key moves anywhere.

A user whose session lacks gateway scopes is not offered the managed
runner, with the same `llm_config_missing` reason the Cloud Agent gives.

### 12.2.1 How the desktop learns the effective setting

The desktop does **not** read `users/me/ai-assistant/config/` and
re-implement resolution. A new endpoint next to the existing config one,
`GET /api/v1/users/me/ai-assistant/agent-profile/` (`assistant/views/`,
OSS; the lane logic is the EE-overlayable seam), gated by
`IsDesktopSession` (§14.5), runs the same `resolve_model_for_user` seam
and returns a Codex-shaped profile:

```json
{ "managed_runner_enabled": true,             // MANAGED_RUNNER_ENABLED — the desktop feature flag
  "lane": "openhub",
  "base_url": "https://…/v1",                  // OPENHUB_GATEWAY_BASE_URL — nothing baked into the bundle
  "model": "…",
  "available": true,
  "reason_code": "" | "llm_config_missing" | "gateway_scopes_missing" | "byok_not_supported_on_desktop" }
```

The gateway token is delivered separately (§14.3) and never in this body.
The host rewrites `config.toml` whenever the profile changes (settings
save → push over the existing desktop update channel, or poll on focus)
and restarts the daemon between runs, never mid-run.

`agent_executor_options` uses the same profile, so the picker and the
desktop always agree on why the managed runner is or is not available.

### 12.3 Per-spawn environment (runner change)

`AppServer::spawn` (`codex/app_server.rs:45`) launches Codex through
`login_shell_command`, i.e. `bash -ilc` on Unix (`util/shell.rs:88`). The
user's rc files run **after** `Command::env` values are applied, so a
user-level `export CODEX_HOME=…` or `PATH=…` would override what the
daemon set. The module already solves this for the working directory by
passing `PIDASH_AGENT_CWD` and re-asserting it inside the wrapper script
after rc files (`util/shell.rs:56`).

Extend the same mechanism:

```
[ -n "${PIDASH_AGENT_CWD-}" ]      && cd -- "$PIDASH_AGENT_CWD"
[ -n "${PIDASH_CODEX_HOME-}" ]     && export CODEX_HOME="$PIDASH_CODEX_HOME"
[ -n "${PIDASH_AGENT_PATH_PREPEND-}" ] && export PATH="$PIDASH_AGENT_PATH_PREPEND:$PATH"
[ -n "${PIDASH_GATEWAY_TOKEN_FILE-}" ] && export PIDASH_GATEWAY_TOKEN="$(cat "$PIDASH_GATEWAY_TOKEN_FILE")"
exec "$@"
```

The agent's own `pidash issue|comment|workpad|…` calls (§11) resolve
their cloud URL, workspace and token from the config in
`PIDASH_CONFIG_DIR` (`CliEnv::resolve`, `api_client.rs:112`: config
first, env second). The daemon therefore injects only
`PIDASH_CONFIG_DIR` and `PIDASH_DATA_DIR`, pointed at the managed tree —
**not** a raw `PIDASH_TOKEN` — and the bundled CLI on the agent's `PATH`
finds `[cli].token` (the machine token) the same way a hand-installed
runner's agent does today. The daemon already knows these two paths
(they are its own).

- `CodexSection` gains `codex_home`, `path_prepend`, and
  `model_token_file`, all `Option<PathBuf>` and empty for user-enrolled
  runners, so existing behaviour is untouched. `Config::validate()`
  requires all three or none.
- At daemon start the runner probes `<binary> --version` once and reports
  it in the session-open body as `engine_version`; the server whitelists
  it into `dev_metadata.codex_version` (`session_service._merge_dev_metadata`).
- The gateway token is read from a `0600` file in `runtime_dir` that the
  desktop host rotates (§14.2); it never appears in `config.toml` or the
  process argv. On Windows (`util/shell.rs:67`, direct spawn, no shell) the
  same values are set with `Command::env` directly.
- `CODEX_HOME` must exist as a directory or Codex refuses to start
  (`utils/home-dir/src/lib.rs:29`); the desktop creates it before the
  daemon starts.

## 13. Isolation and the three user situations

Two rules produce all three outcomes:

1. bundled binaries live inside the app and are never on the user's `PATH`;
2. the managed `CODEX_HOME` is separate from `~/.codex` and injected
   explicitly (§12.3), never inherited.

| Situation                                  | User's own Codex                                   | Pi Dash's Codex      | Result                                                                  |
| ------------------------------------------ | -------------------------------------------------- | -------------------- | ----------------------------------------------------------------------- |
| **A.** Already has Codex + ChatGPT login   | untouched; still works                             | own path, own config | no collision. Their ChatGPT plan is **not** used (§13.1)                |
| **B.** Fresh user, installs desktop        | absent                                             | the only one present | one login, click Run                                                    |
| **C.** Desktop first, installs Codex later | installs normally into `~/.local/bin` + `~/.codex` | unaffected           | no collision. Their Codex cannot reach Pi Dash's gateway token or tools |

### 13.1 Existing Codex users

The desktop **detects** an existing install (existing
`detect_pidash_cli`-style probe, extended to `codex`) to _inform_, never to
depend on. Reusing a personal ChatGPT session **from the laptop** is out of
scope: it is an OAuth session in the user's `~/.codex/auth.json`, and
borrowing it means either reading their credential file or inheriting
their whole config (model, sandbox, MCP servers, hooks) — an
unreproducible support surface.

"Use my own model account" is the BYOK lane of Pi Dash settings, which
the desktop engine does not support in the MVP (§12.2). The code also
carries a `UserCodexCredential` lane — a Codex subscription access token
or minted API key the user connected to Pi Dash
(`pi_dash_cloud/assistant/services.py:73`). Neither is surfaced on the
desktop in v1; if either ever is, it must be delivered through a
deliberate credential endpoint (§21), never by reading the user's
`~/.codex`.

### 13.2 What breaks without the rules

- Writing to `~/.codex`: a later `codex login` overwrites the provider;
  Pi Dash's instructions and policy leak into the user's personal sessions.
- Bundled binary on `PATH`: the user's `codex` resolves to Pi Dash's pinned
  version.
- Relying on inherited env: the login shell's rc files win (§12.3).

### 13.3 Binding to the app — the engine is inert outside Pi Dash Desktop

Requirement: the built-in engine is a component of the desktop app, not a
program. It shares nothing with a standalone Codex — not a name, not a
config directory, not an auth mechanism — and it must be unusable from a
terminal, another app, or after the user signs out of Pi Dash.

An executable the user owns cannot be made un-runnable. It **can** be made
useless outside the app, which satisfies the intent:

| Property                          | How                                                                                                                                                                                                                                                                                               |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Separate namespace**            | own binary path under app resources, shipped as `pidash-agent-engine` rather than `codex` (rename verified in §24.8); own `CODEX_HOME`; own `pidash` config/data/runtime dirs; own workdir pool. Zero reads or writes to `~/.codex`, `~/.config/pidash`, or anything on the user's `PATH`.        |
| **No standalone auth path**       | `requires_openai_auth = false`; no `auth.json` is ever created; ChatGPT OAuth is never invoked. The only credential Codex sees is the gateway token, injected per spawn from a runtime file the desktop host owns and deletes on sign-out (§12.3, §14.2).                                         |
| **Auth inherited from Pi Dash**   | the gateway token is derived from the desktop session by the host and refreshed by the host. Nothing else can mint it.                                                                                                                                                                            |
| **Dead on sign-out**              | sign-out stops the daemon, deletes the token file, revokes the `MachineToken`, and removes the `[cli].token`. A copied binary plus a copied config directory has no way to reach the gateway or the cloud.                                                                                        |
| **Dead when the app is closed**   | the daemon is a child of the app, not a service; the token file lives in `runtime_dir` and is removed on exit.                                                                                                                                                                                    |
| **Not invokable from a terminal** | running the binary by absolute path yields a Codex with no provider credential (`env_key` unset → the provider refuses) and no MCP servers; running the bundled `pidash` without the override paths finds no config. Neither is on `PATH`, neither has a shell completion, neither is documented. |
| **Distinct wire identity**        | desktop model traffic is attributable to the app on the OpenHub side — mechanism is §24.4 (the gateway token is the user's IdP access token, so Pi Dash cannot add a claim to it; attribution has to be a request header or a per-client credential OpenHub defines).                             |

The one thing this does not do is DRM: a determined user with a live
session could extract the short-lived token while the app is open. That
is the same exposure the webview's cookie jar already has, and it is
bounded by the token TTL and by sign-out.

## 14. Security

### 14.1 Trust boundary

The managed runner runs **on the user's machine as the user**, with the
same sandbox Codex provides to any local runner. It is not more trusted
than a user-enrolled runner from the server's point of view; the
`provisioning` field is a routing and product signal, not a privilege
grant. Any tool or write policy relaxed "because it's our binary" must be
argued separately; v1 relaxes nothing.

### 14.2 Credentials on the laptop

| Secret                | Where                                                        | Lifetime                                                                                            |
| --------------------- | ------------------------------------------------------------ | --------------------------------------------------------------------------------------------------- |
| Pi Dash session       | webview cookie jar (existing)                                | existing                                                                                            |
| `MachineToken`        | `managed/pidash/credentials`, `0600`                         | until sign-out / revoke                                                                             |
| OpenHub gateway token | `managed/runtime/model.token`, `0600`, rewritten by the host | the IdP access token's own TTL; refreshed by the host before expiry (§14.3), re-read per spawn      |
| `[cli].token`         | `config.toml` (existing `pidash` layout)                     | **is** the MachineToken (`cli/auth/login.rs:57` writes the machine token there); one token, not two |

No OpenAI key, no ChatGPT credential, no BYOK key, no `auth.json`.

**What the gateway token is.** `get_gateway_token(user)` returns the
user's live **AI Republic (home-page) access token**
(`pi_dash_cloud/openhub/tokens.py:31` → `token_store.get_valid_access_token`),
refreshed server-side from the stored refresh token. Delivering it to the
desktop means the user's own IdP access token sits on the user's own
machine for its TTL — the same class of exposure as the webview's session
cookies, bounded the same way (short TTL, deleted on sign-out, never
written anywhere but the `0600` runtime file). The refresh token never
leaves the server.

Open item §24.4: long runs must survive token expiry. Codex reads
`env_key` at spawn; a run longer than the token TTL fails at the gateway
with a 401 the bridge surfaces as `AwaitingReauth`. Options: a longer-lived
desktop-scoped gateway token, or Codex's command-backed `auth` provider
(`model-provider-info/src/lib.rs:113`) invoking a `pidash` subcommand that
returns a fresh token from the IPC socket. Decide before implementation.

### 14.3 Token delivery endpoint

`POST /api/v1/users/me/ai-assistant/agent-token/` — `IsDesktopSession`
only, rate-limited per user (`assistant_agent_token` throttle scope),
audited (`managed_runner.token_issued` log event with user, dev-machine,
request id — never the token). Returns
`{"token": "...", "expires_at": "<iso8601>"}` from `get_gateway_token(user)`.
Errors map 1:1 to the existing OpenHub taxonomy: `OpenHubAuthError` →
`401 gateway_session_revoked` (app prompts sign-in), `OpenHubUnavailable`
→ `503 gateway_unavailable` (app retries with backoff and keeps the
current token until it expires).

Client behaviour (overlay JS → host via `invoke`):

- fetch on daemon start, then re-fetch at 80% of the remaining TTL;
- write atomically (temp file + rename) to `managed/runtime/model.token`;
- the daemon reads the file **per spawn** (§12.3), so a rotated token
  applies to the next run without a restart;
- on `401` stop the daemon, delete the file, surface "Sign in again".

§24.4 covers the case of a single run outliving one token.

### 14.4 Run visibility

`_can_view_run` (`runner/views/runs.py:87`) already gates runs on a private
runner to the creator and runner owner. A managed runner is private by
construction (`Visibility.PRIVATE`, the only value), so desktop runs are
visible to the creator only, plus workspace admins through the existing
involvement grants. No change.

### 14.5 Server-side validation

`provisioning` is derived server-side from the enrolling `DevMachine`;
the request body cannot set it.

**Desktop-only endpoints** (`desktop-enroll`, `agent-profile`,
`agent-token`) require a session minted by the desktop exchange. Today
the exchange only checks `expected_kind="desktop"` on the OIDC _state_
and then mints an ordinary pair (`tokens.mint_pair(user.id, plan=…)`),
so nothing on the session says "desktop" afterwards. Change (private):
`mint_pair(..., client="desktop")` embeds a `client` claim on the access
token for the desktop path; `IsDesktopSession` is a DRF permission that
reads it. Web sessions carry no claim and are refused with `403
desktop_session_required`. The refresh path preserves the claim.

## 15. Product policy attached to `managed_runner`

The reason for a separate kind. Each is a small, named branch:

| Concern         | Rule                                                                                                                                                                          |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Availability    | online iff the creator's desktop app is open (§8.3)                                                                                                                           |
| Matching        | pinned to the creator's bundled runner; never receives unpinned local work (§8.4)                                                                                             |
| Scheduled work  | bounce with `desktop_not_connected`; no silent fallback (§8.5)                                                                                                                |
| Quota           | bundled runners do not count toward `Runner.MAX_PER_USER`; capped separately at one per (user, project)                                                                       |
| Registration UX | created by the app; hidden from the "Add runner" modal's list and from `pidash runner` CLI output by default                                                                  |
| Metering        | OpenHub meters per user (the gateway token is the user's own); attributing traffic to the desktop client is §24.4                                                             |
| Support         | `Runner.provisioning`, `runner_version`, and `dev_metadata.codex_version` (new whitelisted key in `session_service._merge_dev_metadata`) answer "which build is this user on" |
| Trust           | none relaxed in v1 (§14.1)                                                                                                                                                    |

### 15.1 Observability

Structured log events (same logger conventions as `cloud_agent/`):
`managed_runner.enrolled` / `.removed` (user, dev-machine, project),
`managed_runner.run_pinned` (run, runner), `managed_runner.queued_waiting`
and `.queued_expired` (§8.5), `managed_runner.token_issued` /
`.token_refresh_failed` (§14.3), `managed_runner.engine_version`
(runner, version) at session open.

Metrics (existing `metrics/` endpoint): gauge of online bundled runners;
counters for runs by `executor_kind`; histogram of queued-waiting age for
managed runs. Admin runner list gains a `provisioning` filter and shows
`dev_metadata.codex_version`.

The desktop reports enrollment and daemon-start failures through the
existing desktop update/telemetry channel with the doctor check id
(§17), never with tokens or paths under the home directory.

## 16. Configuration (server)

Registry (`config/registry.py`) and `settings/common.py`, following the
`CLOUD_AGENT_*` pattern:

| Key                                      | Default                    | Meaning                                                                 |
| ---------------------------------------- | -------------------------- | ----------------------------------------------------------------------- |
| `MANAGED_RUNNER_ENABLED`                 | `"false"`                  | kill switch: creation, availability, dispatch                           |
| `MANAGED_RUNNER_MAX_PER_USER_PROJECT`    | `1`                        | bundled runners per (user, project)                                     |
| `MANAGED_RUNNER_HEARTBEAT_GRACE_SECS`    | existing `HEARTBEAT_GRACE` | may be tightened later for "app closed" detection                       |
| `MANAGED_RUNNER_QUEUED_MAX_AGE_SECS`     | `43200` (12 h)             | automatic runs waiting for a closed desktop fail after this (§8.5)      |
| `MANAGED_RUNNER_GRACEFUL_STOP_SECS`      | `30`                       | desktop-side wait for a run to finish before stopping the daemon (§9.5) |
| `DESKTOP_MIN_VERSION_FOR_MANAGED_RUNNER` | `None`                     | refuse enrollment from older desktop builds                             |

Disabling `MANAGED_RUNNER_ENABLED` must not alter project settings; issues
pinned to `managed_runner` bounce with `managed_runner_disabled` until it is
re-enabled (mirrors `.ai_design/inhouse_runner/design.md` §16 rollback).

Desktop build inputs (CI): `PIDASH_BUNDLE_VERSION`, `CODEX_BUNDLE_VERSION`
(upstream release tag), plus the existing `PI_DASH_URL` /
`VITE_API_BASE_URL`. Nothing OpenHub-related is baked into the bundle:
`base_url` and the feature flag arrive in the profile response (§12.2.1).

## 17. Failure semantics

| Failure                                     | Surface                                       | Behaviour                                                                                                                              |
| ------------------------------------------- | --------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| Desktop closed while run `RUNNING`          | heartbeat lapse                               | existing reaper fails the run (`reaped by heartbeat`); issue returns to its previous state per existing rules                          |
| Gateway 401/402 mid-run                     | Codex → bridge `AwaitingReauth` / `Failed`    | 402 maps to OpenHub "wallet empty" wording (`llm.py:classify_exception`); 401 triggers host token refresh + user prompt                |
| `CODEX_HOME` missing                        | Codex exits at start                          | doctor check `managed.E001`; host recreates and retries once                                                                           |
| Bundled `codex` fails `--version`           | `pidash doctor` via IPC                       | app shows "Pi Dash Agent needs repair" with reinstall action                                                                           |
| Enrollment refused (`DESKTOP_MIN_VERSION…`) | HTTP 409                                      | app prompts to update                                                                                                                  |
| Project has no bundled runner yet           | `no_managed_runner_for_project`               | host runs `__managed enroll`, then retries the action                                                                                  |
| Gateway session revoked / sign-in expired   | `agent-token` → `401 gateway_session_revoked` | host stops the daemon, deletes the token file, prompts sign-in; an in-flight run fails at the gateway and surfaces as `AwaitingReauth` |
| Automatic run waiting for a closed desktop  | `QUEUED` + `error_code=desktop_not_connected` | visible on issue and run list; fails after `MANAGED_RUNNER_QUEUED_MAX_AGE_SECS` (§8.5)                                                 |
| App closed / upgraded mid-run               | IPC `StatusGet`                               | host waits `MANAGED_RUNNER_GRACEFUL_STOP_SECS`, then stops; reaper covers the rest                                                     |

## 18. Testing strategy

- **Server unit** (`tests/unit/runner`, `tests/unit/orchestration`,
  `tests/unit/prompting`): enum + constraint migration; `execution_fields`
  pins to the creator's bundled runner and refuses when offline;
  `agent_executor_options` three-way output and reason codes; matcher
  exclusions in both directions; scheduler bounce reason; recipe
  completeness check with the third map; serializer acceptance of the new
  value; `MAX_PER_USER` accounting.
- **Runner** (`runner/tests`): `login_shell_command` re-asserts
  `CODEX_HOME`/`PATH` after an rc file that clobbers them (Unix); Windows
  direct-spawn env; `CodexSection` new fields default to none;
  `spawn_from_config` passes the absolute binary.
- **Desktop** (`private-pi-dash/desktop`): enrollment against a mock cloud;
  config writer idempotency; token file rotation; daemon start/stop tied to
  app lifecycle; existing-Codex detection reports and does not depend.
- **End-to-end** (test env, behind VPN): fresh VM per OS → install → sign in
  → open project → run issue → code change lands in worktree → approval
  round-trips through the cloud UI. Repeat with a pre-installed user Codex
  (situation A) and post-install (situation C).

## 19. Rollout

1. Server: enum, migration, recipes alias, availability, matcher
   exclusions, settings — behind `MANAGED_RUNNER_ENABLED=false`. Ships
   independently and is inert.
2. Runner: `CodexSection` fields and env re-assertion. Backward compatible;
   release as a normal `pidash` tag.
3. Desktop: bundle + enrollment + managed config, gated by
   `managed_runner_enabled` in the profile response (§12.2.1) — i.e. the
   same server flag, no second switch.
4. Enable `MANAGED_RUNNER_ENABLED` on test; internal dogfood on all three
   OSes; measure setup time and first-run success.
5. Enable on prod for a cohort; watch `desktop_not_connected` bounce rate
   and gateway 401s.

Rollback is flag-first: `MANAGED_RUNNER_ENABLED=false` stops creation and
dispatch; existing desktop installs keep working for local-runner and
cloud runs.

## 20. Alternatives rejected

- **Hand-rolled engine.** The engine is the model loop _plus_ tools,
  sandbox, approvals, compaction, and tuning. Months of work to reach where
  Codex already is.
- **Embed `codex-core` in the runner.** Attractive (typed events, no
  subprocess), but `codex-rs/Cargo.toml:609` replaces `tungstenite`,
  `tokio-tungstenite` and `crossterm` with private forks via
  `[patch.crates-io]`. Cargo ignores `[patch]` from dependencies, so a git
  dependency would compile core against unpatched crates; mirroring the
  patches into the runner workspace would also swap the crate under the
  runner's own cloud WebSocket. Plus a Rust 1.95 toolchain pin (runner:
  1.93) and ~80 crates / 700k LOC in the build graph. Not worth it for a
  setup problem.
- **Fork Codex and strip it to "the core".** The pieces you would delete
  (`tui`, `cli`, `app-server`, `cloud-tasks`) are already outside
  `codex-core`'s dependency closure; Cargo never builds them. The pieces
  you would _have_ to delete are load-bearing inside core, and core
  exposes no `[features]`. Stripping is a fork with a manual re-strip on
  every upstream pull.
- **Put prompts in a fork.** Turns a one-line prompt change into a
  five-platform rebuild, sign, notarize, release, and user update.
- **Reuse `local_runner` for the bundled runner.** Zero server work, but
  availability would be project-scoped rather than viewer-and-device
  scoped, and unpinned local work could land on a laptop that closes at
  6pm. The policy in §15 needs a kind to hang on.
- **Route by client (web → cloud, desktop → managed) implicitly.** The two
  executors do different jobs; a silent switch is a support problem (§8.6).
- **Reuse the user's ChatGPT login.** §13.1.
- **A new Django model-proxy endpoint.** OpenHub already is one, and
  already speaks Codex's wire API (§12.2).

## 21. Future extensions

- **Deeper integration without a fork.** Codex ships an in-tree extension
  API (`codex-rs/ext/extension-api/`: `ToolContributor`,
  `ToolLifecycleContributor`, `ApprovalReviewContributor`,
  `Turn/ThreadLifecycleContributor`, `TurnInputContributor`,
  `ContextContributor`, `ConfigContributor`, `McpServerContributor`,
  `AgentSpawner`, `ResponseItemInjector`, …) that 14 of OpenAI's own
  features are built on. Using it requires linking `codex-core` (the
  embedding costs in §20 return, but then they buy something). If that day
  comes, build a single additive `ext/pidash` crate and register it in one
  place, so the diff against upstream stays one directory plus a few
  lines. Keep the `AgentBridge` seam (`runner/src/agent/mod.rs:268`) as
  the swap point; it is already vendor-blind.
- **A Pi Dash MCP server** in the managed config (§11).
- **BYOK on the desktop engine.** Requires the first endpoint that returns
  a stored key to a client — today the plaintext has no read path and
  never leaves a server process (§12.2). If usage data justifies it:
  session-authenticated, desktop-only, rate-limited, audited, delivered
  to the `0600` runtime file like the gateway token; `openai_compatible`
  only (Codex cannot call Anthropic), gated by a `POST /v1/responses`
  probe on the settings test endpoint so incompatible endpoints are
  refused at save time.
- **Managed runners beyond the desktop** (a Pi Dash-hosted VM per user).
  The kind name and `provisioning` enum leave room; nothing else here
  presumes it.
- **Scheduled-work fallback** to the Cloud Agent, opt-in per project.
- **Other agent kinds** in the managed runner, once their auth can be
  session-derived the way OpenHub's is.

## 22. Implementation map

| #   | Change                                                                                                                                                                                                               | Files                                                                                                                                                                                      |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Enum value; check-constraint migration; `Runner.provisioning`, `DevMachine.provisioning`; `RunnerProvisioning`                                                                                                       | `core/agent_execution.py`, `runner/models.py`, `db/migrations/…`                                                                                                                           |
| 2   | Settings + registry                                                                                                                                                                                                  | `config/registry.py`, `settings/common.py`                                                                                                                                                 |
| 3   | New package `managed_runner/` (`policy.py` with `managed_runner_availability`, `errors.py`, `checks.py`); `resolve_executor_kind` gate; `execution_fields` returns `pinned_runner_id`; creation sites forward it     | `pi_dash/managed_runner/`, `cloud_agent/policy.py`, `cloud_agent/creation.py`, `orchestration/service.py:428,725,900`                                                                      |
| 4   | `agent_executor_options` three-way via `managed_runner_availability`; local excludes bundled; `MAX_PER_USER` accounting                                                                                              | `core/agent_execution.py`, `runner/services/…count_active`                                                                                                                                 |
| 5   | Matcher exclusions; preflight executor-awareness                                                                                                                                                                     | `runner/services/matcher.py`                                                                                                                                                               |
| 6   | Scheduler: structural bounce vs visible queue; stale-queued sweep; literal fixes                                                                                                                                     | `orchestration/scheduling.py:515,674,742`, new periodic task, `orchestration/service.py:549,844,890`                                                                                       |
| 7   | `MANAGED_RECIPES`; `recipe_for(kind, executor_kind=…)`; `compose(executor_kind=…)`; startup loop; validation                                                                                                         | `prompting/recipes.py`, `prompting/composer.py:304`, `prompting/apps.py:35`, `prompting/validation.py:231`                                                                                 |
| 8   | Serializer acceptance + messages                                                                                                                                                                                     | `app/serializers/issue.py`, `api/serializers/project.py`                                                                                                                                   |
| 9   | `desktop-enroll` (POST/DELETE) endpoint; `RunnerCreateEndpoint` sets provisioning from the machine, enforces the managed cap; hide bundled runners from modal/CLI listings by default                                | `runner/views/register.py`, `runner/views/enrollment.py`, `runner/views/runners.py`, `runner/urls.py`                                                                                      |
| 9a  | `agent-profile` (GET) and `agent-token` (POST) endpoints; `IsDesktopSession`; `mint_pair(client=…)` on the desktop exchange                                                                                          | OSS: `assistant/views/`, `ee/assistant/model_provider.py` seam; private: `ee-overlay/…/model_provider.py`, `pi_dash_cloud/airepublic/{views,tokens}.py`, `pi_dash_cloud/openhub/tokens.py` |
| 10  | `dev_metadata.codex_version` whitelist                                                                                                                                                                               | `runner/services/session_service.py`                                                                                                                                                       |
| 11  | Doctor check `managed.E001`; `startup checks`                                                                                                                                                                        | `cloud_agent/checks.py`-style module `managed_runner/checks.py`                                                                                                                            |
| 12  | Runner: `CodexSection.{codex_home,path_prepend,model_token_file}`; wrapper-script re-assert + `PIDASH_CONFIG_DIR`/`PIDASH_DATA_DIR` injection; Windows env; `engine_version` in session-open; `\_\_managed bootstrap | enroll                                                                                                                                                                                     | remove` subcommands | `runner/src/config/schema.rs`, `runner/src/util/shell.rs`, `runner/src/codex/app_server.rs`, `runner/src/cloud/protocol.rs`, `runner/src/cli/managed.rs` (new) |
| 13  | Desktop: bundle resources; host controller (spawn `__run`, `__managed`, token file, graceful stop); overlay JS (enroll/profile/token calls, Run affordance, first-run copy); existing-Codex detector; About/licenses | `private-pi-dash/desktop/src-tauri/…`, `tauri.conf.json`, `release-desktop.yml`, `desktop-overlay/apps/web/…`                                                                              |
| 14  | Web: executor picker third option + reason copy; "Runs on this computer" affordance; project setting                                                                                                                 | `apps/web` (OSS) + overlay                                                                                                                                                                 |

Order: 1 → 2 → 7 (boot must pass) → 3–6, 8–11 in any order → 12 → 13 → 14.
Rows 1–11 ship inert behind `MANAGED_RUNNER_ENABLED=false`; row 12 is a
backward-compatible `pidash` release; row 13 depends on 9, 9a and 12.

## 23. MVP acceptance criteria

1. Fresh machine, each of macOS/Windows/Linux: install → sign in → open a
   project → "Run" on an issue → the agent modifies files in a Pi Dash
   worktree and an approval round-trips through the cloud UI. No terminal,
   no second login, no key entry.
2. `which codex` on the user's shell is unchanged before and after
   (absent stays absent; a user install stays theirs).
3. A user-installed Codex with a ChatGPT login is never read: its
   `~/.codex` mtime is unchanged after a managed run.
4. Closing the app takes the bundled runner `OFFLINE` within one heartbeat
   grace; a user-triggered run is refused with `desktop_not_connected`,
   and an automatic run queues **visibly** with that reason, is delivered
   on the next heartbeat, and fails after
   `MANAGED_RUNNER_QUEUED_MAX_AGE_SECS` (§8.5).
5. Unpinned local-runner work is never assigned to a bundled runner, and a
   bundled runner never appears as "local runner available".
6. `MANAGED_RUNNER_ENABLED=false` on the server stops new managed runs
   without touching project settings or breaking local/cloud runs.
7. Boot fails if any phase lacks a managed recipe.
8. The Codex version is visible per runner in admin and equals the pinned
   tag for that app version.
9. The check-constraint migration applies cleanly forward and backward on
   a database containing existing local and cloud runs, and a
   `managed_runner` insert succeeds only after it (§7.3).
10. After sign-out, invoking the bundled engine by absolute path cannot
    reach OpenHub or the Pi Dash API; the runtime token file no longer
    exists (§13.3).
11. A user whose Pi Dash AI provider is BYOK sees the managed runner as
    unavailable with `byok_not_supported_on_desktop` and an actionable
    message; switching the provider to OpenHub in settings makes it
    available without restarting the app (§12.2).
12. Running an issue from the desktop pins the issue
    (`Issue.agent_executor = managed_runner`) and never changes
    `Project.default_agent_executor`; a teammate's web-triggered run on the
    same project is unaffected (§8.6).
13. A web session cannot call `desktop-enroll`, `agent-profile` or
    `agent-token` (`403 desktop_session_required`); a desktop session can
    (§14.5).
14. A user in two workspaces gets one `DevMachine` per workspace, and
    `pidash issue` from inside a managed run resolves the right workspace
    from `PIDASH_CONFIG_DIR` alone, with no token in the environment (§9.3,
    §12.3).

## 24. Open items to close before implementation

1. **Binary size.** Release `codex` size per platform is unmeasured. It
   inflates every installer and doubles on disk for existing-Codex users.
   Build once, record, decide whether it changes anything.
2. **macOS notarization of a foreign-signed binary.** Confirm the
   re-signing step with whoever owns `release-desktop.yml`; Apache-2.0
   permits it.
3. **Windows.** `login_shell_command` on Windows spawns directly with no
   shell; verify the managed env path and Codex's Windows sandbox
   behaviour on a real install rather than assuming Unix parity.
4. **Gateway token TTL vs. run length, and client attribution.** The
   token is the user's home-page access token; its TTL bounds a single
   run unless Codex's command-backed `auth` provider is used to re-read
   the file mid-run (§14.2). Attribution cannot be a claim Pi Dash adds;
   OpenHub must define a request header or a per-client credential.
   Both need an answer from the OpenHub side before §14.3 is final.
5. **OpenHub `/v1/responses` coverage.** The gateway README says
   Responses is "served natively where the provider supports it". Confirm
   that the default desktop model's provider supports streaming +
   tool-calling over Responses, which Codex requires.
6. **Codex config exactness.** Field names in §12.1 are from
   `config_toml.rs` / `model-provider-info` at the pinned commit; re-verify
   against the chosen release tag, and add a CI check that fails when the
   pinned Codex's config schema diverges from what the desktop writes.
7. **Existing-Codex detection UX copy** (§13.1) and whether to expose the
   detection at all in v1.
8. **Binary rename.** §13.3 ships the engine as `pidash-agent-engine`, not
   `codex`. Codex's `arg0` crate dispatches on `argv[0]` when it re-execs
   itself for the Linux sandbox helper; confirm a renamed binary still
   self-locates (via `current_exe()`) on all three OSes, or fall back to
   the original name inside a Pi Dash-owned directory.
9. **`translated_lossy` handling.** Decide whether the engine should
   refuse, warn, or proceed when OpenHub reports lossy Responses
   translation for the user's chosen model (typical for Chat-only
   providers with Codex's encrypted-reasoning hints). Recommendation:
   proceed and record the mode on the run; surface a one-time notice in
   the desktop if a project's default model is lossy.

## 25. References

- `.ai_design/inhouse_runner/design.md` — Cloud Agent; the pattern this
  design mirrors for a new executor kind.
- `.ai_design/worktree_pooling/design.md` — `[[workdir]]` pools the
  managed runner uses per project.
- `.ai_design/n_runners_in_same_machine/design.md` — multiple `[[runner]]`
  blocks on one `DevMachine`.
- `.ai_design/runner_install_ux/design.md` — the manual path this
  automates.
- `core/agent_execution.py`, `cloud_agent/policy.py`,
  `cloud_agent/creation.py`, `prompting/recipes.py`, `prompting/apps.py`,
  `runner/services/matcher.py`, `runner/views/runs.py`.
- `runner/src/agent/mod.rs`, `runner/src/codex/app_server.rs`,
  `runner/src/util/shell.rs`, `runner/src/config/schema.rs`,
  `runner/src/ipc/protocol.rs`, `runner/src/cloud/protocol.rs`.
- `private-pi-dash/desktop/README.md`, `desktop/src-tauri/src/main.rs`,
  `desktop/src-tauri/src/pidash_cli.rs`,
  `pi_dash_cloud/airepublic/views.py` (desktop exchange),
  `pi_dash_cloud/openhub/{llm,tokens,mcp}.py`,
  `ee-overlay/…/ee/assistant/model_provider.py`.
- Upstream Codex at `5c0e582c59`: `codex-rs/Cargo.toml` (`[patch]`),
  `config/src/config_toml.rs`, `config/src/mcp_types.rs`,
  `model-provider-info/src/lib.rs`, `utils/home-dir/src/lib.rs`,
  `core/src/agents_md.rs`, `ext/extension-api/`.
- OpenHub: `openhub/gateway/README.md` (Responses API).
