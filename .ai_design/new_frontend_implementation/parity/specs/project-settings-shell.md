# Project settings — shell, routing, permissions: area spec (NEWFRONT-566)

Rows PSET-001–004, PSET-062, PSET-063. Written from reading the old
settings routes, the settings shell and project sidebar components, and
the workspace redirect route — plus the inventory Acceptance column and
the shell-permissions oracle scenarios. No old code, strings, styles or
assets reused. Oracle scenarios are written from this spec and proven
against the running old app.

Old behavior that the oracle pins as bugs is described as observed and
named with its issue (PSET-001 slashless highlight, NEWFRONT-396). The
new surface keeps the observed behavior except where a bug issue says
otherwise; slices must check the linked bug issues before preserving a
quirk.

## Shell layout (PSET-001)

The per-project settings shell is a two-pane layout: a fixed left
sidebar plus a content pane inside an authorization gate. The sidebar
carries a header block on top and grouped navigation entries below.
Groups render in a fixed order — General, Features, Work structure,
Execution — each with a caption. The content pane shows a per-tab
header row (breadcrumb form, with the project emblem and the tab title)
above a centered, width-capped scrolling body.

The sidebar header holds a back control returning to that project's
issue board, a static caption marking the area as project settings, the
project emblem tile, the project name, and the viewer's project role as
a title. While the viewer role is still unresolved the header renders
nothing at all — blank space, not a loader. The shell itself has no
skeleton either: the gate renders nothing while the project-detail
request is in flight, then either the content or a restriction view.
Slow fetches can therefore present as a hung or partially missing shell;
slices should consider a loading affordance without changing the
observed end states.

Sidebar entries are plain navigation links to slash-terminated
addresses built from the workspace slug, the project id, and a per-tab
suffix. The live highlight marks an entry active by matching the
current path against a pattern that requires the trailing slash (the
General entry uses an exact comparison). Slash-form addresses highlight
correctly. The equivalent slashless deep link renders the same tab with
nothing marked — pinned as bug NEWFRONT-396, including the bare-path
forward landing unhighlighted (see PSET-002).

Role filtering removes entries instead of disabling them. Every tab
carries a list of permitted project roles; the sidebar keeps only
entries whose list contains the viewer role and skips a whole group
when none of its entries survive. There are no greyed-out placeholders.
Admins see all thirteen tabs across all four groups. Members see
General, Members, States and Labels (groups General and Work structure
only). Guests see General and Members (General group only).

At narrow widths the fixed sidebar hides and a top bar appears with a
trigger plus the current-section label derived from the path suffix.
Opening the trigger shows the same sidebar component as a floating
panel with the same role-filtered entries, collapsing again on outside
click.

On shell mount the shell reads project details with
`GET /api/workspaces/{ws}/projects/{pid}/` (the gate) and the viewer
membership with
`GET /api/workspaces/{ws}/projects/{pid}/project-members/me/`, plus
background reads for member preferences, labels, members, states,
intake state, estimates, cycles, modules and views, keyed by project id
and role with no refetch on focus.

## Bare path and guidance (PSET-002)

Visiting the bare collection path with no project id runs a forward
effect: when the joined-project list is non-empty the view navigates to
the first entry. The list is derived client-side from the cached
project collection — filtered to the current workspace, keeping only
projects where the viewer holds a membership role and that are not
archived, ordered by the stored sort order. No request fires at
redirect time beyond whatever populated that cache
(`GET /api/workspaces/{ws}/projects/`).

When the viewer belongs to no project, no forward fires and the bare
path renders a centered guidance view: an illustration, a headline
saying no projects exist yet, a short paragraph about projects as the
foundation of the work, an external learn-more link, and a primary
action opening the project-creation dialog (creation submits
`POST /api/workspaces/{ws}/projects/`; the dialog itself is PSET-013).

## Deep links and restriction views (PSET-003)

