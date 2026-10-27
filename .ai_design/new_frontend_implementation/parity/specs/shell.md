# Shell area spec — workspace shell, home, projects list, command palette / Power K, search (NEWFRONT-275)

Parent epic NEWFRONT-70. Inventory
`.ai_design/new_frontend_implementation/parity/inventory/shell.md` (rows
SHELL-001–109). Oracle scenarios `apps/web_new/e2e/parity/shell/`.

Method: the old frontend was read as a behavior reference only. Nothing is
imported, copied or translated from it; this spec restates what was learned in
prose. The only code-like tokens below are API paths and field names, which the
contracts issues (NEWFRONT-276, NEWFRONT-277) turn into zod contracts. New code
is written from this spec, the wiki pages (Reference not source; Parity; Data
layer; UI kit and editor) and the kit — never with old files open.

Target-behavior rule (NEWFRONT-156): the inventory describes target behavior,
not today's main. Ten rows (017, 032, 034, 040, 041, 042, 046, 059, 070, 105)
were amended to match merged main PRs while the live old app still shows
pre-PR behavior. Those rows are restated precisely in §12; slices implement the
target text and the gate aligns the scenarios.

Status convention: this spec owns no inventory Status changes. Slices append
`built` to the rows they implement.

## 1. Route shells (NEWFRONT-288)

SHELL-001 — Every workspace page renders inside one workspace route shell. The
shell gates on sign-in first and workspace membership second: signed-out
visitors are redirected before any workspace content loads, and non-members
never reach child routes. Inside the gate every page shares the same top bar
and chrome with page content mounted in the content slot; the optional app rail
and the workspace-scoped global dialogs mount here so they are available on
all workspace pages. No endpoint is called directly by the shell; membership
resolution lives in the auth wrappers (auth area). Oracle:
`chrome-tabs/workspace-shell.spec.ts`. Gap carried from inventory: membership
gate, drafts route and global dialogs are unproven in the seeded single-user
stack and must pass at gate time.

SHELL-002 — The projects-area shell keeps the app sidebar mounted across home,
projects list, drafts and sibling pages. Navigating between those pages keeps
sidebar state (no remount); a slide-over slot beside the outlet hosts panels
such as overflow lists, and full-screen overlays render through an overlay
portal rather than unmounting the sidebar. Layout prop only, no API. Oracle:
`chrome-tabs/workspace-shell.spec.ts`. Gap: drafts route and slide-over slot
unproven in seed; proven at gate.

SHELL-106 — The workspace-level browse route renders the project-scoped detail
view, not a workspace-wide browser. Opening it shows the same detail as
in-project navigation, preserving archived state (an archived timestamp on the
item propagates an archived flag to the detail root). No shell-level API; the
identifier in the route resolves to the internal id before detail loads, and
detail fetching belongs to the issues area. Oracle:
`palette-search/browse-route.spec.ts`.

SHELL-107 — The root document shell sets page metadata (product title,
description, social metadata), home-screen icons and install manifests, mounts
the app provider tree over global overlay portals for menus and editors, and
loads the session recorder only when its build flags are present (with flags
absent no third-party snippet loads). No runtime API. Oracle:
`document/document-shell.spec.ts`.

SHELL-108 — Unknown addresses render an illustrated not-found page with
explanatory copy and a working home action — never a blank screen, stack trace
or empty shell — and the page carries a noindex marker. No API. Oracle:
`document/not-found.spec.ts`.

## 2. Home dashboard (NEWFRONT-278, NEWFRONT-279, NEWFRONT-280)

SHELL-003 — The workspace home route renders the dashboard: greeting,
assistant card and widget stack inside a centered content column, with the tab
title carrying the workspace name. Content renders only once workspace context
exists. Data arrives through the home store keyed per workspace slug:
dashboard widget fetch, widget statistics, dashboard details and widget
updates (`GET .../dashboard/`, `GET/PATCH .../dashboard/{id}[/widgets/]`;
exact shapes owned by NEWFRONT-276). Oracle: `home/greeting.spec.ts`.

SHELL-004 — The greeting shows a time-of-day salutation with the user name, a
date line and a live local clock. Morning, afternoon and evening variants
follow the local hour; the clock advances on a client minute ticker without
reload, rendered in the timezone from the user record. Oracle:
`home/greeting.spec.ts`.

SHELL-005 — The salutation localizes while the date keeps a fixed shape.
Proven scope: the open-source build ships English strings only, so translation
itself is unprovable here; unknown or malformed locales keep the English
salutation with its fixed date shape and never render blank. Broken timezones
are rejected by the API (400), so the UI never observes one. Inputs: profile
language read on boot, timezone from the user record. Oracle:
`home/greeting.spec.ts`.

SHELL-006 — Users whose tour flag is incomplete see a first-run tour overlay
covering the dashboard; completing it persists so later visits show the
dashboard directly. Persist call: `PATCH /api/users/me/tour-completed/`; the
flag itself is read from `GET /api/users/me/profile/`. Tour content belongs to
onboarding. Oracle: `home/tour.spec.ts`.

SHELL-007 — The home assistant card sits between greeting and widgets and
offers suggested prompts plus recent conversations, or a setup reminder when
no model key is configured. Guests see no card at all. Empty or whitespace
input is a no-op; starting without a configured key is refused with an error
notice and no API call. Recent threads link into the assistant view. Seam (see §10): the shell slice builds the card shell, prompt chips, thread list read
and setup reminder; the assistant epic (NEWFRONT-75) owns thread internals,
thread creation and model setup. Oracle: `home/assistant.spec.ts`.

SHELL-008 — New or empty workspaces get a quickstart checklist of four cards
(create project, invite team, configure workspace, personalize account),
hidden on wiki-style paths. It shows for fresh workspaces, a dismissal control
hides it per workspace (local flag), and it auto-hides once the workspace is
lived in (user joined projects and membership exceeds one). Reads: project,
workspace and permission stores. Oracle: `home/quickstart.spec.ts`.

SHELL-009 — Each quickstart action is role-gated with no affordance where
forbidden: guests get no project creation, non-admins get no invite or setup
links, the profile action is available to everyone. Forbidden actions show
nothing clickable; allowed ones navigate to their target. Oracle:
`home/quickstart.spec.ts`.

SHELL-010 — Each card tracks its own completion: creating or joining a
project, reaching two members, or visiting settings or profile flips that card
to done. Inputs: joined-project list, member count, local visit flags. Oracle:
`home/quickstart.spec.ts`.

SHELL-011 — The saved reference-link widget supports add, edit, open in a new
tab, copy address and delete. A link needs an address (display name optional;
a missing scheme is auto-completed); open, copy, edit and delete each work
with success or failure notices. Endpoints: `GET/POST/PATCH/DELETE
/api/workspaces/{slug}/quick-links/` (update and delete take the link id;
create body carries title and url). Oracle: `home/quicklinks.spec.ts`.

SHELL-012 — The link add/edit dialog validates inline with progress and safe
cancel: an empty or malformed address blocks submit inline, submit shows
progress, cancel discards, failure toasts. Same quick-link endpoints. Oracle:
`home/quicklinks.spec.ts`.

