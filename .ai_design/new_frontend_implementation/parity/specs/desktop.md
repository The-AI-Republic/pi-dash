# Desktop area spec — agent runtime, bare sign-in, updater, deep links, native shell (NEWFRONT-578)

Parent epic NEWFRONT-77. Inventory
`.ai_design/new_frontend_implementation/parity/inventory/desktop.md` (rows
DESK-001–027, all `oracle green` at split). Oracle scenarios
`apps/web_new/e2e/parity/desktop/` (agent-runtime, sign-in, updater, shell
specs; NEWFRONT-207–210), which pin the web-observable side of each row
(absence or inertness on `apps/web`); the area gate (NEWFRONT-595) proves
the desktop-positive halves on `apps/web_new`.

Method: the old frontend was read as a behavior reference only. Nothing is
imported, copied or translated from it; this spec restates what was learned
in prose. The only code-like tokens below are API paths, field names,
protocol codes, Tauri command names and storage keys, which the contracts
issue (NEWFRONT-585) and the slices turn into contracts and platform
bindings. New code is written from this spec, the wiki pages (Reference,
not source; Parity; Architecture; Data layer; Platform and editions) and
the kit — never with old files open.

Own-code porting area (decision D16): `desktop-overlay/**`,
`packages/services` desktop adapter and event source, and the other paths
listed under "Exception: your own code" on Reference, not source may be
ported into `apps/web_new` and adapted (kit, TanStack Query plus
`@pidash/api-client` contracts, `core/platform` for native calls; no
runtime imports from the old paths). This spec is therefore short where
porting applies: it records the behavior and the inventory row mapping and
names which own-code modules each slice ports (§6), without re-describing
ported logic line by line. Anything the own-code files take from Plane code
— notably everything under `apps/web/ce/components/desktop` — stays
reference-only and is rewritten from this spec (§7).

Status convention: this spec owns no inventory Status changes. Slices
append `built` to the rows they implement. Rows owned: none.

Slice map (each row in exactly one slice): NEWFRONT-588 owns DESK-001–004,
010, 026 (§1); NEWFRONT-589 owns DESK-005–009 (§2); NEWFRONT-591 owns
DESK-011–015, 027 (§3); NEWFRONT-592 owns DESK-016–018 (§4); NEWFRONT-594
owns DESK-019–025 (§5).

## 1. Agent runtime core (NEWFRONT-588)

DESK-001 — Opening a project as a signed-in desktop user prepares the
on-machine agent without any click. The preparation runs a fixed sequence:
a native self-check of the bundled binaries, a read of the server agent
profile, a write of the on-machine engine setup from that profile, a fetch
of a short-lived model credential whenever the cached one is missing or
close to expiry, a per-workspace machine enrollment (once per workspace per
session, remembered in a per-account session cache alongside the host
label; a changed host label resets the cache), a per-project enrollment
resolved through the project read, and a daemon start. A small floating
status notice at the bottom of the project page tracks the sequence: it
opens in a preparing state, then settles into either a connected state
that stays mounted while runs need the app open, or a waiting state until
the server observes the first heartbeat (the project read is re-polled on a
short cadence for the managed-runner entry to turn available). Any failure
replaces the notice with a message naming its cause. Leaving the project
stops the polling. Window focus re-runs the preparation, and a slower
background refresh (about once a minute plus on focus) rewrites the engine
setup and rotates the credential. Direct work-item links with no project
in the route resolve the project from the browse identifier within the
active workspace before any of this runs. Endpoints: agent profile read,
agent credential issue, machine enrollment write, project detail and
project list reads, CSRF fetch before unsafe calls; native commands are the
doctor, engine-setup and credential writes, bootstrap, project enrollment
and daemon start (§14). Oracle:
`agent-runtime/agent-runtime.spec.ts`. Ported from the own-code runtime
service and status-pill component (§6).

DESK-002 — Pressing Run on a work item first makes the local agent ready,
but only when the work item actually targets this machine. The gate reads
the issue detail and the project detail and compares the issue's explicit
executor pin against the project default: when neither selects the bundled
agent, nothing local happens and the selection is left untouched. When the
bundled agent is selected, the gate runs the same preparation as DESK-001
and then waits briefly (tens of one-second polls) for the server to observe
the daemon before the run is created; a daemon that never appears surfaces
a retry-shortly message instead of creating the run. Selection is a
separate user action and is never mutated by the press. Endpoints: issue
detail read (`agent_executor`), project detail read (`default_agent_executor`,
`agent_executor_options`), plus the DESK-001 enrollment path. Oracle:
`agent-runtime/agent-runtime.spec.ts`.

DESK-003 — When the shipped runner or engine binary is absent, the status
notice conveys the installation is damaged and must be reinstalled, instead
of failing later at Run time. The check is the native self-check's presence flags,
evaluated before any enrollment or credential step. No API beyond the
self-check. Oracle: `agent-runtime/agent-runtime.spec.ts`.

