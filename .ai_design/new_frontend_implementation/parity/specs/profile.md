# Profile area spec — member profiles, settings, appearance, tokens, assistant config (PROF-001–028)

Parent epic NEWFRONT-90. Inventory
`.ai_design/new_frontend_implementation/parity/inventory/profile.md` (rows
PROF-001–028, all oracle green). Oracle scenarios
`apps/web_new/e2e/parity/profile/` (10 spec files, green on `apps/web`).

Method: the old frontend was read as a behavior reference only — the member
profile routes, the settings profile routes plus the legacy redirect routes,
the profile, user and appearance component trees, the community-edition
preferences switcher, the settings profile content and sidebar trees, the
token, email, avatar and deactivation dialogs, and the user, file,
assistant, auto-pm, token and integration services with the profile issue
stores. Nothing is imported, copied or translated from those sources; this
spec restates what was learned in prose. The only code-like tokens below
are route shapes, API paths, parameter and field names, and storage keys,
which the contracts issue (NEWFRONT-570) turns into zod contracts. New code
is written from this spec, the wiki pages (Reference not source; Parity;
Architecture; Data layer; UI kit and editor) and the kit — never with old
files open.

Status convention: this spec owns no inventory Status changes. Slices append
`built` to the rows they implement. The area gate (NEWFRONT-586) proves the
targets.

Where the running old app contradicts the inventory row text, the oracle
pinned the observed behavior and filed a bug issue. Those deviations are
recorded in §14. Slices implement the target stated in each section below;
the gate proves the target.

## 1. Profile pages: entry, role tabs, overview (PROF-001, PROF-002, PROF-003, PROF-004, PROF-005, slice NEWFRONT-573)