Every tab renders at its own suffixed address beneath
`/{ws}/settings/projects/{pid}`: the empty suffix for General, then
`/members`, `/states`, `/labels`, `/estimates`, `/automations`,
`/github` (shown as Repository), `/schedulers`, `/features/cycles`,
`/features/modules`, `/features/views`, `/features/pages` and
`/features/intake`. Sidebar links always use the slash-terminated
form; typed slashless forms render the same tab content minus the
highlight (the NEWFRONT-396 bug above). States, labels and members
have no per-record addresses. A legacy redirect maps the retired
per-project settings prefix onto the workspace settings route while
preserving the trailing suffix, so old bookmarks land on the same tab.

Unknown ids fail the detail fetch and the shell shows a not-found
restriction view with no join action. A real project the viewer cannot
access shows one of two variants driven by the fetch status: a missing
membership (or a workspace admin over a private project) shows a join
prompt whose action calls
`POST /api/users/me/workspaces/{ws}/projects/invitations/` with
`project_ids` holding the single project id, then refreshes
workspace-level project data and lands back on the same settings page
authorized. A plain refusal for anyone else shows a contact-an-admin
view with no action. Any other failure falls through to the not-found
view. Loading shows blank; errors surface only as these views.

Leaving a dirty General form for another tab navigates immediately
with no confirm dialog and silently discards the edits. No navigation
guard, draft prompt or unload blocker exists anywhere in the
settings-projects area; the form tracks edits for submit logic only.

## Permissions matrix (PSET-004)

Sidebar visibility follows the role registry: General and Members for
every role; States and Labels additionally for members; Cycles,
Modules, Views, Pages, Intake, Estimates, Automations, Repository and
Schedulers admin-only. Page gates match the sidebar except in two
observed mismatches the oracle pins as current behavior: the Members
page requires project admin or member, so guests who are shown the
Members entry land on a not-authorized view (kept sidebar, refusal
body) — the gate is stricter than the sidebar, and UI-only, since
guest list reads succeed server-side. The Repository page admits
project admins and members, so members from whom the sidebar hides the
entry can open it by direct address and use it — the gate is wider
than the sidebar filter. The General tab never refuses: non-admins
get the dimmed read-only presentation instead of a denial. Every other
refused tab shows the not-authorized view with the sidebar retained.

The server mirrors the matrix on writes: out-of-role project renames
via `PATCH /api/workspaces/{ws}/projects/{pid}/` fail for guests and
members, and out-of-role membership grants via
`POST .../projects/{pid}/members/` fail for guests, while guest reads
via `GET .../projects/{pid}/members/` succeed.

## Pinned non-behaviors (PSET-062)

No keyboard shortcuts exist on any settings tab — Escape only closes
the creation dialog (PSET-013) and drives the project search box
(PSET-017). No drag-and-drop outside states and labels, no bulk
operations, no exports or imports, no settings-level command-palette
entries (the palette offers only global navigation entries), and no
realtime channels. The only timed poller nearby is the scheduler
run-history refresh (PSET-059), which lives outside this shell.

## Edition parity (PSET-063)

No edition, license, self-hosted/cloud or desktop-specific branching
exists anywhere under the project-settings routes, sidebar, gates or
restriction views, and the full thirteen-tab matrix renders for admins
with no edition gate. The same project endpoints back every build, so
a scenario run against any edition observes the same capabilities and
outcomes.

## Reuse map for the shell slice (NEWFRONT-571)

Exists in apps/web_new and reusable as-is: the project reads
(`listProjects`, `getProject`, `listProjectDetails`,
`getMyProjectRoles` in the shared API client), the project-list
screen with search, empty state and create entry, and the lifecycle
dialogs for create, archive, delete, join, leave and publish. New for
this slice: the settings route tree, the grouped role-filtered
sidebar with drawer behavior, the per-tab gates and restriction
views, and the bare-path forward plus guidance view.
