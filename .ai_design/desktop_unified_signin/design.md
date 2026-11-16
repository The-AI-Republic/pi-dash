# Design: one sign-in for the desktop app, the CLI and the engine

- **Issue:** PIDESKAPP-225, to be rewritten as this design's epic.
- **Status:** revision 1, a draft for product-owner review. The product owner decided U-1 to U-4 on 2026-11-16 (section 3). The rules in section 6 follow from those decisions and still need review; section 12 lists them.
- **Dates:** revision 1 on 2026-11-16.
- **Code baseline:** pi-dash `main` at `6e29c6157`. Paths are relative to that tree.
- **Terms:** "engine" is the Pi Dash Agent (`AGENTS.md`); this design keeps BE's word for the credential and its states, which are stored values.
- **Revises:** `.ai_design/built_engine_as_runner/design.md` ("BE" below). This design replaces BE §4.6. It revises BE PO-13, PO-16 and PO-17 and BE's engine sign-in states in §4.3. BE carries pointers to this design.

## 1. Summary

The desktop app, the Pi Dash CLI daemon and the built-in engine (the Pi Dash Agent) share one sign-in:

- Signing in on either side signs the other side in.
- Signing out on either side signs the other side out.
- A CLI that is already signed in signs the app in, with no browser.
- When the two sides disagree, the most recent sign-in wins and the other side switches to it.
- The engine has no sign-in or sign-out of its own. It follows the CLI, and the app renews its credential silently.

When there is no daemon, there is nothing to sync, and the app signs in and out on its own.

## 2. Today

