# Feature inventory: Desktop-only behavior (agent runtime, bare sign-in, updater, deep links, native shell)

Area: Desktop-only behavior (Phase 2). ID prefix `DESK-`.
Editions: desktop.
Source behavior learned by reading the old desktop sources (overlay web layer, edition
desktop seams, native shell commands listed in the coverage checklist below, plus the
overlay test suite that pins their behavior). The packaged desktop app was not run in
this session; the oracle run (NEWFRONT-40) verifies each row against the live build
later. Written from the area spec; no old code, strings, or assets copied.

Row format per the Parity page: ID, capability, who, edition, old entry point, API,
acceptance, parity test (empty for now), status (`not started`).

| ID | Capability | Who | Edition | Old entry point | API | Acceptance | Parity test | Status |
|----|------------|-----|---------|-----------------|-----|------------|-------------|--------|
| DESK-001 | Automatic enrollment of the bundled agent when a project opens, with a small status notice | Signed-in desktop user opening a project | desktop | Floating status pill at the bottom of any project page | Agent availability and profile reads; machine enrollment write; project detail read; native setup, config, credential, doctor and daemon-start commands | Opening a project prepares the on-machine agent without any click: the notice walks from preparing through connected (stays while runs need the app open) or waiting for the first heartbeat, and any failure names its cause; leaving the project stops the polling |  | not started |
| DESK-002 | Readiness gate before creating an agent run on this machine | Signed-in desktop user pressing Run on a work item | desktop | Run action on the work-item detail | Issue detail and project detail reads; same enrollment path as DESK-001 | Pressing Run first makes the local agent ready and waits briefly for the server to observe it; when the work item is not assigned to the local agent nothing local happens and the selection is left untouched |  | not started |
| DESK-003 | Repair notice when the bundled agent binaries are missing | Signed-in desktop user opening a project | desktop | Same status pill as DESK-001 | Native self-check command | When the shipped runner or engine binary is absent, the notice says the install needs repair (reinstall the app) instead of failing later at Run time |  | not started |
| DESK-004 | Plain-language explanation whenever the server will not let the bundled agent run | Signed-in desktop user | desktop | Built-in agent entry in the chat picker; status pill | Agent availability and profile reads | Each server-side refusal (feature off, personal-key setup, no usable model, machine not linked, stale session, revoked access, gateway down) surfaces its own explanation; the picker entry stays visible but cannot start a chat it could never send |  | not started |
| DESK-005 | Built-in on-machine agent offered as a chat contact | Signed-in desktop user opening chat | desktop | Runner/chat picker side list | Agent availability read | A local entry appears alongside cloud runners on desktop only, marked as local; it never appears in a plain browser session |  | not started |
| DESK-006 | Direct chat with the bundled engine bypassing the cloud relay | Signed-in desktop user chatting with the built-in agent | desktop | Runner chat page with the built-in agent selected | Native chat warm / send / cancel / close commands | Warming, sending, cancelling and closing a local chat reach the on-machine daemon; a chat opened right after sign-in first brings the daemon up so the first message does not fail; cancel and close settle quietly without inventing success |  | not started |
| DESK-007 | Per-session approval mode for local chats | Signed-in desktop user chatting with the built-in agent | desktop | Approval mode control above the local chat | Native chat warm / send commands carry the mode | The user picks between asking each time, free movement inside the working copy, and full access (the default, remembered per runner); a change only affects threads started afterwards, never a turn already running |  | not started |
| DESK-008 | Inline approval prompt when the local agent wants to act | Signed-in desktop user mid-turn with the built-in agent | desktop | Prompt card inside the local chat thread | Native approval-answer command | Commands, file edits and network attempts park the turn on a card naming what is wanted plus why; approve and deny continue the turn either way, commands additionally offer always-allow for the rest of the session, and an expiring request counts down while the decision controls stay responsive |  | not started |
| DESK-009 | On-machine chat history scoped to the signed-in account | Signed-in desktop user chatting with the built-in agent | desktop | Local chat session and message lists | Native history commands (create, list, fetch, append, rename, delete, clear, working directory) | Sessions and messages persist on the machine per account and workspace; titles can change, a session deletes with its messages, clearing wipes one account only, and a refresh or restart never loses an already-sent turn |  | not started |
| DESK-010 | Full local teardown on sign-out with history kept by default | Signed-in desktop user signing out | desktop | Sign-out control | Machine enrollment removal call; native daemon-stop, credential-destroy and webview-wipe commands | Signing out stops the daemon, destroys local credentials, removes the server-side machine enrollment and wipes the webview store so no session survives; chat history stays on disk, hidden until the same account signs in again |  | not started |
| DESK-011 | Bare sign-in screen as the desktop entry point | Signed-out desktop user; signed-in user landing | desktop | Desktop root, login and sign-in routes | Session read by the route guard | Signed-out users get the sign-in card directly with no marketing frame on all three routes; signed-in users are forwarded into their workspace honoring a pending destination |  | not started |
| DESK-012 | Visible failure notice when desktop sign-in does not complete | Signed-out desktop user after a failed hand-off | desktop | Sign-in screen with failure query values | none (client mapping) | A provider-side refusal, an aborted flow, or a failed server exchange lands back on sign-in with an explanatory banner instead of a dead form |  | not started |
| DESK-013 | Fallback card where the server has no desktop sign-in hand-off | Signed-out desktop user on the community build | desktop | Same bare sign-in screen | none | Instead of a form that could never establish a session, the card explains desktop login is unavailable there and offers opening the hosted app in the system browser |  | not started |
| DESK-014 | System-browser login completed through a custom-scheme callback | Signed-out desktop user signing in via the browser | desktop | System browser plus automatic return to the app | Server desktop-exchange navigation carrying the returned code and state | The app opens the real browser for login and finishes the session when the callback returns; the app window comes forward, provider refusals land on sign-in with their reason, and unrecognized links are ignored |  | not started |
| DESK-015 | Browser-opening limited to web addresses | Signed-in desktop user following an open-in-browser action | desktop | Sign-in fallback card and any native open action | Native open-in-browser command | Only hosted addresses are opened externally; anything else is refused rather than handed to the OS |  | not started |
| DESK-016 | Native update prompt at launch | Desktop user starting the app with a newer release available | desktop | Native dialog at startup | Update feed check; native download-and-install plus restart | A new release offers installing now (downloads, installs, restarts into it) or deferring to the sidebar button; a failed install says so in a dialog, keeps the current version running, and keeps the update available for retry |  | not started |
| DESK-017 | Silent daily update check while the app stays open | Desktop user with the app running past midnight | desktop | No prompt; feeds DESK-018 | Same feed check on a daily schedule | Roughly once a day the app checks quietly and only lights up the sidebar button when something is found; checks never interrupt, never overlap an ongoing install, and a machine asleep across days checks once on waking |  | not started |
| DESK-018 | Sidebar button that installs a waiting update | Signed-in desktop user with a deferred or background-found update | desktop | Corner button beside the sidebar user menu | Native pending-update query and install commands | The button appears whenever an update is waiting (including ones deferred before it mounted), names both versions, disables itself while downloading, and reports a failure in place with a working retry |  | not started |
| DESK-019 | Shareable work-item links copied from the desktop app | Signed-in desktop user sharing a work item | desktop | Copy-link actions in the detail header, peek header and every list menu | none beyond the normal link builder | Copied links point at the hosted app (never the local bundle origin), keep any query and fragment, and refuse to produce a link when no hosted origin is configured |  | not started |
| DESK-020 | Credential-safe API transport through the native layer | Signed-in desktop user anywhere data loads | desktop | Every authenticated API call and live event stream | Native request, stream and cancel commands | Cookies work despite the bundle origin; file uploads, relative addresses, downloads, timeouts and cancellations behave as in the browser; transport failures read as network errors (never forged logouts), and live streams resume from the last seen event without reconnecting on hard errors |  | not started |
| DESK-021 | Command-line runner setup from inside the app | Signed-in desktop user turning this machine into a runner host | desktop | Local runner setup flow | Device-grant start and approve calls; native CLI detect, install and login commands | The app reports whether the CLI is present and which version, installs it from its own bundle showing plain-text progress, then signs it in through a grant the app itself approves — the CLI owns the resulting credential |  | not started |
| DESK-022 | Tray and window behavior of the desktop shell | Desktop user with the app running | desktop | OS window controls, tray icon and tray menu | none (native shell) | Closing the window parks the app in the tray instead of quitting; the tray offers showing the app and quitting for real, clicking the icon or launching again brings the open window forward, and the window opens at a fixed default size with a minimum |  | not started |
| DESK-023 | Familiar zoom controls in the desktop menu | Desktop user reading small or large text | desktop | View menu and its shortcuts | none (native shell) | Zoom in, out and reset work from the menu with the usual shortcuts on every platform |  | not started |
| DESK-024 | Server-hosted pages never take over the desktop window | Signed-in desktop user following a redirect | desktop | Any navigation landing on the server host | none (native shell) | A page that would render from the server host is bounced back to the equivalent bundled page instead of loading remotely |  | not started |
| DESK-025 | Desktop bundle without web-only routes | Signed-in desktop user navigating the app | desktop | Any route that exists only on the web | none | Marketing and other web-only pages are absent from the desktop bundle and fall through to not-found; the desktop login and sign-in paths render the bare sign-in screen |  | not started |
| DESK-026 | Live engine setup refresh when server settings change | Signed-in desktop user after a model or assistant settings change | desktop | Pi Dash settings (AI assistant) | Agent profile and model-credential reads; native engine-config and token-write commands | Changing the model or assistant setup rewrites the on-machine engine setup and rotates the short-lived model credential without restarting anything; an expired credential stops the daemon and asks for sign-in again |  | not started |
| DESK-027 | Desktop onboarding starts at profile setup with no CLI-install step | Onboarding desktop user | desktop | Onboarding flow (SHOW_CLI_INSTALL_STEP=false overlay seam) | none | Desktop onboarding opens on profile setup, progress order excludes the CLI-install step, and no back affordance appears on profile setup (target per main#494) |  | not started |

## Coverage checklist

Every route file, top-level component folder, native command and API endpoint in the
assigned sources, mapped to rows. Endpoint paths below are backend contracts as observed
from the old frontend's call sites.

### Route files (`desktop-overlay/apps/web/app/…`)

| Source | Covering rows |
|--------|---------------|
| `(home)/page.tsx` (desktop root: bare sign-in or forward to workspace, failure banner) | DESK-011, DESK-012 |
| `routes/redirects/core/login.tsx` (sign-out landing renders the same screen) | DESK-011 |
| `routes/redirects/core/sign-in.tsx` (failed hand-off landing keeps the failure query) | DESK-011, DESK-012 |
| `routes/extended.ts` (empty: no web-only routes in the bundle) | DESK-025 |
| `react-router.config.ts` (plain client-side app, no prerendering) | DESK-025 |

### Top-level component folders and service files

| Source | Covering rows |
|--------|---------------|
| `desktop-overlay/apps/web/core/services/agent-runtime.ts` (enroll, credential, daemon lifecycle) | DESK-001, DESK-002, DESK-003, DESK-004, DESK-010, DESK-026 |
| `desktop-overlay/apps/web/core/components/agent-runtime.tsx` (status pill, focus/interval refresh) | DESK-001 |
| `desktop-overlay/apps/web/core/services/local-chat-transport.ts` (local chat verbs, frame translation, history persistence) | DESK-006, DESK-007, DESK-008, DESK-009 |
| `desktop-overlay/apps/web/core/components/runners/local-chat-contacts.ts` (picker entry + reason) | DESK-004, DESK-005 |
| `desktop-overlay/apps/web/core/services/desktop-session.ts` (webview store wipe after sign-out post) | DESK-010 |
| `desktop-overlay/apps/web/core/services/pidash-cli.ts` (detect, install, self-approved device grant) | DESK-021 |
| `desktop-overlay/apps/web/core/components/desktop-update-button.tsx` (sidebar install button) | DESK-018 |
| `desktop-overlay/apps/web/core/utils/desktop-web-url.ts` (shareable-link origin resolution) | DESK-019 |
| `desktop-overlay/apps/web/core/components/issues/issue-detail/issue-detail-quick-actions.tsx` (detail copy-link path) | DESK-019 |
| `desktop-overlay/apps/web/core/components/issues/issue-layouts/quick-action-dropdowns/helper.tsx` (list-menu copy-link path) | DESK-019 |
| `desktop-overlay/apps/web/core/components/issues/peek-overview/header.tsx` (peek copy-link path) | DESK-019 |
| `desktop-overlay/apps/web/tests/desktop/` (8 suites pinning the above: lifecycle, pill, adapter, event source, button, links, transport, CLI) | DESK-001, DESK-002, DESK-006 through DESK-009, DESK-018, DESK-019, DESK-020, DESK-021 |
| `apps/web/ce/components/desktop/sign-in-card.tsx` (community unavailable card + open-in-browser) | DESK-013, DESK-015 |
| `apps/web/ce/components/desktop/agent-runtime-edition.ts` (reason-code wording, forgery-token path) | DESK-004 |
| `apps/web/ce/components/desktop/chat-approvals.tsx` (mode control + inline prompt wording) | DESK-007, DESK-008 |
| `apps/web/ce/components/desktop/helper.ts` + `sidebar-workspace-menu.tsx` + `index.ts` (community defaults: no-ops) | Dead code on this build: both render nothing / report visible, so no capability rows; an edition replacing them would add rows under its own inventory |
| `apps/web/core/components/desktop-update-button.tsx` (web-build stub rendering nothing) | DESK-018 (documents the web side of the seam: no button outside the desktop bundle) |

### Native shell commands (`desktop/src-tauri/src/*.rs`)

| Command | Covering rows |
|---------|---------------|
| `open_in_browser` (`main.rs`) | DESK-015 |
| `desktop_clear_web_data` (`main.rs`) | DESK-010 |
| Deep-link handler (`main.rs`: custom-scheme callback, error branch, window focus, single-instance) | DESK-014, DESK-022 |
| Window, tray, zoom menu and navigation guard (`main.rs`) | DESK-022, DESK-023, DESK-024 |
| `managed_bootstrap`, `managed_enroll`, `managed_write_engine_config`, `managed_write_model_token`, `managed_start_daemon`, `managed_stop_daemon`, `managed_sign_out`, `managed_doctor`, `managed_paths` (`managed_runner.rs`) | DESK-001, DESK-002, DESK-003, DESK-010, DESK-026 |
| `chat_warm`, `chat_send`, `chat_cancel`, `chat_close`, `chat_decide` (`chat.rs`, frames over `chat://frame` / `chat://error`) | DESK-006, DESK-007, DESK-008 |
| `chat_create_session`, `chat_list_sessions`, `chat_get_session`, `chat_list_events`, `chat_append_event`, `chat_set_thread_id`, `chat_rename_session`, `chat_delete_session`, `chat_clear_history`, `chat_working_dir` (`chat_history.rs`) | DESK-009 |
| `desktop_api_request`, `desktop_api_stream`, `desktop_api_cancel` (`desktop_http.rs`) | DESK-020 |
| `detect_pidash_cli`, `install_pidash_cli` (progress on `pidash-install-log`), `pidash_cli_login` (`pidash_cli.rs`) | DESK-021 |
| `desktop_pending_update`, `desktop_install_update` plus launch prompt and daily scheduler (`updates.rs`) | DESK-016, DESK-017, DESK-018 |
| Shared IPC protocol (`ipc.rs`) | DESK-006 (wire format behind the chat frames; no direct user surface) |
| `tauri.conf.json` (custom `pidash` scheme registration), `capabilities/default.json` (invoke allowlist), `build.rs` (release guards incl. external-sign-in check) | DESK-014, DESK-015. Build-time guards and the dev hot-reload mode are not user behavior: no rows |

### API endpoints observed from these sources

| Endpoint | Covering rows |
|----------|---------------|
| Agent profile and availability reads; agent model-credential issue | DESK-001, DESK-002, DESK-004, DESK-026 |
| Desktop machine enrollment write and removal | DESK-001, DESK-010 |
| Project detail and project list reads; issue detail read | DESK-001, DESK-002 |
| Forgery-token fetch before unsafe calls | DESK-001, DESK-021 |
| Device-grant start and approve | DESK-021 |
| Server desktop-exchange navigation (deep-link completion) | DESK-014 |
| Sign-out post (server session end; cookie deletions do not reach the webview, hence DESK-010) | DESK-010 |

No source mapped to zero rows. No `bug:` rows: nothing observed contradicted its
evident intent; the oracle run marks any such scenario if the live app disagrees.
Easy-to-miss behaviors called out as their own rows: DESK-003, DESK-004, DESK-008,
DESK-010, DESK-012, DESK-013, DESK-015, DESK-017, DESK-019, DESK-023, DESK-024,
DESK-026. No role-gated desktop behavior was found in the assigned sources (every
capability is signed-in vs signed-out); no custom keyboard shortcuts exist beyond the
native zoom menu (DESK-023). The native HTTP transport's web-side seam
(`packages/services` adapter + event source) sits outside the assigned sources and is
covered here through its Tauri commands and overlay suites (DESK-020).
