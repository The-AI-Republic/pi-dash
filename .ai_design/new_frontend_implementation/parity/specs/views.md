# Views area spec — project and workspace saved views (NEWFRONT-717)

Build spec for inventory rows VIEW-001–VIEW-048
(`parity/inventory/views.md`). Parent epic NEWFRONT-78; oracle scenarios
under `apps/web_new/e2e/parity/views/` (twelve spec files plus
`support.ts`). This file is the single source the contracts issue
(NEWFRONT-718), the eleven slices (NEWFRONT-719 through NEWFRONT-726,
NEWFRONT-729, NEWFRONT-736, NEWFRONT-746) and the area gate (NEWFRONT-745)
build and prove against.

Prose only. Nothing here is copied from the old frontend: the old routes,
components, stores, services, palette wiring and desktop overlay were read
as a behavior reference, and what they do is restated below in our own
words. The only code-like tokens are route paths, API paths, query and body
field names, storage keys and the exact user-visible strings the oracle
asserts — those have to be named so contracts, slices and drivers agree.
No old string that the oracle does not assert is reproduced.

Old sources consulted (reference only, never a source): the project views
list and detail route folders with their list/detail headers and mobile
header; the workspace views list, detail and header route files; the
project view list, row, row actions, quick menu, applied-filter chips,
filter selection, sort menu, form, modal and delete-confirmation
components; the workspace view list rows, default rows, quick menus, form,
modal, delete confirmation and the (unmounted) tab-strip header; the
access-controller and publish seams in the community overlay; the
project-view, global-view, project-view filter and workspace-issue filter
stores; the view service and the view methods of the workspace service; the
command-palette creation and navigation entries; the view constants; the
API URL table and view viewsets. The desktop overlay was checked and holds
no view code. The twelve oracle files were read for the exact scenarios
and gap notes recorded per row.

Status convention: this issue owns no inventory Status change. Rows stay
`oracle green`; each slice appends `built` to the rows it implements; the
area gate turns passing rows into `new green`.

Intended behavior wins over pinned old bugs. Where the oracle records a
`bug:` scenario or a gap against the row's acceptance text, the slice
builds the behavior stated here and the gate adjudicates as §18 says.

## 1. Vocabulary and shared rules

**Two scopes, one record.** A saved view is one record shape used at two
scopes. A project view carries a project and lives under the project's
Views tab; a workspace view has no project and lives on the workspace-wide
views surface. The record's fields that this area reads or writes:
`id`, `workspace`, `project`, `name`, `description`, `query`, `filters`,
`rich_filters`, `display_filters`, `display_properties`, `logo_props`,
`access`, `sort_order`, `owned_by`, `created_by`, `is_locked`,
`is_favorite`, `archived_at`, `created_at`, `updated_at`. The server is the
authority; the client never invents a record.

**Display payload.** `display_filters` holds the layout key (`layout`, one
of `list`, `kanban`, `calendar`, `spreadsheet`, `gantt_chart`), grouping
(`group_by`, `sub_group_by`), ordering (`order_by`), the show-empty-groups
flag, the sub-issue flag, and a nested `calendar` object with its mode and
weekend flag. `display_properties` holds one boolean per column
(`key`, `assignee`, `start_date`, `due_date`, `labels`, `priority`,
`state`, `sub_issue_count`, `attachment_count`, `link`, `estimate`) plus
the model-default `created_on` and `updated_on` columns. The
filter expression lives in `rich_filters` as the issues area's expression
tree (conjunctions of field conditions); the legacy `filters` object is
carried through untouched. The issues area owns the builder and the
serialization; this area stores and re-applies the expression exactly as
received.

**Access values.** `access` is a number: `1` is shared (the list badge
shows a globe glyph labelled Public), `0` is owner-only (a lock glyph
labelled Private). Every create lands shared (`access = 1`, the model
default) because the field is serializer read-only and cannot ride a create
or patch body in any edition; a cloud access choice is stored by the cloud
write path in §14, not by the OSS create body.

**Locked views.** `is_locked` is a server-set flag. The acceptance text is
the single target: a locked view offers no layout switcher, no Display
control and no filter toggle — those controls are absent, not disabled — and
no Update view write (the server answers 400 for a locked PATCH). The old
app kept the filter toggle on locked views and let guests reach Add; those
pins are the recorded gap adjudicated in §16, not part of the target. The
issue list still renders the saved layout.

**Workspace add affordance.** A workspace custom view's "Add view" control is
offered only to workspace admins and members (VIEW-034/038's Who column) and
is absent for guests. The oracle's guest-visible scenario is pinned as a gap
(§16).

**Roles and the visibility matrix.** Project roles and workspace roles are
each admin, member, guest. "Can create a work item" means project member or
admin. Create entry points are not one uniform rule:
- **Project** views: the header Add and the zero-views action render and
  stay enabled for every project role that can see the list, guests
  included (VIEW-003/009). The palette's create command is member-plus
  (VIEW-042). The OSS create action carries no role decorator — the
  viewset is only `IsAuthenticated` — so the dialog is the only gate and
  the write is left to the server.
- **Workspace** customs: the detail and strip Add controls are offered to
  workspace admins and members only (VIEW-034/038) and are absent for
  guests. That is a UI rule, as with the ungated row menus (§9): the OSS
  create action itself carries no role decorator and does not refuse a
  guest, so the gate proves absence of the control, not a server 403.

The server limits PATCH to the creator/owner in both scopes and DELETE to
the owner or an admin at the matching level. Favorites are member-plus, so
guests see no star (VIEW-012).

The server builds the visible set as `owned_by = me OR access = 1`, then
narrows it: a **project guest** is narrowed to `owned_by = me` unless the
project sets `guest_view_all_features`; a **workspace guest** is always
narrowed to `owned_by = me`. Therefore:
- Project list — admins and members see every shared view plus their own
  owner-only views. A project guest with the flag off sees only views they
  own (shared by them or owner-only); with the flag on they see all shared
  views plus their own. A guest never sees another member's owner-only view.
- Workspace list — admins and members see every shared custom view plus
  their own; a workspace guest sees only the views they own, regardless of
  their `access`.
- A guest's own owner-only view is fully visible to that guest: it carries
  the Private badge and the lock indicator under VIEW-011/024's rules, and
  the access filter's owner-only option matches it. The narrowing hides
  other people's records, never the caller's own view from the caller.

**No live updates.** Lists and details fetch on mount and on the acting
user's own writes. They never poll, subscribe or refresh on another user's
change, and a window focus does not refetch (VIEW-043). This matches the
issues area's freshness rule and is deliberate.

**Loading and errors.** Every list read has a skeleton (never an empty
state) while its first fetch is in flight. A read failure renders an
inline error state with a retry, except where a row explicitly defines a
missing-view state. Writes toast on success or failure only where a row
says so; the "silent" cases below are deliberate.

## 2. API surface (contracts issue NEWFRONT-718)

`BASE` is `/api/workspaces/{ws}/projects/{p}` and `WS` is
`/api/workspaces/{ws}`. Every call goes through a `SavedView`-shaped zod
contract in `@pidash/api-client`; no slice calls raw fetch.

Project views:
- `GET BASE/views/` — list. Returns the caller's visible project views
  (shared + own; guests limited as above); the server annotates
  `is_favorite` and orders favorites first, then by name. The screen applies
  its own session sort on top (§4), so the default presentation is
  updated-at descending. Query: `fields` (comma list, optional). Response:
  array of records.
- `POST BASE/views/` — create. Body: `name` (required, ≤255), `description`,
  `logo_props`, `rich_filters`, `filters`, `display_filters`,
  `display_properties`. No `access` and no `is_locked`: both are
  serializer read-only fields, DRF drops them, and the model default
  (`access = 1`, shared) applies in every edition. Responds 201 with the
  record. Validation errors are 400 with field messages.
- `GET BASE/views/{viewId}/` — retrieve. 200 with the record. A missing id
  currently answers 200 with an empty serialized record rather than 404
  (bug NEWFRONT-227): blank `name`/`description`, `null` JSON fields
  (`filters`, `display_filters`, `display_properties`, `rich_filters`,
  `logo_props`), no `id`, and no `access` value — not a JSON `null`. Read
  failures are not uniform:
  - A **project guest** asking for a shared view its narrowing hides gets
    **403**.
  - Another member's **owner-only** view is removed by the queryset before
    that check. A member then receives the empty record above (200); a
    project guest hits the permission check with no record and the old view
    raises, so that request ends in a **500**.
  - An id in no queryset at all (deleted, or another project's) yields the
    empty record; there is no 404 path.

  The contract normalises a response with no `id` (or a blank name) to
  “not found”, and the screen treats a 403/404/500 read failure as the
  missing-view state (§7): only a success carrying an `id` renders the
  layout.
- `PATCH BASE/views/{viewId}/` — update, owner only, refuses locked views
  with 400, refuses non-owners with 400. Body: any subset of the create
  fields; `access` and `is_locked` are read-only and cannot be changed
  through this endpoint. Responds 200 with the record (the echo is the
  source of truth after a save).
- `DELETE BASE/views/{viewId}/` — owner or admin. 204. The row is removed
  from list results; retrieve may still answer 200 for a deleted id, so
  list exclusion is the proof of deletion.
- Favorites: the app writes a view favorite through the **generic**
  favorites collection, not the legacy view pair. Add is
  `POST WS/user-favorites/` with `{ entity_type: "view",
  entity_identifier: <view id>, project_id: <project>, entity_data:
  { name } }`; remove is `DELETE WS/user-favorites/{favoriteId}/`, where
  `favoriteId` is the favorite row's own id (read from the workspace
  favorites list or kept from the add response), **not** the view id.
  `GET WS/user-favorites/?all=true` backs the sidebar and is the server
  proof that a view favorite exists; the bare list omits view rows. The
  list's `is_favorite` annotation reads the same `UserFavorite` rows.
  Contracts: reuse the generic favorites contract (SHELL-056) behind one
  helper; do not build a second one for views.
- The legacy `POST BASE/user-favorite-views/` (`{ view }`) and
  `DELETE BASE/user-favorite-views/{viewId}/` **are routed by the server**
  but have no caller in the app — the view service methods that wrap them
  are dead (§17). Do not write through them; their permission gate is
  member-plus and their delete key is the view id, unlike the generic
  path.

Workspace views:
- `GET WS/views/` — list customs visible to the caller (shared + own;
  workspace guests see only their own). The server accepts `order_by` and
  defaults to newest first; the screen keeps that order. Array of records.
- `POST WS/views/` — create. Body as the project create minus project.
  The workspace create dialog sends `display_filters` with `layout:
  "spreadsheet"` and `order_by: "-created_at"` and an empty expression; a
  bare API create with no `display_filters` takes the model defaults
  (`layout: "list"`, `order_by: "-created_at"`). The view lands shared
  (`access = 1`) for the same read-only reason.
- `GET WS/views/{viewId}/` — retrieve; 200 with the record. Missing ids
  return the same empty serialized record as the project retrieve. The
  retrieve is **not** guest-narrowed: its queryset is `owned_by = me OR
  access = 1` with no guest step, so a workspace guest asking directly for
  another member's shared custom gets 200. The list is narrowed (§1), and
  the detail's visibility is the list's: a record the caller's list read
  would not return is treated as not-found even when the retrieve answers
  200 (§11). Do not read "forbidden" into a direct retrieve.
- `PATCH WS/views/{viewId}/` — owner only, refuses locked views with 400.
- `DELETE WS/views/{viewId}/` — owner or workspace admin; 204.

Issue reads behind a view:
- `GET BASE/issues/` with the saved expression serialized into the list
  filter parameters, plus `order_by` from the view's display filters, page
  size 100 and cursor. There is **no** `BASE/views/{viewId}/issues/` route
  (the old service had a method for it but the server does not route it —
  dead code, see §17). The detail therefore applies the stored expression
  to the generic project issue list client-side, exactly as the issues
  screen does.
- `GET WS/issues/` for the workspace all-issues surface (issues area), with
  the same expression mechanism. Static workspace views add their own
  fixed condition: assigned to me, created by me, or subscribed.

Feature flags and favorites:
- Project flags are read from the project record (`issue_views_view`,
  `cycle_view`, `module_view`, `guest_view_all_features`). The views
  switch writes `PATCH BASE/` with `{ issue_views_view: boolean }`
  (owned by project settings, PSET-043).
- `GET WS/user-favorites/?all=true` for favorite state in the sidebar and
  as the server proof in scenarios.

Coverage the contracts issue owns: the list, retrieve (including the null
body normalisation), create, patch echo, delete for both scopes; the
create and patch request types omit `access` and `is_locked` (a caller
that types them as writable would build a selector whose writes are
silently discarded); the favorites pair or a documented reuse of the
generic favorites contract; and the list-read contract for the issue query
the detail mounts. The thin `SavedView` schema already exists (ISS-104) —
extend it, do not fork it.

## 3. S1 — Project views list shell, gate, states and row identity (VIEW-001, 002, 008, 009, 010, 011; NEWFRONT-719)

**Screen (VIEW-001).** The project's Views tab mounts a list screen under
`/{ws}/projects/{p}/views`. The header carries the project breadcrumb trail
(workspace › project) with a trailing "Views" crumb, the list controls, and
a primary button labelled "Add view" on the right. The browser tab title is the project
name, a separator, then "Views". The list reads `GET BASE/views/` once on
mount and shows one row per visible view. Order is the list store's default:
`updated_at` descending; sort is session-local (VIEW-005). The project
record supplies `issue_views_view` for the gate, `cycle_view` and
`module_view` for the filter options, and `guest_view_all_features` for the
role rule.

**Flag gate (VIEW-002).** When the project record has
`issue_views_view === false`, the route does not render the header controls
or any rows. Instead the whole content area shows a centered empty state:
themed illustration, the heading `Views are not enabled for the project.`
(oracle-asserted exactly), a short explanation that views can be turned on
from project settings, and one primary button labelled "Manage features".
The button is enabled only for a project admin and does nothing visually
when disabled; for members and guests it still renders, disabled.
Activating it navigates to the project's features settings surface that
renders the views switch (VIEW-041, PSET-043). Which address that is —
the bare `/{ws}/settings/projects/{p}/features` index or its
`/features/views` leaf — is the settings area's route decision (§19); the
target is a rendered settings page, not a 404. The bug is the old
shortcut's bare `…/features` target, which has no page because only
per-feature leaves exist; the same address in the new tree is still
unrendered in this checkout.

The oracle's bug step asserts the old URL shape (`…/features` with an
optional trailing slash) plus an empty views gate heading. If the settings
area renders the bare index, that step can pass on `web_new`; if the
shortcut lands on the views leaf, the URL assertion is a known oracle
divergence and is not a VIEW-002 failure. Either way the gate proves the
intended behavior on `apps/web_new` (§15/§18): the shortcut resolves to a
rendered settings page, the views switch is present for an admin, and no
views gate heading remains. A bare address that still renders nothing is a
failure, not a divergence. S1 does not add a settings route; the
destination is the settings area's (§19).
The gate replaces the skeleton: no list fetch is made while the gate shows,
and no Add view control renders.