SHELL-013 — The link list collapses behind an expander for long lists, shows
skeleton tiles while loading (fetch-once per workspace), and shows a guidance
empty state at zero links. Oracle: `home/quicklinks.spec.ts`.

SHELL-014 — The recent-activity feed lists recently visited work items, pages
and projects in visit order. Rows with missing payloads are skipped; the list
is capped inside a scrollable, expandable container. Endpoint: `GET
/api/workspaces/{slug}/recent-visits/`. Oracle: `home/recents.spec.ts`.

SHELL-015 — A dropdown in the Recents header filters by activity type; each
selection refetches with its own filter key, and an empty slice shows its own
per-type illustration. The filter value maps to the `entity_name` query
parameter (omitted for All). Oracle: `home/recents.spec.ts`.

SHELL-016 — Recent rows navigate deep: work-item rows open an in-place side
preview without leaving home (title renders as an editable field), page rows
land on the right scope, project rows land on the project work list. Seam (see §10): the shell slice builds a shared peek panel that the issues epic
(NEWFRONT-71) reuses for full peek; the shell does not own issue-data
semantics. Oracle: `home/recents-nav.spec.ts`.

SHELL-018 — Widget visibility toggles and drag ordering apply immediately and
persist per workspace. Toggling shows or hides instantly and silently (no
notice) and survives reload; reordering persists with a confirmation toast.
Endpoints via the home store: widget fetch plus widget update on
`.../home-preferences/` — observed as `GET
/api/workspaces/{slug}/home-preferences/` for the ordered map and `PATCH
/api/workspaces/{slug}/home-preferences/{key}/` carrying `is_enabled` for
toggles and `sort_order` for reorder position (exact shapes owned by
NEWFRONT-276). Oracle: `home/widgets.spec.ts`.

SHELL-019 — A retired widget key never renders, never appears as a toggle, and
never blocks the all-off empty state even if the server marks it enabled. This
is a client-side key filter over the widget map. Oracle:
`home/widgets.spec.ts`.

SHELL-020 — Disabling every visible widget shows a themed guidance
illustration; enabling any widget restores the stack. Derived from the widget
map, no call. Oracle: `home/widgets.spec.ts`.

SHELL-021 — Staged loading skeletons cover the dashboard shell and each
widget: initial load shows multi-block skeletons, and links and recents each
show their own shape until resolved. Driven by home and project store loading
flags. Oracle: `home/chrome.spec.ts`.

SHELL-022 — The dashboard header pairs a home breadcrumb identifying the
screen with a widget-settings shortcut that opens the manage-widgets dialog.
Display chrome only. Oracle: `home/chrome.spec.ts`.

SHELL-023 — Home data never refreshes silently and failures never get a retry
panel. Refocus or reconnect does not reorder widgets, links or recents (no
focus refetch); failed reorder and link edits toast and leave prior state;
list-fetch failure logs without a retry button. A failed toggle also
toasts and keeps the prior visibility: the new app paints optimistically,
rolls back on error and names the failure, matching reorder. The old app is
the deviation (old bug NEWFRONT-131): it PATCHes before updating local state,
so a failed toggle keeps state but shows nothing; its scenario leg pins that
silence as the bug while the new-app leg proves the intended toast. Oracle:
`home/chrome.spec.ts`.

## 3. Projects list (NEWFRONT-281, NEWFRONT-282, NEWFRONT-283, NEWFRONT-284)

SHELL-024 — The projects list fetches the workspace collection once and
renders a responsive card grid: every member-visible project appears as a
card, widening from one to three columns on larger screens. Source: `GET
/api/workspaces/{slug}/projects/details/` (per-workspace fetch), with the
lightweight `GET /api/workspaces/{slug}/projects/` as a supporting read.
Oracle: `projects-list/list-and-grid.spec.ts`.

SHELL-025 — While the collection resolves, skeleton shimmer cards show first,
then real cards — never an empty flash. Loader flags are client store state.
Oracle: `projects-list/list-and-grid.spec.ts`.

SHELL-026 — With zero projects and archive view off, a first-run empty state
explains projects and offers creation gated by role: guests get a disabled
action, admins and members get a working create form (`POST
/api/workspaces/{slug}/projects/`). Oracle:
`projects-list/empty-states.spec.ts`.

SHELL-027 — The no-match and empty-archive states stay distinct: a nonsense
search shows the miss variant, while archive view with nothing archived shows
the congratulatory variant with different art. Client-side only. Oracle:
`projects-list/empty-states.spec.ts`.

SHELL-028 — Sorting orders by manual order, name, creation date or member
count, each direction where meaningful. Every key reorders visibly and the
choice applies per workspace; manual order disables direction switching.
Carried gap: reload resets to creation date (in-memory per-workspace store, no
persistence layer). Observed option keys include name, creation date and
member count in both directions plus the manual key. Oracle:
`projects-list/sort-filter-search.spec.ts`.

SHELL-029 — The filter panel narrows the already-fetched projects by
ownership, visibility, leads, members and creation date, with a value search
over filter values. Lead plus date selections combine; a badge signals active
filters. Creation-date selection is single-select (picking one replaces the
prior, including custom ranges). Intended (NEWFRONT-172): the panel search
matches case-insensitively. Filter persistence goes through `PATCH
/api/workspaces/{slug}/user-properties/` (owned by NEWFRONT-276). Oracle:
`projects-list/sort-filter-search.spec.ts`.

SHELL-030 — An applied-filter strip above the grid shows removable pills with
a match count and one-click clearing: removing one chip updates the count,
clear-all restores the list while preserving the archive toggle.
Client-side only. Oracle: `projects-list/sort-filter-search.spec.ts`.

SHELL-031 — List search expands from a magnifier icon, filters live over the
loaded projects, and dismisses by escape or outside click: typing narrows
live, first escape clears text, second collapses, outside click collapses only
when empty, and a clear button resets and collapses. Client search query in
the filter store; no network call. Oracle:
`projects-list/sort-filter-search.spec.ts`.

SHELL-033 — Card clicks route members into the work-items view and intercept
outsiders toward joining; archived cards never navigate. A member lands on the
issues view; an outsider gets the join dialog (SHELL-035); the archived branch
does nothing. No call until join is confirmed. Oracle:
`projects-list/cards.spec.ts`.

SHELL-035 — The non-member join flow confirms, shows a joining state, then
lands inside the project: confirming routes into the issues view and the card
flips to member state. Endpoint: `POST
/api/users/me/workspaces/{slug}/projects/invitations/` with a project id list
(`project_ids` array). Public joining follows the network setting. Oracle:
`projects-list/join-leave.spec.ts`.

SHELL-036 — The leave flow demands two typed confirmations (exact project
name plus an exact confirmation phrase) and warns about losing assigned work.
Wrong entries block with corrective errors; correct entries remove membership
and route back to the list. Endpoint: `POST
/api/workspaces/{slug}/projects/{id}/members/leave/` (no body). Surfaced from
settings and detail areas as well as the list. Oracle:
`projects-list/join-leave.spec.ts`.