This section was read from the code at the baseline; nothing was run. BE §4.6 (revision 4.0) made the app and the CLI "independent peers" for sign-out, and deferred CLI → app sign-in as "option B". The product owner decided option A of PO-16 (the app's sign-in signs the CLI in). The independent sign-out was not a product-owner decision: BE derived it from PO-1.

| User action                   | CLI daemon and engine                                                                                                                                                                                                                                                                                                 | App                                                                                                                                   |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| Signs in to the app           | Not touched at sign-in. The CLI is signed in later, at the first local chat or "Run on this computer", after a consent dialog (`ensureDaemon` → `requestDaemonSetup` → `setUpDaemon`, `apps/web_new/src/features/runners/runtime/service.ts`). A daemon signed in as another account is refused (`account_mismatch`). | —                                                                                                                                     |
| Signs out of the app          | Stays signed in. The sign-out dialog offers an unticked "Also sign out Pi Dash on this computer" (`SignOutDialog.tsx`, `disposeRuntimeForSignOut`), which runs `pidash auth logout`.                                                                                                                                  | —                                                                                                                                     |
| `pidash auth login`           | The engine is signed in too, when the server issues an engine credential (`runner/src/cli/auth/login.rs`).                                                                                                                                                                                                            | Stays signed out if it was. When signed in as another account, it shows the mismatch and turns its engine features off.               |
| `pidash auth logout`          | The engine is signed out too (`runner/src/cli/auth/logout.rs`).                                                                                                                                                                                                                                                       | Stays signed in. It shows "Pi Dash on this computer is signed out".                                                                   |
| `pidash auth logout --engine` | Only the engine is signed out.                                                                                                                                                                                                                                                                                        | Stays signed in. Chat refuses with "Sign it in from Pi Dash Agent settings", but that screen does not exist (PIDESKAPP-225 as filed). |

The result is extra steps for the user, and an app and a CLI that can stay signed in as different accounts.

## 3. Decisions

| #   | Decision (product owner, 2026-11-16)                                                                                                                                                                        |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| U-1 | The app and the CLI sign in and out together, in both directions. A CLI that is already signed in is used by the app automatically. Replaces BE PO-16 option A and BE's independent sign-out.               |
| U-2 | When the two sides are signed in as different accounts, the most recent sign-in wins and the other side switches to it. Replaces BE PO-13's refusal to cooperate.                                           |
| U-3 | The engine signs in and out together with the CLI and the app. `pidash auth logout --engine` and the web machine page's "Sign out the engine" are removed. Revises BE PO-17 and §4.3.                       |
| U-4 | When the user declines the background service (no daemon), the app keeps working on its own, but the Pi Dash assistant is unavailable, because on the desktop it runs on the Pi Dash Agent. Keeps BE PO-12. |

## 4. Behaviour

The sync applies when the app finds a daemon (BE §4.4).

| Situation                                                                                                                                                | What happens                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| -------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **The user signs in to the app**; the daemon is signed out                                                                                               | The app signs the CLI in at once, with no prompt. It approves a device grant with its new session (`interactive: false`, scoped to the app's current workspace) and hands the code to the daemon (`pidash auth login --device-code-stdin`, the existing `pidash_cli_login` path). The same sign-in signs the engine in. That needs the server change in section 6.3: today a silent approval issues no engine credential when the machine's engine is `never` or `signed_out`, and `signed_out` is exactly the state an ordinary logout leaves. |
| **The user signs in to the app as U**; the daemon is signed in as V                                                                                      | The CLI switches to U the same way (U-2). The switch signs V out of the CLI first, as `pidash auth logout` would, parks V's machine identity and runner blocks, and restarts the daemon as U's machine (section 6.4).                                                                                                                                                                                                                                                                                                                           |
| **The user signs in to the app**; the daemon is signed in as the same user                                                                               | Nothing to do. When the daemon does not hold the app's current workspace, the app adds it (BE §4.9).                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| **The user signs out of the app**                                                                                                                        | The app runs `pidash auth logout`, which revokes every workspace's daemon credential and the engine credential, then signs itself out. If the server cannot be reached, it runs `pidash auth logout --local-only`, so the computer is signed out regardless. The sign-out dialog keeps only "Also delete chat history".                                                                                                                                                                                                                         |
| **The user signs in to the CLI** (`pidash auth login`); the app is signed out                                                                            | The app signs itself in with a single-use handoff code from the daemon, with no browser (section 5). It does this at its next daemon check, or at launch if it was closed.                                                                                                                                                                                                                                                                                                                                                                      |
| **The user signs in to the CLI as U**; the app is signed in as V                                                                                         | The app signs V out of the app only (the daemon is already U's), then signs in as U through the handoff.                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| **The user signs out of the CLI** (`pidash auth logout`, or the last workspace's `pidash auth logout --workspace W`), or removes the computer on the web | The daemon and the engine are signed out, as today. `Hello` reports the sign-out as an event (section 6.1). The app signs itself out with the notice "You were signed out because Pi Dash on this computer was signed out." Chat history is kept.                                                                                                                                                                                                                                                                                               |
| **Engine credential `never`, `signed_out`, `expiring` or `expired`**; both sides signed in                                                               | The app signs the engine in or renews it silently (U-3, section 6.3). The user never sees an engine sign-in.                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| **No daemon** (CLI not installed, or service declined)                                                                                                   | Nothing to sync; the app signs in and out on its own. The Pi Dash assistant, local chat and the engine are unavailable. The app offers set-up from the same entry points as today (BE §4.7, §4.8; U-4).                                                                                                                                                                                                                                                                                                                                         |

**Engine sign-in states** (BE §4.3, revised):

| State                 | Meaning                                                                                                                                  | What the app does                                                                                                                                  |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `never`               | The machine has never had an engine credential: an install from before the engine, or the instance could not serve the engine at sign-in | Signs the engine in silently, when the instance can serve the engine (section 6.3)                                                                 |
| `active` / `expiring` | A live credential; `expiring` within three days                                                                                          | Renews silently when `expiring`                                                                                                                    |
| `expired`             | The credential reached its 14 days                                                                                                       | Renews silently                                                                                                                                    |
| `signed_out`          | The daemon was signed out: `pidash auth logout`, the last workspace's logout, or the app's sign-out. There is no engine-only sign-out.   | While the daemon is signed out, nothing: the app signs out too. The next sign-in on either side signs the engine in again, silently (section 6.3). |
| `revoked`             | An admin or "Remove this computer" revoked it                                                                                            | Nothing                                                                                                                                            |

`pidash login --engine` stays, as a renewal for computers with no app. It needs the daemon to be signed in. It is not a separate sign-in.

## 5. The handoff: a CLI sign-in signs in the app

The app's interface is the web app, so it needs a web session: cookies on the API origin, set by the server. The daemon holds machine credentials, which are not web sessions. The handoff turns one into the other once, through the server, without exposing either credential to the app.

1. The app finds the daemon signed in as an account, and either has no session of its own or has another account's session that the daemon's later sign-in replaces (section 6.2).
2. The app's Rust side generates a verifier: 32 random bytes held only in the app's memory. It asks the daemon over the local socket with a new IPC request, `SessionHandoff { challenge }`, where `challenge` = base64url(SHA-256(verifier)).
3. The daemon calls `POST /api/v1/auth/desktop-handoff/` with one of its workspace daemon credentials and the challenge. The server stores a single-use code: random, stored hashed, bound to the user, the `dev_machine` and the challenge, and valid for 60 seconds. It returns the code and the account it belongs to.
4. The daemon returns the code and account to the app.
5. The app redeems the code with a `POST` through its native transport (`desktop_api_request`, `desktop/src-tauri/src/desktop_http.rs`), not by opening a URL. The body is `{code, verifier, expected_user_id}`, sent to `/auth/desktop-handoff/redeem/` (the cloud serves it under `/api/auth/`). The transport already writes the response's `Set-Cookie` into the webview's cookie store (`desktop_http.rs:467`).
6. Before creating any session, the server checks four things: the code is live and unused; SHA-256(verifier) matches the stored challenge; the code's user is `expected_user_id`; and the user and machine are active. Only then does it consume the code and create the same kind of session a desktop browser sign-in creates (the OSS session; on the cloud, the desktop JWT pair with `client = "desktop"`). On any mismatch it consumes the code and creates nothing.
7. The app reads `/users/me/`, as after any sign-in.

### 5.1 Security

- **Who can get a code.** Only a process that can open the daemon's socket:
  - on macOS and Linux, the socket sits in a 0700 directory of the OS user (`runner/src/util/paths.rs`);
  - on Windows, it is an owner-only named pipe (`pidash_ipc::windows_pipe::create_owner_only_pipe`).

  So that is any process running as the user. Such a process can already read the daemon's credentials from the keychain or the 0600 file (BE §4.3) and act as the user's machine. What it gains is a web session: everything the user can do in the web app, beyond what a machine credential allows. This design accepts that on the same ground as BE §4.3's keychain note. The boundary is the OS user, as for Claude Code and Codex.

- **Who can redeem a code.** Only the app that asked for it. The code alone is useless: redemption needs the verifier, which never leaves the app's process. It is never in a URL, a log, the daemon, the socket or the server's storage; the server keeps only its hash. Someone who sees a code (in the daemon's log, on the socket, in a proxy) cannot redeem it in a browser or another client. Redemption is a native `POST`, so no code appears in webview history or a `Referer`.
- **Limits on a code.** A code is single-use and lives 60 seconds. It is bound to one user, one machine and one challenge. It is refused when the daemon credential it was issued with, the machine or the user is revoked or inactive. Issue and redeem are rate-limited per machine, and audited with user, machine and outcome, never the code.
- **What a code cannot do.** It cannot sign in another user or another machine. A code leaked from a log is useless after 60 seconds or one use. Redeeming a code never prolongs or changes the daemon credential.
- **Talking to the real daemon.** The app must be sure the socket it asks belongs to the user's daemon. Otherwise another process could hand it a code for an account of its choosing, and sign the app in as that account.
  - On Windows, PIDESKAPP-236 reports that the pipe name is predictable, and that the first process to create the pipe owns it. The Windows handoff ships only once that is fixed. Until then, the Windows app uses a browser sign-in for the CLI → app direction.
  - On every OS, the server refuses, before creating any session, a code whose user is not the `expected_user_id` the app took from `Hello` (step 6).
- **Kill switch.** `DESKTOP_SESSION_HANDOFF_ENABLED`, default on. When it is off, the handoff endpoint refuses. The app then asks for a browser sign-in, and that sign-in signs the CLI in as in section 4.
- **Review.** The handoff needs a security review before it ships. BE section 16 required one for option B.

## 6. Sync rules

### 6.1 What the daemon reports

`Hello` today has an `account` only while some workspace token is live (`runner/src/daemon/supervisor.rs`, `hello()`), so a signed-out daemon reports nothing about when or why it was signed out. Signing out the last workspace also signs the engine out (`runner/src/cli/auth/logout.rs`), and leaves the same empty `account` as a full logout. Revision 1 therefore adds an explicit, persisted record of the last sign-in event, separate from `account`.

- **`auth_event`** in `config.toml` (`[auth_event]`), written by every `pidash auth login` and `pidash auth logout` in the same config write as the credentials, and reported by `Hello` whether or not an account is live:
  - `seq`: a counter that only increases on this computer;
  - `kind`: `signed_in` or `signed_out`;
  - `reason` for `signed_out`: `logout` (`pidash auth logout`, or the app's sign-out), `last_workspace_logout`, `removed` (the computer was removed on the web), `revoked` (the server revoked the credentials) or `expired` (the daemon credential reached its 60 days without use);
  - `account`: `{user_id, email, cloud_url}`, also for `signed_out`, so the app knows whose sign-out it was;
  - `at`: wall-clock time, for display and the tie-break below.
- **`auth_state`**: `signed_in` or `signed_out`, computed from the live tokens. It is the state now; `auth_event` says how it got there.

### 6.2 Reconciling the two sides

**The app's record.** The app keeps `{own_event: {kind, account, at}, last_seen_seq}` in the Tauri host's store, not in the webview. The webview's storage is wiped at every sign-out (`apps/web_new/src/features/auth/signOut.ts`), so a record there would vanish exactly when it matters. `own_event` is the app's last sign-in or sign-out, whichever kind. `last_seen_seq` is the daemon `seq` the app last acted on.

**The rule: the newest event wins, whatever its kind and account.** On every daemon check (launch, focus, network recovery, every minute), the app compares two events:

- its `own_event`;
- the daemon's `auth_event`, when its `seq` is above `last_seen_seq`. A daemon event the app has already seen is never newer than the app's own.

| Newest event                                                                                      | What the other side does                                                                                                                                                                                          |
| ------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The app's: signed in as U                                                                         | The CLI follows. If the daemon is not signed in as U, the app signs it in as U (section 4), switching accounts if needed (section 6.4).                                                                           |
| The app's: signed out                                                                             | The CLI follows. If the daemon is still signed in, as any account, the app runs `pidash auth logout`. This covers an app sign-out whose CLI logout failed because the daemon or the network was down at the time. |
| The daemon's: signed in as U                                                                      | The app follows. If the app is signed out, or signed in as another account, it signs in as U through the handoff (section 5).                                                                                     |
| The daemon's: signed out, `reason` `logout`, `last_workspace_logout` or `removed`, as any account | The app follows. If the app is signed in, it signs out. The account the event names does not matter: the user's last action on this computer was a sign-out.                                                      |
| The daemon's: signed out, `reason` `expired` or `revoked`                                         | Not a user's action, so it does not win. If the app's own event is a sign-in, the app signs the CLI in again silently; otherwise nothing happens.                                                                 |
| No answer, no daemon, or an incompatible daemon                                                   | No comparison. The app keeps its state and its record; the comparison runs at the next successful check (BE §4.4).                                                                                                |

- **Ordering.** The two events are compared by wall-clock `at`; both clocks are this computer's clock. A tie, or an `at` that cannot be read, goes to the daemon, because the daemon hosts the user's runners. After acting, the app sets `last_seen_seq` to the daemon's current `seq`.
- **No echo.** When the app changes the CLI itself, it records the `seq` the daemon reports right after, and does not act on that event again.
- **Workspaces are not accounts.** `pidash auth logout --workspace W` with other workspaces still signed in writes no `auth_event`: the account is still signed in, so the app stays signed in and only W's runners stop (BE §4.9). Signing out the _last_ workspace writes `signed_out` with `last_workspace_logout`, and the app signs out.
- **Another cloud.** The app is built for one cloud (`PI_DASH_URL`).
  - When the user signs in to the app, a daemon signed in to another cloud is switched to the app's cloud (`pidash auth login --force`).
  - When the user signs the CLI in to another cloud, the app cannot follow. It stays as it is, shows "Pi Dash on this computer is signed in to <cloud>", and turns its engine features off.
- **Detection delay.** The app reads `Hello` at launch, on focus, on network recovery and every minute, as today (`RuntimeSupervisor`). A sign-in or sign-out in a terminal therefore reaches an open app within a minute, or at once when the user switches back to it. A pushed `AccountChanged` event on the socket could shorten this later.

### 6.3 Server: the app's sign-in signs the engine in

Today a silent approval issues an engine credential only when the machine's engine is `expiring` or `expired`, or when the daemon has lost a live credential. That rule is in `silent_issue_allowed` (`apps/api/pi_dash/runner/services/engine_credentials.py`), applied by `_maybe_issue_engine_credential` and `_check_engine_grant_approval` (`apps/api/pi_dash/authentication/views/cli/device.py`). `never` needs an interactive "Turn on", and `signed_out` needs an explicit sign-in. An ordinary logout leaves `signed_out`, so under today's rule an app sign-in after any logout would sign in the daemon but leave the engine off.

Revision 1 changes the rule, in Django and in `rust-api/`'s device-flow and engine-credential code:

- A silent approval of a `cli` grant issues the engine credential from `never`, `signed_out`, `expiring` or `expired`, when the grant's approver owns the machine and the instance can serve the engine. With no engine-only sign-out (U-3), `signed_out` now only ever follows a full sign-out of the computer, so a new sign-in of the computer may sign the engine in again.
- A silent approval of an `engine` grant (renewal) is allowed from `never`, `expiring` and `expired`, and from `signed_out` while the machine holds a live daemon credential. Since there is no engine-only sign-out any more (U-3), a full sign-out revokes the daemon credentials together with the engine's. So `signed_out` next to a live daemon credential can only come from an engine-only logout made before this change, and renewing it is the migration. No data migration is needed: the rule reads the existing state in both backends.
- `revoked` (an admin, or "Remove this computer") still needs an interactive sign-in. `active` is still left alone.
- Interactive approvals are unchanged.

### 6.4 Switching the CLI to another account

`pidash auth login` keeps `[daemon].dev_machine_id` across sign-ins (`ensure_dev_machine_id`, `runner/src/cli/runner_ops.rs`). The server refuses a machine id owned by another user (`DevMachineOwnershipError` → `dev_machine_not_found`, in `device.py` `_get_or_create_dev_machine` and `runner/views/enrollment.py`). So a plain sign-in as another account fails today.

A machine identity therefore belongs to one account on one cloud, and a switch moves identities as well as runners:

1. **Park.** `pidash auth logout` as V runs first (credentials revoked; `auth_event` `signed_out`). Then V's identity is moved to `<config_dir>/parked/<cloud_host>/<user_id>.toml` (0600). The identity is `[daemon].dev_machine_id` and V's `[[runner]]` blocks, and their per-runner data directories are kept where they are. Nothing is deleted.
2. **Restore or mint.** If a parked identity exists for U on this cloud, its machine id and runner blocks are restored. Otherwise `[daemon].dev_machine_id` is cleared, so the sign-in mints a new id, and the server creates or finds U's machine as for a first sign-in (BE §4.10). The "Is this <label>? Replace it" prompt then applies.
3. **Sign in** as U with the restored or new id.
4. **Adopt.** The running daemon cannot change identity in process: its machine session, `shared_machine_token` and runner set are bound to the old identity. The CLI restarts it with `restart_and_verify` (`runner/src/service/reload.rs`), as other `config.toml` changes do (BE §3).

**Runs during a switch.** A switch is a sign-out of V, and its runs end as they do after any sign-out. The server's existing recovery does this; it is not changed:

- runs in flight on V's runners are finished as `failed` (or `cancelled` while still pending) once the runner is offline past the fail timeout (`fail_runs_on_offline_runners`, `apps/api/pi_dash/runner/tasks.py`; `finalize_offline_run`, `rust-api/crates/jobs/src/runner_runs/offline_recovery.rs`). V can retry them by hand;
- queued runs pinned to V's runners for locality are released to their pod (`release_pins_on_offline_runners`).

A switch does not requeue an active run automatically. Doing that safely would need new server work against running a task twice (a run whose agent already pushed commits or comments), and is out of scope (section 10).

To avoid losing work unknowingly, the CLI warns before switching when V has runs in flight: interactively, `pidash auth login` asks to confirm; when the app causes the switch, its sign-in screen says "Pi Dash on this computer is running N tasks for <V>; signing in as <U> stops them" and switches only on confirm.

The parking code was first written in pi-dash#2459. That version parks runner blocks only, and is extended here to the machine id.

## 7. Work

Every server change lands in Django and in `rust-api/`. The Rust side is standalone (no forwarding to Python) and uses SeaORM, as in BE section 16. Cloud changes land in `pi_dash_cloud` and in `rust-cloud/`.

| Area                                | Work                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ----------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Server (OSS)                        | `POST /api/v1/auth/desktop-handoff/` (daemon credential, with challenge) and the redeem `POST` (code, verifier, expected user; checks before any session). The code store, rate limits, audit and the `DESKTOP_SESSION_HANDOFF_ENABLED` kill switch. The silent engine-issuance rule of section 6.3. Remove "Sign out the engine" from the machine API. All in Django and `rust-api/`.                                                                                                                                                                                                                                              |
| Server (cloud)                      | The redeem view that mints the desktop JWT pair, in `pi_dash_cloud` and `rust-cloud/`. Prior work: private-pi-dash#302.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| CLI (`runner/`)                     | `Hello` gains `auth_state` and the persisted `auth_event` (section 6.1), written by every login and logout. New IPC request `SessionHandoff { challenge }`. Account switch parks and restores the machine id and runner blocks per cloud and account, then restarts the daemon (section 6.4; prior work: pi-dash#2459). Remove `pidash auth logout --engine`.                                                                                                                                                                                                                                                                       |
| Desktop shell (`desktop/src-tauri`) | A command that generates the verifier, runs `SessionHandoff` and redeems through `desktop_api_request`. Wait for PIDESKAPP-236 before enabling it on Windows.                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| App (`apps/web_new`)                | The sync supervisor with the reconciliation of section 6.2. It is mounted at the app root, outside the signed-in workspace layout, so it runs on the sign-in screen too. CLI-first sign-in starts there, with no user yet. Today's `RuntimeSupervisor` returns early without a `userId` (`apps/web_new/src/features/runners/runtime/Supervisor.tsx`), so the sync cannot live in it. It keeps its `{own_event, last_seen_seq}` record through a host command (section 6.2). Sign-out always runs `pidash auth logout`, and the checkbox is dropped. Remove the engine "Turn on" and "Sign in from Pi Dash Agent settings" messages. |
| Web                                 | Remove "Sign out the engine" from the machine page.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Docs                                | BE §4.6 and PO-13, PO-16 and PO-17 point here. The desktop and CLI READMEs describe the shared sign-in.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |

Prior work to reuse: pi-dash#2458 (server handoff), pi-dash#2459 (runner parking and account fields), private-pi-dash#302 (cloud redeem). pi-dash#2460 was written for the legacy desktop app and is superseded.

## 8. Upgrade

- **App signed out, CLI signed in** (the common case after BE's "CLI first"): at the first launch of the new build, the app signs in through the handoff.
- **App and CLI on different accounts:** at the first launch, the most recent sign-in wins (section 6.2).
- **CLIs from before `auth_event`:** the daemon reports no `seq`. The app treats the daemon's present state as the newest event, so the daemon wins, and it starts recording from the first `seq` the updated CLI reports.
- **CLIs from before `SessionHandoff`:** they don't know the request, so the app treats them like a CLI that cannot hand off. The app asks for a browser sign-in, which then signs the CLI in. The app already offers "Update Pi Dash on this computer" for daemons that are too old (BE §4.4).
- **Engine signed out with `--engine` before this change:** the server holds the engine `signed_out` while the machine's daemon credential is live. Under section 6.3's rule that combination allows a silent renewal, so the app signs the engine in again with no prompt, against either backend. The server's state is what changes; nothing is reinterpreted locally.

## 9. Acceptance scenarios

1. **App first.** A user with the CLI installed but signed out signs in to the app. With no prompt, `pidash auth status` shows the daemon and the engine signed in as that user. (4)
2. **Logout, then app sign-in.** After `pidash auth logout`, the engine is `signed_out`. The user signs in to the app: with no prompt, the daemon _and_ the engine are signed in again. This passes against Django and against `rust-api/`. (4, 6.3)
3. **CLI first.** The CLI is signed in. The user opens the app for the first time, and on the sign-in screen, with no user yet, the app signs in as the CLI's user with no browser sign-in. It then shows the existing runners. No second machine row. (4, 5, 7)
4. **Most recent sign-in wins, with machine identity.**
   - The CLI is signed in as A on machine M_A. The user signs in to the app as B: the daemon restarts as B's machine (a new id M_B, or B's parked one), and A's machine id and runner blocks are parked.
   - The user then runs `pidash auth login` as A: M_A and A's runners are restored, with no `dev_machine_not_found` and no new machine row for A. The app signs out of B and back in as A.
   - With a run in flight on one of A's runners, the app's sign-in as B first warns, then switches on confirm. The run ends as `failed` and can be retried by A; a queued run pinned to A's runner goes back to its pod. Nothing runs twice.
   - At no point do the two sides stay signed in as different accounts. (4, 6.4)
5. **App sign-out.** The user signs out of the app: `pidash auth status` shows the daemon and the engine signed out. Offline, the sign-out still clears the computer locally. (4)
6. **CLI sign-out.**
   - With the app open, the user runs `pidash auth logout`. Within a minute, or at once on focus, the app signs out with its notice and keeps chat history.
   - With two workspaces signed in, `pidash auth logout --workspace W` does not sign the app out.
   - Signing out the _last_ workspace does sign the app out, and the engine is signed out too.
   - A daemon that stops answering does not sign the app out. (4, 6.1, 6.2)
7. **Missed changes while the app is closed.**
   - With the app closed, the user runs `pidash auth logout`. At the next launch, the app finds `auth_event` `signed_out` newer than its own sign-in and signs out.
   - Then, with the CLI still signed out, the user signs in to the app. Its sign-in is now newer than the event, so the CLI is signed back in, not the app signed out.
   - With the app closed, the daemon credential expires (`reason` `expired`). At the next launch, the app signs the CLI in again silently and stays signed in.
   - The user signs out of the app while the daemon is stopped, so its `pidash auth logout` fails. When the daemon is back, the app's sign-out is the newest event: the app runs `pidash auth logout` again, and is not signed back in through the handoff.
   - The daemon's newest event is a `logout` for another account than the app's: the app signs out.
   - The app's record survives the sign-out's webview wipe: after a sign-out and a relaunch, the app still knows its last event. (6.2)
8. **Engine never asks.** An engine in state `never`, `signed_out`, `expiring` or `expired` is signed in or renewed with no prompt while the app is signed in. No screen offers an engine sign-in or sign-out. (4, 6.3)
9. **No daemon.** The user declines set-up. The app signs in and out on its own, and the Pi Dash assistant says it needs Pi Dash on this computer. (4)
10. **Handoff cannot be redeemed elsewhere.**
    - A code redeemed with no verifier, a wrong verifier, or another `expected_user_id` fails, creates no session, and is used up.
    - A code copied from the daemon's socket and redeemed from a browser or `curl` fails.
    - A redeemed code fails a second time, and any code fails after 60 seconds.
    - With the kill switch off, the app falls back to a browser sign-in. (5, 5.1)
11. **Legacy engine-only logout.** A machine whose engine was signed out with `pidash auth logout --engine` before this change, its daemon still signed in: with the app signed in, the engine is signed in again with no prompt. Run against Django and against `rust-api/`. (6.3, 8)
12. **Rust parity.** Scenarios 2, 3, 4, 10 and 11 pass with the auth domain served by `rust-api/` and, for the cloud, `rust-cloud/`. (7)

## 10. Out of scope

- One daemon holding several accounts at once (rejected in BE PO-13).
- Requeuing an active run automatically after an account switch. It would need server work against running a task twice; runs end as `failed` and are retried by hand (section 6.4).
- A pushed `AccountChanged` event. The minute poll and the focus check are enough for revision 1.
- Changing what a web session can do. The handoff creates the same session a desktop browser sign-in creates.

## 11. Relation to other designs

- `built_engine_as_runner/design.md`: §4.6 is replaced by this design, and §4.3's engine sign-out is removed. PO-13, PO-16 and PO-17 are revised.
- `managed_runner/design.md`: unchanged; BE already supersedes its sign-in parts.

## 12. Rules for review

These rules follow from U-1 to U-4, but the product owner did not state them. They need confirming:

1. Only a sign-out the user made counts: `pidash auth logout`, the last workspace's logout, the app's sign-out, or removing the computer on the web. These do not sign the app out:
   - a daemon that stops answering;
   - a workspace sign-out that leaves other workspaces signed in;
   - an expired or server-revoked daemon credential. In this case the app signs the CLI in again instead (sections 6.1, 6.2).
2. The newest event wins, whether it is a sign-in or a sign-out, and whichever account it names. The app's own sign-out wins over an older daemon sign-in, so a failed CLI logout is retried and never undone. A tie or an unknown time goes to the CLI (section 6.2).
3. An app sign-in switches a CLI that is on another cloud to the app's cloud. The app cannot follow a CLI signed in to another cloud (section 6.2).
4. An account switch restarts the daemon. Runs in flight end as `failed` and are retried by hand; queued pinned runs go back to their pod. Before switching with runs in flight, the CLI and the app warn and ask to confirm (section 6.4).
5. A terminal change reaches an open app within a minute, or at once on focus (section 6.2).
6. The Windows handoff waits for PIDESKAPP-236; until then, Windows uses a browser sign-in for CLI → app (section 5.1).
7. A silent sign-in of the computer signs the engine in from `never` and `signed_out`. Only `revoked` still needs an interactive sign-in (section 6.3).