**Loading (VIEW-008).** While the first list fetch is in flight the screen
shows a skeleton list of row placeholders sized like the real rows, inside
the same content frame. The skeleton is mutually exclusive with rows and
with both empty states: once data arrives it is replaced in one commit.

**Zero views (VIEW-009).** With a successful, empty list and no search or
filters applied, the screen shows a themed illustration with the heading
`Save custom views for your project` (oracle-asserted exactly), an
explanation that views are saved filter sets for quick access and can be
shared with teammates, and a primary "Create view" action that opens the
create dialog. The action renders and stays enabled for every role that
reached the list, guests included, in every edition: VIEW-009's acceptance
text and the oracle both pin an enabled control for every project role, and
creation itself is left to the server (VIEW-003). It is not disabled by the
palette's member-plus rule, which governs only the command (VIEW-042).

**No match (VIEW-010).** When the list is non-empty but the combination of
name search and list filters excludes every view, the screen shows a
search-themed empty state: heading `No matching results.` (oracle-asserted
exactly) with a suggestion to adjust the search terms. It never mutates or
filters the underlying list data; clearing the query or filters restores
the rows in their sorted order. If the project has no views at all,
VIEW-009 wins even with a search typed.

**Row identity (VIEW-011).** Each row shows, left to right: the view's
custom logo when `logo_props.in_use` is set (emoji or glyph icon, 16px),
otherwise a default views glyph; the view name (ellipsized); then, in this
order: an access badge (globe = shared, lock = owner-only) with a hover
tooltip naming the access; a Live marker only when `anchor` is set (§6,
cloud); the owner avatar with a tooltip; a favorite star; and a trailing
overflow menu on desktop. The whole row is a link to the detail page
`/{ws}/projects/{p}/views/{id}` (middle-click and keyboard activate it like
any link). On narrow viewports the row's quick menu moves into the row and
the action cluster collapses to the star plus menu.

Empty and error copy for this screen must match the oracle-asserted
strings exactly. Pinned by the driver: the gate heading
`Views are not enabled for the project.`; the zero-views heading
`Save custom views for your project`; the no-match heading
`No matching results.`; "Manage features"; "All Views"; and the tab-title
separator. Everything else in these states is free prose.

The complete set of oracle-asserted literals across the area, named where
each belongs: the three headings above; `View work items will appear here`
(layout zero-result, §7); `No matches found` (ISS-094 filter primitive, §4);
"Manage features"; "Add view" (list header and workspace detail header);
"Add work item" (§8); "Create view" (zero-views action); "Create View" and
"Update View" (dialog headings and submit/cancel roles); "Title" and
"Description" (field placeholders), "Title is required" (§5);
"All work items", "Assigned", "Created", "Subscribed" (workspace defaults);
"List" (layout label) and "Display Properties" (display options);
"Public"/"Private" (access labels, cloud legs);
"Go to All work items" (§11 missing-view back); "View does not exist"; "All
Views"; "Views" (crumb); "Clear all" (§4 chips); "Display"; "Save as" and
"Update view" (filter-row triggers); "Open in new tab", "Copy link", "Edit
View", "Delete View" (menus); "Are you sure you want to delete this view?"
(delete-confirm heading); `Link Copied` (copy notice fragment); and
`updated successfully`, `created successfully`, `could not be created` (notice
fragments). Nothing outside this set is copied from the old app; slices
write their own prose for every other state.

## 4. S2 — List search, sort, filters and applied chips (VIEW-004–007; NEWFRONT-720)

All four controls in this section are client-side over the fetched list;
none of them calls the server beyond the initial list read. Their state is
session-local: a reload resets everything to defaults. They compose: the
name search and every filter dimension are conjunctive, and the sort applies
last.

**Search (VIEW-004).** Collapsed, the header shows only a search icon
button. Activating it expands a text field and moves focus into it in the
same interaction; typing filters the rows by case-insensitive substring of
the view name (the display name; a record with no name shows a localized placeholder
   instead of a blank cell). Escape
behaves in two steps: with a non-empty query it clears the query and keeps
the field open and focused; with an empty query it collapses the field and
moves focus back to the trigger. A pointer press outside the field collapses
it only when the query is empty; a non-empty query keeps it open so the
filtered result stays visible. The expand/collapse transition is animated;
the collapsed field must not be focusable or announce a value.

Old gap: the outside detector only saw presses in the header zone, so a
press on the list or sidebar never collapsed the field. Slices implement the
document-wide behavior above; the oracle asserts the header path, so this
widening is compatible.