SHELL-037 — Archiving hides a project from search and lists with a warning;
restoring returns it. The archived project leaves the default list and appears
under archive view; restore returns it; both toast and return to the list.
Endpoints: `POST` and `DELETE
/api/workspaces/{slug}/projects/{id}/archive/`. Oracle:
`projects-list/archive-restore-delete.spec.ts`.

SHELL-038 — Archived cards collapse to a muted state with inline restore and
delete affordances for project admins; other roles see the muted card only.
Archive endpoints plus project delete (below). Oracle:
`projects-list/archive-restore-delete.spec.ts`.

SHELL-039 — Deletion is destructive, double-gated, and reachable only from
archived state. A wrong name or phrase keeps the destructive button disabled;
correct entries remove the project with a success notice. Endpoint: `DELETE
/api/workspaces/{slug}/projects/{id}/`. Oracle:
`projects-list/archive-restore-delete.spec.ts`.

SHELL-043 — The desktop list header combines breadcrumbs, search, sort and
filter with a responsive creation button. Guests see no create button;
archive view gains an archived crumb and hides the button; the button label
shortens on small screens. Navigation plus modal toggle only. Oracle:
`projects-list/headers-breadcrumb.spec.ts`.

SHELL-044 — The mobile list header splits sorting and filtering into a
touch-adapted two-pane bar: phone widths hide the desktop filter row and show
the mobile bar with working sort and filter through the same selection.
Source lives in the cloud tree but renders in the OSS oracle build below the
medium breakpoint, so it is provable. No API. Oracle:
`projects-list/headers-breadcrumb.spec.ts`.

SHELL-045 — The breadcrumb primitive truncates with icon support and
link-versus-text modes: long labels truncate with full text on hover outside
mobile, and the terminal crumb is plain text, not a link. Carried gap: no
long-label crumb exists on the list surface (Projects and Archived are short),
so truncation plus hover reveal are unprovable here; the terminal-crumb rule
is covered. No API. Oracle:
`projects-list/headers-breadcrumb.spec.ts`.

## 4. Sidebar (NEWFRONT-285, NEWFRONT-286, NEWFRONT-287)

SHELL-047 — The sidebar header region offers a quick-create entry that starts
a work item. Guests see nothing usable and the entry stays disabled with zero
joined projects (nowhere to put the item) or when the role forbids it; an
eligible member opens the creation dialog backed by a local draft. Seam (see §10): the quick-create dialog is folded into the SHELL-086 command-registry
decision — one shared work-item creation entry point used by both the sidebar
button and the palette command. Oracle:
`sidebar-nav/shell-047-sidebar-quick-create.spec.ts`.

SHELL-048 — The sidebar Projects group bundles drafts, an aggregate
work-items row and joined projects under a collapsing disclosure that
auto-opens on related routes. Visiting a drafts or views URL expands the group
with the active row visible; collapsing survives client-side navigation while
a full load re-opens the group. Inputs: project stores, personal and project
navigation preferences, a local open flag. Seam (see §10): the shell builds
the group shell plus the drafts-row entry point and the aggregate row that
navigates to workspace-view routes; drafts contents and the views themselves
belong to the drafts and views areas. Oracle:
`sidebar-nav/shell-048-projects-group.spec.ts`.

SHELL-049 — While projects load, the group shows placeholder rows; a fresh
workspace offers a direct create action (admin and member) instead of an empty
list. Driven by the project loader init state. Oracle:
`sidebar-nav/shell-049-project-placeholders.spec.ts`.

SHELL-050 — Sidebar project rows link to the preferred default tab, expand
inline sub-navigation, scroll the active project into view and offer a quick
menu. Clicking lands on the configured default tab (URL builder over
per-project tab preferences); the URL project highlights and scrolls into
view; copy-link confirms a working deep link. Row audience: joined projects,
with publish admin-only, archives admin and member, and leave for users
without project standing. Oracle:
`sidebar-nav/shell-050-project-rows.spec.ts`.

SHELL-051 — Project rows and navigation items drag-reorder with failure
notices and persistence: the new order survives reload, and failed saves
toast an error and keep the old order. Endpoints: project-view order update
and workspace item sort-order update (exact shapes owned by NEWFRONT-277).
Oracle: `sidebar-nav/shell-051-drag-reorder.spec.ts`.

SHELL-052 — An overflow slide-over holds searchable extra projects with
create, copy-link, reorder and an empty-search state. Search filters by name
or short code; no match shows empty art; outside click or sidebar collapse
closes the panel. Browsing is open to all; create stays admin and member.
Inputs: rail open flag, project store, clipboard. Oracle:
`sidebar-nav/shell-052-overflow-slideover.spec.ts`.

SHELL-053 — A workspace slide-over lists extra destinations that reorder and
pin into the main sidebar. Pin and order writes persist on the server
(navigation-links constant; pin plus sort-order preferences via the
sidebar-preferences endpoints, NEWFRONT-277). Per-item permissions apply; the
pin control is cloud. Carried state: the More destination set keeps its
membership across reloads. Oracle:
`sidebar-nav/shell-053-workspace-slideover.spec.ts`.

SHELL-054 — The secondary-destinations disclosure (runners, prompts,
schedulers, dev-machines, analytics, archives routes) auto-opens on their
routes and persists: visiting a member route expands the group with the
matching row active, and toggle state survives reload (static and dynamic
navigation constants plus a local open flag). Per-item permissions apply.
Oracle: `sidebar-nav/shell-054-more-disclosure.spec.ts`.

SHELL-055 — Project sub-navigation exposes work tracking, cycles, modules,
views, pages, intake with a pending count, schedulers and a project workers
panel, hiding unavailable or unauthorized entries. Disabled feature flags hide
entries; the intake count appears only when nonzero; guests see tracking,
views, pages, intake and schedulers while cycles, modules and workers are
admin and member only. Seam (see §10): the shell renders entries from project
feature flags and per-tab permission checks; each target area owns its
destination route and the intake area owns the pending-count semantics. Gap:
the guest path runs live once NEWFRONT-151 lands; currently degraded to a
membership assertion. Oracle:
`sidebar-nav/shell-055-project-subnav.spec.ts`.

SHELL-056 — The Favorites section supports folders with validated create and
rename, drag organization, deep links and removal, hiding entirely when empty.
Admin and member only; guests excluded. Empty names are blocked while
whitespace-only or duplicate names toast notices; drags persist; removals
confirm; each entry deep-links to its target. Endpoints: the favorite store on
`.../user-favorites/` (add, update, list and grouped reads, moves, ordering;
exact shapes owned by NEWFRONT-277), plus favorite link constants and a local
open flag. Oracle: `sidebar-nav/shell-056-favorites.spec.ts`.

SHELL-057 — Favorite entries resolve icons and links by entity type with a
safe fallback for unknown types: project, page and cycle favorites land
correctly, and an unrecognized type lands on the workspace root without
crashing. Known bug (NEWFRONT-144): unknown types currently crash the
sidebar; the scenario records the intended fallback and the fix lands
separately. Oracle: `sidebar-nav/shell-057-favorite-links.spec.ts`.