A workspace member's profile lives at `/{workspaceSlug}/profile/{userId}/`
and renders for any workspace member, including guests viewing the summary.
Viewing your own profile labels the breadcrumb as your work; viewing someone
else names them from their first and last name. Entering the page reads the
member's identity plus per-project breakdown (`GET
/api/workspaces/{workspaceSlug}/user-profile/{userId}/`); the same read
feeds the sidebar in §4. Oracle: `view.spec.ts`.

The tab strip offers five tabs: a summary tab everyone sees, and assigned,
created, subscribed and activity tabs visible only to workspace admins and
members. Guests see the summary tab alone, and a guest opening an
assigned, created or subscribed address directly meets an inline
no-permission notice instead of content. The activity address is the
exception: a guest opening it directly sees the feed view with no denial
and no download control (the layout guard denies only the three issue-tab
addresses). Every tab has its own address under the profile route with a
trailing slash, and an unknown tab id renders nothing while keeping the
surrounding frame. On narrow screens a tab picker replaces the strip and
offers exactly the role's tabs. One deviation is recorded in §14: the
desktop strip never marks the current tab and the mobile picker label stays
empty on every tab (NEWFRONT-433); the target marks and labels the current
tab. Role comes from the workspace permission read (admin or member at
workspace level); the new tree already exposes it through the session
permission hooks. Oracle: `view.spec.ts`.

The summary tab opens with three count cards — created, assigned,
subscribed — each carrying its server total and linking to its tab. While
the stats read is in flight, placeholder blocks stand in for the cards.
Reads: `GET /api/workspaces/{workspaceSlug}/user-stats/{userId}/`, whose
payload carries the three totals plus the state and priority distributions
used below. Oracle: `view.spec.ts`.

Below the cards, a workload row plus priority and state breakdowns render
from the same stats payload. The workload row always shows all seven state
groups (backlog, unstarted, started, review, test, completed, cancelled);
groups with no items show as zero, never as gaps. The priority breakdown
covers the priorities present in the payload, and the state breakdown
mirrors the seven groups with zero-filled counts. A member with nothing
assigned still sees the zeroed workload row while the two charts show their
empty states. Oracle: `view.spec.ts`.

The bottom of the summary carries recent activity: entries naming the actor
by display name with links into the underlying work items (browse
addresses). A member with no recent activity sees a compact empty
illustration instead of the list. Reads: `GET
/api/workspaces/{workspaceSlug}/user-activity/{userId}/` with a page size.
Oracle: `view.spec.ts`.

## 2. Issue tabs with persistent layout and display (PROF-006, slice NEWFRONT-573)

Each of the assigned, created and subscribed tabs lists exactly the
viewed member's matching issues: the list read carries the tab's ownership
selector (assignee, creator, or subscriber set to the viewed user id) over
`GET /api/workspaces/{workspaceSlug}/user-issues/{userId}/`. Tabs render a
list layout by default with a kanban layout one switch away; the switcher
offers exactly those two on desktop and on narrow screens. Header filter
controls (desktop) and a display menu (both) adjust grouping, columns and
extras such as showing empty groups. The chosen layout and display options
persist across visits in browser-local storage scoped to the workspace plus
the viewed member — there is no server endpoint for them — so the oracle
proves persistence by reload. Selecting an issue opens its peek overview;
closing the peek returns to the tab. An unknown tab id renders nothing but
keeps the frame, per §1. Oracle: `view.spec.ts`.

The peek opening is a cross-epic seam, resolved in §13: the shell owns the
shared peek panel host, the issues area owns the peek content and its data,
and this slice renders at most the thin selection-to-content host.

## 3. Activity tab with paging and today's download (PROF-007, slice NEWFRONT-573)

The activity tab pages the viewed member's workspace activity feed, one
hundred entries per page, through an explicit load-more control that
renders only while further pages exist (`GET
/api/workspaces/{workspaceSlug}/user-activity/{userId}/` with page-size and
cursor parameters). Entries name the actor and link into the underlying
work items. A member with no activity sees neither entries nor the
load-more control. Oracle: `activity.spec.ts`.

A download control fetches the current day's activity as a CSV file: it
posts the local date (`yyyy-mm-dd`) to `POST
/api/workspaces/{workspaceSlug}/user-activity/{userId}/export/`, shows a
working label while the export generates, then saves the server's CSV bytes
under a timestamped profile-activity name. The export columns are the
fixed contract shape (actor name, issue id, project, created/updated
timestamps, action, field, old and new values). The control is visible only
to workspace admins and members — guests opening the feed never see it —
which the role read from §1 gates. Oracle: `activity.spec.ts`.

## 4. Identity sidebar (PROF-008, slice NEWFRONT-573)

The side panel identifies the viewed member. Target behavior (per
main#523 and main#574, both still open — the live app still shows the
pre-PR sidebar, pinned as row Gaps): no cover art anywhere in the panel.
The panel shows the avatar photo with an initial fallback, the full name
plus the display name in parentheses, the join date, the member's current
local time as a clock ticking in their own timezone with the timezone name
beside it, and one expandable row per project carrying a completion tint;
expanding a row shows its created, assigned, due and completed counts from
the segregation read. On your own profile an edit pencil navigates to the
settings page general tab at `/settings/profile/general` (per main#573,
still open — the live pencil still opens the settings dialog with the
address unchanged, pinned as a row Gap). Web_new names the account
general tab inside the workspace instead, `/<ws>/settings/account/general/`
(main#670, NEWFRONT-1030). The panel collapses on demand; on
narrow screens it starts collapsed and toggles as an overlay above the
content. Reads: the segregation read from §1 plus project lookups for row
names. Oracle: `view.spec.ts`.

## 5. Settings tabs, shell, legacy forwards (PROF-009, PROF-010, PROF-026, slice NEWFRONT-565, reworked by NEWFRONT-1030 for main#670/#675)

**Where the pages live (main#670).** Personal settings are not a
standalone full-window page any more. Every tab lives inside the workspace
the reader is in: `/<workspaceSlug>/settings/account/<tab>`, rendered by
an ordinary child of the signed-in workspace tree. So the normal app
chrome stays around it — the app rail, the top bar (workspace switcher,
search, inbox, help, account menu), the palette and the toast host are the
same ones every other workspace page gets. Nothing about the settings
pages is mounted outside the shell, so nothing needs to re-create the
chrome or re-run the signed-in gate: the gated workspace layout already
ran above this route. Nine registered tab ids keep their meaning:
general, preferences, ai-assistant, auto-project-management,
notifications, security, activity, integrations and api-tokens.

**Layout.** Inside that shell the settings navigator is the middle panel:
a fixed-width column on the left of the content area holding, top to
bottom, the back control with the settings title, the grouped tab list,
and the workspace section (§5b). The tab body fills the space beside it
and scrolls on its own. On narrow screens the column is replaced by a bar
with a hamburger, the current tab's label, and a floating panel opened
from the hamburger. The panel closes on Escape (returning focus to the
hamburger), on a press outside it, and after a pick. The panel and the
wide column show the same entries; each copy carries its own element ids so
the document never holds duplicates. The back control returns to the
current workspace home, `/<workspaceSlug>`, not to the site root.

**The address names the tab (PROF-009).** The active tab comes from the
path segment after `/settings/account/`. Every tab therefore survives a
reload, a deep link and back/forward, and switching tabs is an ordinary
link navigation to the tab's address (with a trailing slash, the form the
app's own links use; a direct visit without the slash resolves to the same
tab). The bare collection `/<ws>/settings/account` forwards to the general
tab with the query string kept. An unregistered id forwards to the general
tab the same way. A tab this instance withholds — security on a
cloud-managed instance (CLOUD-035) — also forwards to general, so a stale
bookmark lands on a real page rather than a loader or an empty frame. An
address deeper than a tab lands on that workspace's general tab.

**Legacy addresses (PROF-026).** Four families of older addresses keep
working; each is a client redirect that keeps the query string (an empty
query adds nothing):

- `/settings` and `/settings/profile` land on the general tab; `/settings/profile/<tab>` lands on that tab. Anything else below `/settings` is treated as the general tab, not as a 404.
- `/profile` and `/profile/<tab>` land on that tab, defaulting to general.
- `/<ws>/settings/account` and `/<ws>/settings/account/<tab>` land on that workspace's tab, defaulting to general.
- `/<ws>/settings/api-tokens` lands on that workspace's tokens tab, query kept (the target recorded for NEWFRONT-431).

Only a registered tab id is forwarded as a tab: an unknown or misspelled
segment becomes general, so a stale link never strands the page. The
workspace-less families resolve their workspace from the reader's last
workspace while they still belong to it, else the remembered fallback
workspace; a reader who belongs to no workspace continues to workspace
creation. Each forward carries the fragment explicitly, not by luck of the
router: the chat composer's dictation link used to read
`/settings/profile/ai-assistant#voice-dictation`, and a bookmarked copy must
land on that block rather than at the top of the AI Assistant tab, so the
resolver appends `location.hash` to the target address. Only `/profile` and
`/profile/<tab>` forward; two segments below `/profile` (a member address,
not a settings one) falls through to the not-found page. Oracle:
`settings-shell.spec.ts`; the landing addresses (slashed form, query and
fragment) are pinned by
`apps/web_new/src/routes/settings/forwardLanding.test.tsx`.