**Sort (VIEW-005).** A menu in the header shows the active key and offers
three keys — Name, Created at, Updated at — then a divider and two
directions, Ascending and Descending. A check mark marks the active key and
the active direction. Names sort case-insensitively; timestamps sort by the
record's created/updated stamps. The default is Updated at, Descending. The
list re-orders immediately on selection without a server call. The choice is
**not** persisted: reload restores the default, and the trigger label reads
"Updated at" again. Selecting the already-active direction is a no-op. The
control is a compact variant on narrow viewports (same options, smaller
trigger).

**Filters (VIEW-006).** The filters menu opens a panel with its own search
box that narrows the *options* (not the rows) by name. Dimensions:
- Favorites — a single on/off row; when on, only rows whose
  `is_favorite` is true stay. The panel's option search shows the
  oracle-asserted `No matches found` line when the query matches no
  option; that line belongs to the issues-owned filter-section primitive
  (ISS-094), so S2 reuses it rather than writing a views-local one.
- Access — shared/owner-only multi-select. It is a **cloud-only** dimension:
  the OSS build renders no Access section at all (probe: the panel has no
  Access heading and no access values). The list filter, when present,
  matches rows whose `access` is in the selected set.
- Created date — relative presets "1 week ago", "2 weeks ago", "1 month
  ago", and "Custom". Each preset means *older than* that offset (at least
  7, 14 or 30 days before now), not the week or month just gone: a
  "1 month ago" preset alone excludes views created inside the last 30
  days, which is what the oracle asserts. Multi-select is conjunctive — a
  row must satisfy every selected preset, so combining two presets yields
  the older of the two. A custom range offers a date picker and means the
  selected window.
- Created by — project members plus a "You" entry; multi-select of user
  ids matched against the record's `created_by`.

The menu closes on pick for single-value rows and stays usable for
multi-select rows; the applied chips communicate the state either way.
Filters combine with the name search.

Star-clobber note (NEWFRONT-220): the old list issued a duplicate views
read on first mount that could overwrite a just-toggled star. The new screen
issues exactly one list read per mount and applies favorite writes to the
cached record, so this race cannot exist.

**Applied chips (VIEW-007).** When at least one filter dimension is active,
a chip strip renders above the list. One chip per dimension shows the
dimension's label and the selected value(s); each value has its own remove
control and a dimension chip can be removed as a whole. A trailing "Clear
all" resets **all** dimensions at once. Removing the last dimension hides
the strip. Chips never appear for the name search (the search field itself
shows the query). Removing a chip re-runs the client filter immediately; the
underlying view records are never modified or deleted.

Old bug: removing one dimension replaced the whole filter object, wiping
sibling dimensions (NEWFRONT-216). Intended: removal edits only the
dimension being removed and every sibling survives (§15). The oracle pins
the wipe as a `bug:` scenario; the gate proves the intended behavior on
`web_new`.

**Empty interactions.** Any combination that excludes all rows shows
VIEW-010's no-match state, not VIEW-009. The chip strip stays visible so the
user can undo.

## 5. S3 — Create dialog and access selector (VIEW-003, 013, 014, 019; NEWFRONT-721)

**Entry points (VIEW-003).** The list header's primary "Add view" button opens
the create dialog over the list (modal, page stays mounted). The zero-views
empty state's create action opens the same dialog, and the palette's "New
view" command (§12) and the `nv` sequence open it too. One dialog
component, all entry points; opening it never navigates. Opening resets the
form to defaults: title empty, description empty, shared access, List
layout, group by state, default display properties, empty filter expression.

**Dialog fields (VIEW-013).**
- Title — single-line input, required; inline error "Title is required"
  when submitted empty, and a max-length error at more than 255 characters;
  the input autofocuses on open.
- Description — multiline free text, optional, no validation.
- Icon — a picker button showing the current logo or the default views
  glyph. The picker offers two tabs, Emoji and Icon; picking an emoji stores
  `logo_props = { in_use: "emoji", emoji: { value } }`, picking a glyph
  stores `logo_props = { in_use: "icon", icon: { name, color } }`. The
  dialog header shows the selected icon. The picker closes on pick.
- Access — cloud only (VIEW-019): a selector between shared and
  owner-only with one explanatory line each. OSS renders **nothing** in its
  place (no disabled control, no placeholder).
- Layout — a menu offering List, Board, Calendar, Spreadsheet, Timeline in
  that order, with icons; the trigger shows the active layout's icon and
  label. Default List for project views, and no layout menu at all for
  workspace views (fixed Spreadsheet, §10).
- Display — the issues area's display-options control scoped to the chosen
  layout; entries whose project features are off (cycles, modules) are
  absent, not disabled.
- Filters — the issues area's rich filter builder for the project (or the
  workspace), mounted expanded with an empty expression and the display
  filters/properties above in scope, so the user can add conditions before
  saving.

The dialog is a top-aligned wide modal with the form as its body; its
footer has Cancel (secondary) and the submit button (primary, labelled
"Create View", showing a pending label while submitting). Escape closes
without saving; Cancel closes without saving; clicking outside does not
close it while a submit is pending. Closing discards everything.

**Submit (VIEW-014, VIEW-019).** Submitting validates locally first (title
required, ≤255) and then POSTs `BASE/views/` with: `name`, `description`,
`logo_props`, `rich_filters`, `display_filters`, `display_properties`.
The body carries no `access` in any edition: the field is serializer
read-only, so a selector value sent here would be dropped and every create
would still land shared. A cloud access choice writes through the cloud
path decided in §14, never through this body. The layout key is included in
`display_filters`; a Board layout without a grouping is sent with
`group_by: "state"` (a create-time normalization the oracle pins). On 201
the dialog closes, a success notice "View created successfully" appears, and
the router navigates to the new view's detail page. On failure the dialog
**stays open** with the user's input intact, an error notice names the
failure ("Failed to create view…"), and nothing is created. Old bug:
failure wiped the form (NEWFRONT-221); intended is keep-and-retry.

**Access selector (VIEW-019).** Cloud only, and only once the cloud
access write path exists (§14): `access` is read-only on the create and
patch endpoints, so the selector cannot ride the OSS body in any edition.
Target: the shared option is selected by default; owner-only explains that
only the owner will see the view. The cloud build's own write path stores
the value; the list badge, the Access list filter (§4) and the detail lock
glyph (§7) then follow the stored record. OSS renders no selector and every
created view is shared by the model default. The read side is proven per
§14/§16 by planting `access` on the record through the server and reading
it back; nothing writes it from the dialog.

## 6. S4 — Favorite, row menu, edit, delete, publish (VIEW-012, 015–018, 020; NEWFRONT-722)

**Favorite (VIEW-012).** A star control renders on each row for project
members and admins, not for guests. Clicking it stops the row navigation and
optimistically flips the cached `is_favorite`; the footer/star state flips in
the same frame. The write is the generic favorites pair from §2: add
`POST WS/user-favorites/` (entity type `view`, the view id, the project
and the name), remove `DELETE WS/user-favorites/{favoriteId}/` using the
favorite row's id. The legacy `user-favorite-views/` methods exist in the
old service but nothing calls them; they are not the write path. The toggle
is **silent**: no success or failure toast fires. On failure the optimistic value reverts. On the first
successful add, the app opens the sidebar favorites menu (a local
preference flag) so the user sees where the view landed. The list's
Favorites filter reflects the change immediately. The row menu never closes
on the star; the "favorites" chip and star stay in sync.

**Row menu (VIEW-017).** The overflow menu on a row contains, in order:
Edit, Open in new tab, Copy link, Delete. Visibility: Edit only for the
view's owner; Delete for the owner or a project admin; Open and Copy for
everyone who can see the row. On the cloud build the publish entry (§ below)
sits after Copy link. Each action is a menu item with an icon; choosing one
closes the menu. A click on a menu item never triggers the row link.

**Edit (VIEW-015).** Edit opens the same dialog as create, in "Update View"
mode, prefilled from the record: title, description, icon, layout, display
options and the stored expression; the cloud build shows the record's
access read-only beside its own access write path (§14), because `access`
cannot be patched on this API. Saving PATCHes `BASE/views/{viewId}/` with
the changed fields and the normalized payload (same writable keys as
create, including the layout and display objects). Success is
**silent** — no success toast — closes the dialog and refreshes the list row
and the view's live filter state from the PATCH echo; the detail page, if
open behind, adopts the echoed stored values. Failure shows an error notice
("Failed to update view…"), keeps the dialog open, and restores the
prefilled originals (the form resets to what it was opened with — nothing
that was saved is lost). Editing is offered only to the owner; a locked view
refuses the write server-side.

**Delete (VIEW-016).** Delete opens a confirmation dialog with the heading
"Are you sure you want to delete this view?" and body copy warning that the
layout, sort, filter and display choices are permanently deleted with no way
to restore them. Cancel closes and keeps the row. Confirming DELETEs the
record; on success a notice "View deleted successfully" appears, the row
disappears from the list and the router returns to the views list (from a
detail page it navigates there). On failure an error notice "View could not
be deleted…" appears and the row stays. The delete is a soft delete: the
retrieve endpoint may keep answering for the id, so list exclusion is the
proof. A deleted view's favorites row disappears with it.

**Copy link and new tab (VIEW-018).** Copy link writes the absolute URL of
the view's detail page to the clipboard through the shell's shared copy-link
helper (`apps/web_new/src/shared/shell/shareLink.ts`) and shows a success
notice whose text contains the oracle-asserted fragment `Link Copied`, plus
a short confirmation line whose wording the slice owns. The helper is the
client-side copy path, not a server call. Open in new
tab opens that same absolute URL in a new tab; the standalone render is the
detail page (VIEW-021). Both exist for every role that sees the row, on the
list and on the detail header.

