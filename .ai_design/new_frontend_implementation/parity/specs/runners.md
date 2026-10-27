# Runners area spec — runners, runs, approvals, chat, dev machines (RUN-001–049)

Parent epic NEWFRONT-74. Inventory
`.ai_design/new_frontend_implementation/parity/inventory/runners.md` (rows
RUN-001–049; RUN-001–048 oracle green, RUN-049 not started — added after the
oracle run for main#544). Oracle scenarios
`apps/web_new/e2e/parity/runners/` (overview-pods, add-runner,
runs-approvals, chat and dev-machines-detail spec files, green on `apps/web`).

Method: the old frontend was read as a behavior reference only — the
workspace and project runners routes, the AI-dev-machines route, the runners
component tree, the agent-runtime stub and service seams, the runner and pod
services, the chat and runner transports, the desktop-api adapter and
desktop event source, the desktop-overlay agent-runtime plus local chat
pieces, and the cloud-edition desktop approval and edition-copy modules.
Nothing is imported, copied or translated from those sources; this spec
restates what was learned in prose. The only code-like tokens below are API
paths, query parameter names and payload field names, which the contracts
issues (NEWFRONT-530, NEWFRONT-536) turn into zod contracts. New code is
written from this spec, the wiki pages (Reference not source; Parity;
Architecture; Data layer; UI kit and editor) and the kit — never with old
files open.

Much of this area is AI Republic's own code, so this spec stays short where
behavior will be ported: each section records the behavior with the row
mapping and names the own-code modules its slice ports, without
re-describing ported logic. Anything those modules use from Plane code stays
reference-only and is rewritten. The per-slice module map is collected in
§12. Slices adapt ported logic to kit components, TanStack Query with
`@pidash/api-client` contracts, and `core/platform` for desktop calls, with
no runtime imports from the old paths.

Status convention: this spec owns no inventory Status changes. Slices append
`built` to the rows they implement.

Where the running old app contradicts the inventory row text, the oracle
pinned the observed behavior and filed a bug issue. Those deviations, plus
the rows whose target behavior postdates the code at this base, are recorded
in §13. Slices implement the target stated in each section below; the gate
proves the target.

## 1. Area shell — access, overview, tabs, rail, badges (RUN-001–005, RUN-010, RUN-011; slice NEWFRONT-540)

One page module serves the runners overview in both scopes: with a project
id in the route every list and every link base is project-scoped, otherwise
the page is the workspace aggregate. The workspace shell is a side-nav rail
plus a main region hosting the nested route; the project twin reuses the
same modules inside project chrome with its own breadcrumb bar. Entering
either scope loads the runner list and the pod list in parallel. Oracle:
`overview-pods.spec.ts`.

Access is role-gated only, with no edition or feature flag anywhere in the
area (RUN-001). Workspace runner and AI-dev-machines screens require
workspace admin or member; project-scoped runner screens require admin or
member on that project. Guests and non-members get a not-authorized view
rendered inside the normal workspace or project chrome, with surrounding
shells still working. The block fires only once role information has
loaded. One asymmetry is preserved as behavior: the dev-machines shell waits
for the permission record and renders nothing until then, so it never
flashes the blocked view on first paint, while both runners shells check a
bare record and can briefly show it.

The overview table (RUN-002) shows one row per runner with name, pod,
status, operating-system and architecture, version and last-heartbeat
columns, the heartbeat as a localized date with a dash fallback. An empty
table invites adding the first runner; loading and empty are not
distinguished — the empty message doubles as the loading state. The list
re-polls every few seconds so status changes appear without navigation, and
the pod section polls on a slower cadence. Pods render as tiles with name, a
default marker and a runner count; a load failure alone gets an error line.

Three tabs route to the overview base, the runs child route and the
approvals child route in either scope (RUN-003). The active tab derives from
the URL and is deep-linkable; the overview tab matches exactly so it is not
highlighted on child routes. The tab strip and the rail are pure routing
with no data calls of their own.

The rail (RUN-004) shows an overview link plus one chat contact per
connected runner, each with a colored status dot, scoped to the current
workspace or project and refreshed on the same short cadence as the list
under its own cache key. A per-scope empty message shows when no runners are
connected; there is no loading skeleton and no error display for the rail
fetch. The desktop built-in contact section lives in the same rail but
belongs to slice NEWFRONT-553 (§9).

The naming inconsistency (RUN-005, bug NEWFRONT-191) is confirmed and wider
than one label: the rail header and the page title use one area name while
the project breadcrumb and header use another, and the in-page headings and
the CLI verb use a third. NEWFRONT-191 has now settled it (decision D27):
**AI Workers** is the one canonical user-facing name for this area, on every
surface — browser title, rail header, project breadcrumb and header, in-page
headings, project sidebar entry, project-settings tab and our documentation.

Three things do not change with the display name. "Runner" stays the
technical noun of an individual agent host: the CLI verb (`pidash runner
add`), the `/api/runners/…` paths, the `Runner` model and our code
identifiers keep it. A pod stays "pods" and a run stays "runs"; they are not
renamed. And the agents area (schedulers, prompts, assistant) keeps its own
name, which is why "AI Agents" is not available for this one.

The rename is deliberately not built here: RUN-005 still pins the current
split names as the bug the oracle observes, and the rename lands as its own
follow-up slice (linked as NEWFRONT-946). Until that slice runs, the new
app keeps rendering "AI Agents" in the title and rail, "AI Workers" in the
project breadcrumb/sidebar/tab, and "Runners" in the overview heading, exactly
as the oracle expects. No behavior change is implied by this row.

Each runner row (RUN-010) links to the runner detail and offers Delete
always and Revoke only for an enrolled, not-already-revoked runner — the
revoke action is hidden rather than disabled when it does not apply.
Deletion removes the row, forces the daemon offline and preserves historic
runs, after a confirmation that also notes the agent binary stays installed;
revocation invalidates credentials while keeping the row. Both confirmations
block dismissal while in flight, toast on failure, and re-fetch on success.
Deletion from this page offers no local-purge toggle: the call omits the
option so the service default of cascading local teardown applies silently.

Runner status (RUN-011) is one of four states — online, busy, offline,
revoked — each with a distinct badge variant and dot color shared between
the table column and the rail dots. Runners added but not yet enrolled read
as offline until enrolment.

Reads and writes: list runners by workspace with an optional project scope;
list pods with the project filter winning over workspace; delete runner with
the purge-local default; revoke runner. No edition, flag or socket calls.

## 2. Pods — view, filter, create, edit, delete (RUN-012–015; slice NEWFRONT-542)

Clicking a pod tile filters the runner table to that pod with a
clear-filter affordance (RUN-012). The filter is client-side and self-heals:
it clears itself if the selected pod disappears or the workspace changes. A
load-failure message shows when pods cannot be fetched.

A create-pod tile opens a modal collecting a required project, a required
name and an optional description (RUN-013). The project list loads only
while the modal opens; the picker is disabled when the list is empty or
unloaded and a load failure shows inline. Names are trimmed, blank names
rejected, and the project identifier prefix is applied server-side. Submit
shows a working state with cancel disabled; success re-fetches pods and
closes; failure toasts and stays open. The form resets to defaults on each
open.

Editing (RUN-014) renames, edits the description, or promotes the pod to the
project default. The name field edits only the suffix: the project prefix is
stripped for editing and re-applied server-side. The default toggle is
one-way — disabled with an explanatory note on the current default, since a
pod cannot un-default itself and only promoting another pod moves the flag.
Only changed fields are sent after trimmed comparison, and a fully unchanged
submit closes without any call. The form re-seeds from the pod on every
open so stale values never leak between edits. Cross-screen effect: the
default pod is where issue delegation lands, so changing it affects run
assignment elsewhere (seam in §11).

Deletion (RUN-015) warns that assigned issues are unassigned while runners
and past runs are preserved. Server guards map to plain language by code —
pod still has runners, pod has active runs, the default pod cannot be
deleted — with a generic fallback. Deleting the currently filtered pod
clears the filter; the confirm blocks dismissal while deleting.

Reads and writes: list pods; create pod with project, name and optional
description; update pod with the changed subset; delete pod. The
single-pod fetch and the move-runner-to-pod capabilities exist in the
service layer with no entry point on the reviewed screens and get no row
(see §13).

## 3. Add-runner modal — machine path, CLI command, catalog (RUN-006–009, RUN-049; slice NEWFRONT-543)

The modal opens from the overview and the dev-machines pages and offers two
ways to create a runner (RUN-006). A project is required, prefilled and
locked when opened from a project route; pod and project selections cascade
— choosing a project narrows pods, choosing a pod backfills or clears the
project, and a default-pod sentinel omits the pod so the server resolves it.
The name field is format-validated: blank auto-assigns, otherwise the
trimmed value must start with a letter, digit or underscore and continue
with a bounded run of letters, digits, underscore, dot or dash. All state
resets each time the modal reopens. Oracle: `add-runner.spec.ts`.

With a connected machine chosen, the app requests creation on it and polls
for the result through preparing, success, error and timeout states
(RUN-007). The machine picker refreshes while open, auto-selects the first
connected machine once per open when untouched, and snaps back to manual if
the chosen machine leaves the connected set; submit stays disabled while the
chosen id is neither manual nor currently connected. The poll is bounded
(about a minute and a half of short-interval status reads), transient
status-fetch failures do not stop it, and closing or reopening the modal
cancels it. On error or timeout the entered values carry over to the manual
command. Machine-offline has its own message distinct from generic
cloud-unreachable. The daemon-too-old upgrade hint and the supported-agent
gating below are recorded targets, not current modal behavior (see §13):
at this base the modal renders daemon errors verbatim and gates nothing.

The manual path (RUN-008) builds a ready-to-run enrolment command from the
chosen fields with a shell selector for POSIX-style, PowerShell and
Command-Prompt quoting — multi-line with continuations for the first,
single-line for the others, each with its own safe-token and quoting rules —
and omits empty flags. A copy control writes it to the clipboard with a
transient confirmation of a couple of seconds and an error toast on failure.
When no API base URL is configured the command falls back to the browser
origin with an explanatory note shown only in that case.

The agent and model selectors (RUN-009) offer six agent CLIs, each with its
own curated model list, defaulting to Claude Code. A default sentinel omits
the model flag so the agent uses its own default; the preselected default
stays pinned so commands never inherit a stale global. Only the Codex agent
exposes a reasoning-effort dimension (its base models crossed with effort
tiers, each emitting both flags); other agents bake tiers into their slugs
or offer the sentinel only. The model resets to the new agent's default on
every agent change so stale selections never leak. The catalog is advisory:
unknown ids emit no flag and the runner downgrades to the agent default
rather than failing. Gating agent options against the selected machine's
advertised supported set, with upgrade hints and snap-to-first-supported, is
a recorded target (see §13).

RUN-049 extends the catalog: the Claude Code list gains Opus 5.5 above the
Opus 5 entry while the default stays Opus 4.8, and selecting it emits the
corresponding model flag. At this base the entry does not exist yet; slice
NEWFRONT-543 implements it and adds its oracle scenario, which no earlier
oracle run could cover. Cursor options are unchanged.

Reads and writes: list dev machines by workspace; list pods; projects-lite;
create-runner on a machine returning a request id; poll the request status.
The modal owns no Status changes beyond its rows.

## 4. Runs list and detail — browse, stream, diagnostics, cancel (RUN-016–022; slice NEWFRONT-545)

The runs tab (RUN-016) lists runs newest-first — ordering comes from the
server page envelope — with started-time, status and prompt columns; prompt
cells truncate. The current page comes from the address query, is
deep-linkable, and the footer pager rewrites the address by history-replace,
dropping the query on page one; the pager disables at the ends and a page
counter shows only when totals are positive. An empty state shows when there
are no runs. The list re-polls every few seconds with page and scope in the
cache key so rows never bleed across scopes; selecting a run preserves the
page query. Oracle: `runs-approvals.spec.ts`.

Selecting a row opens the detail (RUN-017) showing the run id, status, an
executor badge distinguishing the cloud agent from the local runner and,
when the run came from a scheduler, a link to that scheduler binding — shown
only when both the binding and its detail resolve. The prompt shows in full.
Nothing-selected, loading and not-available states are handled, the failure
case naming deletion or wrong workspace without retries. The detail re-polls
on a short cadence while the run is non-terminal and stops at terminal
status; the run id is part of the address and directly linkable.

The events section (RUN-018) renders a sequenced table; agent-narrative
events with text expand inline beneath their row so the run reads like a
live transcript, while other kinds show as metadata rows. Updates arrive on
the detail poll and stop at terminal status.

Failure and result (RUN-019) show the raw error when a run errors, plus —
when a structured diagnostic is present — a color-coded failure-source
badge, a human failure-kind label with raw fallback, a summary and an
optional suggested action. A result payload renders in full, with a
cloud-agent string summary surfaced above the raw payload.

The cloud-execution panel (RUN-020) appears only for cloud-agent-executor
runs: it explains the run is a stateless, tool-limited agent with no
filesystem or shell access, lists the planned tools, and lists each executed
tool call with risk and status. Local-runner runs never show it.

Cancellation (RUN-021) offers a control only while the run is neither
terminal nor already cancel-requested. The confirmation names the
executor-specific stopping behavior — the cloud agent stops at the next safe
boundary, the local runner stops on signal. The modal cannot close while
submitting; success re-fetches the list, failure toasts, and the server
rejects already-terminal runs.

The run-status badge set (RUN-022) is a shared component mapping the full
run-status set — queued, assigned, running, cancel-requested,
awaiting-approval, awaiting-reauth, paused-awaiting-input, blocked,
completed, failed, cancelled, refused — to variants and readable labels.
Unknown or retired statuses fall back to a neutral badge showing the raw
value rather than crashing. The badge is shared with the schedulers area
(seam in §11); this area owns the mapping.

Reads and writes: list runs with page, workspace and project; get run with
events included; cancel run with a reason. A second, separate six-state
client-derived activity badge exists on the runner detail (§10) and must not
be conflated with this twelve-state run badge.

## 5. Approvals queue, run dispatch and re-tick (RUN-023, RUN-024, RUN-046, RUN-048; slice NEWFRONT-547)

The approvals tab (RUN-023) lists pending approval requests as cards with
the owning run, request time, a human label per kind — shell command, file
change, network access, generic fallback — plus an optional reason, an
optional expiry and the full request payload. An empty state shows when
nothing is pending. The queue re-polls on the shortest cadence in the area
so new requests appear and decided ones drop off; the cache key splits by
project scope versus workspace aggregate. On desktop, chat-originated
approvals are handled inline instead (RUN-034, §9); this queue is the
cloud and web surface.

Each card (RUN-024) offers accept-once, accept-for-session and decline with
no confirmation step and no bulk action. The acted card shows per-card
loading; success re-fetches so the card drops off, failure toasts. Intended
behavior the slices build: the acted card drops off on refetch, the
decision persists with web source and decider, and the run resumes to
running. The live app currently fails both halves — every decide call
errors server-side and session-scoped acceptance is rejected by validation
(bugs NEWFRONT-297 and NEWFRONT-189, see §13). The gate proves the intended
behavior once the backend is fixed and otherwise files or waits on those
trackers; nothing works around them.

Triggering a run from a work item (RUN-046) dispatches through the shared
dispatch service: Run AI and Comment-and-Run post only workspace plus
work-item ids with a trigger flag, leaving prompt rendering and executor
choice to the server, with a queued shape for the already-active case. The
executor resolves server-side from the issue pin, else the project default,
else a fallback, and is read-only from the client's view — a run click
never changes it. On desktop, only when the target resolves to the managed
local runner does the app connect the local project and wait briefly for
the first heartbeat before creating the run, surfacing queued or started or
an honest error; on a plain browser that preparation makes no calls, and
explicit non-managed selections proceed on their existing paths. At this
base the web-tree preparation step is an inert stub and the Run AI control
carries no desktop gating — the real preparation lives in the desktop
supervisor service ported by slice NEWFRONT-552, which this slice consumes
but does not duplicate. The mount points (run controls and status cards on
issue surfaces) are owned by the issues area (seam in §11).

Re-tick (RUN-048) either grants additional runs — returning the new budget
and any queued run — or refuses with a machine-readable reason rather than
an error. The control renders only when the server-provided re-tick flag on
the issue is true; a grant toasts success, a refusal toasts informationally,
and only transport failures toast errors; the issue refreshes after either
outcome. Re-tick is a human action — an agent run cannot re-tick itself —
enforced server-side with no client-side rule beyond the flag.

Reads and writes: list pending approvals with optional project scope;
decide approval with the three-way decision; create run in the run-ai and
comment-and-run variants; cancel run with a reason (shared with §4 — the
issue card's abort path posts to the same endpoint through the dispatch
service); re-tick by work item.

## 6. Chat shell — contact route, history, warm-up, gating (RUN-025, RUN-026, RUN-028, RUN-032; slice NEWFRONT-549)

The runner to chat with comes from the runner id in the address and is
deep-linkable (RUN-025). Contacts show live status dots; changing runner
resets in-memory chat state. Empty and unavailable states are handled.
Oracle: `chat.spec.ts`.

The history panel (RUN-026) lists prior conversations newest-first by last
activity, falling back to creation time without mutating the cache, with
last-activity labels and status suffixes; a new-chat control starts a fresh
session. A session query parameter deep-links one conversation. With no
explicit selection the page auto-picks the most recent usable open session —
preferring one with messages, then any open session — while an explicit
click or address wins even for a closed session so it stays readable; a
stale or foreign session id falls through to the automatic pick. New-chat
shows a working state and toasts on failure; creation prepends to the list
without revalidation and selects before mutating so no render can re-warm
the old session.

Opening a runner warms a session so the first message streams promptly
(RUN-028). Warming happens only when the runner is online or busy — busy
explicitly no longer blocks chat — and is skipped for offline and revoked
runners. Viewing an explicitly chosen history session never spins one up,
while a stale id still allows auto-create. Warming is de-duplicated by
refs so it happens once per runner.

The header shows the runner name, a secondary line preferring the session
working directory then pod then status, a status badge, the approval-mode
selector only when the transport supports approvals, and a close control
whenever a session is resolved (RUN-032). The composer blocks input with a
specific reason — still loading, runner offline, runner revoked, session
closed, a response already in progress — and a busy runner does not block
chat.

Reads: list runners polled; runner detail; list and create chat sessions;
get session. The message verbs belong to §7.

## 7. Chat messaging — send, stream, stop, close (RUN-027, RUN-029–031; slice NEWFRONT-551)

The composer (RUN-027) sends a non-blank draft on Enter and keeps newline
on Shift-Enter; the send control disables on an empty or whitespace draft,
and while sending the composer disables. Sending clears the draft
optimistically, reuses or creates an open session, pushes the approval mode,
sends, then refetches messages and sessions; on failure the draft is
restored verbatim with an inline banner carrying the real reason plus an
error toast. Push-to-talk voice dictation appears only where dictation is
configured and enabled — hold to record, release to transcribe, with elapsed
display, settings routing when unconfigured and retry after error or
denial — staying hidden while its configuration loads or fails. While the
agent replies the send control becomes a stop control.

The assistant reply streams token by token and renders markdown, never
showing a blank bubble (RUN-029). An activity strip above the composer shows
recent agent actions — commands run, files edited, warnings, awaiting
approval, failures — capped to the last few, collapsing adjacent
started-or-completed pairs for the same command and suppressing lifecycle
narration and transcript-duplicating kinds. Deltas apply by sequence number
with freshness checks against mount time, duplicates skipped, and only
while realtime; terminal frames reconcile the stream with saved history and
trigger message plus session refetch. A transient stream error shows a
dismissible banner cleared by the next frame. Cloud uses a resumable
server-sent event stream with last-event-id resume; desktop maps local
daemon frames into the same shape through the registered local transport,
whose subscription accepts but ignores the cloud resume sequence since its
history backfills from the local store.

Stop (RUN-030) cancels the in-flight turn with a user-cancelled reason,
clears any parked inline approval immediately, and refetches sessions;
message refresh on that path arrives via terminal stream events rather than
a second fetch. Close (RUN-031) closes the session and refetches sessions.
On cloud the session moves to a closed state that disables the composer and
labels the history entry; on desktop there is no closed state, so the
transcript stays readable and the session continues to read as open.

Writes: send chat message creating a session first if needed, with the mode
attached; cancel chat with a reason; close chat. The stop and close
handlers refetch sessions only.

## 8. Desktop runtime lifecycle — supervisor, enroll pill, sign-out (RUN-044, RUN-045, RUN-047; slice NEWFRONT-552)

A background supervisor (RUN-044) keeps the local agent connectable after
sign-in on desktop only; on web and cloud the component renders nothing and
the service calls are inert. One effect resumes the runtime for the signed
account, registers the local chat transport once, and refreshes on a
one-minute interval plus window focus: it restores the per-user enrollment
cache, fetches the agent profile (an unavailable server stops the daemon
and surfaces the reason), writes the engine config, and renews the
short-lived model credential whenever under a couple of minutes of life
remains, rejecting expired or malformed credentials. Refresh restarts each
workspace daemon and stops the daemon when a refresh fails on an expired
credential. Any unauthorized response stops the runtime, invalidates
in-flight work, forgets enrollments, clears the credential and signs the
native side out. A readiness map keyed by workspace memoizes the chat path
so later warms and sends only renew credentials and check the daemon.
Oracle: `desktop-runtime.spec.ts`.

Opening a project (RUN-045) — or a direct work-item link, resolving the
project by identifier within the active workspace — shows a small banner
that transitions from preparing through waiting to runs-locally as the
machine enrols: host health check with a reinstall message when the runner
or engine is missing, host-change forgets enrollments, configuration,
server enrollment plus native bootstrap for new workspaces, project
resolution defaulting to the workspace default else the oldest active
project, native enrollment and daemon start. Availability then polls every
few seconds and stops as soon as the managed-runner option appears; a
focus-triggered reconnect restarts the wait if the agent later goes away,
and a failed re-check shows the error without stranding the poll. Errors
are shown honestly — including a needs-repair message when local runner or
engine pieces are missing — and connectivity is never falsely claimed. An
unauthorized response triggers a full local sign-out. Leaving the project
stops the poll.

Signing out (RUN-047) tears the local runtime down on desktop only: mark
stopped and invalidate in-flight work, drop the enrollment cache, native
sign-out immediately — passing history deletion only when the confirmation
dialog chose it — then best-effort server-side enrollment deletion with a
warning on failure, native sign-out again, forget enrollments and zero the
credential expiry. If server revocation fails the sign-out still completes
so the user is never trapped; a two-phase stop avoids racing an enrolment
still completing. Approval modes are stashed per account before the wipe
and restored on the same account's return, with newer local values winning;
both wipe steps swallow failures so sign-out still lands on the sign-in
page. On web and cloud all of this is inert. The confirmation dialog
explains sign-out removes credentials but keeps that account's
built-in-agent chats on the machine, hidden until the account returns, with
an initially unticked delete-history choice noting task folders are kept.

Reads, writes and native calls: agent profile read; agent-token issue;
desktop enroll create and delete; project and issue reads for resolution;
daemon and host lifecycle over native IPC (doctor, write engine config and
model token, bootstrap, enroll, start and stop daemon, sign-out); desktop
cookie, storage and cache wipe. The user-facing reason-code copy
(feature-disabled, key restrictions, missing model config, disconnected
desktop, session requirements, gateway states, outage) lives in the
edition module, which slices treat as reference-only and rewrite (§12).

## 9. Desktop chat — local contact, inline approvals, mode, local exec (RUN-033–036; slice NEWFRONT-553)

On desktop only, a built-in local-agent contact appears above the runner
contacts in the rail (RUN-033). It is clickable when the server reports the
managed agent available and otherwise shown disabled carrying the server's
reason, so a chat that cannot send never looks like a hang. A shim resolves
the synthetic built-in id to an always-online runner so the shared chat
page renders unchanged; availability fetches slowly without focus
revalidation. On cloud and web the section is absent — the cloud seam
returns an empty list. The entry is shown even when the server disables the
feature, with the disabling reason blocking navigation.

When the local engine wants to run a command, change a file or make a
network call, an inline prompt appears above the composer with the most
specific target — command, path, address or host — plus the engine's reason
(RUN-034). Only one prompt shows at a time. Decisions are approve, deny,
and — for command execution only — always-allow for the session; the prompt
clears optimistically on decision and restores on failure, auto-expires at
the request's time-to-live on a client timer, and is cleared by stop or any
terminal frame. Cloud and web never show inline prompts; they use the
approvals queue (§5). Chat-originated approvals also have list and decide
contracts on the cloud path.

A three-way approval-mode control in the chat header (RUN-035) — ask every
time, restrict to the working copy, full access, ordered most to least
cautious — sets how the local agent seeks approval. The choice defaults to
full access, persists per runner, validates on load, writes through on
change, and applies to the next thread the agent spawns, never to a turn
already in flight. Absent on cloud and web.

Local execution (RUN-036) runs chat against a same-machine bundled daemon
over native IPC with no cloud relay. Sessions and transcripts persist to a
local per-account store and survive restart; the daemon is brought up on
demand before send and warm. The user turn persists before submit and the
streamed reply persists on completion with accumulated deltas as fallback,
so a mid-stream refresh loses nothing; the engine thread id persists on
thread start so a session resumes its thread. One turn at a time per session
is enforced in memory with a daemon refusal as backstop, and a background
watcher releases sessions on terminal frames even after navigation away.
History roles outside the UI vocabulary map to system; stored sessions
always report open with working directory, thread id and last-write time.
The outgoing runner selector is deliberately omitted since the synthetic
route id matches no daemon runner.

Native calls: chat warm, send, cancel, close and decide; session and event
store reads and writes; approval-mode stash and load; availability check.
The desktop CLI install-and-login helper (bundle install plus device-code
grant) is observed but unowned: it provisions the desktop's bundled CLI
and belongs to the desktop area, so this spec adds no row for it and the
gate confirms the boundary.

## 10. Dev machines and runner detail (RUN-037–043; slice NEWFRONT-555)

The dev-machines page (RUN-037) tables machines that authenticated or host
runners: display name plus host label or short id, a client-derived status
(revoked when a revocation timestamp exists, else active with a positive
online runner count, else offline with any runners at all, else
registered), active-over-total runner counts and last-seen and
last-heartbeat times. Loading, load-error and empty states each get a
full-width row. Rows refresh every few seconds. The page also carries its
own Add-runner button opening the modal owned by slice NEWFRONT-543 (§3).
Oracle: `dev-machines-detail.spec.ts`.

Rotate (RUN-038) warns the token is invalidated and hosted runners stop
connecting until login repeats; revoke (RUN-039) warns of permanent
revocation including hosted runners, after which the row stays showing
revoked with only delete available; delete (RUN-040) warns of full removal
and teardown while noting the on-machine agent itself is not uninstalled,
defaulting to cascading local teardown. Each action is offered only where
it applies — rotate and revoke for non-revoked machines, delete for any —
and each confirmation blocks dismissal while in flight, closes and
re-fetches on success, toasts on failure.

The install section (RUN-041) shows platform install blocks — macOS and
Linux pipe installer, Windows PowerShell pipe installer, Windows
multi-line script with a separate download link — each with its own copy
control confirming transiently and toasting on failure, off the releases
latest channel. A prerequisite note names the agent CLIs expected on the
path and points at the doctor command after install. No endpoint is called.

The runner detail (RUN-042) shows the runner name and status with a back
link to the scope-appropriate list and an Open-chat control routing to the
runner's chat. A metadata grid carries id, pod, project, dev machine with
host fallback, OS and architecture, version, working directory, protocol
version, capabilities as badges, connection, owner, heartbeat, enrolment
with a pending-enrolment fallback, optional revocation time and reason, and
created and updated timestamps with dash fallbacks. Loading and error states
are handled; the page refreshes every few seconds and is deep-linkable.

The agent-activity panel (RUN-043) derives its badge client-side from live
state — no server agent-state enum exists — with precedence: subprocess
dead when the liveness flag is explicitly false, awaiting approval when any
approvals pend, unknown when the event timestamp is missing, invalid or in
the future, then active within half a minute of last activity, thinking
within three minutes, else stalled. A telemetry grid shows last activity
with full-summary tooltip, last event kind, agent process id, subprocess
liveness, pending approvals, token totals with compact formatting, model
and turn count, each with dash fallbacks. The panel ticks on its own short
interval so a run visibly ages between server fetches.

Reads and writes: list dev machines by workspace; rotate, revoke and
delete machine with the purge default; runner detail polled. The activity
derivation reads no endpoint.

## 11. Cross-area seams (resolved)

RUN-022 run-status badge — shared with the schedulers area. The runners
area owns the badge component and the twelve-state mapping end to end:
variants, readable labels, and the neutral raw-value fallback. Slice
NEWFRONT-545 builds it; the schedulers area consumes it on the
scheduler-binding detail and never duplicates the mapping. Matching
schedulers-side spec text resolves the same split from that side. Proven at
gate time by the badge rendering identically on runs surfaces and the
scheduler-binding detail, including the unknown-status fallback, plus unit
tests pinning the mapping. Slice NEWFRONT-547 mentions the badge only as a
dependency and owns no part of it.

RUN-046 and RUN-048 controls — mounted on issue surfaces owned by the
issues area. The runners area owns the dispatch service, the executor
read-only resolution contract, the local-preparation contract fulfilled by
the desktop supervisor service, and the re-tick grant-or-refuse contract
with its server-driven flag. Slice NEWFRONT-547 ports the dispatch service
and builds those contracts; slice NEWFRONT-552 ports the desktop
preparation the dispatch consumes. The issues area owns every mount point —
run controls, agent-status cards, re-tick and abort controls — and consumes
the runners-owned service through its public interface, never duplicating
dispatch or budget logic. The issue-detail hooks that wire those mounts
today sit outside the own-code list, so slices rewrite them rather than
port them. Proven at gate time by dispatch, queued, error and re-tick
scenarios driven through the issue mounts.

RUN-014 default pod — where issue delegation lands. The runners area owns
the default flag: the promote-to-default toggle, its one-way rule, and the
update call carrying it. Slice NEWFRONT-542 builds it. Delegation on the
issue and run surfaces consumes the project default server-side at dispatch
time; no runners slice implements delegation itself. Proven at gate time by
changing the default pod and observing subsequent delegations land on it.

## 12. Per-slice own-code module mapping

Every path below is in the own-code exception on Reference, not source, so
its slice may port the logic and adapt it — kit components instead of old
UI packages, TanStack Query with `@pidash/api-client` contracts instead of
old stores and axios services, `core/platform` for native calls, no runtime
imports from the old paths, new license headers. Anything these files use
from Plane code — old UI, stores, helpers, components — stays
reference-only and is rewritten. Paths outside the exception are
reference-only even when this spec describes their behavior.

- Slice NEWFRONT-540 (shell, RUN-001–005, RUN-010, RUN-011): the workspace
  and project runners route shells with their headers, the shared page
  module's shell duties, the side-nav rail, the tab strip, and the
  four-state runner-status map. The run-status badge module is ported by
  slice NEWFRONT-545 instead.
- Slice NEWFRONT-542 (pods, RUN-012–015): the create-pod and edit-pod
  modals, and the pod service for list, create, update and delete.
- Slice NEWFRONT-543 (add-runner, RUN-006–009, RUN-049): the add-runner
  modal, the CLI-command builder, the agent and model catalog, and the
  runner-service create-on-machine and status-poll methods. The modal is
  shared with the dev-machines page, which slice NEWFRONT-555 owns.
- Slice NEWFRONT-545 (runs, RUN-016–022): the runs route page with its
  list, detail, event, diagnostic and tool-plan rendering, the shared
  run-status badge, and the runner-service run list, get, and cancel
  methods.
- Slice NEWFRONT-547 (approvals and dispatch, RUN-023, RUN-024, RUN-046,
  RUN-048): the approvals route page, the dispatch service for run-ai,
  comment-and-run, abort and re-tick, and the runner-service approval list
  and decide plus run create and cancel methods. The desktop preparation
  step is fulfilled by the supervisor service ported under NEWFRONT-552;
  the issue-surface hooks are reference-only and rewritten.
- Slice NEWFRONT-549 (chat shell, RUN-025, RUN-026, RUN-028, RUN-032):
  the chat route page with its history panel, auto-select, warm-up and
  header gating, the pluggable chat transport with its cloud default, the
  native event source backing stream resume, and the runner-service chat
  session verbs the transport delegates to.
- Slice NEWFRONT-551 (chat messaging, RUN-027, RUN-029–031): the event
  subscription hook, the shared composer with its dictation control, the
  message send and cancel and close verbs through the transport, and the
  activity-strip derivation. Streamed markdown rendering is consumed from
  the shared message component owned with the kit, not duplicated here.
- Slice NEWFRONT-552 (desktop lifecycle, RUN-044, RUN-045, RUN-047): the
  desktop-overlay supervisor component and service, the desktop session
  teardown override, the sign-out dialog, the approval-mode carry-over,
  and the native HTTP adapter. The web-tree stubs are replaced, not
  ported.
- Slice NEWFRONT-553 (desktop chat, RUN-033–036): the desktop-overlay
  local-chat contacts override and local chat transport with its store,
  plus the cloud-side empty contact seam. The cloud-edition approval
  prompt, mode selector and reason-code copy modules predate the
  exception list only by location: they are reference-only and rewritten,
  with reclassification — if a slice believes they qualify — going
  through a process comment on NEWFRONT-1 first, never assumed.
- Slice NEWFRONT-555 (dev machines and detail, RUN-037–043): the
  dev-machines route page with its table, confirmations and install
  section, the runner detail route page, the agent-activity panel, and
  the runner-service machine list, rotate, revoke, delete and detail
  methods.
- Contracts NEWFRONT-530: runners, pods, dev machines, runs and approvals
  endpoints from the §14 groups. Contracts NEWFRONT-536: chat sessions,
  messages, stream, chat approvals, desktop enroll and agent
  profile-or-token endpoints.

## 13. Recorded deviations and targets (old bugs, linked issues)

Each item below is an observed old behavior the target intentionally
differs from, or a target newer than the code at this base. None is encoded
silently; each names its tracker.

- NEWFRONT-191 (RUN-005): the area shows three different names across the
  rail header and page title, the project breadcrumb and header, and the
  in-page headings with the CLI verb. Target: one canonical name on every
  surface (§1). Decided (D27): the name is **AI Workers**; keep "runner" as
  the technical noun for a host (CLI verb, API paths, model, code). The
  rename itself is a follow-up slice, not this issue (NEWFRONT-946).
- NEWFRONT-297 (RUN-024): every approval decide call currently errors
  server-side, so the acted card stays with a failure toast. Target: the
  card drops off on refetch with the decision persisted (§5).
- NEWFRONT-189 (RUN-024): session-scoped acceptance is rejected by
  validation. Target: accept-for-session persists and the run resumes (§5).
- main#539 targets (RUN-007, RUN-009): gating agent options against the
  selected machine's advertised supported set with upgrade hints and
  snap-to-first-supported, plus daemon-too-old upgrade hints on
  unsupported-agent rejections and unknown-agent write-backs. At this base
  the modal gates nothing and renders daemon errors verbatim; slice
  NEWFRONT-543 builds the target (§3).
- main#544 target (RUN-049): the Claude Code model list gains Opus 5.5
  above Opus 5 with the default staying Opus 4.8 and the matching model
  flag emitted. At this base the entry does not exist; slice
  NEWFRONT-543 implements it and adds its oracle scenario (§3).

Smaller inventory-vs-code notes, all honored above: the dev-machines shell
never flashes the blocked view while the runners shells can (§1); loading
and empty are conflated on the overview page (§1); revoke hides rather than
disables (§1); deletion applies the purge default silently (§1); the
machine picker auto-selects once per open and snaps back to manual (§3);
copy confirms transiently with toast-only failure handling (§3, §10);
newest-first ordering is server-driven (§4); the scheduler-binding link
needs both binding and detail (§4); stop and close refetch sessions only,
with message refresh via terminal frames (§7); the local transport ignores
the cloud resume sequence in favor of its store (§7, §9); web-tree desktop
preparation is an inert stub fulfilled by the supervisor service (§5, §8);
the desktop CLI provisioning helper is unowned by this area (§9).

No-entry-point observations, flagged for gate sign-off rather than treated
as covered: move-runner-to-pod and get-one-pod exist in services with no
reviewed-screen entry point, so they get no row. If an entry point appears,
a row is added with the next ID.

## 14. Endpoint reference (contracts issues NEWFRONT-530 and NEWFRONT-536)

Every endpoint the area uses, with the parameters and fields the slices and
contracts build to. Path prefixes below are contracts; exact shapes live in
`@pidash/api-client`.

Runners, pods, machines, runs and approvals (contracts issue
NEWFRONT-530):

- List runners: list by workspace with an optional project scope.
- Runner detail: single fetch by id, polled on the detail page.
- Delete runner: by id with the purge-local default; revoke runner: by id.
- List pods: by workspace with the project filter winning; create pod with
  project, name and optional description; update pod with the changed
  subset; delete pod.
- List dev machines by workspace; rotate, revoke and delete machine with
  the purge default.
- List runs with page, workspace and project; get run with events
  included; cancel run with a reason.
- Create run in the run-ai and comment-and-run variants with workspace
  plus work-item ids and a trigger flag, including the queued
  already-active shape.
- List pending approvals with optional project scope; decide approval with
  the three-way decision.
- Re-tick by work item, returning the grant-or-refuse envelope with budget
  counters.
- Projects-lite by workspace slug for pickers; project detail for
  managed-runner availability.

Chat, stream and desktop (contracts issue NEWFRONT-536):

- Chat sessions: list, create, get, warm, send, cancel and close, with
  message listing and the resumable event stream; chat approvals: list and
  decide for chat-originated requests.
- Agent profile read and agent-token issue for the managed agent.
- Desktop enroll create and delete; project and issue reads for executor
  resolution.
- Native IPC surface (through `core/platform` in new code, never called
  directly): daemon and host lifecycle, chat warm, send, cancel, close and
  decide, session and event store access, approval-mode stash and load,
  availability and CLI provisioning, desktop cookie, storage and cache
  wipe, native HTTP request, cancel and streaming.

No other endpoint is called by these screens. Real-time updates are
short-cadence polling everywhere except the cloud chat event stream and the
desktop native chat frames: runners list and rail every few seconds, pods
slower, runs list every few seconds, run detail every few seconds stopping
at terminal, approvals fastest, dev machines and runner detail every few
seconds, the activity panel on its own short tick, the machine picker while
open, create-status on a short poll under a bounded deadline, desktop
agent-availability slow without focus revalidation, desktop project
availability every few seconds, supervisor refresh every minute plus window
focus with credential renewal before expiry. The contracts issues cover
any of the above not already under contract.