**Serving the new address (coexistence routing).** `/<ws>/settings/account`
must reach web_new through the proxy from the moment web_new forwards
`/settings/profile` there: while the proxy still hands that subtree to the
old app, the old app's own `/<ws>/settings/account/*` redirect bounces the
reader back to `/settings/profile/*`, which web_new forwards to the account
address again — a loop. The migrated-route list therefore needs a
`/{ws}/settings/account/*` entry, regenerated into both Caddyfiles, before
the area gate can prove the scenarios. The entry is in the migrated-route
list and in both generated Caddyfiles already (added with the route in
NEWFRONT-1030 under a coordinator path grant), so the area gate only has
to confirm it is present rather than regenerate it.

**In-app links.** Every in-app entry to personal settings names the new
address, using the workspace the reader is in: the user menu's Settings
entry and the profile sidebar's edit pencil go to
`/<ws>/settings/account/general/`; the assistant setup card, the composer's
keyless dictation redirect and the thread's "manage servers" link go to
`/<ws>/settings/account/ai-assistant/` (with `#voice-dictation` where the
dictation block is meant); the home quickstart "Personalize now" goes to
`/<ws>/settings/account/general/`; and the workspace-not-found screen's
"Open profile" goes to the first workspace the reader belongs to. No
in-app link points at the legacy addresses any more; they exist only so
old bookmarks and external links keep working. Oracle: `view.spec.ts`,
`shell-059-user-menu.spec.ts`, `setup-card.spec.ts`,
`setup-redirect.spec.ts`.

## 5b. The navigator's workspace section (PROF-010, main#675)

Below the tab list the navigator keeps a Workspace section listing every
workspace the reader belongs to, in the order the workspace list returns,
each with its initial mark and name. Each workspace expands in place to
that workspace's settings pages instead of linking straight to its home:
pressing the row's chevron opens and closes the group, the workspace the
reader is currently in starts expanded, and every other workspace starts
collapsed. Pressing a row does not navigate; pressing an entry inside an
open group goes to `/<that-ws>/settings...`, so a reader can jump into
another workspace's settings without first opening that workspace.

A workspace whose role lets the reader open nothing (a guest there) shows
as a plain row with no expander: pressing it goes to that workspace's
home, exactly as the row behaved before the nesting arrived. Entries are
filtered by the reader's role **in that workspace**, taken from the
workspace list, never by the role in the workspace on screen: an admin in
one workspace sees the admin-only entries under that workspace and not
under a workspace where they are only a member. The catalog behind those
entries is the one the standalone workspace-settings navigator at
`/<ws>/settings*` renders (§1 of the workspace-settings spec): the same
section keys, labels, order, minimum roles and addresses come from one
module, so the two lists cannot drift; this section adds no entries of
its own. The create-workspace and workspace-invites shortcuts stay beneath
the workspace rows.

Highlighting follows the address the reader is on. Only entries of the
workspace being viewed can read current; entries of another workspace
never do, and no entry reads current while an account tab is open (the
account pages are not workspace settings), including the general entry.
Within the viewed workspace, General matches only the settings root
itself, while every other entry matches its own address and anything
below it, so a nested page such as a webhook detail keeps its Webhooks
entry lit. The wide column and the narrow panel apply the same rules.

## 5c. Edges and failure states (PROF-009, PROF-010, PROF-026)

A visitor who is not signed in is bounced to the entry page with the
address they asked for as the return path, and a member who has not
finished onboarding continues into onboarding — both checked before any
forward resolves a workspace. Reads that fail do not take the page down:
a failed settings or workspace-list read leaves the forward to resolve
what it can and, when no workspace is known, sends the reader to workspace
creation. While the workspace list is still loading the navigator renders
its tabs and shortcuts with the workspace section simply empty, and fills
it in when the list arrives.