**Publish (VIEW-020).** Cloud only. The record served by this checkout has
no `anchor` field — the anchor on models in the OSS API belongs to projects,
not views — so the publish seam is cloud-side data, not an OSS read. Where
the field exists, a publish entry appears after Copy link for the owner or
a project admin. Activating it opens a small dialog that
shows the view's public read-only link (derived client-side from the record's
`anchor` and the public app's base URL) with a copy control, and offers
publish or unpublish. Publishing writes the view payload so the server sets
`anchor`; the row then shows the Live marker, and unpublishing clears both.
The dialog can be opened by clicking the Live marker too. OSS renders no
publish entry, no Live marker and no published link; the server never sets
`anchor` there. Cloud legs per §14.

## 7. S5 — Project view detail, switcher, lock and issue body (VIEW-021–024, 028; NEWFRONT-723)

**Screen and title (VIEW-021).** The detail route is
`/{ws}/projects/{p}/views/{viewId}`. It reads `GET BASE/views/{viewId}/`
and the project record, then renders the view's saved layout populated with
the project's issues filtered by the stored expression (§2). The browser tab
title is the project name, a separator, then the view name. The breadcrumb
shows the project trail, a "Views" crumb linking back to the list, and the
current view as the last crumb (a switcher, below). The header's right side
carries the layout switcher, the Display control, the filter toggle, the Add
work item button and the view overflow menu (desktop). The body mounts the
issues layout root with the view's own display settings as the source of the
rendered layout — the view opens in the layout it was saved with, not the
viewer's personal preference.

**Layout rendering (VIEW-028).** Grouping, pagination, skeletons, row
behavior, quick actions and per-issue capabilities are exactly the issues
area's (NEWFRONT-71); this area only hands the layout root (a) the stored
expression, (b) the stored `order_by`, (c) the stored display settings, and
(d) the saved view record. The list query is the generic project issue list
with the expression serialized; **there is no per-view issues endpoint**
(dead code, §17). A zero-result list shows the layout's own empty state,
whose heading is the oracle-asserted `View work items will appear here`
(the layout owns the rest of the copy; the views area does not re-render
it); a matched-but-empty page shows the no-match state. Sampling proves
the set: an urgent-only expression shows urgent issues and hides low-priority
ones, and the same set follows into another layout.

**Missing or forbidden (VIEW-022).** Any read failure that means "this view
is not visible to you" — 404, 403, a 500 raised on the excluded record
(another member's owner-only view, §2), or a 200 carrying an empty
serialized record with no `id` (the observed old behavior for unknown
ids, §2) —
renders, instead of the layout, an empty
state with an illustration, the heading "View does not exist", an
explanation that the view does not exist or the viewer lacks permission, and
a primary action returning to the project views list (the slice picks its
label; the driver matches a control whose accessible text reads as views,
back or list).
The header chrome is not rendered in this state (no layout controls, no Add,
no menu). This is the intended behavior; the old app rendered a nameless
phantom for unknown ids (§15, §18, NEWFRONT-227).

**Switcher (VIEW-023).** The last breadcrumb is a dropdown listing every
saved view of the project, with a search box that filters by name
(case-insensitive substring) and marks the current view. Each option shows
the view's icon and name. Picking an option navigates to that view's detail
page in the same tab. Opening the switcher never writes anything; the list
it shows is the project's fetched view list (same order as the list screen).
Escape closes it; the dropdown is keyboard navigable.

**Lock indicator (VIEW-024).** When `access` is owner-only, a lock glyph
sits next to the breadcrumb with a hover tooltip reading "Private". Shared
views show nothing extra. The glyph is presentational only. The oracle notes
the old tooltip does not open under automation; its wiring is asserted in a
unit test and the glyph is the parity-visible behavior.

## 8. S6 — Detail layout, display, filters, Add item, Save as (VIEW-025–027, 048; NEWFRONT-724)

This slice owns the detail's editing chrome and its seam to the issues
filter row. It does not implement the layouts or the filter builder.

**Layout switcher (VIEW-025).** The header offers List, Board, Calendar,
Spreadsheet and Timeline, in that order, as an icon row on wide viewports and
a named dropdown on narrow ones. Picking one re-renders the current issues in
that layout immediately and does not change the saved record until it is
persisted. Persistence happens through the view's display update path (§8):
the switch marks the view dirty and enables Update view; saving sends the
layout inside `display_filters` and the echo replaces the local state. A
reload before saving restores the stored layout. Locked views render no
switcher at all.

**Display options (VIEW-026).** The Display control offers exactly the
sections the active layout supports (Display properties, Group by, Order by,
Show empty groups, Show sub-work items, Sub-group by only where the layout
supports it, Calendar options for Calendar). Sub-group by is a Board section
and follows the issues area's rule (ISS-077) rather than this row's: it
appears only once a Group by dimension is chosen, so a Board with no
grouping lists Group by but not Sub-group by. The old app lists Sub-group by
on any Board regardless of grouping; the target is the ISS-077 rule, which
`web_new` already implements. The VIEW-026 scenario asserts each app's own
answer and then proves the section appears once a grouping is picked. It
asserts the rule rather than the board's opening state: a switch into Board
may already seed a Group by (the issues area stores State when none was
chosen), so "no Sub-group by immediately after the switch" is only true
while the board carries no grouping.
Entries for cycle and module follow the project's `cycle_view` and
`module_view` flags: absent when the feature is off, present when on. Changes
update the view's live display state and mark it dirty; Update view persists
them through `display_filters`/`display_properties` and the echo replaces
local state. Locked views hide the Display control.

**Filter toggle and saved query (VIEW-026).** The filter affordance is the
issues area's (ISS-096): with no condition and nothing unsaved it is an
"Add filter" button that opens the property picker, and with a condition or
unsaved change it toggles the rich filter row. Either way it edits this
view's saved expression and display settings, never the viewer's personal
filters. Its Save as/Update view triggers are the issues-owned controls from
`parity/specs/issues.md` §9/§21 (ISS-104/105) and must **delegate to this
area's dialog without changing the payload**: the row computes the same
snapshot (normalized expression, display filters, display properties) and
hands it to the views-owned create/edit dialog; the dialog adds only the
view-specific fields (name, description, icon, and the cloud access/publish
path) and POSTs/PATCHes the same keys — never `access` or `is_locked`,
which the API ignores (§1). The views area owns the dialog chrome; the
issues area owns the builder and the snapshot. Save as opens the create
dialog prefilled with the current snapshot; Update view is offered only for
the owner of an unlocked, changed, non-default view and PATCHes that record
(echo adopted). On a bare issues list the same triggers create a project view
through the same dialog. The issues gate proves the payload with a minimal
form; the views slices replace that form with the real dialog and the issues
gate must keep passing.

Old gaps: layout/display edits were local-only until Update view (intended:
dirty state plus explicit persistence), cycle/module entries were absent
regardless of the flag (intended: follow the flag), and locked views kept a
working filter toggle (intended for locked views: no toggle at all, per
VIEW-025/026's acceptance text; the pinned old behavior is adjudicated in
§16).

**Add work item (VIEW-027).** The header shows a primary button labelled
"Add work item" (oracle-asserted exactly) only for users who can create work items in the project — project
member or admin. Guests and other read-only roles never see it. Activating it
opens the issues area's create modal (ISS-117–123) scoped to the project;
submitting creates the issue through the issues endpoint. When the new issue
matches the saved expression it appears in the detail list after the create
settles; when it does not match, it does not appear. The view record is never
modified by adding an item. No quick-add affordance is added by this area;
the layouts' own quick-add behavior belongs to the issues area.

**Save as (VIEW-048).** When the view is dirty — the live expression or
display state differs from the record — the filter row offers a Save-as
trigger next to Update view. Save as opens the create dialog prefilled with
exactly two things:
- the current record name plus a flat ` 2` suffix (exactly `${name} 2`;
  it is not a counter and does not append further copies when that name
  already exists),
- the live expression and display snapshot from the filter row.

It does not carry the record's description, icon or access; those stay at
the dialog's create defaults. A record with no name prefills a localized
placeholder name, not a copied string. The user may change any field.
Saving POSTs a new project view with the current payload and navigates to
the new view's detail; the original record is untouched. The oracle asserts
the `${name} 2` prefill, the duplicate's presence in the list, and the
snapshot it was created from; it asserts nothing about carried metadata.

**Rework S6 owns (NEWFRONT-724).** Today's `apps/web_new` row comes from
the issues area and must change in two ways:
- `offersSaveAs` currently offers Save as on any unlocked saved view, clean
  or dirty, and the issues unit test asserts that clean-view offer. The
  target is dirty-only: even when the record already stores a filter
  expression, Save as is **absent** until something changes. The old app
  offered it whenever any filter existed, which let a user duplicate an
  already-saved view without changing anything. VIEW-048's acceptance text
  ("after any unsaved filter or layout change") is the target; the gate
  proves both the absent clean case and the present dirty case, so the
  narrowing is deliberate rather than a lost capability.
- The prefill stays exactly name plus snapshot as above. Carrying any
  further record field is a new behavior, not the ported one, and would
  need its own proof.

## 9. S7 — Workspace list, defaults, custom rows and loading (VIEW-029–032, 039; NEWFRONT-725)

**Workspace list screen (VIEW-029).** The route is
`/{ws}/workspace-views`. It shows a search field across the top, then the
four static default views, then the workspace's custom views (each newest
first). The browser tab title is the workspace name, a separator, then "All
Views". The search filters **both** groups by a case-insensitive substring of
the displayed name; a query that matches nothing in a group hides that group
entirely. The list reads `GET WS/views/` once on mount. Guests see the
defaults plus only the customs they own (server-enforced, §1; owner-only
ones included — the screen renders whatever the server returns). No Add control lives on this
page (creation lives on the detail header, §10).