DESK-004 — Whenever the server will not let the bundled agent run, the
user gets a plain-language explanation naming the specific refusal, and
the chat picker entry stays visible but cannot start a chat it could never
send. The profile read carries an availability flag plus a reason code;
each code maps to its own explanation covering the distinct situations the
server distinguishes: the feature switched off, personal-key setups the
desktop cannot serve yet, no usable model configured, the machine not
linked, a stale desktop session, missing gateway grant, revoked gateway
access, and the gateway temporarily down. Unmapped codes surface as-is.
The mapping backs both the status notice and the picker entry, and it
never throws: an unreachable API or an expired session reports as
unavailable with the message. A 401 anywhere in the runtime path stops the
daemon, clears the enrollment and session caches, and destroys the local
credentials without forging logouts elsewhere. Endpoints: agent profile
read (availability plus reason code). Oracle:
`agent-runtime/agent-runtime.spec.ts`. The reason-code vocabulary comes
from the server; the wording is rewritten from this spec, never ported
(§7).

DESK-010 — Signing out tears down everything local while keeping chat
history on disk by default. The teardown stops the daemon, destroys the
local credentials (model credential plus machine configuration, including
per-workspace copies), removes the server-side machine enrollment, and
wipes the webview store (cookies first of all, plus storage and caches) so
no session survives — the wipe runs after the sign-out request, which
still needs its cookie, and before the navigation to sign-in, and its
failure is swallowed because landing on sign-in stays the right outcome.
Server-side enrollment removal failures are likewise swallowed: an expired
session or unreachable server must never trap the user in the app. History
stays on disk, scoped to the account, hidden until the same account signs
in again; the native teardown offers an explicit per-account wipe for a
future clear-on-sign-out choice, which the web layer does not send today.
Endpoints: enrollment removal plus the regular sign-out post; native
commands are daemon stop, credential destroy and the webview wipe (§14).
Oracle: `agent-runtime/agent-runtime.spec.ts`. The surrounding sign-out
call order (server post, then the platform teardown hook, then navigation)
is shared with the auth area (§8).

DESK-026 — Changing the model or assistant setup rewrites the on-machine
engine setup and rotates the short-lived model credential without
restarting anything. The refresh runs on the same triggers as DESK-001
(window focus plus the background cadence): it re-reads the profile,
rewrites the engine setup, issues a fresh credential when the cached one
is near expiry, and restarts the daemon per enrolled workspace. An expired
credential stops the daemon and asks for sign-in again. Endpoints: agent
profile and credential reads plus the native setup and credential writes.
Oracle: `agent-runtime/agent-runtime.spec.ts`.

## 2. Local chat with approvals and history (NEWFRONT-589)

This slice builds on the runtime core (§1): daemon lifecycle and account
scoping come from NEWFRONT-588. All chat traffic below bypasses the cloud
relay and reaches the on-machine daemon over native chat commands, with
frames streamed back over dedicated native events (§14).

DESK-005 — The built-in on-machine agent appears as a chat contact on
desktop only, marked as local. The entry is offered even when the server
currently refuses the agent, in which case it carries the DESK-004 reason
and is not clickable. Availability is re-read on a slow cadence (minutes,
not renders). No entry ever appears in a plain browser session. Endpoint:
agent availability read (shared with DESK-004). Oracle:
`agent-runtime/agent-runtime.spec.ts`. Ported from the own-code picker
contacts module (§6); the entry's display copy is rewritten (§7).

DESK-006 — Direct chat with the bundled engine: warming, sending,
cancelling and closing a local chat reach the on-machine daemon. A chat
opened right after sign-in first brings the daemon up through the same
preparation as DESK-001 minus the caller-supplied project (the workspace's
first project stands in; a workspace with no project explains that one is
needed first) — cheap and idempotent once the daemon runs, so it precedes
every warm and send. Cancel and close settle quietly without inventing
success. Streaming works in both directions: commands submit turns, and a
subscription translates the daemon's ordered frame stream (start, message
start, live events such as assistant deltas, approval requests,
completion, failure, close) into the server-shaped chat events the shared
chat UI already consumes, with a monotonic sequence the UI dedupes on.
Durability is prompt: the user's message is persisted before the send
submits, the engine thread id is recorded when the thread starts (for
resume), and the assistant reply is persisted on completion — falling back
to the accumulated streamed deltas when the completion frame carries no
text — before the completion event dispatches, so a refresh or restart
never loses an already-sent turn. No runner selector travels with chat
commands: the daemon resolves its single managed runner itself. Native
commands are the chat warm, send, cancel, close and decide verbs (§14).
Oracle: `agent-runtime/agent-runtime.spec.ts`. Ported from the own-code
local chat transport (§6).

DESK-007 — Each local chat session carries an approval mode chosen above
the chat: asking each time, free movement inside the working copy, or full
access. Full access is the default and the choice is remembered per runner
across sessions; a change only affects threads started afterwards, never a
turn already running, because the runner captures the mode when it spawns
the session's engine thread. The mode travels on every warm and send. No
mode control exists outside local chats. Native commands: the mode rides
the warm and send verbs. Oracle:
`agent-runtime/agent-runtime.spec.ts`. The mode control's labels and help
copy are rewritten from this spec, never ported (§7).

DESK-008 — When the local agent wants to act, the turn parks on an inline
prompt card inside the thread naming what is wanted plus why. Commands,
file edits and network attempts each get their own card; the card leads
with the kind of act, shows the most specific target available (command
text, file path, network destination, in that order of preference), the
agent's reason when one arrived, and a countdown while an expiring request
runs down — with the decision controls staying responsive throughout.
Confirming or refusing both continue the turn; commands additionally
offer a remember-for-the-session choice. Decisions travel on the
native decide verb with the workspace, session, approval id and decision.
There is no separate queue surface on desktop: the prompt is the decision
point. Oracle: `agent-runtime/agent-runtime.spec.ts`. The card wording and
button copy are rewritten from this spec, never ported (§7).