## 6. Account: names, avatar, email, deactivation, password (PROF-011, PROF-012, PROF-013, PROF-014, PROF-015, slice NEWFRONT-565)

The general tab edits the signed-in member's own names. First and display
names are required; last name is optional. Person names accept letters,
spaces, hyphens and apostrophes up to fifty characters; display names
accept letters, numbers, periods, hyphens and underscores up to fifty
characters. Violations report inline per field, and failed validation
submits nothing. Saving writes `PATCH /api/users/me/` with the name
fields, showing progress and success or failure feedback; the email field
stays locked to the account address and is never editable here. One
deviation is recorded in §14: an emptied last name is currently rejected
as required despite carrying no required marker (NEWFRONT-432 — the shared
person-name check rejects empties); the target saves it. Oracle:
`settings-account.spec.ts`.

The avatar control opens an upload dialog over the general tab. Non-image
and oversized files (above five megabytes) are rejected inline; a valid
pick uploads with an auto-save and persists with feedback; a remove option
clears back to the placeholder and persists the removal. Target behavior
(per main#523 and main#574, still open): no cover banner, gallery picker
or cover-change control exists anywhere on the tab — the live tab still
shows the cover control (pinned as a row Gap). The save sends names,
avatar and display name only; no cover fields travel (main#523 removes
them server-side too, so contracts carry none). The avatar asset flow runs
through the user-asset endpoints: create the asset record with its entity
info plus file metadata, upload the bytes to the signed address, confirm
completion, and delete through the user-asset or legacy user file-asset
delete on removal or replacement. Oracle: `settings-account.spec.ts`.

The email change entry sits below the locked email field and appears only
on instances with outgoing mail configured. It runs a two-step flow: the
first step checks the address shape, rejects already-taken addresses
through the email-check write, then sends a one-time code to the new
address; the second step locks the address and verifies the code. Wrong
codes fail inline without touching the account email; the mailed code
moves the account and signs the member out. Writes: `POST
/auth/email-check/` with a forgery token, `POST
/api/users/me/email/generate-code/` with the address, then `PATCH
/api/users/me/email/` with address plus code. Oracle:
`settings-account.spec.ts`.

Account deactivation lives in the general tab's danger zone and appears
only where the instance reports self-managed; on managed instances no
control exists. A confirmation dialog warns that removal is permanent;
cancelling keeps the account intact, while confirming deletes the account
through `DELETE /api/users/me/`, signs out, and lands on the signed-out
entry with feedback. The oracle proves the present state on a throwaway
account; the managed absence is code-gated and unobservable on the seeded
stack. Oracle: `settings-account.spec.ts`.

The security tab changes the password. Accounts created through a
provider skip the current-password field; otherwise it is required. The
new secret must pass the strength meter, differ from the current one, and
match its confirmation, with the mismatch note following focus-aware
timing (no note while a short guess still has focus; always once it
outgrows the guess or blurs) and the same-as-old guess called out; each
field has a reveal toggle. A guess the server rates weak maps to a field
error plus feedback; a wrong current password reports without resetting
or saving; success reports, resets the form, and takes effect. Writes: a
forgery-token fetch plus `POST /auth/change-password/` carrying the old
and new secrets. Oracle: `settings-account.spec.ts`.

## 7. Appearance, locale, own activity (PROF-016, PROF-017, PROF-019, slice NEWFRONT-576)

The preferences tab applies interface themes the moment they are picked:
system, light, dark, a high-contrast pair, and a custom variant. Each pick
persists through `PATCH /api/users/me/profile/` carrying the theme bag
(theme name, primary and background colors, dark-palette flag) with
progress and failure feedback. Picking custom reveals a designer for
primary and background colors plus a dark-palette switch: typing previews
live through the swatches with nothing saved, and saving persists then
reloads to apply. Re-picking custom re-applies previously saved custom
colors immediately. One deviation is recorded in §14: on a failed save the
switcher and server roll back but the document keeps the failed pick
(NEWFRONT-434); the target rolls the applied theme back too. Oracle:
`preferences.spec.ts`.

Language, timezone and week-start selectors save the moment they are
picked, each with success or failure feedback. Timezone writes `PATCH
/api/users/me/` with the timezone and shifts every displayed time
(profile sidebar clock, greeting clock, stored dates rendered in the
member timezone). Week start writes `PATCH /api/users/me/profile/` with
the zero-based day index (Sunday at zero) and reorders day columns on all
calendars. Language writes `PATCH /api/users/me/profile/` with the tag
and re-renders the interface locale. The OSS build ships English only:
the picker offers English alone, and the save path is proven through a
staged-French round trip with locale re-render revisited at the area
gate. Which side owns what across areas is resolved in §13. Oracle:
`preferences.spec.ts`.

The settings activity tab pages the signed-in member's own cross-workspace
history (`GET /api/users/me/activities/` with page-size and cursor
parameters, one hundred per page) through a load-more control visible
only while further pages exist. A member with no activity sees a themed
illustration matching the current light or dark mode instead of entries.
Oracle: `own-activity.spec.ts`.

## 8. Notification preferences pointer (PROF-018, slice NEWFRONT-576, no code)

This row is a pointer, not an implementation: per-topic email toggles and
their delivery effect are inventoried as NTF-024 and NTF-025 in the
notifications area. The profile slice writes no preference code here; it
verifies through the settings tab seam that the notifications tab renders
the email entry whose keys match the server's preference read. Toggle
depth and delivery proof belong to the notifications area's scenarios.
The exact split is resolved in §13. Oracle: `preferences.spec.ts` (thin
render check).

## 9. Personal access tokens (PROF-020, slice NEWFRONT-583)

The tokens tab under the developer section lists the signed-in member's
own tokens behind a loader: an empty state carrying its own creation
action until the first token, otherwise one row per token with a
liveness marker and an expiry line. Expiry wording has three variants: a
dated line for expiring tokens, a never-expires line where unset, and an
elapsed line for expired tokens; liveness agrees with the server. Oracle:
`tokens.spec.ts`.

Creation takes a label, an optional description, and an expiry: four
presets (one week, one month, three months, one year), a custom date
floored at tomorrow, or a never-expires switch that disables the picker
and hides the preview until flipped back. The label is required and
length-capped with inline errors; submitting without a date reports the
missing expiry while keeping the dialog open; the picked expiry previews
live. Submitting reveals the secret exactly once — a heading, the raw
token value with copy support, and a downloadable backup file carrying
the title, description, expiry and secret columns — then the value is
unrecoverable: closing the reveal leaves no copy on the page or the
server read. Deletion asks for confirmation naming the irreversible
action; cancelling keeps the row, confirming deletes with a notice and
returns to the empty state when the last token goes. Endpoints: token
list, create, item read and delete (`GET`/`POST`
`/api/users/api-tokens/` plus item read/delete); the create payload
carries label, description and expiry timestamp or null. Oracle:
`tokens.spec.ts`.

## 10. Assistant provider surfaces (PROF-021, PROF-022, PROF-023, slice NEWFRONT-580)

The AI-assistant tab stacks three sections sharing the same save, test
and remove shape. The language-model section saves a provider kind
(defaulting to the URL-based kind with an endpoint field; the named
provider kind hides the endpoint), model and key. Saving is gated on a
key; the write clears the key field, the read never echoes the key (a
presence flag only), and testing dials the provider and reports its pass
or failure code without stamping a verified time on failure. Validation
failures keep the form and report; a server message surfaces when the
body carries one. Removal clears the configuration and restores the
keyless form. Endpoints: config read, write, delete and connection test
(`GET`/`PUT`/`DELETE /api/users/me/ai-assistant/config/` plus `POST`
`.../config/test/` with no body). Oracle: `assistant.spec.ts`.

The voice-dictation section mirrors that lifecycle without a provider
kind: endpoint, model and key save, test and removal with the same
feedback and no key echo, over the parallel `stt-config` endpoints.
Target behavior (per main#586, still open — the live section renders
unflagged, pinned as a row Gap): the config read carries the kill-switch
state, and with the switch off the Voice section is removed entirely
unless a key is already saved, in which case it shows only the heading,
an unavailable note and the remove control. Oracle: `assistant.spec.ts`.

The tool-servers section registers external tool servers by name plus
endpoint with an optional auth header. Adds are gated on name and URL; a
server saved with credentials lists with a locked marker, all rows show
a derived tool prefix that drops while disabled. Rows flip between
enabled and disabled and are removable; duplicate names fail with the
list kept. Every outcome reports through feedback and the server list
stays the source of truth. Endpoints: tool-server list, create, item
update and delete (`GET`/`POST`
`/api/users/me/ai-assistant/mcp-servers/` plus item `PATCH`/`DELETE`).
Oracle: `assistant.spec.ts`.

The boundary with the assistant area is resolved in §13: this slice owns
the settings-tab configuration surfaces only, never the chat, composer
or transcription consumption. The assistant and upkeep services are AI
Republic's own code, so their logic may be ported and adapted (no
runtime imports; anything they use from Plane code stays reference-only
and is rewritten).