**Static defaults (VIEW-030).** The four defaults always render, in order:
All work items, Assigned, Created, Subscribed. Their names are localized
labels from the app's copy layer; their links point to
`/{ws}/workspace-views/all-issues`, `…/assigned`, `…/created`,
`…/subscribed`. They have no record, cannot be edited, deleted, renamed,
favorited or reordered; no star, no edit/delete entries. They always show
even when no custom views exist.

**Default row actions (VIEW-031).** Default rows on the list are plain
links with no overflow menu. On a default view's detail page the header's
overflow menu offers exactly Open in new tab and Copy link (the same
behaviors as VIEW-018, with the same copy notice). No edit or delete entry
exists for a default.

**Custom rows (VIEW-032).** A custom row shows the view name, capped with an
ellipsis at a display length the slice picks (the old row capped at 75
characters; the oracle pins only that a very long name renders visibly
shorter, so the exact cap is a design choice), its description underneath
when present
(omitted otherwise, no empty line), and links the whole row to
`/{ws}/workspace-views/{id}`. A trailing overflow menu offers Edit View and
Delete View. The menu renders for every member who can see the row, as the
oracle pins; the server enforces ownership on PATCH (400 for non-owners) and
ownership-or-admin on DELETE. Edit opens the workspace edit dialog (§10);
Delete opens the confirmation (§10).

**Loading and empty (VIEW-039).** While the customs fetch is in flight a
skeleton list of row placeholders renders below the search field; it never
renders alongside rows. The defaults may render immediately (they need no
fetch), but the customs area shows the skeleton rather than an empty gap
while loading. With no customs, the list shows only the four defaults: **no**
custom empty state, no illustration, no call to action. A read failure shows
an inline error with a retry that refetches. Unknown or forbidden custom ids
on the detail resolve through the workspace missing-view state (§11), which
is the same shape as VIEW-022 but returns to the workspace default view.
Old bug: the old store returned `[]` while loading so no skeleton ever ran
(NEWFRONT-230); intended is the skeleton above.

## 10. S8 — Workspace CRUD and tab strip (VIEW-033–037; NEWFRONT-726)

**Tab strip (VIEW-033) — target, no oracle.** The workspace detail page
carries a horizontally scrollable tab strip directly under the page header.
It renders one tab per static default view, then one per custom view, in the
same order as the list. The active tab is visually distinguished and is
brought into view automatically when the page mounts or the active view
changes (centered when it is far off-screen, nearest otherwise; smooth
scrolling). Each tab carries per-tab quick actions: the default tab's menu
offers Open in new tab and Copy link; a custom tab's menu follows the same
ownership rules as VIEW-037. A trailing add button at the right of the strip,
rendered only for workspace members and admins, opens the create dialog. The
strip scrolls with a thin horizontal scrollbar and never wraps. The oracle
pins the old app's **absence** of this strip because the old header
component was never mounted (dead code, §17). The acceptance text requires
the strip, so slices build it and the gate proves it on `apps/web_new` as
"target, no oracle" (§18).