SHELL-109 — Favorite entries nest into folders on folder-target drops and
leave them on drag-out, with both moves persisting (found by the
NEWFRONT-286 review; SHELL-056 proves sibling reorder only). Admin and
member only; guests excluded. Dropping a loose entry onto a closed
folder nests it: the move writes the folder as the entry's parent and
the grouped read gains the entry through reloads. Dragging a nested
entry above a top-level row returns it to the top list: the move clears
the parent and the top list gains the entry through reloads. Folders
themselves never nest. Writes go through the favorite update endpoint
(`parent` PATCH on `.../user-favorites/`); reads split between the flat
top-level list and the per-folder grouped read. Oracle:
`sidebar-nav/shell-109-favorite-folder-drops.spec.ts`.

SHELL-058 — The workspace switcher lists workspaces with role and member
counts, marks the active one, and offers settings, invite, creation, invites
inbox and sign-out shortcuts. Listing is open to all; the settings shortcut is
member-plus and invite admin-only. Switching navigates and remembers the last
workspace (profile update); disabled creation hides its row; failed sign-out
toasts. Inputs: workspace store, profile update for last workspace, sign-out,
instance creation-disabled flag; a cloud subscription pill may accompany the
list. Oracle: `sidebar-nav/shell-058-workspace-switcher.spec.ts`.

SHELL-060 — The help menu links docs, support contact and forum plus an
in-app shortcut list, product updates and the version number (version readout
cloud). External entries open in a new tab; the shortcut and updates dialogs
open in place. The palette shortcut-list toggle drives the dialog. Oracle:
`sidebar-nav/shell-060-help-menu.spec.ts`.

SHELL-061 — The workspace logo shows the uploaded image or the workspace
initial with accessible labeling: workspaces without a logo show their
initial; logo uploads render the image (file-URL helper). Oracle:
`sidebar-nav/shell-061-workspace-logo.spec.ts`.

SHELL-062 — The sidebar hides itself on the notifications route (full-width
rendering, no app sidebar) and collapses on narrow screens after navigation:
tapping any sidebar link under 768px closes the sidebar. Inputs: theme
collapse state, sidebar-width local key. Oracle:
`sidebar-nav/shell-062-sidebar-autohide.spec.ts`.

## 5. App rail, top bar, edition chrome (NEWFRONT-289)

SHELL-063 — The far-left app rail offers icon-only or icon-plus-caption
density with docking control and right-click options. Density changes width
and captions and survives reload; undocking removes the strip until re-docked;
context choices apply immediately. Density plus per-workspace visibility
persist in local storage; no network calls. Gap: density, docking and context
options need a rail-enabled build (seeded OSS build: suppressed only).
Oracle: `chrome-tabs/rail-suppressed.spec.ts`.

SHELL-064 — The rail settings entry stays pinned below dock entries and
highlights only on workspace settings routes: it does not highlight inside
projects. Route-based active state only. Gap: pinning and highlight need a
rail-enabled build (no rail entry mounts in seed). Oracle:
`chrome-tabs/rail-suppressed.spec.ts`.

SHELL-065 — The cloud top bar composes the workspace menu, sidebar toggle,
command search, inbox, help and an account fallback. The inbox dot appears
only with a positive unread count; the account control appears only when the
sidebar is unmounted or collapsed. The bar also carries a repository star
link. Seam (see §10): the shell reads the unread count and renders the dot;
the notifications area owns fetching semantics, fixtures and inbox behavior.
Unread-count read: `GET
/api/workspaces/{slug}/users/notifications/unread/` via the workspace
notification store (list, read, mark-all-read and archive endpoints owned by
the notifications area). Gaps: the positive-dot case needs a notification
fixture; the collapsed-sidebar fallback is unproven. Oracle:
`chrome-tabs/top-bar.spec.ts`.

SHELL-066 — The sidebar toggle button is desktop-gated and clears peek state:
hidden where suppressed; clicking flips collapse and clears peek; its hint
reflects the collapsed state. Inputs: theme collapsed and peek flags in local
storage. Gap: peek-clearing and hint text unproven (flip covered). Oracle:
`chrome-tabs/top-bar.spec.ts`.

SHELL-099 — The cloud top frame pairs the top navigation bar with a
contextually hidden app rail: rail-suppressed pages render without the rail
and reclaim its padding, all other pages keep it. Rail-visibility provider,
no API. Gap: railed pages need a rail-enabled build. Oracle:
`chrome-tabs/rail-suppressed.spec.ts` plus `edition-chrome.spec.ts`.

SHELL-100 — Negative row: no edition badge mounts in shell chrome, and there
is no version tooltip or paid-plans dialog entry. No badge renders in any
build (target per main#572; the cloud overlay file is a dead orphan with no
importer, so the cloud build is likewise badge-less). No API. Oracle:
`chrome-tabs/edition-chrome.spec.ts`.

SHELL-101 — An upgrade pill marks paywalled destinations without affecting
navigation: marked rows display the pill in both size variants and still
navigate normally. Informational, no API. Gap: small-variant sidebar rows dead
or owned elsewhere in seed (header mount covered). Oracle:
`chrome-tabs/edition-chrome.spec.ts`.

SHELL-102 — The desktop-only update control lives in the sidebar chrome:
desktop builds show the control in the sidebar; its update behavior is rowed
with the desktop runtime (Desktop area). No API in the shell. Gap: updater
states need a desktop build. Oracle: `chrome-tabs/edition-chrome.spec.ts`.

SHELL-103 — The repository-star action adapts its icon to the color theme and
opens externally: the icon swaps between light and dark themes, and
activation opens the repository in a new tab with safe link attributes.
Cloud top navigation bar, external repository address only. Oracle:
`palette-search/repo-star.spec.ts`.

## 6. Sidebar shell and personalization (NEWFRONT-290)

SHELL-067 — The collapsible sidebar shell carries a title header, quick
actions, a scroll area and a bottom account area with a single-account rule:
expanding and collapsing animates, and the account area renders only when
fully visible so exactly one account menu exists. Input: collapsed flag from
the theme store. Gap: animation timing unproven (open/closed states and the
single-account rule covered). Oracle: `chrome-tabs/sidebar-shell.spec.ts`.

SHELL-068 — The sidebar resizes by drag with double-click collapse and hover
peek: drags clamp between minimum and maximum, double-click toggles, and
hovering the edge trigger after collapse shows a temporary overlay that
auto-dismisses. Width and collapse callbacks to the parent; no server call.
Gap: no hover-peek trigger mounts after collapse in seed — the scenario proves
collapse holds under hover, and peek UI needs a build that mounts the
trigger. Oracle: `chrome-tabs/sidebar-shell.spec.ts`.

SHELL-069 — On small screens the sidebar floats above content and collapses on
outside interaction: below the breakpoint an open sidebar auto-collapses and
tapping outside collapses it; touch layouts float it above content. Inputs:
window-size hook plus outside-click detector. Gap: touch input and overlay
z-order unproven (narrow-viewport auto-collapse and outside-tap collapse
covered). Oracle: `chrome-tabs/sidebar-shell.spec.ts`.