DESK-009 — On-machine chat history is scoped to the signed-in account and
workspace: sessions and messages persist on the machine per account, most
recently touched sessions first, messages in insertion order. Titles can
change; a session deletes together with its messages (idempotent, so a
double delete is harmless); clearing wipes one account only; appending to
an unknown session is refused rather than silently orphaned. Sessions
resolve and create their own working copy under the chat tree — deliberately
separate from the managed-runs tree so a chat and an issue run never share
a directory — and the UI can ask for that directory to show it. History
survives refresh and restart, and sign-out keeps it by default (DESK-010).
No server endpoint: the store is the native history commands (create, list,
fetch, append, thread-id record, rename, delete, clear, working directory;
§14). Oracle: `agent-runtime/agent-runtime.spec.ts`. Ported from the
history half of the own-code local chat transport (§6).

## 3. Bare sign-in, deep links, onboarding (NEWFRONT-591)

The auth epic already built the desktop entry pieces this slice extends
(AUTH-022/023); §8 records per row what exists versus what this slice
adds. This slice owns the desktop-bundle route wiring, the deep-link
completion half, the browser-open guard and the onboarding seam.

DESK-011 — The desktop entry point is a bare sign-in screen: signed-out
users on the desktop root and on the login and sign-in routes get the
sign-in card directly, with no marketing frame. Signed-in users landing on
any of the three routes are forwarded into their workspace, honoring a
pending destination. Sign-out lands on the login route, which renders the
same screen. The screen never starts sign-in on its own: opening the
system browser stays an explicit click. Guard input is the session read.
Oracle: `sign-in/sign-in.spec.ts`. The screen, the card slot and the three
route forks already exist in `apps/web_new` (§8); this slice completes the
wiring the rows demand (pending-destination honoring on every route, the
failure query surviving on sign-in).

DESK-012 — When desktop sign-in does not complete, the user lands back on
sign-in with a visible explanatory banner above the card instead of a dead
form. The banner reads the failure query: a provider-side refusal or an
aborted flow arrives with an error code and optionally a provider
description; the banner pairs a heading conveying the sign-in did not
complete with the provider description when one arrived, else a retry line
naming the error. A plain visit with no failure query shows the card
alone, and these landings plant no session. No API (client mapping).
Oracle: `sign-in/sign-in.spec.ts`. The failure mapping already exists in
`apps/web_new` (§8).

DESK-013 — Where the server offers no desktop sign-in hand-off, the same
bare screen shows a fallback card instead of a form that could never
establish a session: it explains desktop login is unavailable there and
offers opening the hosted app in the system browser, resolved against the
baked web base URL with the API origin as fallback (and a plain navigation
fallback when no native bridge answers). The card's heading deliberately
avoids the wording an edition hand-off card is recognized by, so a
misbuilt bundle fails its build check instead of passing silently. No API.
Oracle: `sign-in/sign-in.spec.ts`. The card already exists in `apps/web_new`
(§8).

DESK-014 — System-browser login completes through a custom-scheme callback.
The app opens the real browser for login; the identity provider redirects
to the desktop callback scheme carrying the authorization code and state
(or an error pair when the provider refused or the user aborted). The
native shell brings the app window forward and finishes the session by
navigating its webview to the server desktop-exchange address with the
returned code and state, which sets the session cookies on the webview's
own response without ever putting a session token in a URL. Provider
refusals land on sign-in with their reason; callbacks with missing code or
state and unrecognized links are ignored. Second launches hand their links
to the running instance (single-instance forward). Endpoint: the server
desktop-exchange navigation carrying code and state; the community server
exposes no such route. Oracle: `sign-in/sign-in.spec.ts`. The exchange
navigation and link handling live natively; the web side this slice adds
is the deep-link platform seam plus the failure landing (§8, §10).

DESK-015 — Browser opening is limited to web addresses: only hosted
addresses are handed to the OS for external opening, and anything else is
refused rather than opened. The guard sits on the native open command and
is mirrored on the web side so every open-in-browser action (fallback card,
any native open action) enforces it. Native command: the guarded browser
open (§14). Oracle: `sign-in/sign-in.spec.ts`. The platform open exists
but the scheme guard is this slice's to add (§8, §12).

DESK-027 — Desktop onboarding starts at profile setup with no CLI-install
step: onboarding opens on profile setup, the progress order excludes the
CLI-install step, and no back affordance appears on profile setup because
no earlier step exists. (Target per main#494; the live old app still shows
the step on desktop.) No API. Oracle: `sign-in/sign-in.spec.ts`. The
mechanism already exists in `apps/web_new` — the flow keys the step's
presence off the platform (§8); this slice proves the desktop leg at the
gate.

## 4. Updater (NEWFRONT-592)

The shell owns the sidebar mount point (SHELL-102); this slice builds the
update detail that mounts there plus the platform update bindings (§9).
The launch prompt and the daily scheduler live natively and are wired,
not reimplemented (§10).