## 11. Upkeep and integrations (PROF-024, PROF-025, slice NEWFRONT-580)

The upkeep tab schedules automatic project-upkeep jobs. A master switch
pauses everything without touching individual jobs: pausing flips
instantly, disables the job switches while paused, and keeps the jobs
themselves on. Each scheduled job toggles on its own with the same
instant flip. Both roll back with an error note on failed writes, and
the server read stays the source of truth. Instances with nothing
scheduled explain so instead of showing switches. Reads and writes:
upkeep-settings read, master-toggle write and per-job toggle writes
(`GET`/`PATCH /api/users/me/auto-pm/` with the enabled flag plus `PATCH`
`/api/users/me/auto-pm/jobs/{slug}/`); unknown slugs refuse
server-side. Oracle: `upkeep-integrations.spec.ts`.

The integrations tab connects the code-hosting app per workspace. A
workspace picker lists the admin's workspaces and swaps the panel
between them; members see a no-admin view with no workspaces. An
unconnected panel explains the state with an install control that stays
disabled while the instance configuration is missing (with a
missing-configuration warning); starting the install posts the workspace
slug and hands off to the host. A connected panel renders the connection
identifiers (account, account type, repository access, visible
repositories, verified and last-checked times) with a reconnect control
and a manual refresh; refresh failures report the server message and
keep the panel. Returning from the host with a success or failure marker
in the address shows the matching result toast, selects the workspace,
and scrubs the markers while keeping unrelated query parameters.
Endpoints: connection-status read, install-start and connection-refresh
writes (`GET /api/users/me/integrations/github/app/`, `POST`
`.../app/install/` and `POST .../app/refresh/`, each scoped by workspace
slug); refreshes without an installation or adminship refuse
server-side. Oracle: `upkeep-integrations.spec.ts`.