SHELL-071 — Project-list display preferences switch accordion versus tabbed
rendering and cap the listed count: the mode choice changes how feature
entries render under projects, enabling the cap with a count limits listed
projects, the count input strips non-digits and enforces a minimum of one,
and zero means uncapped. Endpoint: `GET/PATCH
/api/workspaces/{slug}/user-properties/` carrying layout mode and project
limit (exact shapes owned by NEWFRONT-277). Oracle:
`chrome-tabs/personalization.spec.ts`.

SHELL-098 — Sidebar collapse and peek states persist locally per panel and
restore on load: collapsed sidebars return collapsed after reload. Local keys
only (main, extended, project, profile, analytics and detail sidebars). Gap:
peek-state persistence unproven (nothing sets peek in seed). Oracle:
`chrome-tabs/sidebar-shell.spec.ts`.

## 7. Project switcher, header, tab strip (NEWFRONT-291)

SHELL-072 — The project switcher dropdown lists joined projects and navigates
to the picked project's stored default tab. The dropdown shows icon plus name
for joined projects only; picking navigates to that project's default tab; it
renders nothing when current project data is missing. Joined-project id list
plus partial project cache, client-side. Gap: joined-only exclusion
unprovable in seed (guests cannot create projects, API 403) and the
missing-data render unproven. Oracle: `chrome-tabs/project-tabs.spec.ts`.

SHELL-073 — The project header button truncates long names with full text on
hover, beside a role-gated quick-actions menu: publish for project admins,
archives for admin and member, leave for users without project standing,
settings for all. Each role sees exactly its permitted entries and each entry
navigates or dialogs. Gap: per-role entry variation unproven in seed (the
seeded owner sees every entry; navigation and dialog per entry covered).
Oracle: `chrome-tabs/project-tabs.spec.ts`.

SHELL-074 — Copy-current-location copies the active tab route with a fallback
and distinct notices: it copies the active tab route, falling back to the
default tracking route, with success and failure toasted differently.
Clipboard write with toast feedback. Oracle: `chrome-tabs/project-tabs.spec.ts`.

SHELL-075 — The horizontal project tab strip renders per-tab role and feature
gating in a fixed order. Guests see tracking, views, pages, intake and
schedulers while cycles and modules are member-plus; flag-hidden or
role-lacking tabs never render and the remainder sorts in fixed order.
Inputs: project feature flags plus per-tab permission checks (item list
cloud, strip core). Gap: guest role-gating unproven in the seeded single-user
stack (flag gating and fixed order covered). Oracle:
`chrome-tabs/tab-strip.spec.ts`.

SHELL-076 — Active-tab indication survives nesting with work-item and epic
special cases: nested routes keep the parent highlighted, a plain work item
highlights tracking, an epic highlights its epic entry. Inputs: route path
plus work-item detail state. Gap: the epic special case is uncovered — epic
status derives from the issue type, not creatable via the plain create API.
Oracle: `chrome-tabs/tab-strip.spec.ts`.

SHELL-077 — Bare project URLs redirect to the stored default tab with a safe
fallback: the address rewrites to the stored default tab when present, else
the standard tracking tab, and unknown stored keys fall back. Endpoint:
`GET/PATCH /api/workspaces/{slug}/projects/{id}/user-properties/` via the
member store. Known bug (NEWFRONT-141): the bare URL renders not-found with
nothing active; the scenario asserts actual behavior and the redirect lands
separately. Oracle: `chrome-tabs/tab-strip.spec.ts`.

SHELL-078 — Responsive tab overflow pins the active tab and pools user-hidden
tabs with restore controls: shrinking moves trailing tabs into the overflow
menu, the active tab swaps into view, at least one tab stays visible, and
hidden tabs join the same menu with restore. Container measurements only.
Known bug (NEWFRONT-142): measurement never recomputes and widths clip
without pooling; the scenario asserts actual behavior and pooling lands
separately. Oracle: `chrome-tabs/tab-strip.spec.ts`.

SHELL-079 — Per-tab default and hide preferences persist per member with
optimistic revert and notices: setting a default changes future landings and
marks the tab, clearing reverts, hiding moves the tab to overflow with
restore, and failures revert and toast. Same project user-properties
endpoint. Gap: failure revert-and-toast unproven (needs failure injection).
Oracle: `chrome-tabs/tab-strip.spec.ts`.

## 8. Command palette and search (NEWFRONT-292, NEWFRONT-293)

SHELL-080 — The command palette opens from anywhere on a single modified
chord, even while typing: the chord summons the focused palette on any
workspace page including inside text fields. No route, parameter or slash
press opens it. No API for open. Oracle:
`palette-search/palette-open-and-reset.spec.ts`.

SHELL-081 — The top-bar search box embeds the palette with commands plus
grouped server results: focus expands the field and opens results, typing
filters commands and shows grouped hits, and a repeated chord or outside click
closes. Mounted by the cloud top navigation; result-path grouping helpers are
shared. Endpoint: `GET /api/workspaces/{slug}/search/`. Oracle:
`palette-search/palette-topbar-search.spec.ts`.

SHELL-082 — The centered modal palette mounts from layout providers and
resets to a fresh state on close: backdrop click closes, and reopening shows a
cleared query, the top page and no stale grouping. Openable by chord, single
keys and two-key sequences inside covered layouts. No API for open. Oracle:
`palette-search/palette-open-and-reset.spec.ts`.

SHELL-083 — In-palette keyboard flow moves, activates, clears-then-closes and
steps back from sub-pages: arrows move highlight, enter activates, escape
clears first then closes (modal) or closes at once (top bar), and backspace on
an empty query steps back or hides the group. No API. Oracle:
`palette-search/palette-keyboard-and-blank.spec.ts`.

SHELL-084 — Server search waits for an idle pause (about half a second) and
scopes to the project unless widened to the workspace, with an in-flight
pulse: one request fires per pause, inside a project only its hits return
until the footer scope toggle flips, and a heading pulses mid-flight.
Endpoint: `GET /api/workspaces/{slug}/search/` with search text, optional
project id (`project_id`) and a workspace-search flag (`workspace_search`;
project scoping forces it false). Oracle:
`palette-search/palette-server-search.spec.ts`.

SHELL-085 — Blank queries show commands only with zero network calls and hide
the results section: clearing restores grouped commands filtered by substring
over titles plus alternate keywords, and no request fires. No API when blank.
Oracle: `palette-search/palette-keyboard-and-blank.spec.ts`.

SHELL-086 — Palette creation entries open scoped creation dialogs with their
own global sequences: the work-item entry pre-scopes to the current cycle or
module, and workspace creation routes to its dedicated page instead of a
dialog. Entries are gated by create rights (project creation needs
workspace-level membership). Dialogs submit later through their own flows; the
palette itself makes no calls. Seam (see §10): the shell builds the command
registry plus the entry points and sequences; each area registers its
creation commands and owns its dialog. Oracle:
`palette-search/palette-create.spec.ts`.