DESK-016 — At launch with a newer release available, the app offers the
update in a native dialog: installing now downloads, installs and restarts
into the new version, while deferring keeps the update for the sidebar
button. A failed install reports itself in a dialog, keeps the current
version running, and keeps the update available for retry. The whole
behavior exists only when the build configures an update feed (key plus
endpoints); without it nothing is ever offered or checked. Endpoints: the
distributor's update feed check; native surface is the launch prompt plus
the install command (§14). Oracle: `updater/updater.spec.ts`.

DESK-017 — While the app stays open it checks quietly about once a day,
shortly after midnight UTC with a per-app jitter spread so the fleet does
not check in one second. A found update is announced to the web UI, never
prompted for — it lights up the sidebar button (DESK-018). Checks never
interrupt, never overlap an ongoing install (an install in flight skips
the check), and a machine asleep across days checks once on waking (short
naps re-read the wall clock instead of one long sleep firing late). Check
failures log and leave the app on the current version. No web UI of its
own; the feed check plus the daily scheduler live natively (§14). Oracle:
`updater/updater.spec.ts`.

DESK-018 — The sidebar button installs a waiting update. It appears beside
the sidebar user menu whenever an update waits — whether deferred at
launch before the button mounted (so it re-queries the pending update on
every mount) or announced while running (via the native available event).
It names both the waiting and the current version, disables itself while
downloading (a remount landing mid-download must also read disabled, from
the native installing flag rather than component state), and reports a
failure in place with a working retry; the native side re-announces after
a failure so a remounted button drops out of its installing state. With no
update waiting it renders nothing — the web-build seam likewise renders
nothing outside the desktop bundle. Native commands are the pending-update
query and the install (§14). Oracle: `updater/updater.spec.ts`. Ported
from the own-code update button (§6).

## 5. Native shell: links, transport, CLI, window and tray (NEWFRONT-594)

DESK-019 — Shareable work-item links copied from the desktop app point at
the hosted app, never at the local bundle origin. Every copy-link path
(detail header, peek header, every list menu) resolves its in-app path
against the configured hosted origin, keeps any query and fragment, and
refuses to produce a link when no hosted origin is configured or the
configured base is not a web address. No API beyond the normal link
builder. Oracle: `shell/shell.spec.ts`. Ported from the own-code hosted-URL
resolver (§6).

DESK-020 — Every authenticated API call and live event stream on desktop
travels through the native layer so cookies work despite the bundle
origin. File uploads (including multipart), relative addresses resolved
against the page, downloads, timeouts and cancellations all behave as in
the browser; only API-shaped addresses on the advertised origin take the
native path, everything else keeps the page transport. Transport failures
read as network errors — an abort rejects as aborted, a blown timeout as
timed out, an unroutable host as a network failure — never as forged
logouts. Live streams resume from the last seen event without reconnecting
on hard errors (a non-success status or a non-event-stream answer closes
the stream for good; end-of-stream and transport drops reconnect with the
last event id). Native commands are the request, stream and cancel verbs
(§14). Oracle: `shell/shell.spec.ts`. The web_new platform transport
already carries most of this (§12, §13); the slice ports the remaining
own-code adapter and event-source behavior (§6).

DESK-021 — Turning the machine into a runner host from inside the app runs
through the command-line tool the bundle already ships. The flow reports
whether the tool is present and which version (known install locations
first, then the OS resolver, then a version probe), installs it when
missing — from the app's own bundle where available (offline and instant),
else from the pinned network installer with plain-text progress lines the
UI renders verbatim in sequence order — guarded so a second install while
one runs is refused rather than raced. Login is a device-code grant the
app plays both halves of: it starts the grant, approves it with its own
signed-in session (the same pair the browser device page uses), and hands
the approved device code to the tool, which exchanges it and owns the
resulting credential, workspace binding and file format. Registering a
runner host proper (project, working directory, agent choice) is a UI flow
of its own and explicitly out of scope here. Endpoints: device-grant start
and approve; native commands are the tool detect, install and login verbs
plus the install-log event (§14). Oracle: `shell/shell.spec.ts`. Ported
from the own-code CLI setup service (§6).

DESK-022 — The desktop shell parks instead of quitting: closing the window
hides the app into the tray, the tray offers showing the app and quitting
for real, clicking the tray icon or launching again brings the open window
forward (single instance), and the window opens at a fixed default size
with a smaller minimum. Entirely native; no web_new code and no API (§10).
Oracle: `shell/shell.spec.ts`. The slice marks this row built with that
justification; the gate verifies it against the desktop build.

DESK-023 — Familiar zoom controls live in the desktop View menu with the
usual shortcuts on every platform (in, out and reset, with the platform's
conventional accelerator variants), stepping within a clamped range.
Entirely native; no web_new code and no API (§10). Oracle:
`shell/shell.spec.ts`. Built-marked with justification like DESK-022.

DESK-024 — Server-hosted pages never take over the desktop window: any
navigation that would render from the server host is bounced back to the
equivalent bundled page instead of loading remotely, enforced both at
navigation policy time and as a post-load fallback. Entirely native (the
dev hot-reload mode is exempt); no web_new code and no API (§10). Oracle:
`shell/shell.spec.ts`. Built-marked with justification like DESK-022.