## 12. Cross-cutting rows (PROF-027, PROF-028, slice NEWFRONT-580)

Nothing in this area acts without its visible control: no custom key
bindings, no drag gestures, no import paths, and no live updates. Tabs
never refetch on their own and open no sockets — server-side changes
stay invisible until navigation — and lists update on navigation, paging
or filter change only. The only export anywhere in the area is the
activity CSV in §3. The slices build no shortcuts, polling, sockets,
drag targets, file inputs or import/export paths beyond that download.
Oracle: `cross-cutting.spec.ts`.

Every settings edit touches only the sign-in member's own record: no
tab offers editing another member, no member picker exists on any tab,
and workspace role changes nothing inside settings. The only role-gated
surfaces in the area are the profile-view tabs (§1), the activity
download (§3), and the admin-scoped integrations workspace list (§11);
instance flags additionally gate the email change (§6, mail configured)
and deactivation (§6, self-managed). Oracle: `cross-cutting.spec.ts`.

## 13. Cross-epic seams (resolved)

PROF-018 notification preferences — render here, own there. The
notifications area (NTF-024/025) owns the preference toggles, their
endpoints, their instant-save semantics and the delivery effect: the
profile slice builds no toggle and calls no preference endpoint. The
profile slice owns the settings tab entry and shell slot the
notifications tab renders through (§5), and the profile gate proves
only that the tab renders the email entry whose keys match the
server's preference read. Toggle persistence, failure rollback and
per-topic delivery gating are proven by the notifications gate. Neither
side duplicates the other's part.

PROF-006 issue-tab peek — consume, do not build. The shell owns the
shared peek panel host in the shared kit (built for SHELL-016 and reused
by home recents), and the issues area (NEWFRONT-71) owns the peek
overview content with its issue-data fetching. The profile slice
therefore builds no peek host and no peek internals: selecting an issue
renders at most a thin selection host inside the tab that mounts the
issues-owned peek content through its public feature interface, exactly
as home recents reuse the shared panel today. Until the issues area
ships those pieces, the tab stays gate-provable with stub content behind
the same host contract; list versus kanban rendering, ownership
filtering, persistence and unknown-tab handling are proven through the
tab itself. Owned by other areas and never duplicated here: peek
overview internals, issue-data fetching, the overlay panel shell.

PROF-017 locale, timezone and week start — profile owns the writes,
areas share the reads. The profile slice owns the three selectors, their
instant-save writes, their feedback, and the document stamping (locale
tag, applied theme-adjacent clock rendering). Every other screen
consumes through shared sources, never through profile-owned copies:
the interface-language provider tag, the session user timezone, and the
first-day-of-week preference. The profile gate proves the writes
persist, the profile-owned surfaces react (sidebar clock, dates), and
unit tests pin the propagation contracts; each consumer area proves its
own screens (greeting clock, stored-date rendering, calendar columns)
against the same shared sources at its own gate.

PROF-021–023 assistant surfaces — settings config here, assistant
behavior there. The profile slice owns the AI-assistant tab's
configuration surfaces end to end: the provider, dictation and
tool-server forms, their reads, writes, deletes and connection tests,
and the kill-switch gating in §10. The assistant area (NEWFRONT-75)
owns everything that consumes that configuration: threads, chat,
the composer (including its dictation gating off the same switch
state), transcription calls and tool invocation. The profile slice
never calls the transcription endpoint and builds no composer or chat
surface; the assistant area never builds a settings form for these
configs. Shared read-only state (switch state, key presence) flows
through the config reads both sides already consume.

## 14. Recorded deviations (old bugs, linked issues)

Each item below is an observed old behavior the target intentionally
differs from. None is encoded silently; each names its tracker.

- NEWFRONT-430 (PROF-009, PROF-026): an unknown settings tab id strands
  the page on the loader with no recovery. Target: fall back to the
  default tab instead (§5).