SHELL-087 — Palette navigation entries jump directly or through filterable
entity pickers: direct jumps land home, inbox, personal list, project list,
filtered views, analytics, drafts, archives and settings, while picker entries
list entities and navigate on pick. Gated per destination; client-side routing
only. Result-group order is contextual, create, navigation, general, settings,
account, miscellaneous, preferences, help. Oracle:
`palette-search/palette-navigation.spec.ts`.

SHELL-088 — Palette preference entries change theme, timezone, first day of
week and language with persistence: theme applies immediately, persists and
reloads the app; the others persist with confirmation toasts and error toasts
on failure. Endpoints: user and profile update calls (theme and language via
`PATCH /api/users/me/profile/`; timezone via `PATCH /api/users/me/` — see
SHELL-095–097). Oracle: `palette-search/preferences-palette.spec.ts`.

SHELL-089 — Palette account, utility and help entries cover invites, sign-out,
sidebar toggle, copy-URL, focus-search, the shortcut viewer and external help
links. Copy-URL lands on the clipboard with a toast; sign-out failure toasts
and retains the session; external links leave the app. No API in the entries
(sign-out delegates to the auth service). Oracle:
`palette-search/palette-account-help.spec.ts`.

SHELL-092 — The palette hides unauthorized entries entirely instead of
disabling them: viewers see no creation or edit-gated entries, disabled
features remove their entries, and globally disabled workspace creation
removes that entry. Inputs: workspace and project permission checks,
per-setting access lists, feature flags, instance creation flag. Oracle:
`palette-search/palette-unauthorized.spec.ts`.

SHELL-093 — Palette pickers show spinners and plain empty lines, search
failures render as empty with no error UI, and no history section ever
appears: slow stores spin, empty lists show a plain line, a no-hits row resets
the query, an unreachable backend yields the no-hits row and never an error,
and typing never shows recents. Endpoint: the search endpoint; store loading
flags. Oracle: `palette-search/palette-picker-states.spec.ts`.

SHELL-094 — The shortcut reference dialog lists every bound command grouped by
family with filtering and platform-adapted glyphs: filtering by a command name
leaves only its row plus binding, and contextual commands split per entity
family. Openable from the help command, its own chord, or the sidebar help
menu. No API. Oracle: `palette-search/shortcuts-dialog.spec.ts`.

## 9. Contextual palette, appearance, shared kit (NEWFRONT-294)

SHELL-090 — The work-item contextual palette group edits state, priority,
assignee, estimate, cycle, module and labels with multi-change pickers, plus
toggles, delete and copy actions. Picks apply on selection with assignee,
label and module pickers staying open; a chip names the item and dismisses;
delete confirms then redirects to the list. Audience: project members and
admins, never on archived items. Seam (see §10): the shell builds the group
shell, pickers chrome and chip; the issues area owns the work-item update,
subscription, cycle and module endpoints rowed there. Oracle:
`palette-search/palette-work-item-contextual.spec.ts`.

SHELL-091 — Cycle, module and page contextual palette entries follow
per-entity capabilities instead of a global role: cycle pages offer favorite
toggle and copy-URL; module pages offer gated member and status management;
pages offer lock, visibility, archive, favorite and copy entries only where
allowed (edit gates, archive blocks, page lock and visibility flags). Seam
(see §10): entity endpoints are rowed in their own areas; the shell builds
only the contextual entries shell. Oracle:
`palette-search/palette-entity-contextual.spec.ts`.

SHELL-095 — Appearance preferences offer system-following, light and dark, a
high-contrast pair and custom palettes with immediate shell-wide re-skin. The
system choice tracks OS light and dark flips; every option re-skins
immediately including notices; the choice survives reload; custom reveals a
palette designer. Endpoint: `GET/PATCH /api/users/me/profile/` for the theme
payload plus a local theme key; a one-time server-to-client sync plus a
custom-style applier at the app root (the cloud preferences switcher reloads
after save). Oracle:
`palette-search/appearance-language-timezone.spec.ts`.

SHELL-096 — Language switch re-renders the entire shell and persists on the
server and locally, with English-only in the open-source build: header,
filters, cards and dialogs translate immediately and the choice survives
reload; fuller builds expand the same picker beyond English. Endpoint:
`GET/PATCH /api/users/me/profile/` for language plus a local language key,
document language and cleared message cache. Oracle:
`palette-search/appearance-language-timezone.spec.ts`.

SHELL-097 — The timezone picker searches the full zone list and persists to
the user account: picking a zone persists across reload with success or
failure notices and calendars reflect it. Endpoints: `GET/PATCH
/api/users/me/` for the timezone field and the zone list `GET /api/timezones/`.
Oracle: `palette-search/appearance-language-timezone.spec.ts`.

SHELL-104 — The shared empty-state kit offers four presentation tiers with
theme-aware art: centered illustration, wide marketing, text-first detailed
and compact section variants, each rendering with and without actions.
Illustration paths resolve by category, type and color scheme with mandatory
light and dark variants. No API. Oracle:
`palette-search/shared-empty-states.spec.ts`.

## 10. Cross-epic seams

Each seam below is drawn so the shell rows are provable at gate time without
depending on Backlog epics. The rule in every case: the shell slice builds the
chrome plus the entry point, and the owning epic registers content or owns the
mutation endpoints.

SHELL-007 assistant card (assistant view owned by NEWFRONT-75). The shell
slice (NEWFRONT-278) builds the card shell: placement between greeting and
widgets, suggested-prompt chips, the recent-threads list read, the setup
reminder for unconfigured members, and guest suppression. Thread links
navigate to the assistant routes by thread id; every thread behavior past the
link — thread creation, chat internals, model setup — belongs to the
assistant epic. Gate-provable with stub thread rows.

SHELL-016 work-item preview (peek owned by NEWFRONT-71). The shell slice
(NEWFRONT-280) builds a shared peek side panel in the shared kit: the panel
shell with the title rendered as an editable field, opened from recent rows.
The issues epic reuses that panel for full peek and owns issue-data fetching
semantics; the shell reads through the issues area hooks and never duplicates
them.

SHELL-048 drafts and aggregate rows (drafts owned by the drafts area,
workspace views by the views area). The shell slice (NEWFRONT-285) builds the
Projects group shell: collapse behavior, the drafts-row entry point gated by
personal preference, and the aggregate work-items row that navigates to the
workspace-view routes. Drafts contents and the views themselves are owned
elsewhere; the group stays gate-provable with stub destinations.

SHELL-055 subnav into other areas (cycles, modules, views, pages, intake,
schedulers, workers). The shell slice (NEWFRONT-286) renders the entries from
project feature flags and per-tab permission checks using static route
constants; each target area owns its destination route, and the intake area
owns the pending-count semantics (the shell displays the count only when
nonzero). Unavailable or unauthorized entries stay hidden on the shell side,
so the gate passes regardless of target-area readiness.