DESK-025 — The desktop bundle ships no web-only routes: marketing and
other web-only pages are absent and fall through to not-found, while the
desktop login and sign-in paths render the bare sign-in screen. In
`apps/web_new` this is the route tree plus the desktop target: web-only
subtrees mount in the web target only, and the retired sign-in addresses
fork to the desktop screen under the desktop target (§11). No API. Oracle:
`shell/shell.spec.ts`. The slice builds the exclusion wiring; the gate
proves both the fall-through and the sign-in render on the desktop build.

## 6. Own-code port map per slice (seam 1)

All paths below are own code per Reference, not source (decision D16):
`desktop-overlay/**` wholesale, the desktop adapter and event source under
`packages/services`, and the named agent-runtime service, session and
component paths. Slices port their logic and adapt it — kit components
instead of the old UI packages, TanStack Query plus `@pidash/api-client`
contracts instead of the old stores and services, `core/platform` for
native calls — with no runtime imports from the old paths. Anything these
modules take from Plane code is rewritten from §7, never ported.

NEWFRONT-585 (contracts) ports nothing: contracts are hand-written zod
from the endpoint shapes in §14 (decision D8), following the existing
contract file patterns.

NEWFRONT-588 (runtime core) ports the overlay agent-runtime service
(enrollment and credential sequence, daemon lifecycle, availability and
readiness gates, teardown orchestration, account-scoped enrollment cache),
the status-pill component (notice lifecycle across project open, leave,
focus and interval refresh), and the desktop session wipe call; plus the
native bindings it drives (self-check, engine-setup and credential writes,
bootstrap, project enrollment, daemon start and stop, credential destroy,
webview wipe). The picker-entry reason mapping for DESK-004 lives here;
the chat contact itself belongs to NEWFRONT-589. The 401 path (stop,
clear, destroy, never forge) ports with the service.

NEWFRONT-589 (local chat) ports the overlay local-chat transport in full
— session and message verbs over the native history commands, warm, send,
cancel, close and decide over the native chat commands, the frame-to-event
translator with its monotonic sequencing, the subscription with its
persistence-before-dispatch ordering, the per-session approval-mode
carriage with full-access default — and the picker contacts module (the
local entry plus its availability polling). The runner-detail shim for the
synthetic built-in id ports as the seam the shared chat page needs. All
approval and entry wording is rewritten (§7).

NEWFRONT-591 (sign-in) ports no overlay module: there is no overlay JS for
the deep-link completion (it is native, §10) and the sign-in card sources
are Plane code (§7). This slice reuses the auth epic's pieces (§8) and
writes the thin web_new remainder from this spec: the deep-link platform
capability, the browser-open scheme guard, and the route-wiring completion
for DESK-011/014/015/027.

NEWFRONT-592 (updater) ports the overlay update button (pending query on
mount, available-event subscription, installing-state handling from the
native flag, install with in-place failure and retry, null render when
nothing waits) and the native update bindings it drives (pending query,
install, available event) into the platform updates capability (§12).

NEWFRONT-594 (shell) ports the hosted-URL resolver (origin resolution,
web-scheme check, refusal without a configured origin) for DESK-019; the
CLI setup service (detect, install orchestration, self-approved device
grant, tool login) plus the native tool bindings for DESK-021; and the
`packages/services` desktop API adapter and desktop event source (native
detection, API-address allowlist, frame codec, request cancellation,
timeout and error taxonomy, stream resume from the last event id, hard-
error close) into the platform transport for DESK-020 — extending what
`core/platform` already carries (§12, §13). DESK-022/023/024 need no port:
they are native-only with no web_new side (§10). DESK-025 needs no port:
it is route-tree exclusion wiring (§11).

## 7. Plane-code boundary inside the port (seam 2)

`apps/web/ce/components/desktop` is Plane code — it is not in the own-code
list — including the three pieces own-code files import. Slices rewrite
all of the following from this spec; none of it is ported, quoted or
structurally translated:

- Reason-code wording (the edition reason map behind DESK-004): the reason
  codes themselves are server API vocabulary and are kept exactly (§14),
  but every human explanation is written fresh. The map's shape (one entry
  per code, unmapped codes surfacing as-is) may be followed as behavior;
  its sentences, and the CSRF-path constant's reuse as an import, may not
  be carried over — the endpoint path itself is a contract (§14).
- Chat-approval wording (the approval seam behind DESK-007/008): the mode
  spectrum and its order, the mode labels and help lines, the per-kind act
  descriptions, the prompt card's buttons and help text, and the segmented
  control's structure and styles are all rewritten. The mode, kind and
  decision identifiers are protocol vocabulary and are kept exactly (§14).
