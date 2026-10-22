# Design: the built-in engine as a first-class runner

- **Issue:** PIDESKAPP-121 (parent PIDESKAPP-120)
- **Status:** proposed, awaiting product-owner approval
- **Date:** 2026-10-13
- **Code baseline:** pi-dash `main` at `3267ceb9`. Paths are relative to that tree; `api/` means `apps/api/pi_dash/`.
- **Revises:** `.ai_design/managed_runner/design.md` (see section 14 for the list of replaced rules)

Every "today" statement below was read from the code at the baseline; nothing was run. Recommendations that need a product-owner decision are marked **[PO-n]** and collected in section 15.

## 1. Summary

| # | Question | Answer |
| --- | --- | --- |
| 1 | Machine identity | One computer with the desktop app is one dev machine (per workspace). Its daemon hosts both kinds of runner. A separately hand-installed `pidash` service stays its own machine. |
| 2 | Runner identity | The engine is identified per machine. The stored unit stays one `Runner` row per machine and project, created only when the engine is turned on for that project. The per-user cap goes away. |
| 3 | Executor model | The built-in engine becomes a runner like any other with a fixed agent. `managed_runner` stops being a selectable executor; "which runner" is expressed by pod or by a pinned runner. |
| 4 | Dispatch and pods | A run can be pinned to one runner (new `Issue.assigned_runner`). An engine takes unpinned pod work only after its owner turns that on, and ranks after ordinary runners. |
| 5 | Who may use it, who pays | Only the owner's work, billed to the owner's OpenHub wallet. A teammate's click never starts a run on it unless the owner opts in. BYOK stays unsupported. |
| 6 | Availability | An offline engine is treated like any offline runner for pod work. Runs pinned to an offline engine queue visibly and expire after 12 hours. |
| 7 | Default work dir | A per-machine, per-account setting holding the root folder. Existing task folders stay bound and are never moved or deleted. |
| 8 | Visibility | Engines appear in runner lists, machine pages, pickers and pods, labelled by machine, with a reduced action set. |
| 9 | Chat routing | Routed per runner. Only this computer's built-in contact uses the daemon socket; everything else goes through the server. |
| 10 | Own agent on desktop | Added as an ordinary runner on the desktop's own daemon through the existing Add-runner flow. No second CLI install or sign-in. |
| 11 | Migration | Rows and ids are kept. Engines take what they took before, with two stated exceptions: projects whose default is the built-in engine (their engines become pod members), and issues set to the built-in engine (they now name one person's engine). |

## 2. Terms

- **Desktop machine:** a `DevMachine` with `provisioning = desktop_bundled`, backed by the daemon the desktop app starts.
- **Built-in engine:** the bundled Codex driven by that daemon. Shown to users as "Built-in engine · <machine name>".
- **Engine runner:** a `Runner` row with `provisioning = desktop_bundled`; one per desktop machine and project.
- **Own-agent runner:** a `Runner` row with `provisioning = manual` whose agent the user supplies.

## 3. Constraints the design works within

These are facts of the current model that the design keeps, because changing them is a larger project than this feature.

- A runner belongs to exactly one pod and a pod to exactly one project (`api/runner/models.py:52-176`, `:382-498`). The daemon enforces "one runner, one project" and an exclusive, non-nested working dir per runner (`runner/src/config/schema.rs:681-690`, `:733-754`).
- A runner executes one run at a time (`runner/src/daemon/supervisor.rs:1385-1402`).
- One dev machine is one daemon, one machine token and one live session (`machine_session_one_active_per_machine`, `api/runner/models.py:752-757`). The desktop starts one daemon per workspace, so today there is one desktop `DevMachine` per computer and workspace (`api/runner/views/desktop.py:108-119`).
- Every runner is private. A runner only takes a run tied to its owner: created by the owner, already billed to the owner, on an issue the owner created or is assigned to, or from a scheduler binding the owner authored (`filter_runs_usable_by_runner`, `api/runner/services/permissions.py:83-123`). Lists and chat are owner-only.
- The engine's model credential is the signed-in user's OpenHub token, written by the app to one file on the machine (`desktop/src-tauri/src/managed_runner.rs:398-405`). Whatever runs on the engine is billed to the person signed in to that desktop.

## 4. Machine identity (point 1)

**Decision: one computer with the desktop app is one dev machine hosting both kinds of runner.** The desktop's daemon is the pidash daemon for that computer. **[PO-1]**

- The machine row is the existing `desktop_bundled` `DevMachine`. It stays one row per computer and workspace; the AI Dev Machines page is already workspace-scoped, so the user sees one row per computer.
- Own-agent runners on a desktop machine are added to the same daemon (section 12), so they sit under the same machine row. No second record is created by the app.
- A `pidash` service the user installed by hand in a terminal is a separate daemon with its own credential and lifecycle (it runs with the app closed). It remains its own `manual` machine and is not merged. The two rows are told apart by a "Desktop app" badge.

This is preferred over "two records presented as one" because merging two daemons into one row needs a way to recognise the same computer from two installs, a relaxed one-session rule, and UI merge logic, all to hide a second sign-in the user no longer needs.

**Identity key.** Today the machine is found by `host_label = desktop-<uuid>` from `managed/device-id`, and that string is also the display label. Changes:

- Add `DevMachine.device_key` (indexed, not shown). `desktop-enroll` looks machines up by `(owner, device_key, workspace)`. Backfilled from `host_label`.
- `host_label` becomes the OS hostname and `label` a user-editable name that defaults to it. Add `PATCH` on the machine for `label` (no rename exists today).
- Every lookup that finds a machine by `host_label` must stop matching desktop machines, because the hostname is now shared with a hand-installed `pidash` service on the same computer and with the same computer's desktop machine in another workspace. Today `_get_or_create_dev_machine` matches on `(owner, host_label)` with no provisioning filter (`api/runner/views/enrollment.py:103-108`, `:124`), so a CLI enrolling without a machine id would attach to the desktop machine. That lookup excludes `desktop_bundled` rows, and the desktop sign-out `DELETE` (`api/runner/views/desktop.py:176`) selects by `device_key`.

**App data wiped.** A wipe loses `device-id`, so the next sign-in would create a new machine and orphan the old one with its runners and pinned issues.

- `desktop-enroll` also sends `machine_key`: a SHA-256 of the OS machine id (`/etc/machine-id`, `IOPlatformUUID`, `MachineGuid`) with a fixed salt. The raw id never leaves the computer.
- When no machine matches `device_key` but one of the caller's own live desktop machines in the workspace has the same `machine_key`, the server adopts it: it re-keys that row and returns its id. Runner rows, pod membership and pins survive. Matching never crosses owners.
- Adoption happens only when that machine has no live session. Two installs can share an OS id and both be in use: a development build beside a release build (separate app data, so separate `device-id`), or virtual machines cloned from one image. If the matching machine is online, a new machine is created instead, so the two installs do not take the row, and its single session, from each other.
- Adoption restores the server side only; the wipe also removed the local runner definitions. At the next daemon start the reconcile step (section 5) re-enrols, on this computer, every live engine runner row of the adopted machine that has no local block. The existing reuse lookup returns the same row, so pins and pod membership work again without the user turning the engine on per project. Own-agent runner rows cannot be rebuilt this way (their agent and folder were local); they show as offline with "Set up again on this computer" or remove.
- When the OS id is unavailable, a new machine is created. The old one shows as offline and the user removes it from AI Dev Machines. Removing a machine revokes its runners and clears their pins.

**How it appears in AI Dev Machines.** One row per computer: name, "Desktop app" badge, status, last seen. Expanding it lists the built-in engine (with the projects it is on) and the own-agent runners. Actions are in section 11.

## 5. Runner identity (point 2)

**Decision: the engine's identity is the machine; the stored unit is one engine runner per machine and project.** A true single row per machine would require a runner to serve several pods and projects, which breaks the constraints in section 3 for every runner type.

- **Uniqueness replaces the cap.** At most one live engine runner per `(dev_machine, project)`. The enrolment reuse lookup (`api/runner/views/enrollment.py:705-720`) is keyed on machine and project instead of machine and pod, and `_managed_cap_error` with `MANAGED_RUNNER_MAX_PER_USER_PROJECT` is deleted. A second computer on the same account enrols its own row. This resolves PIDESKAPP-96.
- **Abuse guard.** A new `MANAGED_RUNNER_MAX_MACHINES_PER_USER` (default 10 live desktop machines per workspace) replaces the cap as the only count limit.
- **Enrol on use, not on page open.** Today every project route opened enrols a runner (`desktop-overlay/apps/web/core/components/agent-runtime.tsx:65-141`). New rule: an engine runner is created when the user turns the engine on for a project, which happens on the first Run on this computer, on adding it to a pod, or from the engine's project list in desktop settings.
- **Removal exists.** "Remove from project" revokes the row. The desktop reconciles at daemon start and on focus: local `[[runner]]` blocks whose server row is revoked are stripped with `pidash __managed remove`. The folder on disk is kept.
- **Local limit.** `__managed enroll` checks `MAX_RUNNERS_PER_DAEMON` (50) before calling the server, so the limit can no longer leave a half-enrolled runner. Together with the two points above this resolves PIDESKAPP-118. The limit itself remains: one computer can have the engine on in 50 projects per workspace.
- **Provisioning is per runner.** Today a runner inherits `provisioning` from its machine (`enrollment.py:701-704`). With own-agent runners on a desktop machine it is decided by the path: `__managed enroll` asks for an engine runner, accepted only on a `desktop_bundled` machine; every other create path yields `manual`.
- **Naming.** The server assigns engine runner names: `builtin-<machine-slug>-<project-identifier>`, with a numeric suffix when the name is taken in the pod. The project is part of the name because a name must be unique in two scopes: per pod on the server (`runner_unique_name_per_pod`) and per daemon config on the computer (`ConfigError::DuplicateName`, `runner/src/config/schema.rs:717`), and one computer's engine runners for several projects share one config. Client-supplied names are ignored for engine runners. This removes the `desktop-desktop-<uuid>-<PROJECT>` names and the collision in PIDESKAPP-53. The UI shows "Built-in engine · <machine label>" from `provisioning` and the machine, so renaming the machine renames its engine everywhere. Engine runners are not renamed individually.

Two engine runners on one machine can run at the same time (two projects). That is today's behaviour and is kept. **[PO-7]**

## 6. Executor model (point 3)

**Decision: the engine becomes a runner like any other that happens to have a fixed agent.** The executor answers "cloud or a machine"; which machine is a dispatch question. **[PO-2]**

- **Selectable executors** are `cloud_agent` and `local_runner` (shown as "Runner"). `managed_runner` is no longer offered.
- `Project.default_agent_executor`**:** `managed_runner` rows become `local_runner`. The project's default pod then decides who runs the work (section 7).
- `Issue.agent_executor`**:** `managed_runner` becomes `local_runner` plus the new `Issue.assigned_runner` pointing at a specific engine runner.
- `AgentRun.executor_kind`**:** new runs on an engine are `local_runner`. Whether a run used the engine is read from `run.runner.provisioning`. Existing rows keep `managed_runner`; history is not rewritten and the enum value stays readable. The check constraint is unchanged.
- **Policy that hung on the kind** moves to `Runner.provisioning`: availability reasons, the queued-run expiry, metering labels and prompt recipe aliases.
- **Compatibility:** the API keeps accepting `agent_executor = managed_runner` from older desktop builds and translates it to `local_runner` plus `assigned_runner` = the caller's online engine runner for that project, which is the old behaviour. `agent_executor_options` keeps returning a `managed_runner` entry for those builds. Both are removed once `DESKTOP_MIN_VERSION_FOR_MANAGED_RUNNER` is raised past the first build with this feature.

The alternative is to keep `managed_runner` as a kind meaning "pinned to one engine". It is less work on the server but leaves two ways to say "run on a machine", and a pod run that lands on an engine would carry a kind that does not match.

## 7. Dispatch and pods (point 4)

**Single runner.**

- New nullable `Issue.assigned_runner`. Setting it also sets `assigned_pod` to that runner's pod. It can be set to any runner the user owns in the project; the main use is an engine.
- A run created for such an issue gets `pinned_runner` and a new `AgentRun.pin_strict = true`. A strict pin is never released to the pod. Today the same distinction is implied by the executor kind (`api/runner/tasks.py:125-145`).
- The picker lists the user's runners in the project with machine name and online state, replacing the single "Pi Dash Agent (desktop)" option.
- **A run the pinned runner could not take under its usage rule is refused at creation**, with reason `assigned_runner_not_owner`. For an engine that is a run started by anyone other than its owner, unless the owner has turned on "Teammates can start runs on my issues" (section 8). For an own-agent runner it is a run not tied to its owner (section 3). It is not queued: the runner would never take it, a strict pin is never released, and it would sit for 12 hours under a "Waiting for <machine>" label that is not true. The UI offers "Run on my engine instead", which re-pins the issue to the caller's own runner. For ticks and schedulers the run's principal is the runner's owner when section 8 allows it; otherwise the issue is bounced by the existing preflight (`preflight_eligibility_or_bounce`, `api/orchestration/scheduling.py`). **[PO-10]**
- This differs from today, where the engine is resolved from whoever the run is created for (`scheduling.py:938-948`): an issue set to the built-in engine runs on each person's own desktop. The new field names one runner.

**Pods.**

- An engine runner is structurally always in a pod: the project's default pod at enrolment. A new `Runner.pod_dispatch` field says whether it takes pod work: `pinned_only` (default for engine runners) or `pod_member` (default for own-agent runners, which is their behaviour today).
- "Add to pod" sets the pod and `pod_member`. "Remove from pod" sets `pinned_only`. Moving between pods uses the existing owner-only `PATCH` (`api/runner/views/runners.py:384-422`), restricted to pods of the same project for engine runners.
- Only the owner can do either. Workspace admins cannot add someone's engine to a pod.
- Adding another computer's engine to a project it is not yet on requires turning it on from that computer. Doing it remotely while that desktop is online can reuse the machine-control create path later; it is not in the first version.

**Matching** (`api/runner/services/matcher.py`):

- An engine runner takes runs strictly pinned to it, and, when it is a `pod_member`, unpinned runs in its pod that pass the usage rule in section 8.
- The exclusion of engine runners at `matcher.py:109` and the kind split at `:173-176` are replaced by the two rules above.
- **Ranking:** when a pod has an idle ordinary runner and an idle engine that could both take a run, the ordinary runner is chosen. The engine takes pod work when the eligible ordinary runners are busy or offline. Reason: the engine spends wallet money; the user's own agent usually does not. **[PO-4]**

## 8. Who may use it, and who pays (point 5)

- **Who pays:** always the engine's owner, through their OpenHub wallet. `AgentRun.owner` is already set from the runner's owner at assignment.
- **Who can cause a run:** the owner. An engine runner takes a run only when one of these holds: **[PO-3]**

  - the owner created the run (clicked Run, from any client);
  - the run is automatic (tick or scheduler) and is on an issue the owner created or is assigned to, or comes from a scheduler binding the owner authored.
- A teammate clicking Run on the owner's issue does not start a run on the owner's engine. This is stricter than the rule for ordinary runners, which would allow it.
- **Owner controls:**

  - pod work is off until the owner turns it on, per project (the one exception is the migration of projects whose default is the built-in engine today, section 13);
  - a per-machine "Pause agent runs" switch (`Runner.dispatch_paused`, set for all of the machine's engine runners), which the desktop also sets itself when the model profile is unavailable or the wallet is empty, so a pod stops routing to an engine that can only fail. The owner's switch and the app's automatic pause are recorded separately (a pause reason), so the app clearing its own pause never undoes the owner's;
  - an opt-in "Teammates can start runs on my issues", which applies the ordinary-runner rule;
  - removing the engine from a project or removing the machine.
- **Project defaults and scheduled work** cannot reach an engine whose owner has not made it a pod member, and then only for the owner's own work as defined above.
- **BYOK:** unchanged. The engine supports the OpenHub lane only (`managed_runner/design.md` §12.2), because no stored key is returned to a client. A BYOK user's engine does not come online and is shown with the existing reason.
- Sharing an engine with a team is not part of this design. No runner can be shared today.

## 9. Availability (point 6)

- **Pod work:** an offline engine is an offline runner. Unpinned runs go to whichever eligible runner is online; they never wait for the engine in particular. A pod member that is usually offline costs nothing while offline.
- **Structural check:** `pod_has_runner_for_issue_principal` counts a `pod_member` engine runner as a registered runner for its owner. A pod whose only eligible runner is an offline engine therefore queues the run rather than bouncing the issue to Backlog, as for ordinary runners.
- **Strictly pinned runs, engine offline:**

  - *User-triggered:* accepted and queued, where today it is refused. The click may now come from the web or another computer, so the UI asks first: "<machine> is offline. Queue until it comes online?" **[PO-5]**
  - *Automatic:* queued, as today.
  - Both carry `error_code = desktop_not_connected` and are shown as "Waiting for <machine name>". The web app has no such label today (`issue-detail/agent-status.tsx:196-201` does not read `error_code`); adding it is in scope.
  - The sweep in `managed_runner/tasks.py` fails them after `MANAGED_RUNNER_QUEUED_MAX_AGE_SECS` (12 h), now selected by "strict pin to an engine runner" rather than by kind. This replaces the "waiting for your desktop" queue of `managed_runner/design.md` §8.5 with the same mechanics and a wider entry.
- **Paused engine:** treated as offline for matching, with its own label.
- **App closed mid-run:** unchanged (`managed_runner/design.md` §9.5).

## 10. Default work dir (point 7)

**Boundary with PIDESKAPP-42.** PIDESKAPP-42 owns the default root location and the folder layout below a root. This design owns the setting, its validation, how it reaches the runner and what a change does. A custom root uses the same layout below it as the default root.

- **Where it lives:** on the machine, per account: key `engine.task_root` in the host-written per-account settings file (`managed/accounts/<account-key>/settings.json`, `desktop/src-tauri/src/account_settings.rs`). It is not stored on the server and not per project, because it is a path on one computer.
- **Default:** unset, meaning the default task root from PIDESKAPP-42. Until that lands this is today's `<app-data>/managed/workdirs`.
- **Where it is edited:** a new "Built-in engine" section in desktop settings, with a folder picker. The desktop has no settings page and no folder-picker permission today; both are part of this work.
- **Validation,** done by the Rust host and never by the webview:

  - absolute, canonicalised, an existing or creatable directory, writable (probe file);
  - not a filesystem root or the home directory itself;
  - not inside the app's resources or its private data (daemon config, chat data, engine home);
  - not the same as, inside, or containing another runner's working dir in the same daemon;
  - short enough for the deepest project path on Windows.
  - A network or removable drive is allowed with a warning. A failed check leaves the old value in place and reports which rule failed.
- **Effect of a change:** **[PO-6]**

  - nothing is moved or deleted;
  - projects the engine is turned on for afterwards use the new root;
  - projects already set up keep their current folder. Settings lists each project with its folder, "Open folder", and "Use the default location", which rebinds that project to the new root with a fresh folder and leaves the old one on disk. A "Switch all" action does the same for every project.
- **How it reaches the runner:**

  - new projects: `pidash __managed enroll --working-dir`, as today;
  - existing projects: a new `pidash __managed set-working-dir --project <p> --working-dir <abs>` that rewrites the block under the config lock and re-runs schema validation. The host applies it when no runner on that daemon is mid-run, because the restart stops every runner the daemon hosts, and then restarts the daemon;
  - the runner reports the canonical path at session attach and the server stores it in `dev_metadata.working_dir` (`api/runner/services/session_service.py:62-93`), so the restart is what keeps the server's copy accurate. The server only displays it.
- **Per-project folder chosen by the user** (pointing the engine at an existing local clone) uses the same command and is left as a follow-up.
- The chat working directory is not covered by this setting. It belongs to PIDESKAPP-42.

## 11. Visibility and management (point 8)

Server: the `include_bundled` filter (`api/runner/views/runners.py:326-329`) is removed. `provisioning`, `pod_dispatch`, `dispatch_paused` and the machine are added to the runner payload and to `IRunner`; `provisioning` to `IDevMachine`. Lists stay owner-only.

| Surface | Change |
| --- | --- |
| AI Dev Machines | One row per computer with a "Desktop app" badge; expands to its engine and own-agent runners. |
| Runners list and detail | Engine runners listed with a "Built-in" badge and machine name. Detail shows the reported work dir. |
| Pod management | Pod shows its member runners. "Add runner to pod" lists the user's runners, engines included. No UI calls the existing move endpoint today. |
| Issue execution picker | Cloud Agent, pods, and "Specific runner" (section 7). |
| Project default executor | "Runner" or "Cloud Agent". The "Pi Dash Agent (desktop)" option is removed. |
| Runs page | The label comes from the run's runner: "Built-in engine · <machine>" instead of "Local Runner". |
| Desktop AI Agents nav | "This computer": the built-in engine (local chat). "Runners": every other runner the user owns, including other computers' engines, named by machine. |

Actions, given that the app owns the engine's install and lifecycle:

| Action | Engine runner | Desktop machine |
| --- | --- | --- |
| Rename | No. Rename the machine. | Yes (new). |
| Add to, remove from, move between pods | Yes, owner only. | n/a |
| Pause and resume | Yes (per machine). | Yes. |
| Remove | "Remove from project": revokes the row, keeps the folder. | "Remove this computer": revokes token and runners. The app on that computer returns to "set up" without a restart (PIDESKAPP-108). |
| Rotate credential | n/a | Hidden. The app mints its own token at sign-in. |
| Edit agent, model, work dir from the web | No. Work dir is set on the computer (section 10). | n/a |
| Chat | Yes (section 12). | n/a |

## 12. Chat routing (point 9) and the user's own agent (point 10)

### 12.1 Chat routing

The transport is chosen per runner, not once for the app.

| Chat target | Transport | Sessions stored | Working folder |
| --- | --- | --- | --- |
| This computer's built-in engine (the "This computer" contact) | Daemon socket | Local SQLite on this computer | Chat folder chosen by the host (PIDESKAPP-42) |
| An own-agent runner on this computer | Server (HTTP + SSE) | Server | That runner's work dir |
| Any runner on another computer, including its built-in engine | Server | Server | That runner's work dir, i.e. the project's task folder on that computer |

- **Implementation shape:** `registerLocalChatTransport` installs a routing transport that sends the built-in contact's sessions to `LocalChatTransport` and everything else to the cloud transport, in place of the global swap at `desktop-overlay/apps/web/core/services/local-chat-transport.ts:714-722`. Session-scoped calls resolve the owner transport from the session. Approvals follow the same split.
- **No duplicates:** this computer's own engine runner rows are not listed under "Runners" on this computer; they are represented by the local contact. On the web and on other computers they are listed per project as "Built-in engine · <machine> · <project>".
- **Chatting with another computer's engine** needs no new server work: server-routed chat already accepts an engine runner for its owner (`api/runner/views/chat.py:174-186` has no provisioning check); only the list hid it. It is billed to the same owner. The composer is disabled while that computer is offline, as for any runner.
- **Local chat no longer borrows a project's runner.** Today a local chat turn runs on whichever project runner the daemon resolves, which fails once the daemon hosts two (PIDESKAPP-41), forces chat to enrol a project (PIDESKAPP-14), and blocks chat for the length of a run (PIDESKAPP-101). The daemon gets a machine-level engine profile (engine binary, engine home, token file) written at bootstrap. IPC chat with an absolute folder runs on that profile, with no project runner and no shared busy flag. The host guarantees the chat folder is outside every task folder (PIDESKAPP-25). **[PO-8]**
- Server-routed chat to a runner that is busy with a run still answers `runner_busy`, because that chat shares the run's folder. The UI should say so. The server comment claiming the two run concurrently (`api/runner/views/chat.py:247-251`) is wrong and should be corrected.

### 12.2 The user's own agent on a desktop machine

- **Set-up:** the existing Add-runner modal already creates a runner remotely on a machine that is online for control (`add-runner-modal.tsx`, `MachineMsg::CreateRunner`, `runner/src/daemon/machine_control.rs:156-230`). On desktop, "Add runner" preselects this computer and offers a folder picker for the work dir. The user picks project, pod, agent and folder. There is no CLI install and no second sign-in.
- **What this requires:**

  - the desktop starts the workspace daemon at sign-in even when no engine runner exists (the daemon already supports a machine with no runners, `runner/src/cli/run.rs:44`);
  - the daemon must find the user's agent binary. A daemon started from a GUI app does not get the user's shell `PATH` (notably on macOS), so the host resolves the binary at creation and stores an absolute path, with a doctor check;
  - sign-out must keep runner definitions and remove only credentials. Today sign-out deletes `config.toml` (`managed_runner.rs:579-610`), which is harmless only because engine runners can be rebuilt. Where the config lives is PIDESKAPP-42's layout decision; this requirement is an input to it.
- **Coexistence with the engine:** same daemon, separate runners, separate work dirs (enforced by config validation), separate agent credentials. Own-agent runners keep today's dispatch behaviour.
- **Trade-off:** own-agent runners on the desktop daemon are online only while the app is open. A user who needs an always-on runner installs the `pidash` service in a terminal as today, and gets a separate machine row.
- **PIDESKAPP-114:** the unreachable `setUpCliForThisMachine` flow installs a second CLI and signs it in as a second machine, which is the opposite of the decision in section 4. It is not given a UI. PIDESKAPP-114 is rescoped to the Add-runner flow above. Installing the bundled CLI onto `PATH` can remain as a convenience for terminal use, without the sign-in step.
- **Coexistence with a hand-installed service:** the two daemons already use separate config, data and socket locations (`runner/src/util/paths.rs:32-46`) and can run together. Neither validates work dirs against the other, so the desktop's folder validation cannot catch a folder the other daemon uses; the settings text says so.

## 13. Migration (point 11)

**Server** (one release, deployed before the desktop build):

| Item | On upgrade |
| --- | --- |
| Desktop machines | Kept. `device_key` backfilled from `host_label`. Hostname and `machine_key` filled at the next sign-in from a new build. Duplicates left by earlier data wipes stay as offline rows for the user to remove. |
| Engine runners | Kept with the same ids, pods and work dirs. Renamed to the new scheme. `pod_dispatch = pinned_only`, so they take exactly what they took before. |
| Never-used engine runners | Revoked when they have no run and no issue pins them, evaluated after the issue row below has set `assigned_runner`. This clears the rows created by opening projects (PIDESKAPP-118). |
| Projects with default `managed_runner` | Set to `local_runner`. Engine runners in those projects are set to `pod_member`, so each person's own work still reaches their own desktop. Two differences. A person who also has an own-agent runner in the default pod will see it preferred. And the engine becomes a pod member without its owner turning that on, so the owner's issues in that project that are explicitly set to "Runner" can now overflow to the engine and spend the wallet when their own-agent runner is busy or offline. The desktop shows a one-time notice naming the projects, with the switch to turn pod work off. **[PO-11]** |
| Issues with `agent_executor = managed_runner` | Set to `local_runner` with `assigned_runner` = the pinned runner of the issue's latest engine run, else the issue creator's most recently seen engine runner in the project, else none (pod-level). Difference: the issue now names one person's engine, so a different teammate's Run is refused with the offer to re-pin (section 7, PO-10), where today it ran on that teammate's own desktop. |
| Queued runs pinned to an engine | Untouched apart from `pin_strict = true`. The matcher keeps serving legacy `managed_runner` runs to the runner they are pinned to, and the 12 h expiry still applies. |
| Finished runs | Untouched. |
| Settings | `MANAGED_RUNNER_MAX_PER_USER_PROJECT` removed. `MANAGED_RUNNER_QUEUED_MAX_AGE_SECS` kept. `MANAGED_RUNNER_MAX_MACHINES_PER_USER` added. |
| Older desktop builds | Keep working through the compatibility rules in section 6. |

**Desktop:**

- `device-id` is kept; the hostname and `machine_key` are sent from the first sign-in.
- Local `[[runner]]` blocks are kept, names refreshed from the server, work dirs unchanged.
- Enrol-on-open stops. Blocks for revoked rows are removed by the reconcile step.
- The daemon's engine profile for chat is written at the next bootstrap. Local chat history is untouched.
- Whether existing task folders move under the account root is PIDESKAPP-42's decision; this design only requires that a runner's bound folder keeps resolving.

## 14. What this replaces in `.ai_design/managed_runner/design.md`

| Section | Status |
| --- | --- |
| §2.5 "Third executor kind" | Replaced by section 6. |
| §5 Conceptual model | Replaced: the engine is a runner with a fixed agent, not a separate executor. The three user-facing rules stay, with "online while the app is open" unchanged. |
| §7.1 `AgentExecutorKind` | `managed_runner` kept as a legacy value only. |
| §7.2 `Runner.provisioning` | Kept, but set per runner rather than inherited from the machine. |
| §7.3 check constraint | Stays. |
| §8.2, §8.3 resolution and availability | Replaced: availability is per runner (online, paused, model profile), not per viewer and project. The reason codes stay. |
| §8.4 "always pinned", excluded from pod matching | Replaced by section 7. |
| §8.5 scheduled work and the waiting queue | Replaced by section 9. Queue, label and 12 h expiry stay. |
| §8.6 choosing the executor from the desktop | Replaced by the runner picker. The rule that the desktop never changes a project's default stays. |
| §9.3 step 3 (runner per project opened, client naming, cap) | Replaced by section 5. Steps 1, 2, 4, 5 and 6 stay. |
| §9.4 work directory location | Location replaced by section 10 and PIDESKAPP-42. The Git-optional contract stays. |
| §15 policy table | Matching, scheduled work, quota and registration rows replaced. Metering, support and trust rows stay. |
| §16 configuration | Cap removed, machine limit added; the rest stays. |
| §20 "Reuse `local_runner` for the bundled runner" (rejected) | Reversed. Its two objections are met by opt-in pod membership and the usage rule. |
| §12 (model settings, BYOK), §13 (isolation), §14 (security) | Stay unchanged. |

## 15. Choices left for the product owner

| # | Choice | Recommended | Alternative |
| --- | --- | --- | --- |
| PO-1 | One machine per computer, own agent on the desktop daemon | Yes. Own-agent runners there are online only while the app is open. | Keep a separate always-on CLI install per computer and merge the two rows in the UI by a hardware-derived key. |
| PO-2 | Fold `managed_runner` into "Runner" | Yes. | Keep the kind for pinned-only runs. |
| PO-3 | Whose work an engine takes | Only runs the owner starts, plus automation on the owner's issues; teammates' clicks need an opt-in. | Same rule as ordinary runners (teammates can start runs on the owner's issues by default). |
| PO-4 | Ranking in a pod | Ordinary runners first, engine as overflow. | Equal ranking. |
| PO-5 | Run pinned to an offline engine, started by a person | Queue after a confirmation, expire at 12 h. | Refuse, as today. |
| PO-6 | Changing the work dir with projects already set up | Existing projects keep their folder; switch per project or all at once; nothing is moved. | Switch every project at once on change. |
| PO-7 | Limit on simultaneous engine runs per computer | None beyond one per project. | A per-machine limit in settings. |
| PO-8 | Local chat on its own engine profile | Yes; resolves PIDESKAPP-41, -14 and -101 for local chat. | Keep borrowing a project runner and pass its name; -101 stays. |
| PO-9 | Old duplicate desktop machines | Leave for the user to remove. | Auto-remove desktop machines not seen for 90 days. |
| PO-10 | Run on an issue pinned to someone else's runner | Refuse at creation and offer "Run on my engine instead". | Run it on the caller's own engine in that project for that run, leaving the pin as it is (today's per-person behaviour). |
| PO-11 | Projects whose default is the built-in engine today | Make their engines pod members on upgrade, with a one-time notice. Explicit "Runner" issues there may overflow to the engine. | Leave them `pinned_only`; unpinned work in those projects waits for an ordinary runner until each owner turns pod work on. |

## 16. Implementation epics

| Epic | Verdict | Scope after this design |
| --- | --- | --- |
| PIDESKAPP-122 identity and enrolment | Confirmed, widened | Sections 4, 5 and the machine and runner rows of 13: `device_key`, `machine_key` adoption, hostname and rename, per-machine-project uniqueness, cap removal, server naming, per-runner provisioning, enrol on use, reconcile and remove, local limit pre-check. |
| PIDESKAPP-123 dispatch | Confirmed, widened | Sections 6 to 9 and the rest of 13: `assigned_runner`, `pin_strict`, `pod_dispatch`, `dispatch_paused`, matcher and ranking, usage rule, tick and scheduler paths, expiry sweep, executor migration and compatibility for old builds. |
| PIDESKAPP-124 default work dir | Confirmed | Section 10. Also delivers the first desktop settings page and the folder picker. Needs PIDESKAPP-42's default root and layout before its default can be final. |
| PIDESKAPP-125 show and manage | Narrowed | Section 11 only: lists, machine page, pickers, pod UI, run labels, the waiting label. Chat and own-agent set-up move out. |
| New: chat routing on desktop | Added | Section 12.1: routing transport, nav de-duplication, daemon engine profile for local chat. Absorbs PIDESKAPP-62 and PIDESKAPP-41. |
| New: own agent on the desktop daemon | Added | Section 12.2: daemon at sign-in, Add-runner on this computer, agent binary resolution, runner definitions surviving sign-out. Absorbs PIDESKAPP-114. |

Order: 122 first. Then 123, 124, chat routing and own-agent in parallel. 125 last among them, because it shows fields 122 and 123 add. The two new epics are not filed yet; they are filed with the sub-issue split once this design is approved.

What remains of the three linked bugs once the feature is built:

- **PIDESKAPP-62:** nothing separate. Its fix is the routing transport in the chat epic. Re-verify there and close.
- **PIDESKAPP-96:** resolved by 122. Its request for a way to release a computer no longer in use is met by "Remove this computer". Close after the two-computer scenario passes.
- **PIDESKAPP-118:** resolved by 122 (enrol on use, removal, pre-check, pruning of unused rows). What remains is the hard limit of 50 projects per computer and workspace, which now fails cleanly before anything is registered.

Also affected: PIDESKAPP-53 (resolved by server naming), PIDESKAPP-108 (covered by machine removal in 125), PIDESKAPP-14 and PIDESKAPP-101 (resolved for local chat if PO-8 is accepted).

## 17. To confirm during implementation

These were read but not exercised, and should be checked first in the relevant epic:

- that the desktop daemon's config qualifies for machine control, so remote runner creation works against it (`supports_machine_control`, `runner/src/cli/run.rs:44`);
- whether a running runner picks up a changed working dir without a daemon restart (the design assumes a restart);
- the OpenHub wallet and token code lives in private-pi-dash and was not read; the pause-on-empty-wallet signal in section 8 depends on what it exposes.

This document lives at `.ai_design/built_engine_as_runner/design.md`. Once it is approved, `managed_runner/design.md` should be marked with the pointers in section 14.