SHELL-086 palette creation entries (other areas' dialogs). The shell slice
(NEWFRONT-293) builds the command registry plus the creation entry points
with their global two-key sequences; each area registers its own creation
commands and owns its dialog and submit flow. SHELL-047's sidebar quick-create
button is folded into the same decision: one shared work-item creation entry
point used by both the sidebar button and the palette command, with the same
disabled rules (guest, zero joined projects). Workspace creation routes to
its dedicated page from either entry.

SHELL-090 and SHELL-091 contextual edits (endpoints rowed in their own
areas). The shell slice (NEWFRONT-294) builds the contextual group shell:
context detection from route params, the picker chrome that stays open across
multi-change picks, the naming chip with dismiss, and the delete-confirm plus
redirect. All mutation endpoints — work-item update, subscription, cycle,
module, label, member, status, page lock and visibility — are rowed and owned
in the issues, cycles, modules, views and pages areas. The palette never calls
entity endpoints directly, so the shell gate passes with stub mutations.

SHELL-065 notification unread count (notifications area). The shell slice
(NEWFRONT-289) renders the inbox bell and the dot-from-positive-count rule off
a single unread-count read (`GET
/api/workspaces/{slug}/users/notifications/unread/`). Fetching semantics,
fixtures, the positive-dot case and all inbox behavior belong to the
notifications area.

## 11. Target-behavior rows (NEWFRONT-156 restatement)

These ten rows were amended to match merged main PRs while the live old app
still shows pre-PR behavior. Slices implement the target text below; oracle
scenarios are aligned at gate time.

SHELL-017 — Recent rows show relative visit time plus type context, degrading
to a generic icon on missing data. Each row shows age plus status, priority,
assignee or owner and project identifiers as fitting; missing data never
breaks the row. Target per main#894: the priority tooltip shows the display
label, never the raw lowercase enum value. Oracle: `home/recents.spec.ts`.

SHELL-032 — Project cards show identity, visibility, description fallback,
member stack and a favorite star. Logo, name plus short code, private mark,
description or generated creation line, avatar stack with count, star (star
renders for workspace admin and member only). Target per main#523: no cover
slot renders on any card. Starring persists and first add opens the favorites
section. Endpoints: `GET/POST/DELETE
/api/workspaces/{slug}/user-favorite-projects/`. Oracle:
`projects-list/cards.spec.ts`.

SHELL-034 — Card quick actions and right-click menu stay strictly contextual:
settings for project admin and member; join and open-in-tab for non-members;
restore and delete for project admins on archived cards; copy-link for all on
active cards. Each entry performs its navigation or dialog. Target per
main#523: quick actions mount on the header row, never on a cover. Oracle:
`projects-list/cards.spec.ts`.

SHELL-040 — Create flow captures identity, visibility, lead and icon with an
auto-derived short code and targeted duplicate errors: name and short code
are validated, the code derives from the name until manually edited, and a
reused name or code yields a field-specific error. Target per main#523: the
form offers no cover control, and creation is a single `POST
/api/workspaces/{slug}/projects/` with no cover upload, no asset confirmation
and no follow-up patch. The identifier-availability read (`GET
/api/workspaces/{slug}/project-identifiers?name=`) is a supporting check.
Oracle: `projects-list/create.spec.ts`.

SHELL-041 — No random cover default: new projects start with an emoji icon,
browser timezone, public visibility and work-items-only features. Target per
main#523: the fresh form shows a prefilled icon and timezone with no cover
value sent or rendered, and the submitted project has cycles, modules, views,
pages and intake disabled. Client defaults sent inside the create post.
Oracle: `projects-list/create.spec.ts`.

SHELL-042 — Negative row: no cover upload exists on the creation path, so
there is no cover-failure mode. Creation submits with no cover step and no
asset-backend dependency; no cover warning can appear. Target per main#523:
`POST /api/workspaces/{slug}/projects/` only. Oracle:
`projects-list/create.spec.ts`.

SHELL-046 — Sidebar rows are permission-gated, highlight from the URL, and
hide unless pinned or built in. Guests never see member-only rows; the
current-URL row reads active; unpinned non-built-in rows stay hidden. Inputs:
workspace-level permission check, per-workspace pin preferences, user record.
Target per main#672: there is no "Your work" sidebar row — it lives in the
user menu popup (SHELL-059). Oracle:
`sidebar-nav/shell-046-sidebar-row-gating.spec.ts`.

SHELL-059 — The user menu shows identity with avatar (no cover), a Your-work
entry, a settings-page entry and sign-out, in sidebar and compact top-bar
variants, with no community plan entry. Targets: the menu shows a plain
avatar-plus-name-plus-email block with no cover art or scrim and truncated
text lines (main#523, main#574); a Your-work entry with its icon sits above
Settings in both variants for admins and members, hidden from guests, and
navigates to the current user's profile, closing the menu (main#672); the
Settings entry navigates to the settings page general tab and no dialog opens
(main#573); no Community entry exists and no plan dialog opens from the menu
(main#572); the compact variant never disturbs sidebar peek state. Inputs:
user store and sign-out. Oracle:
`sidebar-nav/shell-059-user-menu.spec.ts`.

SHELL-070 — The sidebar personalization dialog toggles and reorders personal
entries with server persistence. Toggling hides or shows entries in the
sidebar; drag-reordering changes sidebar order; both survive reload.
Endpoints: `GET/PATCH /api/workspaces/{slug}/sidebar-preferences/` (pin plus
sort order per key per workspace; bulk writes send key, pinned and order
triples). Target per main#672: the personal group lists Drafts only — Your
work is no longer pinnable or reorderable, and a stored preference key for it
is ignored with no migration. Known bug (NEWFRONT-137): the bulk patch is
unscoped, newest-wins; the scenario asserts the actual UI-only behavior and
per-member persistence lands separately. Oracle:
`chrome-tabs/personalization.spec.ts`.

SHELL-105 — Negative row: no cover-image primitive exists anywhere in the new
app. No cover slot, shimmer placeholder, remote-URL cover or fallback art
renders on cards or detail headers. Target per main#523. No API. Oracle:
`palette-search/shared-cover-image.spec.ts`.

## 12. F-06 reuse map (NEWFRONT-17 survey)

NEWFRONT-17 built a working foundation in `apps/web_new`: a title bar with
brand, workspace crumb, palette trigger and user block; a sidebar with
workspace switcher, project rows and footer user plus sign-out; a command
palette with the command registry (`shared/commands`) and per-screen
registration; toasts; sign-in; and a read-only issue list. Per slice, what
exists versus what must be added or reworked:

- Route shells, browse, document, not-found (SHELL-001–002, 106–108):
  reuse the root query provider, workspace route gate with prefetch, the
  issue-list retry panel and unknown-project pointer; add the browse and
  document shells, a real illustrated not-found page, and later the
  god-mode and spaces subtrees.
- Home top (SHELL-003–010): reuse the workspace route gate and title-bar
  crumb; the current home is a forward-to-first-project placeholder, so the
  dashboard, greeting, tour, assistant card and quickstart are all new.
- Quicklinks plus widget management (SHELL-011–013, 018–020): reuse the
  query and prefetch pattern and the content slot; the link model, widget
  grid and manage dialog are new.
- Recents plus dashboard chrome (SHELL-014–017, 021–023): reuse
  workspace-scoped query keys, cache invalidation and the empty-state
  pattern; the recents query, dashboard layout, greeting clock, skeletons
  and shared peek panel are new.
- Projects list shell (SHELL-024–027, 043–045): reuse the projects query
  options, sidebar project rows and loader prefetch; the list screen with
  status and archive grouping, row-action shell, desktop and mobile headers
  and breadcrumb assembly are new.
- List sort, filter, search (SHELL-028–031): reuse the validated search-param
  pattern from the issues route; sort, filter and search params, the filter
  bar UI and key-factory extensions are new (the issues query currently
  hardcodes paging and ordering).
- Cards plus join and leave (SHELL-032–036): reuse the permission hook and
  project list query; the card layout, join and leave mutations with
  optimistic helpers, and member-count display are new.
- Archive, restore, delete plus create (SHELL-037–042): reuse the toaster
  and the mutation-plus-invalidation pattern; the create dialog, the three
  mutations, confirm dialogs and palette create commands are new.
- Sidebar rows, quick-create, projects group (SHELL-046–051): reuse the
  sidebar rows, menu and workspace switching; richer row states, counts,
  sections, the quick-create entry, drag reorder and the collapsible group
  are new or reworked.
- Slide-overs, subnav, favorites (SHELL-052–057): reuse dialog usage and
  cross-app links for navigation; the slide-over primitive, subnav
  component, favorites store and query plus sidebar section are new.
- Switcher, user and help menus, logo, autohide (SHELL-058–062): reuse the
  switcher menu, brand block and kit menu, button and avatar primitives;
  logo assets, full menu behaviors with workspace search and keyboard
  support, and collapse persistence are new.
- Rail, top bar, edition chrome (SHELL-063–066, 099–102): reuse the title
  bar, flex-column shell and the edition extension interface; the rail
  component and top-bar extension points wired to edition flags are new
  (the OSS edition is empty, so all edition rendering is new).
- Sidebar shell plus personalization (SHELL-067–071, 098): reuse the
  presentational sidebar shell and the store-reset pattern; collapsible
  sections, density and order preferences with persistence, and the
  personalization store are new.
- Project switcher, header, tab strip (SHELL-072–079): reuse the
  active-project derivation, the issues header and the title-bar section
  slot; the switcher dropdown, header actions, tab bar and route wiring are
  new (only the single issues route exists).
- Palette core plus server search (SHELL-080–085, 093): reuse the palette
  filter, grouping, activation and empty state, the open and query store,
  the modified-chord wiring and per-screen registration; fuzzy ranking,
  async sources and the preview pane are new (the current filter is
  substring over title, hint and section).
- Palette entries plus shortcuts dialog (SHELL-086–089, 092, 094): reuse the
  command shape, shortcut matching, key-cap rendering and the global
  sign-out and go-home commands; the per-screen entry catalogs, chord and
  sequence support and shortcut customization are new (three commands
  registered today).
- Palette contextual plus appearance, preferences and shared kit
  (SHELL-090–091, 095–097, 103–105): reuse kit dialog, input, key-cap,
  avatar, button, menu, badge, virtual-list and toast primitives plus the
  token stylesheet; contextual sections and scoping, theming, appearance
  settings and any kit gaps are new (no contextual filtering or theme code
  exists; theming, internationalization and telemetry cores are empty
  stubs).

Biggest gaps overall: the home dashboard, project mutations, sort, filter
and search, favorites, subnav and slide-overs, the rail, personalization,
theming, palette ranking, scoping and sequences.

## 13. API calls in scope

Paths with parameters, grouped by slice owner for the contracts issues. Exact
request and response shapes are owned by NEWFRONT-276 (home, projects list)
and NEWFRONT-277 (sidebar, palette, preferences); the names below are what
the old code calls.

Home and projects list (NEWFRONT-276): dashboard widget fetch, statistics,
details and update (`GET .../dashboard/`, `GET/PATCH
.../dashboard/{id}[/widgets/]`); quick-links CRUD (`GET/POST/PATCH/DELETE
/api/workspaces/{slug}/quick-links/`, update and delete take the link id,
create body carries title and url); `GET
/api/workspaces/{slug}/recent-visits/` (query `entity_name`, omitted for
All); `PATCH /api/users/me/tour-completed/`; home preferences (`GET
/api/workspaces/{slug}/home-preferences/`, `PATCH
/api/workspaces/{slug}/home-preferences/{key}/` with `is_enabled` and
`sort_order`); projects details (`GET
/api/workspaces/{slug}/projects/details/`); lightweight project list (`GET
/api/workspaces/{slug}/projects/`); identifier-availability check (`GET
/api/workspaces/{slug}/project-identifiers?name=`); project create, update
and delete (`POST /api/workspaces/{slug}/projects/` with name, identifier,
description, repository URL, base branch, network visibility, lead, icon,
timezone and feature flags — no cover fields; `PATCH/DELETE
/api/workspaces/{slug}/projects/{id}/`); archive and restore (`POST/DELETE
/api/workspaces/{slug}/projects/{id}/archive/`); favorite projects
(`GET/POST/DELETE
/api/workspaces/{slug}/user-favorite-projects/`, create body carries the
project); join (`POST
/api/users/me/workspaces/{slug}/projects/invitations/` with a `project_ids`
array); leave (`POST
/api/workspaces/{slug}/projects/{id}/members/leave/`); favorites store
(`.../user-favorites/` add, update, list, grouped reads, moves, ordering);
workspace and project user-properties (`GET/PATCH
/api/workspaces/{slug}/user-properties/` for display and filter state;
`GET/PATCH
/api/workspaces/{slug}/projects/{id}/user-properties/` for default tab and
hidden-tab lists).

Sidebar, palette and preferences (NEWFRONT-277): sidebar preferences (`GET`
and `PATCH /api/workspaces/{slug}/sidebar-preferences/` with key, pinned and
order triples, plus `PATCH .../sidebar-preferences/{key}/` for single-item
writes); project-view order update and workspace item sort-order update;
search (`GET /api/workspaces/{slug}/search/` with search text, optional
`project_id` and `workspace_search` flag); profile read and update (`GET/PATCH
/api/users/me/profile/` for theme, language and the tour flag); account read
and update (`GET/PATCH /api/users/me/` for the timezone field); zone list
(`GET /api/timezones/`); unread notification count (`GET
/api/workspaces/{slug}/users/notifications/unread/` — list, read,
mark-all-read and archive endpoints belong to the notifications area);
work-item, cycle, module, label, member, subscription, page and scheduler
mutation endpoints belong to their own areas and the palette never calls them
directly.

Local-only state (no endpoints): user language and theme keys, favorite-menu
open flag, sidebar collapse and width keys, rail density and visibility, the
drafts flag, menu open flags, per-workspace dismissal and progress flags,
view-state search params. Dead and out of scope: the entity-search endpoint
(referenced only by commented-out palette code, never fired), the
release-notes endpoint (no caller in the surveyed tree), analytics element
ids and the session-recorder snippet (dropped by decision), and third-party
telemetry (none observed in scope).