- Sign-in card sources (the edition card behind DESK-013): heading, body,
  action label, layout and styles are rewritten; the behavior contract is
  kept (explanatory card, no dead form, browser-opening action resolved
  against the baked base with API-origin fallback, heading wording that
  avoids the hand-off card's recognition phrase). The auth epic's card
  already embodies this rewrite (§8).
- Everything else the own-code modules take from Plane code: UI package
  components, old service base classes, old stores and hooks, shared
  constants, data-fetching helpers, class strings and styles. Ports
  replace them with kit, Query plus contracts, and platform calls.

Review rule: the reviewer compares each slice diff against both the
own-code module it ports (adapted, not translated: new structure, kit,
Query, platform) and the Plane files above (no shared wording, strings,
classes or structure beyond API and protocol vocabulary).

## 8. Auth seam (seam 3)

The auth epic (AUTH-022/023) already built the desktop entry in
`apps/web_new`; NEWFRONT-591 extends it only where the DESK rows demand
more. The edition card slot keeps its stable name (decision D18): slices
use it as specified and never rename or fork it.

DESK-011 — Exists: the bare screen component (failure alert above the
edition card), the card slot with the unavailable card as the default, and
the three route forks (root, login, sign-in) rendering the screen under
the desktop target with the signed-out-only guard forwarding signed-in
users to landing or onboarding. The slice adds: pending-destination
honoring verified on every one of the three routes, and the failure query
surviving on the sign-in route (the web build's bare redirect drops it).

DESK-012 — Exists: the entry-search parse (failure code, provider
description, pending destination, all lenient) and the failure description
(pairing the did-not-complete heading with the provider description or a
retry line naming the error; null without a failure code). The slice adds
nothing except gate proof; the mapping is complete.

DESK-013 — Exists: the unavailable card (explanatory copy, browser-opening
action through the platform open with plain-navigation fallback, baked
base resolved against the API origin, recognition-safe heading). The slice
adds nothing except the DESK-015 guard the card's action inherits (§12).

DESK-014 — Exists: the failure landing above (provider refusals arrive as
the DESK-012 query) and the platform open for the browser leg. The slice
adds the deep-link platform capability (subscription to incoming app
links, §12): the exchange navigation itself is native (§10), and an
edition hand-off card behind the stable slot stays an edition's own
module, not this slice's.

DESK-015 — Exists: the platform open plus its no-bridge fallback chain.
The slice adds the web-address guard mirrored from the native command:
refuse anything but hosted addresses before any native call is made.

DESK-027 — Exists: the onboarding flow keys the CLI-install step's
presence, the initial step and the back affordance off the platform
(desktop builds start at profile setup with no earlier step to return
to). The slice adds nothing except gate proof of the desktop leg.

Desktop-bundle route wiring (owned by NEWFRONT-591, shared with DESK-025):
the root renders the desktop screen under the desktop target; the retired
login and sign-in addresses fork to the same screen instead of redirecting;
signed-in users forward honoring the pending destination; sign-out lands
on login. §11 covers the web-only exclusions on the same tree.

Sign-out call order (shared with the auth area, DESK-010): the server
sign-out post first, then the platform session-teardown hook (daemon stop,
credential destroy, enrollment removal, webview wipe), then navigation —
with teardown failures swallowed so landing on sign-in stays right.

## 9. Shell seam (seam 4)

SHELL-102 draws the line: the shell owns the sidebar mount point, the
desktop area owns the update detail (DESK-018). The mount contract the
updater slice builds against is the sidebar's optional update-control
slot, rendered inside the account area beside the user menu: the shell
passes through whatever node the desktop area supplies and renders
nothing in its place when none is supplied (the web build always supplies
none). The desktop detail owns everything else — pending query on mount,
available-event subscription, version naming, installing state from the
native flag, install, in-place failure with retry, and the null render
when nothing waits. Neither side reaches into the other: the shell never
queries update state, and the desktop detail never moves the mount.

Cross-reference NEWFRONT-527 (shell gate deferral): the shell gate proves
only the web-negative half (no control in the web sidebar); the
desktop-positive half — the control rendering in the desktop sidebar — is
proven on top of NEWFRONT-592. The updater slice keeps this mount contract
stable and notes any change on NEWFRONT-527.

## 10. Native-only surface (seam 5)

`desktop/src-tauri` is out of bounds for every sub-issue: no slice touches
it. For each natively owned behavior, what `apps/web_new` code (if any) is
needed versus what the area gate verifies as native behavior:

- DESK-022 (tray, window, single-instance focus, sizes): no web_new code.
  Nothing in the page can observe or drive the tray, the hide-on-close, or
  the window geometry. The gate verifies it against the desktop build
  (close parks, tray menu shows and quits, relaunch focuses, default and
  minimum sizes hold). NEWFRONT-594 marks the row built with this
  justification and invents no code for it.
- DESK-023 (zoom menu and shortcuts): no web_new code. The menu and its
  accelerators are registered natively; the page holds no zoom state. The
  gate verifies in, out and reset on the desktop build. Built-marked with
  justification like DESK-022.
- DESK-024 (server-host navigation bounce): no web_new code. The policy
  hook and the post-load fallback both live natively. The gate verifies
  the bounce on the desktop build. Built-marked with justification like
  DESK-022.
- DESK-016 launch prompt and DESK-017 daily scheduler (halves): the dialog,
  the feed check, the schedule, the jitter, the install itself and the
  failure dialog all live natively; the web side is only the platform
  update bindings NEWFRONT-592 builds (pending-update query, install
  invoke, available-event subscription, §12, §14) plus the DESK-018
  detail. The gate verifies the native halves (prompt choice, deferral
  lighting the button, daily announce, failure dialog keeping the app
  running) against a desktop build with a configured feed.
- DESK-014 deep-link handling and exchange navigation: the scheme handling,
  window focus, single-instance forward and webview navigation to the
  exchange address all live natively; the web side is the deep-link
  platform capability plus the DESK-012 failure landing (§8). The gate
  verifies completion, refusal landing and ignored links on the desktop
  build.
- DESK-015 scheme guard: enforced natively and mirrored in the platform
  open (§8, §12); the gate verifies refusal of non-web addresses.

## 11. DESK-025 bundle route exclusions (seam 6)

In `apps/web_new` there is no overlay file-swap: the route tree plus the
desktop target produce the exclusion. The desktop bundle is built with the
desktop target, which selects the desktop platform module at build time
and excludes web-only route subtrees — the god-mode and public-boards
subtrees mount in the web target only (decision D13) — so those addresses
fall through to the normal not-found page on desktop. The retired sign-in
addresses fork the other way: under the desktop target the login and
sign-in routes render the bare desktop screen (DESK-011) instead of
redirecting to the entry. Unknown paths keep the healthy catch-all in
both targets. NEWFRONT-594 builds the exclusion wiring; NEWFRONT-591 owns
the sign-in fork half; the gate proves the fall-through and the sign-in
render on the desktop build.

## 12. Transport seam (seam 7)

`core/platform` is the boundary every slice programs against and the only
place that knows the build target: no feature checks the target directly,
and no slice reaches past the platform into native globals. The platform
surface is the fetch transport, the event-stream transport, the external
open, the key-value storage, the focus subscription, and the optional
desktop-only capabilities (deep links, agent runtime, updates, session
teardown, window and menu) — features render nothing when a capability is
absent. That boundary is also what each slice mocks: unit tests inject a
fake platform object (or fake capability), never native globals and never
another slice's modules, so no slice depends on another for transport.
Concretely: the runtime and chat slices drive the agent-runtime and chat
capabilities; the sign-in slice drives the external open (with the
DESK-015 guard) and the deep-link subscription; the updater slice drives
the updates capability; the shell slice extends the fetch and stream
transports themselves plus the tool bindings. The API client sends every
request through the platform fetch via its transport adapter, so credential
handling, cancellation and the error taxonomy live in exactly one place.

## 13. F-06/F-07 reuse map (seam 8)

Surveyed new code (not old code): `src/core/platform` (web and desktop
implementations, transport adapter, storage, build-time selection),
`src/target.ts`, the desktop build wiring, the edition contract, the
session gates, and the auth entry pieces. Per slice, what exists versus
what must be added (all paths under `apps/web_new/src` unless noted):

- Foundations every slice uses. Exist: the platform contract with web and
  desktop implementations chosen at build time (the web bundle provably
  contains no desktop code); the desktop fetch with its frame codec,
  API-address allowlist and cancellation, and the desktop event stream;
  the transport adapter into the API client; key-value storage; the focus
  subscription; the desktop target flag with its build define and the
  desktop output bundle; the edition contract with the stable desktop
  sign-in card slot; session reads, page-type guards and landing
  resolution. To add across slices: the deep-link subscription, the
  updates capability, the agent-runtime and chat capabilities, the
  session-teardown hook implementation, the browser-open scheme guard, and
  the transport parity gaps in §12 (timeout mapping, multipart uploads,
  stream resume from the last event, hard-error close).
- NEWFRONT-585 (contracts). Exists: device-grant, sign-out, CSRF, project
  and issue contracts to reuse. To add: the agent profile and credential
  contracts, the machine enrollment write and removal, and the sharpened
  executor-option fields (§14).
- NEWFRONT-588 (runtime core). Exists: Query plus the API client, the
  session user read for account scoping, the focus subscription for
  refresh timing, kit notice primitives. To add: the runtime hooks and
  status notice, the readiness gate, the refusal mapping with rewritten
  wording, the teardown orchestration, and the agent-runtime platform
  capability with the managed-command bindings.
- NEWFRONT-589 (local chat). Exists: the shared chat UI and event shapes
  it consumes, the runtime core's daemon and account seam (blocked-by
  edge). To add: the local transport (verbs, translator, subscription,
  history mapping), the picker entry, the approval-mode control and prompt
  cards with rewritten wording, and the chat platform capability with the
  chat and history command bindings.
- NEWFRONT-591 (sign-in). Exists: nearly the whole row surface (§8) —
  screen, slot, card, failure mapping, route forks, guards, onboarding
  platform keying. To add: the deep-link capability, the open guard, and
  the route-wiring completion (pending destination on all three routes,
  failure query surviving on sign-in).
- NEWFRONT-592 (updater). Exists: the sidebar mount slot (§9), kit button
  and tooltip primitives. To add: the update detail component and the
  updates platform capability (pending query, install, available-event
  subscription).
- NEWFRONT-594 (shell). Exists: the desktop fetch and stream transports to
  extend, the device-grant contracts, the route tree and not-found page,
  the copy-link call sites to repoint. To add: the transport parity gaps,
  the tool bindings and setup flow, the hosted-origin link builder on
  every copy path, and the web-only route exclusions.

## 14. API and native-command catalog

Server endpoints observed from the old call sites (shapes owned by
NEWFRONT-585; already-covered contracts are reused, not duplicated):

- Agent profile read: `GET /api/users/me/ai-assistant/agent-profile/`,
  answering availability, a reason code, the engine base address and the
  model name. Serves DESK-001/002/004/026.
- Agent credential issue: `POST /api/users/me/ai-assistant/agent-token/`
  with an empty body, answering a short-lived credential plus its expiry.
  Serves DESK-001/026.
- Machine enrollment write and removal: `POST` and `DELETE
/api/v1/runner/dev-machines/desktop-enroll/`, carrying the workspace
  slug and host label on write (answering a machine credential plus a
  machine id) and the host label on removal. Serves DESK-001/010.
- Project detail read: `GET /api/workspaces/{slug}/projects/{id}/`,
  carrying the identifier, the executor options (kind, availability,
  reason code) and the default executor. Serves DESK-001/002.
- Project list read: `GET /api/workspaces/{slug}/projects/`, carrying the
  identifiers the chat preparation picks from. Serves DESK-006.
- Issue detail read: `GET .../projects/{id}/issues/{issue}/`, carrying the
  executor pin. Serves DESK-002.
- Forgery-token fetch: `GET /auth/get-csrf-token/`, answering the token
  sent back as the forgery header on unsafe calls. Serves DESK-001/021.
- Device grant: `POST /api/v1/auth/device/start/` with an empty body
  (answering device code, user code, verification address, expiry and
  poll interval) and `POST /api/v1/auth/device/approve/` carrying the
  user code. Serves DESK-021.
- Desktop exchange: a server navigation to
  `/api/auth/desktop-exchange/` carrying the returned code and state,
  which sets the session and redirects into the app — or redirects to
  sign-in with the failure query. Serves DESK-014.
- Sign-out post: the regular server session end (existing contract),
  followed by the platform teardown hook. Serves DESK-010.

Server vocabulary kept exactly (codes are API contracts; all human
wording around them is rewritten per §7): the eight agent reason codes
(feature off, personal-key-on-desktop, no usable model, machine not
linked, stale desktop session, missing gateway grant, revoked gateway
access, gateway down); the executor kinds (bundled agent, local runner);
the approval modes (ask, workspace, full access), approval kinds (command,
file, network, other) and approval decisions (accept, decline,
accept-for-session); the chat frame result names (started, message
started, event, approval request, message completed, failed, closed).

Native commands by family (names and payloads read from
`desktop/src-tauri/src/*.rs`; no slice modifies them):

- Agent lifecycle: self-check (no payload; answers presence flags plus the
  host label), engine-setup write (profile), credential write (credential),
  bootstrap (cloud address, workspace, machine credential, machine id),
  project enrollment (workspace, project, host label), daemon start
  (workspace), daemon stop (grace period), teardown (optional
  per-account history wipe), diagnostics report. Serves
  DESK-001/002/003/010/026.
- Local chat: warm and send (workspace, session, message content on send,
  working directory, mode, thread resume ids), cancel and close
  (workspace, session, reason), decide (workspace, session, approval id,
  decision). Frames stream back over the chat frame event; transport
  failures over the chat error event. Serves DESK-006/007/008.
- Local history: create (account, title, workspace, runner, engine
  version), list and fetch (account, session), append (account, session,
  role, content), thread-id record (account, session, thread id), rename
  (account, session, title), delete and clear (account, session), working
  directory (account, workspace, project). Serves DESK-009.
- HTTP transport: request (framed head plus body; answers a framed head
  plus body), stream (request plus channel; emits head, text and end
  events), cancel (request id). Serves DESK-020.
- Tool setup: detect (no payload; answers presence, path, version),
  install (no payload; progress over the install-log event), login
  (device code, cloud address, workspace). Serves DESK-021.
- Shell: guarded browser open (address), webview wipe (no payload),
  pending-update query (answers the waiting version, current version and
  installing flag), install update (no payload; success restarts the app).
  The daily scheduler announces over the updater-available event. Serves
  DESK-010/014/015/016/017/018.

## 15. Coverage checklist

Every assigned old source mapped to rows (endpoint paths above are
backend contracts as observed from the old call sites):

- Overlay agent-runtime service (enroll, credential, daemon lifecycle,
  gates, teardown): DESK-001/002/003/004/010/026.
- Overlay status-pill component (notice lifecycle): DESK-001.
- Overlay local-chat transport (verbs, translation, persistence, modes):
  DESK-006/007/008/009.
- Overlay picker contacts (local entry plus reason): DESK-004/005.
- Overlay session wipe, tool setup service, update button, hosted-URL
  resolver: DESK-010, DESK-021, DESK-018, DESK-019.
- Overlay routes (bare root, login and sign-in overrides, empty extended
  table): DESK-011/012/025.
- Community desktop seam — Plane code, reference only (§7): reason
  wording DESK-004, approval wording DESK-007/008, fallback card
  DESK-013/015, helper stubs dead code (no rows).
- Web update-button stub (renders nothing): documents the DESK-018 seam.
- Native shell (deep links, tray, window, zoom, navigation guard, browser
  open, webview wipe): DESK-014/015/022/023/024/010.
- Native lifecycle, chat, history, HTTP, tool and updater commands:
  DESK-001/002/003/010/026, DESK-006/007/008, DESK-009, DESK-020,
  DESK-021, DESK-016/017/018.
- No source mapped to zero rows. No `bug:` rows: nothing observed
  contradicted its evident intent; the gate marks any such scenario if
  the desktop build disagrees. Easy-to-miss behaviors keep their own
  rows: DESK-003, DESK-004, DESK-008, DESK-010, DESK-012, DESK-013,
  DESK-015, DESK-017, DESK-019, DESK-023, DESK-024, DESK-026. No role-gated
  desktop behavior was found (signed-in versus signed-out only); no
  custom shortcuts exist beyond the native zoom menu (DESK-023).