- NEWFRONT-431 (PROF-026): the workspace api-tokens forward drops its
  query parameters while its sibling forwards keep them. Target: carry
  parameters like its siblings (§5).
- NEWFRONT-432 (PROF-011): a cleared last name is rejected as required
  despite the field carrying no required marker — the shared person-name
  check rejects empty values. Target: an emptied last name saves (§6).
- NEWFRONT-433 (PROF-002): the desktop tab strip never marks the current
  tab and the mobile picker label stays empty on every tab — the tab
  match paths carry trailing slashes the pathname never has. Target: the
  strip marks and the picker labels the current tab (§1).
- NEWFRONT-434 (PROF-016): a failed theme save rolls the switcher and
  the server back but leaves the document on the failed pick. Target:
  the applied theme rolls back with the switcher (§7).

Smaller oracle-vs-inventory notes, all honored above: the activity
address renders its feed for guests without denial or download (§1);
layout and display persistence is browser-local, proven by reload (§2);
the email and deactivation success feedback fires before its sign-out
navigation, so only the durable outcomes are pinned (§6); the
managed-instance absence of deactivation and the unconfigured-install
start are unobservable on the seeded stack (§6, §11); the OSS build
offers English alone with the save path proven via a staged round trip
(§7).

## 15. Endpoint reference (contracts issue NEWFRONT-570)

Every endpoint the area uses, with the parameters and fields the slices
and contracts build to. Workspace-scoped paths interpolate the workspace
slug; per-user paths interpolate the viewed user id.

- Identity plus per-project breakdown: `GET
/api/workspaces/{workspaceSlug}/user-profile/{userId}/`, no
  parameters. Response carries the identity fields (names, display
  name, avatar, timezone, join date) plus one row per project with its
  issue counts.
- Counts plus distributions: `GET
/api/workspaces/{workspaceSlug}/user-stats/{userId}/`, no parameters.
  Response carries the created, assigned and subscribed totals, the
  state distribution (state group plus count per group) and the
  priority distribution.
- Issue lists: `GET
/api/workspaces/{workspaceSlug}/user-issues/{userId}/` with the tab's
  ownership selector (assignee, creator or subscriber set to the viewed
  user) plus the shared layout, display and filter parameters.
- Activity feed: `GET
/api/workspaces/{workspaceSlug}/user-activity/{userId}/` with page
  size (`per_page`, 100 per page) and cursor. Envelope carries the rows
  plus the totals the load-more control follows.
- Activity export: `POST
/api/workspaces/{workspaceSlug}/user-activity/{userId}/export/` with
  the local date (`date`, `yyyy-mm-dd`). Answers the CSV bytes.
- Own history: `GET /api/users/me/activities/` with page size and
  cursor, same paging shape as the activity feed.
- Current user: `GET /api/users/me/` to load; `PATCH /api/users/me/`
  with the name fields, the avatar reference and the timezone
  (`first_name`, `last_name`, `display_name`, `avatar`,
  `user_timezone`). No cover fields travel.
- Current profile: `GET /api/users/me/profile/` to load; `PATCH
/api/users/me/profile/` with language, week start, role passthrough
  and the theme bag (`language`, `start_of_the_week`, `role`,
  `theme` carrying `theme`, `primary`, `background`, `darkPalette`).
- Avatar assets: `POST /api/assets/v2/user-assets/` with entity info
  plus file metadata, bytes to the signed address, `PATCH
/api/assets/v2/user-assets/{assetId}/` to confirm; removal through
  the user-asset delete or the legacy user file-asset delete.
- Email change: `POST /auth/email-check/` with the address (forgery
  token header) for the taken check; `POST
/api/users/me/email/generate-code/` with the address; `PATCH
/api/users/me/email/` with address plus code.
- Deactivation: `DELETE /api/users/me/`, no body.
- Password: `GET /auth/get-csrf-token/` for the forgery token; `POST
/auth/change-password/` with the old and new secrets (old omitted
  for provider-created accounts), token in the `X-CSRFTOKEN` header.
- Notification preferences: `GET
/api/users/me/notification-preferences/` to load; `PATCH` the same
  path per toggle (property change, state change, comment, mention,
  completed-only refinement). Owned by the notifications area; listed
  here for the §8 render check only.
- Tokens: `GET`/`POST /api/users/api-tokens/` to list and create
  (create carries label, description and expiry timestamp or null);
  item read and `DELETE /api/users/api-tokens/{id}/` to remove. The
  create response carries the one-time secret.
- Assistant provider: `GET`/`PUT`/`DELETE
`/api/users/me/ai-assistant/config/`plus`POST .../config/test/`
  with no body. Write fields are provider kind, endpoint, model and
  key; the read carries the presence flag and last-verified time and
  never the key.
- Dictation provider: the parallel `stt-config` paths with the same
  verbs; the read additionally carries the kill-switch state.
- Tool servers: `GET`/`POST
/api/users/me/ai-assistant/mcp-servers/` plus item `PATCH`/`DELETE`.
  Fields are name, endpoint, optional auth header (write-only; the
  read carries a presence flag) and the enabled flag.
- Upkeep: `GET`/`PATCH /api/users/me/auto-pm/` with the enabled flag;
  `PATCH /api/users/me/auto-pm/jobs/{slug}/` with the enabled flag
  per job.
- Integrations: `GET /api/users/me/integrations/github/app/` for the
  status (configuration flag plus per-workspace rows); `POST