**Create a custom workspace view (VIEW-034).** The "Add view" button on the
workspace detail header and the strip's trailing add button open the
workspace create dialog. Both are offered to workspace members and admins
and hidden for guests (VIEW-038's acceptance text); the oracle records the
old app rendering the detail Add for guests as a gap the slices correct. Fields:
title (required, ≤255, inline errors), description, the cloud access
control (read-only access on this API — its write path is §14's),
Display (fixed to the spreadsheet set — no layout picker, no Sub-group by),
and the workspace-level rich filter builder, mounted expanded. Submitting
POSTs `WS/views/` with the title, description,
`display_filters` (spreadsheet, ordering default newest first),
`display_properties` and `rich_filters`; the view lands shared. Success toasts "View created
successfully" and navigates to the new view's detail. Failure toasts an
error ("View could not be created…") and keeps the dialog open with input
intact. Escape and Cancel close without saving.

**Edit (VIEW-035).** The dialog opens prefilled from the record's current
title, description, display settings and expression (cloud access is
read-only and cannot be patched). Saving PATCHes `WS/views/{viewId}/`,
adopts the echo, refreshes the live filter
state of the open detail, and shows a success notice whose text contains
`updated successfully` (the wording beyond that fragment is the slice's).
Failure shows the error notice and keeps the dialog open on the entered
values; it must **not** be silent. Old bug: the store swallowed the failure, so nothing
was shown (NEWFRONT-232); intended is the error notice above.

**Delete (VIEW-036).** A confirmation dialog with the same permanent-loss
copy as VIEW-016 requires explicit confirmation. Cancel keeps the view.
Confirming DELETEs it and removes it from the list; success shows the
success notice, and failure shows an error notice ("Failed to delete…")
and keeps the row. The detail's local filter state for that view id is
dropped. The oracle's "delete confirms, then removes silently" scenario
pins the old silent success — a recorded gap, not the target. The
acceptance text asks for a success notice, so the slice builds the notice
and the gate proves it on `web_new` as target, no oracle (§16).

**Quick menu (VIEW-037).** On a custom view's detail header the overflow
menu offers Edit, Open in new tab, Copy link and Delete. Edit and Delete are
gated as in VIEW-016/017 (owner for edit, owner or workspace admin for
delete); Open and Copy are available to everyone who can see the view. On a
default view's detail the menu offers only Open in new tab and Copy link.
Copy uses the same shared copy-link helper and the same `Link Copied`
notice fragment as VIEW-018 and writes the absolute detail URL; open in new
tab loads it standalone. The row-level menu
in the list (§9) offers only Edit View and Delete View per the oracle.

## 11. S9 — Workspace detail and cloud layout (VIEW-038, 040; NEWFRONT-729)

**Detail screen (VIEW-038).** The route is
`/{ws}/workspace-views/{viewId}`. For a custom view it reads the caller's
visible custom list and treats membership in that read as the access check;
the retrieve (`GET WS/views/{viewId}/`) fills in the record once the id is
known to be visible. Reading the retrieve alone is wrong: it is not
guest-narrowed, so a workspace guest would otherwise see another member's
shared custom that the list hides (§2). For a default the screen uses the
static definition and makes no record read. The browser tab title stays the workspace name + "All Views" for both
defaults and customs (the oracle pins this); the crumbs identify the current
view — a "Views" crumb and the current view's name, or the localized
default's label. The header's left side is the crumb/switcher; the right
side carries, in order: the layout selector (cloud, §below), the filter
toggle, the Display control, an "Add view" button (workspace admins and
members only) that opens the create dialog, and the view quick menu (custom)
or the default quick menu. On a locked custom the layout selector (cloud),
filter toggle and Display control are all absent; the "Add view" button and
quick menu remain as above. The body mounts the issues area's all-issues layout root for both
defaults and customs: defaults apply their fixed condition (assigned to me,
created by me, subscribed, or no extra condition for all items) and customs
apply their stored expression. Adding a work item is not part of this header;
the all-issues body's own capabilities are the issues area's.

The switcher spans the four defaults and the customs with a search box; a
pick navigates to that view's detail in the same tab.

Locked customs: the layout selector, filter toggle and Display control are
absent, and no update is possible (server 400 if attempted). The "Add view"
button is unaffected by another view's lock but still follows the role rule
(workspace admins and members). The old app's guest-visible Add and
locked-view toggle are pinned bugs, proved as target on `web_new` (§16).
Unknown ids and ids the caller's list does not contain — another member's
shared custom opened by a workspace guest, or a project guest's view with
the flag off — show the missing-view empty state: illustration, the heading "View does not
exist" (oracle-asserted exactly), the explanation, and a primary button
labelled "Go to All work items" (oracle-asserted exactly) that returns to
`workspace-views/all-issues` — the default view, not the list. The old app
kept the detail header above this state; the target renders the state
without the header chrome, matching VIEW-022.

**Layout choice (VIEW-040).** OSS renders the spreadsheet layout with **no**
layout selector; the saved display payload decides the body, and API-created
customs must carry a spreadsheet display payload to render (the support
helper's shape is the canonical example). Cloud fills the layout selector
with the available layouts for the workspace all-issues surface, and the
selection follows the same dirty/persist path as VIEW-025 through the
workspace view's display payload. Locked views show no selector. The cloud
leg is proved per §14.

## 12. S10 — Project settings flag and command palette (VIEW-041, 042; NEWFRONT-736)

**Views switch (VIEW-041).** The project's features settings has a dedicated
Views tab at `/{ws}/settings/projects/{p}/features/views` (owned by project
settings, PSET-004/PSET-043). It shows a page title and one switch that reads
the project's `issue_views_view` and writes `PATCH BASE/` with
`{ issue_views_view }`. Non-admin project members see a not-authorized
notice in place of the switch; only a project admin can flip it. Flipping on
makes the Views tab, the list and every creation entry point appear;
flipping off replaces the list with the VIEW-002 gate on the next visit (and
the already-mounted list is not expected to live-update, §1). The switch is
the destination the VIEW-002 shortcut points at: the shortcut opens the
rendered features settings surface that contains this switch, not a bare
`features` address that renders nothing (this is the seam that fixes
NEWFRONT-247; §3 states how the gate treats the oracle's URL assertion). This area binds the switch's effect to `issue_views_view`
reads in the tab model, the list gate, the palette creation command and the
detail filter row; the switch component itself belongs to NEWFRONT-582.

**Command palette — create (VIEW-042).** A "New view" command sits in the
palette's create group with the two-key sequence `nv`. Visibility and
enablement: a project context exists, the project's `issue_views_view` is
on, and the caller is a project member or admin (guests never see it). It is
hidden in every other context. Activating it opens the same views-owned
create dialog as VIEW-003, scoped to the current project, over whatever page
is open. The command is registered by this area's routes (the shell owns the
registry and the sequence firing, SHELL-086); it appears and disappears as
the project context and flags load, so the oracle settles the page before
asserting. The palette's title rendering may carry a key-sequence badge; the
oracle matches by substring.

**Command palette — jump (VIEW-042).** An "Open a project view" command
(with sequence `ov`) lists the current project's views as a palette
sub-page. It is visible for every role when a project context exists and
`issue_views_view` is on (guests included), because it only navigates.
Selecting a view navigates to that view's detail. The sub-page reads the
project's view list through the same query the list screen uses (one cache
entry), shows a spinner while it loads and a plain empty line when there are
no views; a failed read renders as empty with no error UI (SHELL-093's rule,
reused rather than re-implemented). The palette, search box and sequence
handling remain shell-owned; this slice registers only these two entries and
their page.

**Seams to the issues filter row (ISS-104/105).** The row's Save
view/Save as/Update view triggers are issues-owned. They must call into the
views-owned dialog (§8) with the same payload computed by
`buildViewCreate`/`buildViewPatch` in the issues feature: `name`,
`description`, `rich_filters`, `display_filters`, `display_properties`.
The views dialog must not re-serialize the expression or the display
settings differently, and it adds no `access` (read-only, §1) and no
publish field to that payload. Update view remains
owner/unlocked/changed-gated; default workspace views never update. When this
slice lands, the issues area's minimal save form is replaced by the real
dialog and the issues gate keeps passing.

## 13. S11 — Negatives: no live, keyboard, deep links, desktop, no DnD (VIEW-043–047; NEWFRONT-746)

**No live updates (VIEW-043).** Reading list and detail screens makes one
fetch per mount and never opens a websocket, SSE stream or poller. Another
user's create, rename or delete does not change the current viewer's rows or
title until the page is reloaded or navigated. The acting user's own writes
do update immediately (star, create, edit, delete). Window focus does not
refetch. The no-live rule holds on both the project and the workspace
surfaces and for both list and detail.

**Keyboard (VIEW-044).** Escape closes the project and workspace create/edit
dialogs without saving, and cancels the delete confirmation. In the list
search Escape clears then collapses (VIEW-004). Dialogs expose a logical tab
order: the title field, then the description field, then the remaining form
controls and footer actions (Cancel, then submit), with no focus trap escape
leak and a visible focus ring on every control. The only other keyboard entry
is the palette's `nv` (VIEW-042); the jump entry is `ov`. Both are two-key
sequences typed without a separator, matching the shell registry. There are no views-specific modifier
shortcuts. Old bug: the old dialog's Description field never received focus
on Tab (NEWFRONT-241); intended is the order above, and slices build it.

**Deep links (VIEW-045).** Every list row's link, every switcher pick and
the copy-link action produce a URL of the form `/{ws}/projects/{p}/views/{id}`
or `/{ws}/workspace-views/{id}` (or a default key). Opening such a URL while
signed in with access lands on the same detail state (same view, same
content; the view's own saved state travels in the record and query, not in
the link). A user without access — a guest on an owner-only or
`guest_view_all_features`-off view, or an unknown id — lands on the
missing-view state (§7, §11) rather than the content. The workspace tab title
stays "All Views" for customs; the crumbs carry the view name, which is the
oracle's proof of state.

**Desktop (VIEW-046).** The desktop build serves exactly the same pages,
with no desktop-only views capability, layout difference, runtime banner or
offline behavior. While browsing the list, detail and switcher the page opens
no desktop runtime bridge and issues no runtime-endpoint request; there is no
native-window-handle bridge exposed to the views surface. The desktop
platform layer owns capability flags generally (NEWFRONT-77); this area
consumes none for views. The gate runs the desktop leg with the same
scenarios on the desktop target and asserts the absence of runtime traffic.

**No drag, export or import (VIEW-047).** Rows cannot be reordered by drag
on either list; a drag leaves the sorted order untouched before and after a
reload. View order comes only from the sort control (VIEW-005) and the
server's default. There is no export, import or print action on the list,
the detail, the dialog or the header menus. The cloud export epic
(NEWFRONT-87) may add exports to issues layouts; it must not add one to
views, and the gate's no-export probe covers the views surfaces.

## 14. Cross-epic seams

**Issues epic NEWFRONT-71.** The layout root (ISS-001–006), list
(ISS-007–014), spreadsheet (ISS-015–020), calendar (ISS-021–027), kanban
(ISS-028–043), gantt (ISS-044–055), create modal (ISS-117–123), peek
(ISS-165–172), bulk (ISS-108–115) and the rich filter row (ISS-104/105) are
issues-owned. This area mounts them: the detail passes the view's expression,
order and display; the filter row's save triggers delegate to this area's
dialog with the unchanged payload (§12). Issues defaults that do not apply
to views: the view's display settings override the viewer's personal
preferences while the detail is open; a saved view never edits the personal
user-properties record. Peek and bulk inside a view operate on the
project's issues and are otherwise the issues area's; they must remain
reachable from the layout rows exactly as on the project issues screen. The
create modal from Add item is the issues modal (§8).

**Shell epic NEWFRONT-70.** The sidebar's aggregate work-items row and the
project navigation list (SHELL-048) link to the workspace all-issues default
and to project Views; the views pages highlight the aggregate row on
`/workspace-views/…` and the project tab on the project views routes. The
project header's tab strip (SHELL-055/072–079) shows the Views tab when
`issue_views_view` is on and the role may see it (members, admins, guests);
its default-tab preferences and hiding (SHELL-079) use the project
user-properties record and are shell-owned. Palette creation and jump entries
are registered by this area into the shell's registry (§12, SHELL-086/093);
the shell owns the sequence firing, the sub-page frame, the search box and
the no-hits row. Copy-link and save/delete notices use the shell's toast
component (success/error), never a views-specific one. No views shortcut
beyond the palette sequence exists; the search Escape is local to the
views list.

**Project settings NEWFRONT-88.** PSET-043 owns the views switch and the
features settings route; this area owns the gate's destination and the
flag's downstream effects (§12).

**Desktop NEWFRONT-77.** VIEW-046 is proved with the desktop target; no
desktop slot or flag is added for views. If a desktop-only entry ever
appears, it is a new row, not a silent capability.

**Cloud edition (VIEW-019/020/040).** The edition contract exposes three
named slots (`apps/web_new/src/core/edition/types.ts`:
`issueDetailSidebarAfter`, `desktopSignInCard`, `godModeCloudEntry`) and the
flags `app-rail`, `sidebar-toggle` and `issue-dedupe`. **None of them
covers access, publish or layout selection**, and `apps/web_new` today builds
only the OSS module. The cloud module lives outside this repository's tree
and is swapped in through the `@pidash/edition` alias at build time.

The parity stack's cloud mode (`PARITY_ORACLE_EDITION=cloud`, demonstrated
by `e2e/parity/stack/parity-cloud.test.mjs`) serves the **old cloud app**
behind the proxy. It is an oracle: it can record what the old cloud build
does, but it cannot prove `apps/web_new` behavior. Cloud legs therefore run
for reference only, and the new app's cloud behavior is provable only once a
cloud edition of `apps/web_new` exists.

What each leg covers:
- OSS legs assert absence. VIEW-019: no access selector in the create/edit
  dialogs and a created view stores shared access. VIEW-020: no publish
  entry, no Live marker, no published link. VIEW-040: no workspace layout
  selector; the saved spreadsheet renders.
- Cloud legs on the oracle (reference) exercise: the access selector
  (create a private view, see the lock badge and lock indicator, filter by
  access), the publish flow (publish sets `anchor`, the row shows Live, the
  link copies, unpublish clears both), and the workspace layout selector
  (switching changes rendering and persists in the custom's display
  payload). These scenarios are the target text for the slices; they are
  not evidence about `web_new`.
- Cloud legs on `web_new`, once a cloud edition of the new app is buildable
  in the runner's checkout, prove the same scenarios against `web_new` and
  are what turns these rows `new green` on the cloud edition.

**Slot decision.** At least one of these three rows needs a new edition
slot: access/publish have no seam today, so the cloud build cannot add the
selector or publish entry without one. The access slot is also the **write
path**: `access` is serializer read-only (§1), so the cloud selector's
value cannot ride the OSS create or patch body; the slot must own its own
call. Adding a slot (its name, its position, which edition fills it, and
where its write goes) is a **Decisions-page decision** — the PR that needs
one must name it on "Decisions and open questions" and link this issue. The
spec does not settle it; until that decision lands, the slices build the
OSS legs and the local seam modules only, never an ad-hoc condition on the
OSS build.

**Write path, settled (NEWFRONT-948; Decisions D23/D24 amendments).** Each
cloud write is the owning scope's standard view PATCH with its own key,
and each only counts when the PATCH echo carries the value back — the
server stays the authority, and a server that drops the key fails loudly
instead of faking the state. Access stores `{ access }` on the project or
workspace view route; publish proposes an opaque `{ anchor }` token and
unpublish clears it with `{ anchor: null }` on the project view route.
The public link is the edition's shareable origin plus
`/views/published/{anchor}/`; no public read route serves that address
yet, so the gate proves derivation and copy only. The workspace surface
offers `list` and `spreadsheet` (the old `my_issues` page's known
layouts); the workspace body renders the flat list for any offered layout
until the issues area builds workspace-capable renderers.

**Blocked cloud leg.** If a cloud edition of `web_new` cannot be built in
the checkout the gate runs in, the gate marks each cloud leg
`blocked: cloud build not available in this checkout` (or the specific
reason), files **one** fix sub-issue under the epic naming the blocker, and
does not hold the rest of the area. The row keeps `oracle green`; it does
not become `new green` until its cloud leg passes. OSS legs alone are not
enough for VIEW-019, VIEW-020 and VIEW-040.

**Gate note (NEWFRONT-745, 2026-10-27).** NEWFRONT-948 landed the three
slots, so `web_new` has a cloud edition to run these rows against. Two
limits were measured, not assumed:

- **No oracle half.** The old cloud app renders none of the controls: its
  `ce` modules are empty stubs (`apps/web/ce/components/views/access-controller.tsx`,
  `publish/use-view-publish.tsx`, and `GlobalViewLayoutSelection` in
  `ce/components/views/helper.tsx`), and the private overlay carries no
  views override for either app. A cloud leg is therefore target-only,
  like VIEW-033, and never proves the old behaviour.
- **No server write path.** `IssueView`
  (`apps/api/pi_dash/db/models/view.py`) has an `access` column that
  `IssueViewSerializer.Meta.read_only_fields` marks read-only and **no
  `anchor` field at all**; no migration adds one, and both `partial_update`
  handlers answer 200 while ignoring the key. Measured against the gate
  stack: `PATCH {access: 0}` echoes `access: 1`, and a `PATCH {anchor}`
  comes back with no `anchor` key, on both the project and workspace
  routes.

VIEW-019 and VIEW-020 are therefore `blocked: cloud view write path stores
neither access nor anchor` until NEWFRONT-972 lands; the slot code already
fails loudly rather than faking the state. VIEW-040 needs no server change
— its persist is the custom's `display_filters` PATCH, an OSS endpoint —
so its cloud leg is run work, not a product gap.

## 15. Bug markers (observed → intended → adjudication)

The rows whose oracle cells carry a `bug:` scenario: VIEW-002, 007, 014,
022, 035, 039 and 044. In every case the slice builds the intended behavior
stated here; the gate proves the intended behavior on `apps/web_new` and
treats the pinned scenario as "target, no oracle" (§18).

**VIEW-002 — NEWFRONT-247.** Observed: the gate's "Manage features" button
navigates to the bare `/{ws}/settings/projects/{p}/features` address, which
has no page (only per-feature sub-pages exist), so the user lands on a 404.
Intended: it navigates to a rendered features settings page where the
VIEW-041 switch sits. The gate proves on `web_new` that the shortcut's
target renders with the views switch reachable and no views gate heading,
and that non-admins still see the disabled button on the views gate. The
oracle's bug step also asserts the bare `…/features` URL; if the settings
area renders that index the step passes, otherwise that URL assertion is a
target, no-oracle divergence and is not required to pass on `web_new`.

**VIEW-007 — NEWFRONT-216.** Observed: removing one applied-filter chip
replaces the entire filter object, so every sibling dimension is wiped and
the strip disappears. Intended: removal edits only that dimension (or that
value) and the other chips/rows survive; removing the final one hides the
strip. The gate proves the sibling-survival path in addition to the
single-dimension path.

**VIEW-014 — NEWFRONT-221.** Observed: a failed create resets the form to
blank defaults while the dialog stays open. Intended: the dialog keeps the
user's input for retry, shows the error notice, and creates nothing. The
gate proves input retention after a failed submit.

**VIEW-022 — NEWFRONT-227.** Observed: the retrieve endpoint answers 200
with an empty serialized record (no `id`, blank name, null JSON fields) for
an unknown id instead of 404, so the detail renders a nameless phantom with
no error state and no current-view crumb. Intended: a record without an
`id` is treated as not-found and the missing-view state renders with its
way back.
The gate proves the phantom is gone (error heading, way back, no layout
controls) for an unknown id.

**VIEW-035 — NEWFRONT-232.** Observed: the workspace view store swallows a
failed PATCH, so the dialog stays open on the entered values with no notice.
Intended: the error notice fires and the dialog keeps the input for retry.
The gate proves the notice and that the server record is unchanged.

**VIEW-039 — NEWFRONT-230.** Observed: the workspace list goes straight
from the static defaults to the customs with no skeleton, so the first paint
flickers and the defaults move once customs arrive. Intended: the skeleton
list in §9 renders while customs load, is mutually exclusive with the
defaults and the loaded customs, and is replaced in one commit; with no
customs the defaults stand alone with no special empty art. The gate proves
skeleton → rows (never both) and the defaults-only result.

**VIEW-044 — NEWFRONT-241.** Observed: Tab from the title skips the
Description field and jumps to the footer actions. Intended: Title →
Description → remaining fields → Cancel → submit. The gate proves the
focus order through the dialog (including Description) on both the project
and workspace dialogs.

## 16. Target-without-oracle rows and oracle-recorded gaps

The oracle cells record places where the old app cannot prove the target.
Each is built to the acceptance text and proved on `apps/web_new` as
"target, no oracle" (the NEWFRONT-295 maintainer decision point 1
precedent): the gate runs the scenario against `web_new` and records the row
as `new green (target, no oracle)` instead of requiring an oracle leg.

- **VIEW-033 — tab strip.** Oracle pins absence (dead code in the old app).
  Target: the strip in §10. Proof on `web_new`: defaults + customs tabs,
  active tab auto-scrolled, per-tab menus, member-plus add button, no strip
  for a guest's add affordance. Marked target, no oracle.
- **VIEW-039 — loading skeleton.** Oracle pins the old bug (no skeleton).
  Target: §9's skeleton. Proof on `web_new`: hold the list response and
  observe skeleton rows, then rows, never both. Target, no oracle.
- **VIEW-034/036 — success notices.** The oracle records a silent delete and
  a kept-dialog failure path; the acceptance text asks for notices. Target:
  §10. Proof on `web_new` for the notice and the failure retention.
- **VIEW-038 — locked and guest rules.** Oracle records the old app hiding
  Display on locked views while keeping the filter toggle, and showing Add
  to guests. Target is VIEW-025/026/034/038's acceptance text (§1, §10,
  §11): a locked view hides the layout switcher, Display **and** the filter
  toggle, and Add is offered only to workspace admins and members. Proof on
  `web_new`: locked view has no toggle, no Display, no switcher; a guest
  sees neither the list/detail Add nor the strip's add button. The oracle
  scenarios that pin the old gap are recorded as bugs and not re-run as
  passes.
- **VIEW-040 — layout selector.** OSS oracle pins absence; the cloud leg
  proves presence (§14). If cloud is blocked, the row stays `oracle green`.
- **VIEW-006 — access filter.** OSS oracle pins absence; cloud leg proves
  the Access section and its filtering (§14).
- **VIEW-011 — Live marker.** OSS oracle pins the unmarked row; cloud leg
  proves the marker after publish (§14).
- **VIEW-031 — default row menus.** Oracle records bare links on the list
  and the open/copy pair on the detail. Target: §9 (no list menu, detail
  pair). Proof on `web_new`.
- **VIEW-019/020/027/024 tooltips and cloud-only surfaces** — noted in their
  sections; the unit tests pin the wiring (tooltip, selector, publish) where
  the browser oracle cannot.

Other gaps the oracle cells record, and how the spec resolves them:
- **VIEW-004 outside-click scope** — old detector saw only header presses;
  target is document-wide collapse when empty (§4). The oracle's header path
  still passes.
- **VIEW-025/026 local-only layout edits and flag-independent entries** —
  target is dirty-plus-persist and flag-driven entries (§8).
- **VIEW-027 guest phantom** — the guest test uses a
  `guest_view_all_features` project so the detail renders; Add is hidden
  there (§8). The default guest path is the missing-view state.
- **VIEW-022 owner-only view for a guest** — the oracle's forbidden case is
  a shared view (403). Another member's owner-only view is excluded before
  the permission check: a project guest's retrieve raises (500), a member's
  returns the empty record (200). Target: the same missing-view state in
  every case; the gate proves a 500 read and an empty record both map to it,
  as target, no oracle.
- **VIEW-032 ungated row menus** — target is the oracle-pinned ungated
  render with server enforcement (§9); a client-side hide would lose the
  server's admin delete path.
- **VIEW-042 palette title badges** — matched by substring; target keeps the
  registry rendering (shell-owned).
- **VIEW-013 icon picker** — the old emoji grid never settles under
  automation, so the oracle pins only the Emoji/Icon tabs and proves
  persistence with a glyph pick. Target: both tabs, the emoji pick writing
  `logo_props.in_use = "emoji"`; the gate proves the emoji write on
  `web_new` through the picker's own grid (no virtualization timing) and
  the glyph path on both apps.
- **VIEW-028 issue source** — no `views/{id}/issues/` route exists; the
  oracle proves the set through the generic list. Target: unchanged. The
  gate adds a cheap request-spy probe (the VIEW-046 precedent) asserting the
  detail issues no request to the unrouted path; this is a new gate probe,
  not something the oracle already covers.
- **VIEW-044 no-other-shortcuts** — the oracle asserts Escape, tab order
  and the palette sequence only; the "no other view shortcuts" half is
  source-verified (the sole views key handler is the search Escape). Target:
  keep it that way; a unit test on the views screens' key handlers proves no
  modifier chord is bound, and the gate does not invent a browser probe.
- **VIEW-043 owner-only PATCH** — the watcher is a member and the mutator
  the owner, because that is the only pair the server allows to mutate.
- **VIEW-045 workspace tab title** — target keeps "All Views" and proves
  state through crumbs (§13).
- **VIEW-046 desktop spy** — target is zero runtime traffic (§13).
- **VIEW-047 default order** — "newest first" is the default sort; target
  unchanged (§13).

## 17. Dead code (do not rebuild)

Confirmed dead in the old app and **not** carried into the target:
- The workspace view tab-strip header component is never mounted by the
  workspace list or detail route; no route imports it. Its auto-scroll, tab
  menus and add button are dead. VIEW-033's acceptance text reintroduces the
  strip as a target (§10, §16).
- The workspace layout selection, workspace additional layouts and
  additional header items seams in the community overlay are empty
  components; the workspace detail renders no layout selector on OSS.
- The workspace view-properties write (`POST WS/workspace-views/`) has no
  caller and is not part of the views UX.
- The legacy view-favorite endpoints (`POST BASE/user-favorite-views/`,
  `DELETE BASE/user-favorite-views/{viewId}/`) are routed but uncalled; the
  service methods wrapping them are dead. The app stars views through the
  generic favorites collection (§2, §6). This is the inverse of the
  per-view issues endpoint: that one is uncalled *and* unrouted.
- `getViewIssues` in the project view service targets
  `BASE/views/{viewId}/issues/`, which the server does not route. The
  detail must use the generic issue list with the stored expression (§7).
- The old per-view issue store is live, not type-only: it is constructed by
  the root store and fetches through the generic issue list with the view's
  expression. Only its `getViewIssues` service method (the unrouted
  `/issues/` endpoint) is dead. The workspace issue store class is
  superseded by the filter store; do not rebuild either store, but do keep
  the filter-store fetch wiring the new tree already has.
- The `ce` re-export stores for project views, global views and team views
  are pure re-exports or never-used subclasses.
- Analytics element ids on the header buttons are dropped by decision (see
  the inventory intro).
- Empty-state image assets are presentational only.

## 18. Gate adjudication

At the area gate: run the whole views suite on `apps/web_new` for every
edition the rows are tagged with (OSS; desktop target for VIEW-046; cloud
for VIEW-019/020/040). For every row:
- A row with only oracle-passing scenarios turns `new green` when the same
  scenario passes on `web_new`.
- A row with a `bug:` scenario (§15) turns `new green` only when the
  intended-behavior scenario passes on `web_new`; the old pinned scenario is
  recorded as "target, no oracle" and is not required to run on the old app.
- A target-without-oracle row (§16) turns `new green (target, no oracle)`
  when its `web_new` scenario passes.
- A cloud leg that cannot run is `blocked: <reason>`, files one fix
  sub-issue and does not hold the area (§14); the row stays `oracle green`.
On 100% green the gate adds the views path prefixes to the migrated-route
list (F-09); the epic can close once every one of its rows is green, and
the gate does not move the epic itself. On a real failure it files fix
sub-issues and waits on them.

## 19. Reuse map (survey of `apps/web_new` on `web-new-dev`)

This is a survey of the **new** tree, not the old code. What exists today
and what each slice adds or reworks:

- **Routes.** The project views list mount is a thin placeholder
  (`routes/$ws/_gated/projects/$projectId/views/index.tsx` → `TabAreaMount`).
  The saved-view detail host exists
  (`views/$viewId/{index.tsx,SavedViewScreen.tsx}`) with the issues filter
  row, display control, layout root and expression applied to the issue
  list; its search schema and preload module exist. There is **no**
  `workspace-views` route tree yet. S1 replaces the list mount; S4/S5/S6
  replace the detail host with the full views-owned screen while keeping the
  issues seam; S7–S9 add the workspace routes.
- **Issues feature seams (reuse, don't fork).**
  `features/issues/filters/rich/{savedViewPreload,savedViewQuery,views}.ts`
  already provide the record read, expression, display snapshot, create/
  patch payload builders and offer/ownership rules; `ViewActions.tsx` and
  `IssueFilters.tsx` carry the row and its Save/Update entries (the
  issues gate's minimal form). `features/issues/index.ts` exports
  `IssuesLayoutRoot`, `LayoutSwitcher`, `DisplayControl`, `ListSkeleton`,
  `issuesQueryOptions`, display-prefs helpers, `WorkItemCreateEntry`/
  `WorkItemCreateDialog`, `parseIssueSearch`, the analytics entry and lazy
  list leaves. S5/S6/S9 mount these; S5 reworks the detail host; the views
  dialog must be passed into the row rather than the row opening its own
  minimal form.
- **Kit.** Dialog (title/description/actions), Input, Textarea, Button,
  IconButton, Menu, Popover, Tooltip, Badge, Avatar, Switch, EmptyState,
  Skeleton, ContextMenu, toasts. S1–S9 use these; no new primitive is
  expected except an icon picker (emoji/glyph), which is new and lives in
  the views feature (kit gets a generic popover-plus-grid, not a views
  component).
- **Shell.** `shared/commands` registry + palette + sequences;
  `shared/shell/entries/WorkspaceEntries.tsx` (workspace catalogs);
  contextual command hooks; toasts; the shared copy-link helper
  (`shared/shell/shareLink.ts`); the top bar,
  tab strip and sidebar aggregate row. S10 registers two commands plus a
  picker page and the `nv` sequence; S1/S7 rely on the shell's tab model
  (`features/projects/tabs.ts`, `Views` tab gated by `issue_views_view`)
  and sidebar (`features/projects/sidebar.ts`, aggregate row active on
  `/workspace-views`).
- **Data.** `@pidash/api-client` has the thin `SavedView` schema and the six
  request helpers (project/workspace get, create, update, delete) plus the
  generic favorites and issue-list contracts. NEWFRONT-718 extends the
  schema/helpers (list reads, empty-record retrieve normalisation, favorites
  helper, groups/ordering params). Query keys: `savedViewKeys` exists;
  S1/S7 add list keys and a workspace-scope favorites key.
- **Settings/edition.** The features-views route
  (`settings/projects/$projectId/features/views.tsx`) is a placeholder
  mounting `ProjectSettingsTabMount` with owner NEWFRONT-582; the switch
  lands there and S10 only binds its effect. The tree has only the five
  feature **leaves** (`features/{cycles,modules,views,pages,intake}.tsx`) and
  no rendered `features` index; VIEW-002's shortcut destination is therefore
  a project-settings area route that renders the views switch (PSET-043
  owns the switch there), not a route this area adds. S1 renders the gate
  and its shortcut and links to that settings page; it does not add a
  settings route itself (§3). Edition: `core/edition`
  (OSS module, flags, slots) as §14; desktop target via `src/target.ts`
  (`PIDASH_TARGET`) and the shell's runtime banner rules.
- **Per slice.**
  - S1 (719): new list screen, list query/hook, row component, gate, three
    states; reworks the views list mount; reuses kit list row, badges,
    empty states, skeleton, page title/breadcrumb pattern.
  - S2 (720): new search/sort/filter controls, chips and a pure client
    filter/sort module; reuses kit popover/menu/input/check and the
    query-param or context pattern used by the search hook.
  - S3 (721): new create/edit dialog, icon picker, access selector seam;
    reuses kit dialog/input/textarea/switch/button, issues filter builder
    and display control.
  - S4 (722): new row quick menu, edit/delete/publish dialogs and favorite
    toggle; reuses kit menu/dialog/switch, the workspace favorite write,
    the shared copy-link helper and toasts.
  - S5 (723): new detail header, switcher, lock glyph, missing-view state;
    reworks `SavedViewScreen.tsx`; reuses issues layout root, breadcrumb
    dropdown pattern, kit popover/menu.
  - S6 (724): new detail display/layout/filter wiring and Add item entry;
    reuses issues layout switcher, display control, filter row seam and
    create modal; adds the Save as flow.
  - S7 (725): new workspace views route (list) and rows; reuses the S1 list
    patterns, kit skeleton, search field, empty/no-match rules.
  - S8 (726): new workspace CRUD dialogs and the tab strip; reuses S3's
    dialog and S4's menu/forms, kit horizontal scroller.
  - S9 (729): new workspace detail route/header/body and missing-view state;
    reuses issues all-issues layout root, S5's header pattern, edition seam.
  - S10 (736): palette entries, picker page, settings binding; reuses
    command registry, palette page frame, tab model, project settings mount.
  - S11 (746): no new components; audits and fixes keyboard order, link
    shapes, desktop absence, no-dnd/no-export, no-live; reuses S1–S9 code.

## 20. Row → slice map

Each row belongs to exactly one slice.

| Slice | Issue | Rows |
| --- | --- | --- |
| S1 project list shell, gate, states, row identity | NEWFRONT-719 | VIEW-001, VIEW-002, VIEW-008, VIEW-009, VIEW-010, VIEW-011 |
| S2 list search, sort, filters, chips | NEWFRONT-720 | VIEW-004, VIEW-005, VIEW-006, VIEW-007 |
| S3 create dialog + access selector | NEWFRONT-721 | VIEW-003, VIEW-013, VIEW-014, VIEW-019 |
| S4 favorite, row menu, edit, delete, publish | NEWFRONT-722 | VIEW-012, VIEW-015, VIEW-016, VIEW-017, VIEW-018, VIEW-020 |
| S5 project detail, switcher, lock, issue body | NEWFRONT-723 | VIEW-021, VIEW-022, VIEW-023, VIEW-024, VIEW-028 |
| S6 detail layout, display, filters, Add item, Save as | NEWFRONT-724 | VIEW-025, VIEW-026, VIEW-027, VIEW-048 |
| S7 workspace list, defaults, custom rows, loading | NEWFRONT-725 | VIEW-029, VIEW-030, VIEW-031, VIEW-032, VIEW-039 |
| S8 workspace CRUD + tab strip | NEWFRONT-726 | VIEW-033, VIEW-034, VIEW-035, VIEW-036, VIEW-037 |
| S9 workspace detail + cloud layout | NEWFRONT-729 | VIEW-038, VIEW-040 |
| S10 settings flag coupling + palette | NEWFRONT-736 | VIEW-041, VIEW-042 |
| S11 negatives | NEWFRONT-746 | VIEW-043, VIEW-044, VIEW-045, VIEW-046, VIEW-047 |
| Contracts | NEWFRONT-718 | endpoints for all of the above (no rows) |
| Area gate | NEWFRONT-745 | proves all 48 (no rows) |

Coverage check: 6+4+4+6+5+4+5+5+2+2+5 = 48 distinct rows, no duplicates.

## 21. Verification and local checks

This issue adds only this file (plus no code). The applicable local checks
are the similarity check (`node apps/web_new/checks/check-similarity.mjs`,
prose is token-compared against old TS/TSX/CSS so no ≥50-token block may
match) and the paths gate (`git fetch origin web-new-dev`; export
`HEAD_REF`; run `.github/scripts/newfront-check-paths.mjs` from the base
branch). No package typecheck/lint/test applies because no package changed.
The evidence for each command and its exit code is recorded in the
workpad and the PR.