.../app/install/` and `POST .../app/refresh/`, each scoped by
  workspace slug.

No other endpoint is called by these screens: profile issue-tab layout
and display choices persist in browser-local storage, and settings
tabs hold no socket, polling or native calls. The contracts issue
covers any of the above not already under contract; the current-user
and profile reads, the name, avatar, timezone, language, week-start
and theme writes, the timezone list, the assistant thread reads and
the LLM config read already exist (see §16).

## 16. F-06 reuse map (NEWFRONT-17 survey)

NEWFRONT-17 delivered the app shell, sign-in, command registry, toasts
and a read-only issue list, and later foundation work added the session,
theme, language, peek-panel and kit pieces below. Per slice, what exists
versus what must be added or reworked (surveyed on `web-new-dev` at the
shell gate):

- Slice NEWFRONT-573 (profile pages, PROF-001–008). Exists: the app
  shell with gated workspace routes, the session permission hooks for
  the admin/member/guest split, kit breadcrumbs, avatar, skeleton,
  spinner and empty-state pieces, the issues feature's read-only list
  plus its query-key patterns, the shared peek panel host, and the
  platform storage helper for the browser-local layout persistence.
  Added: the profile route composition (overview, issue tabs, activity
  tab, sidebar), the count cards, workload and distribution charts
  through the shared chart wrappers, the recent-activity and paged
  feed lists, the CSV download, the live sidebar clock, the kanban
  layout for the issue tabs, and the thin peek-selection host (stub
  peek content until the issues area ships its pieces).
- Slice NEWFRONT-565 (settings shell plus account, PROF-009–015,
  PROF-026). Exists: file routes with validated search params, the
  shell layout, kit dialog, input, button, avatar, menu, skeleton,
  spinner and toast pieces, the current-user and profile reads, the
  name and avatar writes, the instance-flags read for the mail and
  self-managed gates, and the auth sign-out path. Added: the settings
  route composition with the grouped tab registry and workspace
  shortcuts (page only — no dialog variant per the §5 target), the
  legacy forwards, the general form with its inline validation, the
  avatar upload dialog, the two-step email flow, the deactivation
  dialog, and the password form with its strength, timing and reveal
  rules. Reworked: nothing — no existing surface implements settings
  today.
- Slice NEWFRONT-576 (appearance, locale, own activity, PROF-016–019).
  Exists: the full theme model with provider, live application and
  custom-color designer, the language provider with profile-backed
  switching, the persisted week-start and timezone writes, the profile
  and timezone contracts, and kit switch, menu, skeleton and
  empty-state pieces. Added: the preferences-tab controls wired to
  those providers (theme picker with rollback-correct failure
  handling, language, timezone and week-start selectors), the
  notifications-tab render check (no preference logic, per §8 and
  §13), and the own-activity tab with its paging plus a newly drawn
  light/dark empty illustration. Reworked: nothing.
- Slice NEWFRONT-583 (tokens, PROF-020). Exists: the settings tab seam
  from slice NEWFRONT-565, kit dialog, input, menu, switch, skeleton,
  empty-state and toast pieces. Added: the tokens tab with its loader,
  rows and expiry wording, the create dialog with presets, custom
  floor, never-expires switch and live preview, the one-time reveal
  with copy and backup-file download, and the delete confirmation.
  Reworked: nothing.
- Slice NEWFRONT-580 (assistant, upkeep, integrations, PROF-021–025,
  PROF-027–028). Exists: the settings tab seam, kit switch, input,
  button, menu, skeleton and toast pieces, the assistant-thread reads
  and the LLM config read, and the port-allowed assistant and upkeep
  service logic (ported and adapted to queries plus contracts, never
  imported at runtime). Added: the provider, dictation and tool-server
  sections with the kill-switch gating, the upkeep master and per-job
  switches, and the integrations picker, panels and callback handling.
  PROF-027 and PROF-028 add no components: they are proven by the
  absence of shortcuts, sockets, drag targets and member pickers, plus
  own-record scoping in every mutation. Reworked: nothing.
- Contracts NEWFRONT-570. Exists: current-user and profile reads; the
  name, avatar, timezone, language, week-start and theme writes; the
  timezone list; the assistant-thread reads and LLM config read; the
  unread-count read. Added: every §15 endpoint not yet under
  contract — the profile segregation, stats, issue-list, activity and
  export reads, the own-activity read, the email, deactivation and
  password writes, the token endpoints, the assistant write, delete,
  test, dictation and tool-server endpoints, the upkeep and
  integration endpoints, and the avatar asset endpoints — each with
  its contract test.
