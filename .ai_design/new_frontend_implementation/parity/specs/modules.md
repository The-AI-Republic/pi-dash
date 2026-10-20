# Modules area spec — project modules, detail, analytics, archives and entry points (NEWFRONT-791)

Build spec for inventory rows **MOD-001–MOD-043**. It is written for the
modules contracts issue (NEWFRONT-792), the fifteen build slices
(NEWFRONT-793–807) and the area gate (NEWFRONT-808), all under the Modules
epic **NEWFRONT-82**. Every section names the rows it covers; §14 maps each
row to its section and its oracle scenario. A slice author should be able to
build its rows from this file without opening the old app.

Prose only. The old routes, the modules component folder, the module and
module-filter stores, the timeline store, the module services, the hooks and
the API views were read as reference, never as a source. Nothing here is a
translation of those files, and no component, hook, store, class, style, icon
or asset is reused. Two kinds of literal text appear deliberately:

- **Contract text** — API paths, request/response fields, enum values, URL
  keys and storage keys. These must match the server exactly and are named
  verbatim.
- **Parity labels** — a short user-visible string the oracle asserts
  literally (a heading, a menu entry, a toast). Each is quoted and marked
  _pinned_. Everything else is described by meaning, so slices write their
  own copy in the product's voice.

Old sources consulted (reference only): the project modules list and detail
routes with their desktop and mobile headers; the archives modules tab; the
project settings modules feature page; the components for the modules list,
list item, board card, list action bar, view header, filters dropdown,
ordering control, applied-filter strip, module form, create/edit/delete/archive
dialogs, row quick actions, archived modules root/header/view, module peek
overview, analytics sidebar root, issue-progress section, progress statistics,
links section and the modules timeline; the module, module-filter, issue-module
and modules-timeline stores; the module and module-archive services; the
module and module-filter hooks; the command-palette module commands; and the
API module views, serializers and URL routes. Workspace-module empty-state
assets were read only to inventory the states; no art is reused.

Current-state source of truth for what is already built (survey of
`apps/web_new` on `web-new-dev`, head `c6ad77aaf`): the module detail route
host mounts `features/issues/list/EntityListScreen` for the issues body
(`routes/$ws/_gated/projects/$projectId/modules/$moduleId.tsx`), the modules
list route is still a `TabAreaMount` placeholder, and the shared issues,
dropdown, picker, analytics, archives, project-settings and palette pieces
named in §12 already exist. §12 states which existing piece each module
surface uses and which sibling issue owns it.

## 1. Mental model (MOD-001–043)

A **module** is a project-scoped container for work items, with its own
lifecycle status, lead, member list, date range, progress summary, attachments
called **links**, and a place in the project's archives once it is finished.
Modules are not cycles and not saved views: they group work without moving
items out of the project list, and an item may live in several modules at once.

Three structural facts shape every screen:

1. **Two levels of navigation.** The project **modules list** and an
   individual **module detail page** are different screens with different
   headers. The list header carries search, ordering, filters, layout and the
   create action. The detail header carries breadcrumbs, a module switcher,
   the work-item count badge, the work-item layout/display/filter controls,
   analytics, and the collapse toggle for the analytics sidebar. Do not merge
   them into one header.
2. **The issues body is hosted, not owned.** A module detail page shows the
   module's work items using the same layouts, display options and filters as
   the project issues screen. The modules area owns the frame (breadcrumbs,
   switcher, badge, tabs, header actions) and the module-specific sidebar. The
   layout renderers, row and card components, quick actions, create modal and
   filter engine belong to the Issues area and are reused (see §12). The
   module context differs in only three ways: the data scope is
   `module`, grouping by “Module” is never offered, and empty states are
   module-worded.
3. **One sidebar, two mounts.** The module sidebar — identity fields plus the
   progress section and the links section — is **one component tree**. The
   detail page mounts it, and the **peek panel** mounts the same tree with a
   read-only flag. The peek never re-implements fields, progress or links, and
   the detail page never has a second variant. “Read-only” means every
   editable control renders disabled or as text, the breakdown entries do not
   write filters, and no create/edit/delete action appears; it does **not**
   mean a different layout. If a slice cannot express a difference as a prop,
   it extends the shared component rather than forking it.

**Roles.** Three project roles matter:

- **Admin** and **member** are editors: they create, edit, archive, restore,
  delete, favorite, drag the timeline and edit work items in the module.
- **Guest** is a read-only project participant: they see the list, the detail
  page, the sidebar and the peek, but every mutation control is absent or
  disabled. Guests see no favorite star, no Add action, no create dialog, no
  row-menu edit/archive/delete/restore, and no command-palette create command.

Workspace admins who are not project members follow the project access rules
of the Issues area; module rows do not invent a separate ladder.

**Editions.** No module behavior is edition-specific. The OSS, cloud and
desktop builds serve the same routes, the same endpoints and the same
capabilities (MOD-042, MOD-043). Module code must not read edition flags to
gate a module surface.

## 2. The modules list: frame and header (MOD-001–007)

### 2.1 Frame, breadcrumbs and page title (MOD-001)

Route: `/{ws}/projects/{projectId}/modules` (canonical trailing slash is
irrelevant to routing).

The screen has the project chrome above it — project breadcrumb, project tabs
or sub-navigation and the project-level header the other project areas use —
then the **modules header**:

- Breadcrumb: the project name, then a trailing crumb _pinned_ **“Modules”**.
  On the list, the project crumb and the modules crumb are not links to a
  deeper module level (the list is the level). The tab title is
  `{projectName} - Modules` — _pinned_ shape: the project name, a hyphen and
  the word “Modules”.
- The header holds the create action (§2.3), search (§2.4), ordering (§2.5),
  filters (§2.6), and the layout switcher (§3.4), in the order the desktop
  design uses: primary action at the leading edge of the control cluster,
  then search/order/filter/layout towards the trailing edge. The exact
  arrangement may be redesigned; every control must be reachable on desktop
  and the small-screen variants must exist as described in their sections.
- The body is the module list for the active layout. Every **non-archived**
  module of the project appears, regardless of status. Archived modules never
  appear here (they live under §7.3).

Data: one list read for the project (`GET modules/`). The list is client
sorted and filtered; search, filters, ordering and layout are display state,
not server queries.

**States:** loading (§3.1), flag-off (§2.2), zero modules (§3.2), no match
(§3.3), rows or cards or bars (§3.4–§3.6). A flag-off project shows the gate
instead of any list state; it must not flash the skeleton or an empty state
first.

### 2.2 Feature-off gate (MOD-002)

When the project's modules feature is off, the list body is replaced by a
gate: illustration or art, an explanation that modules are **not enabled**
(the oracle matches the phrase “not enabled” _pinned_ as a substring of the
explanation), and a button _pinned_ **“Manage features”**.

- The button navigates to
  `/{ws}/settings/projects/{projectId}/features/` (the project features
  settings page). This is the same destination the project settings area owns
  (NEWFRONT-582, PSET-046); the modules area only links to it.
- For an admin the button is enabled. For every non-admin (member and guest)
  it renders **disabled**, because only an admin can flip the project feature
  switch. It is still visible so the explanation makes sense.
- No rows, no chips, no layout switcher, no search and no create action
  render in the gated state. The gate is exclusive: it never appears above
  or beside rows.

The gate reads the project record's feature flags. It does not probe the
modules endpoint and does not treat a 403 or 404 from that endpoint as
“feature off”.

### 2.3 Add module (MOD-003)

The list header carries the create action:

- Label _pinned_ **“Add Module”** on regular and large screens, _pinned_
  **“Add”** on small screens (the label switches with the viewport, not with
  the layout).
- Activating it opens the **create dialog** (§7.1).
- Admins and members see it; **guests never see it**. Everyone sees the same
  list otherwise.
- Dismissing the dialog (Escape or Cancel) writes nothing.

Server-side creation validation lives in §7.1; the header action only opens
the dialog.

### 2.4 Search (MOD-004)

A collapsible name search, present on the modules list header and on the
archived-modules header (§7.3). Behavior:

- Collapsed, it is an icon-only control (magnifier). Activating it expands a
  text field and **moves focus into it**.
- Typing filters the visible modules by **case-insensitive name substring**,
  live, over the already-fetched list. It does not send a request and does
  not remove rows from the server.
- First Escape (while the field has text) **clears the query**, keeps the
  field open and keeps focus so typing can continue.
- Second Escape (empty field) **collapses** it and returns the unfiltered
  list.
- Clicking outside collapses the field **only when the query is empty**. With
  text present, an outside click leaves the field open and the filter
  applied.
- Search combines with the filter selections (§2.6): the visible set is the
  intersection.

When a search or filter hides everything, the body shows the no-match state
(§3.3), never the zero state.

### 2.5 Ordering (MOD-005)

An ordering control offers exactly these keys, in this order (labels
_pinned_):

1. **Name**
2. **Progress**
3. **Number of work items**
4. **Due date**
5. **Created date**
6. **Manual**

Rules:

- The default is **Name, ascending**. The trigger shows the active key's
  label (the direction is a separate affordance; it does not change the
  trigger label).
- **Name** — lexicographic on the module name; ties keep a stable order
  (creation order).
- **Progress** — completion share, `(completed + cancelled) / total`, where
  `completed` is the server's completed-work-item count and `cancelled` its
  cancelled count. A module with no work items sorts as zero progress. Ties
  keep a stable order.
- **Number of work items** — the server's `total_issues`, ascending.
- **Due date** — the module's target date. Modules without a target date sort
  after dated ones under ascending order; among themselves keep a stable
  order.
- **Created date** — creation order (`created_at`), which the oracle proves
  by creating modules in a deliberately non-alphabetical order.
- **Manual** — the module's stored `sort_order`, ascending. This is the
  placement edited by timeline row reordering (§6.3).

Direction: **Ascending** and **Descending** are offered and toggle the
current key. **Manual has no direction control**: while Manual is active the
direction options are hidden and the stored order is used as-is.

Changing the key re-sorts immediately. Ordering is display state for the
current user and page; the list read returns the server's own order, and the
client sorts. Manual placement is the one key whose order is persisted by the
server (via the timeline reorder in §6.3).

### 2.6 Filters (MOD-006)

The filters control opens a menu with its own search box. Groups, in order:

1. **Favorites** — a single on/off toggle. Active list only; the archived
   header omits this group.
2. **Status** — the six lifecycle states, each with its colored indicator:
   **Backlog, Planned, In Progress, Paused, Completed, Cancelled**. The
   archived header omits this group.
3. **Lead** — workspace/project members. The current user appears first,
   labelled _pinned_ **“You”**. Five options are shown initially; when more
   exist, a **view-all** expander reveals the rest. Multi-select.
4. **Members** — the same member list with the same “You” first ordering,
   five-then-expand, multi-select.
5. **Start date** — relative choices _pinned_ **“1 week from now”**,
   **“2 weeks from now”**, **“1 month from now”**, **“2 months from now”**,
   and **“Custom”**. Relative choices select modules whose start date falls
   inside the window ending at that offset from today. Custom opens a
   date-range dialog (§2.6.1).
6. **Due date** — the same relative choices plus Custom, evaluated against
   the target date.

Semantics:

- Options inside a group are OR-ed; groups are AND-ed; the result is then
  AND-ed with the name search.
- Favorites narrows to modules this user favorited.
- Lead matches modules whose lead is a selected user.
- Members matches modules carrying **any** selected member.
- Date filters select modules whose corresponding date lies inside the chosen
  range. A module with a missing date never matches a date filter.
- The menu's search box narrows the offered options (for example typing
  “cancel” leaves only the Cancelled status option). It does not filter the
  module list itself.
- Closing the menu keeps the selections. The filters control shows an active
  marker while any filter is applied.
- Clearing every filter restores the full (searched) list.

**2.6.1 Custom date range.** The Custom entry opens a range dialog. Its
default is **today through one month from today**. Applying the range closes
the dialog and applies the filter; cancelling changes nothing. Choosing
Custom again while a custom range is applied removes the custom range
(the entry acts as a toggle), while picking any relative option replaces it.
Relative and custom choices in the same dimension are mutually exclusive:
one chip per dimension showing the current choice (§2.7). The applied
range raises one chip under the relevant dimension (§2.7). The picker is the
shared date-range control described in §12; do not build a module-only
calendar.

Filter state is **per project and per user**, persisted in the module
user-properties record alongside display state (§13.3). On first visit the
defaults are: no filters, layout `list`, ordering `name` ascending.

### 2.7 Applied-filter chips (MOD-007)

While any list filter is active, a chip strip sits above the list. One chip
per **active dimension**, in this order and with these keys (the key is the
stable identity the oracle reads):

- `status` — the selected statuses, each with its colored dot.
- `lead` — the selected leads, each with an avatar or user mark.
- `members` — the selected members.
- `start date` — the chosen relative label or the custom range.
- `target date` — the same for the due date dimension.
- `Modules` — the favorites chip; its key is the word “Modules”, not
  “favorites”.

Behavior:

- Removing a single value updates the chip and the list without touching the
  other values in the same chip or the other chips.
- Removing a whole dimension (its own remove control) clears every value in
  it and removes that chip.
- A **clear-all** control resets every filter and hides the strip.
- Removing the last filter hides the strip entirely. While nothing is
  applied, the strip does not render (not even an empty container).
- Chips never represent the name search. Search has its own field and is
  cleared there.

## 3. List states and layouts (MOD-008–012, 020)

### 3.1 Loading (MOD-008)

While the first list fetch is in flight, the body shows a **skeleton that
matches the active layout**: row-shaped for list, card-shaped for board,
bar/row-shaped for timeline. The skeleton is exclusive — it never renders
with rows, cards, bars, a zero state or a no-match state, and it gives way to
content in the same layout. A reload with a different active layout shows
that layout's skeleton.

The header controls may render during loading if their project data is
available; the create action must not write anything until its dialog is
submitted.

### 3.2 Zero modules (MOD-009)

When the project has no modules at all (and no search or filters are active),
the body shows an onboarding state:

- themed art, a short title, and an explanation of what modules are;
- a creation action that opens the create dialog.

The creation action is **enabled** for admins and members, and **disabled**
for guests and any other read-only role. A guest sees the same art and copy
with a disabled action; no separate guest copy is needed.

Zero state is distinct from no match: if modules exist but the search or
filters hide them, the no-match state (§3.3) renders instead.

### 3.3 No match (MOD-010)

When modules exist in the project but the current search/filters leave
nothing visible:

- a search-themed empty state with a heading and a hint that tells the user
  to adjust the search or filters (the oracle asserts the hint matches
  `/adjust/i`, so the sentence must include a word such as “adjust”);
- the underlying modules are untouched, and clearing the search/filters
  restores them in their prior order.

Do not replace the header in the no-match state; the search and filter
controls stay available so the user can undo the narrowing.

### 3.4 Layouts and switcher (MOD-011)

Three layouts are offered:

- `list` — one row per module;
- `board` — gallery-style cards in a grid;
- `timeline` — a Gantt-like chart (§6).

The switcher offers exactly these three. The oracle confirms the option count
is 3.

- Changing the layout re-renders the same module set in the new layout,
  immediately, with no refetch.
- The choice is **persisted per project per user** and survives a reload. In
  the new app this is the module user-properties record (§13.3), the same
  place the detail page stores its work-item layout; the old app used a
  browser key, which is not a contract. Do not persist it on the project.
- **Small screens** hide the desktop switcher and show a compact layout menu
  with exactly two options, _pinned_ **“List layout”** and _pinned_
  **“Gallery layout”** (list and board). Timeline is not offered on small
  screens from the list.
- Switching layout does not change filters, search, ordering or favorites.

### 3.5 List row (MOD-012)

Each list row shows, in reading order:

1. **Progress marker** — a circular ring whose fill reflects completion, with
   the integer **percentage** (floored) inside. Special cases:
   - when every work item is done (100%), a **check** glyph replaces the
     number;
   - when the module's status is **`completed`** but completion is below
     100%, an **alert** (“!”) glyph replaces the number. (`cancelled` shows
     the percentage, not the alert.)
   The percentage uses the same completed-and-cancelled share as ordering
   (§2.5). The ring is decorative; the accessible name carries the value.
2. **Module name**, linking to the module detail page
   (`/{ws}/projects/{projectId}/modules/{moduleId}`). The link is a real
   anchor so copy/open semantics work.
3. **Info control** that opens the peek panel (§5) instead of navigating.
4. Inline controls: status pill (§4.2), date range (§4.1), lead (§4.3), star
   (§4.4), and the row quick menu (§4.5).

Responsiveness: on desktop the peek trigger and the row actions appear on
hover (and stay reachable by keyboard focus); on small screens the hover
actions are replaced by a compact row menu that is always visible, and the
name, progress marker and peek trigger remain. A row is a
navigation target on its name/link, not on the whole surface (the controls
inside must never navigate).

**Archived rows** reuse this exact row component (§7.3) but change the click
behavior: clicking anywhere on an archived row that is not a control opens
its **archived peek** and keeps the address on the archives screen
(`/archives/modules`), rather than navigating to the active detail route.

### 3.6 Board card (MOD-020)

Each board card shows:

- the module **name**, truncated when it overflows, with a **tooltip**
  carrying the full name;
- the **status pill** with the current status label;
- the **peek trigger**;
- the **work-item count** as done-over-total (e.g. `1/2`) using the server's
  completed and total counts;
- the **lead**: avatar when set, otherwise the shared no-lead marker;
- a **segmented progress bar**: one segment per work-item state group present
  in the module, colored by that state group, widths proportional to counts.
  A module with no work items renders an empty bar, not a zero-width segment;
- the **date range** control (same control as the row, §4.1);
- the **favorite** star and the **quick menu**.

Navigation: clicking the card **body** navigates to the detail page; clicking
any control inside (status, dates, favorite, menu, peek, name link) performs
that control's action and never navigates.

While the peek is open, the board grid narrows its column count so rows stay
readable; closing the peek restores the wider grid (§5).

## 4. Row and card inline editing (MOD-013–018)

All inline edits share one behavior contract:

- Editors only; guests see the control disabled (star absent entirely).
- Archived modules render these controls disabled.
- A successful write shows a success toast — for module field edits this is
  _pinned_ **“Module updated successfully.”** — and updates the UI
  immediately (optimistic where the old flow was optimistic, reconciled with
  the server response).
- A failed write shows an error toast and restores the previous value; it
  never leaves the UI showing a value the server rejected.

### 4.1 Start and end dates inline (MOD-013)

The date-range control shows the saved range formatted for the user's locale,
or the placeholders _pinned_ **“Start date”** and **“End date”** when a side
is empty.

- Picking a range saves both sides immediately via
  `PATCH modules/{moduleId}/` with `start_date` and `target_date`.
- Re-opening the calendar and clicking a single day **re-anchors both ends**:
  the first click sets the start, the second sets the end. Clicking the
  already-selected single day again **clears both dates** and saves the empty
  range.
- There is no per-side clear button on the row/card calendar; clearing is
  the re-click gesture above (inventory note NEWFRONT-308). The detail
  sidebar (§9.1) uses the shared date control with the same semantics; a
  clear action may be offered there because the sidebar is a full editor
  surface only if the shared control provides it — do not invent a
  module-specific calendar.
- Server rules: `start_date` must not be later than `target_date`
  (400 otherwise). The client surfaces the error and keeps the old value.
- A guest cannot open the picker; the control renders disabled.

### 4.2 Lifecycle status (MOD-014)

Six statuses exist, with these wire values and colors (the color is the
contract the oracle reads from the rendered pill):

| Wire value | Label | Color |
| --- | --- | --- |
| `backlog` | Backlog | `#a3a3a2` |
| `planned` | Planned | `#3f76ff` |
| `in-progress` | In Progress | `#f39e1f` |
| `paused` | Paused | `#525252` |
| `completed` | Completed | `#16a34a` |
| `cancelled` | Cancelled | `#ef4444` |

- The pill shows the current status in its color. The menu lists all six in
  that order.
- Picking a status saves immediately with `PATCH modules/{moduleId}/`
  (`status`), a success toast, and the same rollback rules as §4.
- New modules default to `backlog`.
- Only `completed` or `cancelled` modules may be archived (§7.4). This is
  reflected in the row menu's archive entry being disabled with an
  explanation for other statuses.
- The same control appears in the row, the card and the analytics sidebar
  (§9.1); all three read and write the same field.

### 4.3 Lead (MOD-015)

The row and card show the lead's avatar when a lead is set. With no lead they
show a neutral person glyph and a tooltip explaining that the module has no
lead (_pinned_ tooltip text contains **“No lead”**). Reading the lead is open
to every role; changing it is an editor action available in the row menu's
edit dialog and the sidebar, not as a separate inline picker on the row.

### 4.4 Favorite (MOD-016)

A star control on each row and card toggles this user's favorite.

- Admins and members see the star; **guests see no star at all** (not a
  disabled star).
- Favorite:
  `POST user-favorite-modules/` with body `{"module": "<moduleId>"}` →
  `204`. The server stores a user favorite row for the module.
- Unfavorite: `DELETE user-favorite-modules/{moduleId}/` → `204`.
- The star reflects state immediately and shows _pinned_ toasts
  **“Module added to favorites.”** and
  **“Module removed from favorites.”**.
- Favorites also feed the `Favorites` filter (§2.6) and the shell's sidebar
  favorites list (SHELL-057). On the user's **first-ever** module favorite,
  the sidebar favorites section expands so the user sees the new entry. A
  subsequent favorite does not force an already-expanded group open; it must
  not collapse it either.
- Archived modules cannot be favorited; archiving a module removes its
  favorites server-side.

### 4.5 Row and card quick menu (MOD-017)

The overflow menu content depends on role and archive state, in this order:

**Editor, active module**

1. **Edit** — opens the edit dialog (§7.2).
2. **Open in new tab** — §4.6.
3. **Copy link** — §4.6.
4. **Archive** — opens the archive confirmation (§7.4). **Disabled** unless
   the status is completed or cancelled, with an explanatory description that
   includes the phrase **“completed or cancelled”**.
5. **Delete** — opens the delete confirmation (§7.5).

**Editor, archived module** (archived screen only)

1. **Open in new tab**
2. **Copy link**
3. **Restore** (§7.6)

No Edit, Archive or Delete entries on an archived module.

**Guest (any state)**

1. **Open in new tab**
2. **Copy link**

Each entry performs its own action; the menu closes on selection. On small
screens the trigger is a compact icon button on the row/card.

The same entries appear in the **detail header overflow menu** for an active
module, with Edit, Copy link, Open in new tab, Archive and Delete, gated the
same way.

### 4.6 Copy link and open in new tab (MOD-018)

- **Copy link** writes the full absolute module URL,
  `{origin}/{ws}/projects/{projectId}/modules/{moduleId}`, to the clipboard
  and shows _pinned_ **“Module link copied to clipboard.”**. If the clipboard
  API rejects, show the error toast and do not claim success.
- **Open in new tab** opens that same URL in a new tab, standalone: the tab
  loads the module detail page (not the list with a peek). It does not change
  the current tab's address.

## 5. Module peek panel (MOD-019)

The peek is the module sidebar (§9) in a side panel over the current list or
archived screen.

- **Trigger.** The info control on a row or card, a click on an archived row,
  or the address itself. Opening sets the URL key **`peekModule`** to the
  module id, on the current list address:
  `.../modules?peekModule={moduleId}` or
  `.../archives/modules?peekModule={moduleId}`. Do not use a path segment.
- **Data.** The peek reads the same detail endpoint as the detail page
  (`GET modules/{moduleId}/`), and the archived peek reads
  `GET archived-modules/{moduleId}/` on the archived screen. It does not
  reuse the list row's partial record for fields the list does not carry.
- **Content.** The same sidebar component as the detail page — identity
  fields, counts, progress and links — with the **read-only flag**: fields
  disabled, breakdown entries not clickable, no link mutation actions. The
  panel has its own close control, and Escape closes it.
- **Closing** removes `peekModule` from the address (leaving the other search
  keys such as layout untouched) and hides the panel. The panel's own close
  control and the browser back gesture both work.
- **Shareable.** Loading a URL that already carries `peekModule` opens the
  same module's panel in the same state, fresh. If the module no longer
  exists, the panel shows the missing-module state (not a blank panel) and
  the address is left reachable for the user to close.
- **Layout effects.** On the board layout, an open peek reduces the grid
  column count; closing restores it. On the list and timeline it overlays or
  narrows the body without changing the active layout.
- **Interplay with delete (bug disposition).** Deleting the module whose peek
  is open must remove `peekModule`, close the panel and leave the user on the
  plain modules list. The old app leaves the deleted id in the address
  (NEWFRONT-329, §15).

Peek for archived modules is read-only in every role, including editors.

## 6. Timeline layout (MOD-021)

Timeline is the third modules-list layout. It plots each module as a bar on a
shared date axis.

**Sidebar column.** Each row shows a status glyph (the status color or icon
for the module's status) and the module name linking to its detail page. Row
order follows the active ordering (§2.5).

**Bars.**

- A bar spans the module's start date through its target date (inclusive) and
  uses the **status color** from §4.2.
- Hovering a bar shows a tooltip with the module name and a date summary
  (e.g. “Sep 01, 2026 - Oct 18, 2026”). The oracle asserts the name is
  present; the date format is locale-aware.
- A module without a complete date range has no bar. The row still appears so
  the module is reachable.

**Dragging.**

- Dragging a bar horizontally **shifts both dates** by the same number of
  days and saves with `PATCH modules/{moduleId}/`.
- Dragging the **right** edge moves only the target date.
- Dragging the **left** edge moves only the start date.
- A drag that would invert the range is clamped (start cannot exceed target,
  and vice versa).
- After a drop, the UI updates immediately and the server write is
  reconciled; a failure restores the previous dates with an error toast.

**Reordering.**

- Row drag-reorder is enabled **only** when ordering is **Manual** and the
  user is an editor. Reordering writes `sort_order` for the affected modules
  (the server's manual ordering; the old app PATCHes the moved rows' order).
  The new order persists across reloads and is what the Manual sort reads.
- Under any other ordering (Name, Progress, …), attempting to drag a row
  does **not** reorder; it shows a warning that manual placement is required
  (the message names manual sort).
- Under Manual ordering, dragging a row onto itself or a no-op drop raises no
  warning and changes nothing.

**Permissions.** A guest sees the timeline and bars but any drag attempt is a
no-op: no dates change, no toast. Reorder likewise does nothing.

**Not a board.** Timeline bars are the only draggable module surface (§11,
MOD-041). Adding new modules happens from the header action, not by
“dragging in a block”.

## 7. Dialogs, archive and the archived screen (MOD-022–028)

### 7.1 Create dialog (MOD-022) and submit (MOD-023)

**Entry points:** the list header Add action (§2.3), the zero-state action
(§3.2), and the command palette's **New module** command (§10.2). All three
open the same dialog component.

Heading _pinned_ **“Create module”**. Fields:

1. **Project** — only in creation mode. A project picker restricted to
   projects where this user may create modules (admin or member of that
   project, modules feature on). The project the user opened the list from is
   pre-selected. Guests never see the dialog. A user with only guest standing
   on the current project cannot open it in the first place.
2. **Title** — text input, required, **maximum 255 characters**. Inline
   validation: submitting empty shows an error containing the word
   **“required”**; a 256-character title shows an error containing
   **“255”**. The dialog stays open and nothing is written. Typing a valid
   title clears the error. Initial value empty.
3. **Description** — free text (multi-line). Initial empty.
4. **Start date / end date** — the shared date-range control, with the
   placeholders “Start date” / “End date” when empty. Initial empty.
5. **Status** — required select among the six statuses; initial
   **Backlog** (_pinned_ the control shows “Backlog”).
6. **Lead** — single member picker; unselected shows _pinned_ **“Lead”** as
   placeholder.
7. **Members** — multi-member picker; unselected shows _pinned_
   **“Members”**.
8. **Cancel** and **Create Module** actions (submit label _pinned_
   **“Create Module”**).

**Tab order (intended).** Keyboard order is the visual field order:
title (1), description (2), the date control (3), status (4), lead (5),
members (6), cancel (7), submit (8). The date control is **third** — the old
app renders it without a tab index so it is reached after Submit
(NEWFRONT-330, §15). This is a bug disposition: the new app must make the
date control tabbable in place. The project picker, when present, comes
before the title.

**Cancel/Escape** close the dialog without writing. After a **successful**
submit the dialog closes, a success toast shows, the list refreshes (the new
module appears under the active search/filters/order if it matches), and the
form state resets so the next open is blank with defaults.

**Failure (MOD-023, intended).** On a failed create the dialog **stays open**,
an error toast shows, and **every field keeps exactly what the user entered**
(title, description, dates, status, lead, members, project). Nothing is
written. The old app resets the form to defaults on failure (NEWFRONT-328,
§15); the new app must not.

**Create request.**

```
POST /api/workspaces/{ws}/projects/{projectId}/modules/
{
  "name": "...", "description": "...",
  "start_date": "YYYY-MM-DD" | null, "target_date": "YYYY-MM-DD" | null,
  "status": "backlog|planned|in-progress|paused|completed|cancelled",
  "lead_id": "<uuid>" | null,
  "member_ids": ["<uuid>", ...]
}
```

A successful create returns the created module (201). Duplicate names in the
same project are rejected by the server (`{"error": "Module with this name
already exists"}`); show the error toast, keep the dialog open with input
intact. `start_date` later than `target_date` is rejected.

### 7.2 Edit dialog (MOD-024)

**Entry:** row/card menu **Edit**, or the detail header menu. Heading _pinned_
**“Update module”**.

- Opens **prefilled** with the module's current title, description, date
  range, status, lead and members. **No project picker** in edit mode.
- Saving (`PATCH modules/{moduleId}/`, partial) shows a success toast and
  closes the dialog; the row/card, list, sidebar and detail header reflect
  the new values.
- A **failed save** shows an error toast, keeps the dialog open, and keeps
  the user's edits intact (intended; NEWFRONT-328 covers both modes).
- Cancel/Escape writes nothing.
- Guests cannot open it.

### 7.3 Archived-modules screen (MOD-027)

Route: `/{ws}/projects/{projectId}/archives/modules`. The screen's **shell,
tab strip, badge and title** belong to the Archives area (NEWFRONT-698,
ARCH-001–003); the **archived-modules screen itself is owned by NEWFRONT-702
(ARCH-020–025)**. The Modules area must **not fork** the archived screen, its
search, its filters, its chip strip, its row reuse, its empty states or its
peek. MOD-027 records the modules-side requirements that NEWFRONT-702
implements; the Modules slices consume that screen and only add what the
archived screen lacks for a module-specific rule (nothing in this spec needs
such an addition).

Requirements the archived screen must meet (so the module slices can rely on
them):

- It sits under the archives tab strip with the Modules tab active. The tab
  strip follows the archives shell; do not render a second strip.
- Header: the archives area's title plus the modules header controls — name
  **search** (same behavior as §2.4), **ordering** (same keys as §2.5), and
  **filters** limited to **Lead, Members, Start date, Due date** (no
  Favorites, no Status — an archived module is by definition finished).
- Applied filters produce the same chip strip and removal rules as §2.7.
- Rows reuse the standard module list row (§3.5) read-only, with archived
  behavior: clicking a row opens the **archived peek**
  (`?peekModule=`) and keeps the address on `/archives/modules`; the row menu
  offers Open in new tab, Copy link and Restore (§4.5).
- Data: `GET archived-modules/` for the list and
  `GET archived-modules/{moduleId}/` for the peek. The live detail endpoint
  returns 404 for archived modules; do not use it.
- **Empty states:**
  - **zero** — no archived modules at all: onboarding copy explaining that
    archiving finished or dropped modules fills this screen.
  - **search** — archived modules exist but the search hides them: search
    guidance.
  - **filters** — archived modules exist but filters hide them: filter
    guidance with a way to clear.
  The three are distinct; the oracle reads which kind is showing.
- A skeleton shows while the archived list fetches. **Cold entry must not
  hang**: a direct load or reload of `/archives/modules` loads its own data
  and leaves the skeleton (old bug NEWFRONT-228 / NEWFRONT-239, owned by the
  archives area; see §16).
- Viewing is open to every project role including guests (archives ARCH-028);
  restore is the only module mutation here and needs admin or member.

### 7.4 Archive (MOD-026)

**Entry:** row/card menu Archive or the detail header menu, enabled only for
`completed` or `cancelled` modules.

- The confirmation dialog names the module and notes that an archived module
  can be **restored** later (the oracle looks for the word “restored” in the
  body). Cancel changes nothing.
- Confirming calls `POST modules/{moduleId}/archive/`. Success returns
  `{ "archived_at": "<iso>" }`, shows a success toast whose title is
  **“Archive success”** and whose body points at the project archives, and
  **navigates to the modules list** (`/modules`). The module disappears from
  the active list.
- Archiving removes the module's favorites server-side; the sidebar favorites
  entry disappears on its next read.
- **Failure (intended).** A failed archive shows an **error** toast, makes no
  navigation, keeps the module active, and closes or keeps the dialog
  without pretending success. The old store swallows the error and reports
  success (NEWFRONT-233, §15).
- A server refusal (status not finished, 400) is an error like any other.

### 7.5 Delete (MOD-025)

**Entry:** row/card menu Delete or detail header menu Delete; active modules
only.

- Confirmation dialog _pinned_ heading **“Delete module”**; the body names
  the module and warns that its data is **permanently removed** and that the
  action **cannot be undone**.
- Cancel changes nothing.
- Confirming calls `DELETE modules/{moduleId}/`, shows a success toast and
  removes the row. On the list the user stays on the list. On the **detail
  page** the user returns to the modules list
  (`/{ws}/projects/{projectId}/modules/`).
- **With the peek open (intended).** Deleting the module shown in the peek
  clears `peekModule` and returns to the plain list. The old app leaves the
  address pointing at the deleted module (NEWFRONT-329, §15).
- **Permission failure:** the server refuses with 403; show the dedicated
  notice _pinned_ **“You don't have permission to perform this action.”** and
  keep the module in place.
- Only project admins and members see Delete; guests do not.

### 7.6 Restore (MOD-028)

**Entry:** archived row menu Restore.

- Calls `DELETE modules/{moduleId}/archive/` → `204`. The module becomes
  active again.
- Success shows a toast titled **“Restore success”** whose message explains
  the module can now be found in the project's modules (oracle matches
  **“project modules”**) and **stays on the archives modules screen**
  (`/archives/modules/`); the restored row disappears from the archived list.
- **Failure (intended).** A failed restore shows an **error** toast and keeps
  the module archived and on the screen. The old store swallows the error and
  reports success (NEWFRONT-233, §15).
- Restore is admin/member only.

## 8. Module detail: frame and work items (MOD-029–032, 039)

### 8.1 Breadcrumbs, switcher, badge and title (MOD-029)

Route: `/{ws}/projects/{projectId}/modules/{moduleId}`.

**Breadcrumbs** (three levels):

1. the **project** name — links to the project's issues screen,
   `/{ws}/projects/{projectId}/issues` (the project home for a reader);
2. the **modules list** — labelled **“Modules”**, links to
   `/{ws}/projects/{projectId}/modules`;
3. the **module name** — the current page, not a link.

Each crumb navigates to its own level.

**Module switcher.** A searchable control beside or below the breadcrumbs
lists **every non-archived module of the project**. Typing narrows the option
list by name. Picking another module navigates to its detail page **without
leaving the module area** (same project, same layout and display preferences
for that module) and shows that module's sidebar. It is a navigation
control, not a select that edits the current page.

**Work-item badge.** A badge shows the module's work-item total from the
detail record (`total_issues`). Its tooltip is _pinned_:
`There are N work items in this module`, with the singular `work item` when
N is 1. When the module has zero work items the badge is **absent** (not a
zero badge). The badge updates after the user's own mutations (adding an item).

**Tab title:** `{projectName} - {moduleName}` (_pinned_ shape).

**Missing or unknown module.** Any unresolvable module id — nonexistent,
deleted, or archived (the live detail 404s archived modules) — shows a
dedicated empty state: a title _pinned_ **“Module does not exist”**, a short
explanation, and a button/back link to the modules list. It must not
silently redirect. If the address carries `peekModule` for an unknown id, the
peek shows the same missing state inside the panel (§5).

**Access.** A user with no project access never sees the detail state: they
get the project access wall — the project-level block with a join/request
action — instead of module content. A project guest, once the backend guest
read exists, sees the detail read-only (§9 header paragraph; bug NEWFRONT-334
in §15).

### 8.2 Detail header and work-item layouts (MOD-030)

The detail header (desktop) offers:

- the layout switcher with **list, board (kanban), calendar, spreadsheet,
  timeline (Gantt)** — the same five layout kinds as the project issues
  screen;
- a **display options** dropdown whose entries follow the active layout (the
  Issues display matrix: fields, ordering, grouping as applicable);
- a **work-item filter** control (the rich filter row) and the shared
  analytics entry (§8.4).

**Small screens** offer **list, board and calendar** only (oracle reads
`["list","kanban","calendar"]`). The small-screen bar shows the display
control but **not** the filter toggle. The active layout persists per module
and survives reload (the module user-properties record, §13.3).

**Grouping rules on a module page:**

- **Module** grouping is **never** offered (grouping a module by module is
  meaningless), regardless of the project flag.
- **Cycle** grouping follows the project's cycle-view flag: offered when the
  flag is on, absent when it is off.
- Other display entries follow the shared Issues matrix unchanged.

**Body.** The module's work items render with exactly the matching project
Issues layout. The data scope is the module (`GET modules/{moduleId}/issues/`
and the shared issue list contracts; the hosted `EntityListScreen` already
does this). Menu actions, bulk actions, quick create and row rendering are
the Issues area's and must not be forked inside the modules feature. The
module-specific piece is the *frame* and the *empty states* (§8.3).

Viewing is open to every project role; layout and display choices are
personal.

### 8.3 Adding work items (MOD-031)

**Header Add.** The detail header has an Add (create work item) button,
visible to admins and members, hidden for guests. It opens the shared
work-item create modal **scoped to this module**: the created item is
attached to the module and appears in the list and the badge.

**Empty module (zero work items).** The empty state title is _pinned_
**“No work items to show in this Module”**. It offers two actions:

- **Create work item** (_pinned_) — opens the scoped create modal.
- **Add existing work item** (_pinned_) — opens the shared searchable picker
  of project work items not yet in the module.

Adding an existing item calls
`POST modules/{moduleId}/issues/` with `{"issues": ["<issueId>", ...]}`.
Success shows _pinned_ **“Work items added to the module successfully.”** and
the list and badge refresh. Failure shows
**“Selected work items could not be added to the module.”** and nothing is
attached.

**Filtered-but-empty module.** When the module has work items but the active
filters hide them all, the empty state offers to **clear the filters**
(_pinned_ **“Clear filters”**) instead of the create/add actions.

Guest behavior: guests see the work-item list but no Add button and no create
or add actions in the empty state (once guest reads land, §16). Work-item
creation, editing and bulk actions themselves are owned by the Issues area;
this spec only defines the module-scoped entry points.

### 8.4 Analytics dialog and sidebar collapse (MOD-032)

**Analytics button.** The detail header (and the small-screen bar) carries an
analytics button. It opens a **work-items analytics dialog scoped to the
module** — the same analytics dialog the Issues area mounts
(`features/issues/analytics/AnalyticsEntry` consumer; the scoped dialog is
NEWFRONT-738, area gate NEWFRONT-739). The dialog's subject is the module
(name in its header) and its counts are the module's work items. The modules
slice mounts the trigger and passes module scope; it does not build a second
analytics implementation.

**Collapse toggle.** A control beside the analytics button collapses or
expands the analytics sidebar. The choice persists in local storage (a
personal display choice) and **survives a reload**. The sidebar also has its
own close control; both paths update the same persisted value. Expanding the
sidebar again restores the same sections with their state.

### 8.5 Freshness (MOD-039, negative row)

Modules never live-update:

- The list, the detail page, the peek and the analytics sidebar fetch on
  mount (and on the user's own mutations) and **do not revalidate in the
  background** from other users' changes. A rename by another user stays
  invisible until navigation or reload.
- No polling, focus revalidation, websocket subscription or cache
  invalidation driven by another actor's activity may act on module data.
- The acting user's own mutation shows up at once (optimistic or refreshed
  from the response).

The old detail page revalidates behind the scenes and shows other users'
renames without navigation (NEWFRONT-332, §15); the new app must keep the
detail stale like the list and the peek. Configure the query client for
module reads with no refetch-on-focus, no refetch-on-reconnect and no
interval, and do not share a live-issue cache entry that the Issues area
refreshes on other events.

## 9. Module sidebar (MOD-033–035)

The sidebar renders on the detail page (full editor) and inside the peek
(read-only flag). It is **one component tree** (§1.3). Sections, top to
bottom: identity and dates, work-item and points counts, progress, links.

### 9.1 Fields and counts (MOD-033)

- **Status** — the status control (§4.2); inline save.
- **Name** — editable text field. Renames persist with `PATCH
  modules/{moduleId}/`; the page title, breadcrumb, row and switcher update.
  Empty names are refused (required).
- **Description** — inline text editor; saving persists `description`. An
  empty description clears it.
- **Dates** — the shared date-range control (§4.1), inline save.
- **Lead** — the shared member picker, single select, inline save.
- **Members** — the shared member picker, multi-select, inline save.
- **Work items** — `completed/total`, from the detail record
  (`completed_issues` / `total_issues`).
- **Points** — `completed/total` points, shown **only when the project's
  estimate system is of type `points`**. Under a time estimate system or with
  no estimate system there is **no points row at all** (not a zero row).

Every field saves inline: a success toast on save, rollback with an error
toast on failure. The sidebar shows a **loader** while the detail fetch is in
flight (it is the same read as the page, so the first paint is a skeleton or
spinner for the panel).

**Permissions and archive.** Guests and read-only roles see the values but
cannot edit: controls are disabled. **Archived modules** (only reachable
through the archived peek) render every field disabled, including status,
dates, lead and members; the panel still shows the values and progress.

**Optimistic writes.** A field change should reflect immediately and settle
to the server response. If the server rejects (e.g. start > target), revert
the field and show the error.

### 9.2 Progress section (MOD-034)

A collapsible section below the fields, with a completion percentage badge
in its header.

**Open/closed default.** The section opens by default **only when the module
is active**: it has a start date on or before today and a target date on or
after the start date. Otherwise it renders collapsed with an explanatory
notice instead of a chart:

- start date in the future — _pinned_ **“This module isn't active yet.”**
- missing or invalid dates (either side absent, or start after target) —
  _pinned_ **“Invalid date. Please enter valid date.”**

In the notice states there is no chart and no measure selector.

**Burndown chart.** When active, the section shows a burndown of completion
over the module's date window.

- The measure selector offers **Work items** and **Points** (_pinned_
  labels). It is present **only when the project uses a points estimate
  system**; otherwise it is absent (not disabled) and the chart is in work
  items.
- Switching measure **refetches the module details** so the chart and the
  breakdowns use that measure, and keeps the chart visible. The completion
  data comes from the server's `distribution.completion_chart` (work items)
  and `estimate_distribution.completion_chart` (points); the client does not
  compute the plot itself.
- The header percentage follows the selected measure's completed share.

**Breakdown tabs.** Three tabs, _pinned_ labels **“States”**,
**“Assignees”**, **“Labels”**. Each tab shows **its own** breakdown:

- **States** → the module's work items grouped by **state group** (Backlog,
  Unstarted, Started, Completed, Cancelled as present in the module's data),
  with each group's count and completion share;
- **Assignees** → **assignees** (`distribution.assignees`), one row per
  assignee;
- **Labels** → **labels** (`distribution.labels`), one row per label with its
  color.

The old app rotates the contents so each tab shows the next dimension's data
(NEWFRONT-337, §15). The new app must show the correct dimension per tab.

The active tab is remembered **per module** (a browser key scoped by module
id, e.g. a per-module analytics-tab entry in local storage) and survives
reload and switching between modules.

**Entries filter the list.** Clicking a breakdown entry toggles that value as
a filter on the module's **work-item list** (the rich filter instance the
detail page hosts): a state group filters by state group, an assignee by
assignee id, a label by label id. Clicking again clears it. The filter is
persisted in the module user-properties `rich_filters` and reflected in the
list immediately (the oracle reads both the list and the persisted record).

**Peek is read-only.** In the peek, tabs switch and show the same data, but
entries are **not clickable** and write no filter, and the section never
mutates the module. Tabs still work.

**Archived modules** show the progress section read-only with the same
default-open rule applied to the archived record's dates.

### 9.3 Links section (MOD-035)

A collapsible section at the bottom of the sidebar listing reference links
attached to the module.

**States.**

- **No links:** the section is **collapsed** by default; expanding shows the
  notice _pinned_ **“No links added yet”** and an **Add link** action
  (editors).
- **Links exist:** the section **opens by default**, including after a
  reload. The old app stays collapsed after reload even though links exist
  (NEWFRONT-342, §15); the new app must compute the default from the loaded
  links, not only from the pre-fetch state.
- The section header carries the disclosure and, for editors, an add action
  even when expanded.

**Link dialog.** Add (_pinned_ dialog title **“Add link”**) and edit
(_pinned_ **“Update link”**):

- URL input, **required**. Submitting empty keeps the dialog open and marks
  the URL field invalid; the oracle reads an invalid-marked field and no
  stored link (there is no separate error sentence).
- A URL without a scheme is **defaulted to `http://`** (so `example.com`
  becomes `http://example.com`). An explicit `https://` is preserved.
- Optional **title**. Untitled links display their URL as the row title.
- Cancel/Escape writes nothing.
- Requests:

```
POST   modules/{moduleId}/module-links/            { "url": "...", "title": "..." }
PATCH  modules/{moduleId}/module-links/{linkId}/   { "url": "...", "title": "..." }
DELETE modules/{moduleId}/module-links/{linkId}/
```

Success toasts _pinned_: **“Module link created successfully.”**,
**“Module link updated successfully.”**, **“Module link deleted successfully.”**
A failed write shows an error toast, keeps the dialog open with the input
(create/edit), and stores nothing.

**Link rows.** Each row shows:

- a **type icon** derived from the URL (the shared icon for the link's
  category — do not copy old icon assets);
- the **title or URL** as the link target, opening in a **new tab**
  (`target="_blank"` with `rel="noopener noreferrer"`);
- a **copy-URL** action that copies the link and shows _pinned_
  **“Copied to clipboard”**;
- metadata: the relative **age** (“Added …”) and the **author's display
  name**;
- **edit** and **delete** actions for editors.

Rows are ordered newest-first (the server returns `-created_at`); keep the
server order. Guests and read-only roles see the rows, open and copy links
but no add/edit/delete actions. Archived modules are read-only throughout.

## 10. Settings, palette and deep links (MOD-036–038, 040)

### 10.1 Project settings modules switch (MOD-036)

The project **features** settings screen has a modules section with an on/off
switch. That screen and its persistence are owned by the project-settings
area (NEWFRONT-582, PSET-046). The modules area defines the effects:

- Non-admins (members and guests) opening the modules feature settings see a
  **not-authorized** notice **instead of the toggle** (no disabled toggle).
- Turning modules **on** unlocks the modules list, the create action and every
  creation entry point (header, zero state, palette).
- Turning modules **off** replaces the list with the §2.2 gate and removes
  the palette create command; existing modules are preserved (turning it back
  on shows them again).
- The switch writes the project record's `module_view` flag. The modules
  surfaces read that flag from the project record loaded with the route, so
  flipping it and returning to the list shows the correct state; no separate
  modules settings copy.

### 10.2 Command palette (MOD-037)

The palette is the shell's (`shared/commands`, SHELL-080–094); the modules
area registers module commands and consumes the entity-context hook.

**New module command.**

- _pinned_ command title **“New module”**, with the project-scoped two-key
  sequence **`nm`**.
- Visible and enabled only when **all** hold: there is a project context, the
  project's modules feature is on, and the user is a project admin or member.
  It is hidden with no project context, in a flag-off project, and for a
  project guest.
- Activating it (click or `nm`) opens the same create dialog as §7.1. The
  two-key sequence works **regardless of how the palette was used before**:
  click-activating a palette command must not break `nm` until navigation
  (NEWFRONT-333, §15).
- Dismissing the dialog writes nothing.

**Open a module (jump) page.**

- _pinned_ command title **“Open a module”**. It opens a palette page
  listing the project's **non-archived modules**, each with its **status
  icon** and name. No checkmarks in the jump list (it is navigation, not
  selection). Picking an entry navigates to that module's detail page.
- The list uses the project module list read; it is not a command per
  module.

**Work-item module picker.** The palette's **Add to modules** page (the
work-item contextual command, `Shift+M` on a work item) lists the project's
modules with the work item's **current memberships pre-selected**. Toggling
writes through `POST issues/{issueId}/modules/` with `{modules,
removed_modules}`. This picker belongs to the Issues area's work-item
context (NEWFRONT-615 member/module pickers, shell contextual entries); the
modules area owns only the module list contract it reads.

**Module detail contextual entries.** On a module detail page the contextual
palette offers the per-entity module commands the shell defines
(SHELL-091): a status change and a member toggle for admins/members, and
copy-URL; both gated by role and by archived state. They call the same
module endpoints as the sidebar. The shell builds the entries; the modules
area exposes the module context (id, name, status, member ids, favorite,
archived, standing).

**Other sequences** (`gh` workspace sequences, etc.) keep working while `nm`
does; the modules area does not register any other module shortcut.

### 10.3 Deep links and access (MOD-038)

- Copy-link and open-in-new-tab produce
  `{origin}/{ws}/projects/{projectId}/modules/{moduleId}`; the peek produces
  `.../modules?peekModule={moduleId}` (active) or
  `.../archives/modules?peekModule={moduleId}` (archived). Both
  forms reload into the same state.
- An unresolvable module id on the detail route shows the missing-module
  state with a way back to the list (§8.1); it does not loop, redirect
  silently or crash. A `peekModule` id that does not resolve shows the
  missing state in the panel.
- A **user with no project access** gets the project **access wall**
  (join/request action), never module content and never the missing-module
  state. The wall is the project shell's, shared with every project area; the
  modules area does not build its own.
- The switcher, copy link, new tab and peek all use these same addresses;
  there is no alternate module URL shape.

## 11. Keyboard, drag, export and build negatives (MOD-040–043)

### 11.1 Keyboard (MOD-040)

- **Escape** closes the create and edit dialogs without saving, closes the
  delete and archive confirmations without acting, closes the link dialog,
  and closes the peek. On the list and archived search: first Escape clears
  the query, second Escape collapses the field (§2.4).
- The create and edit dialogs have a **logical tab order** across fields and
  actions (title → description → date → status → lead → members → cancel →
  submit; project first in create), with the date control tabbable in place.
- The only module shortcut is the palette sequence **`nm`** (§10.2). No other
  single-key or two-key module shortcut exists: ordinary letters, `Shift+M`
  on the modules list, and similar keypresses open nothing and change no
  data.
- Palette click-activation must not poison subsequent `nm` sequences. The old
  app loses `nm` until a navigation (NEWFRONT-333, §15); the new app keeps
  it working.

### 11.2 Drag and import/export (MOD-041)

- Modules are ordered only by the ordering control (§2.5). List rows and
  board cards are **not draggable**: dragging them changes neither the UI
  order nor the server, and produces no reorder toast.
- The only drag interaction in modules is the timeline bar move/resize and
  the manual-sort row reorder (§6).
- There is no **export**, **import** or **print** action for modules on the
  list, the detail page, any dialog or the archives screen. Do not add a CSV,
  JSON or print affordance “for parity”; the old app has none.

### 11.3 Desktop build (MOD-042, negative row)

The desktop build serves the same module routes with the same behavior, the
same endpoints and the same layouts. No module surface reads the target
(`web` vs `desktop`) to change capability, copy or layout, and no
desktop-only offline mode exists for modules. The desktop shell may present
the same pages inside its window; the module feature must not add desktop
markers, banners or gates.

### 11.4 Editions (MOD-043, negative row)

No module capability is cloud-only or OSS-only. Favorites, filters, timeline,
analytics, links and archives render ungated in every edition and behave
identically. Module code must not consult edition flags to hide, stub or
unlock a surface, and must not call edition-specific endpoints. The module
entry traffic stays on the known module endpoints (§13); no billing, license,
upgrade or premium request belongs to a module screen.

## 12. Reuse map (what each module surface uses, and who owns it)

The rule for every row: **extend or mount the owning component; do not fork
it and do not build a stub.** If a piece cannot express a module need with a
prop, the modules slice adds the prop/callback to the owner (with the owner's
issues notified) rather than writing a module-only copy.

| Module surface | Existing `apps/web_new` piece | Owner / sibling issue | How the modules area uses it |
| --- | --- | --- | --- |
| Module detail work-item body | `features/issues/list/EntityListScreen.tsx` (already mounted by `routes/$ws/_gated/projects/$projectId/modules/$moduleId.tsx`) with `features/issues/layouts/IssuesLayoutRoot.tsx` | Issues: list 619 (Done), spreadsheet 620 (Done), calendar 621 (In Review), kanban 622/623, gantt/timeline 624 (Done) | Mount the shared entity host with `scope: "module"` and `moduleId`. Do not re-render layouts or rows. Slice J (NEWFRONT-802) owns the module **frame** around it (breadcrumbs, switcher, badge, analytics trigger, sidebar) and the empty states (slice K). |
| Layout switcher, layout kinds, icons | `features/issues/layouts/kinds.ts`, `features/issues/components/LayoutSwitcher.tsx`, `LayoutIcon.tsx`, `layoutMenuItems.tsx`, `dropdowns/LayoutPicker.tsx` | Issues 615/616 + layout slices | Use the same kinds (`list`, `board`/kanban, `calendar`, `spreadsheet`, `gantt`) and the same switcher. The **modules list** (slice A) passes only list/board/timeline and the compact menu with “List layout”/“Gallery layout”; the **detail** (slice J) passes the five (mobile three). No module-specific switcher. |
| Display options | `features/issues/display/DisplayControl.tsx`, `DisplayPanel.tsx`, `matrix.ts` | Issues 618 (Done) | Detail header mounts the display control with the module context; the matrix supplies per-layout entries. Modules passes “Module grouping never offered, Cycle grouping follows the flag”; do not edit the matrix for modules — supply the excluded grouping. |
| Work-item filters and filter chips | `features/issues/filters/rich/*` (`IssueFilters`, `useRichFilters`, `expression`, `legacyDimensions`), `filters/shared` | Issues 626–629 (Done) | Detail filter row and the list it filters. Breakdown clicks (slice L) write into this filter instance. The **modules list's own** filters (§2.6) are a separate, lighter client-side filter over modules — build it in the modules feature from kit primitives; do not reuse issue filter state. |
| Dropdown primitives (menu, combobox, popover, searchable lists, expanders) | `@pidash/kit` (`Menu`, `Popover`, `Combobox`, `Tooltip`, `Switch`) and the patterns of `shared/pickers/`, `features/issues/dropdowns/` | Issues 615/616 | The module ordering menu, filters menu, custom-date dialog trigger and row quick menu are built from kit primitives in the modules feature. Do **not** import issue dropdowns for module-specific menus. |
| Member picker (lead, members, filters) | `shared/pickers/MemberPicker.tsx`, `members.ts` | Issues 615 | Reuse for the dialog's lead/members and the sidebar. The list filter's “You” first, five-then-expand group is the filter menu's own combinator built on the same member source. |
| State/status picker | `shared/pickers/StatePicker.tsx` (workflow states) vs module statuses | Issues 615 | **Not the issue state picker**: module status is its own six-value enum (§4.2). Build the module status control in the modules feature from kit `Menu` + the color tokens. Do not reuse issue state picker or state groups for module status. |
| Module picker (palette, work-item membership) | `shared/pickers/ModulePicker.tsx`, `modules.ts`, `reference.ts` | Issues 615 + shell palette 292/293/294 | Palette jump/“Add to modules” and the work-item picker use this. The modules area does not rebuild it. Pre-selection comes from the work item's module ids. |
| Project picker (create dialog) | `shared/pickers/ProjectPicker.tsx`, `projects.ts` | Issues 615 | Create dialog's project field, restricted to projects where the user may create modules. |
| Date range picker | `features/issues/dropdowns/DateRangePicker.tsx`, `calendar.ts`, `shared/pickers/*` date pieces | Issues 616 | Row/card inline dates, dialog dates, sidebar dates and the custom filter range. Same clearing semantics described in §4.1/§2.6.1. |
| Work-item create modal | `features/issues/components/WorkItemCreateDialog.tsx` / `WorkItemCreateEntry.tsx`, `create/*` | Issues create slice (630) and list slice 619 | Detail header Add and empty-state “Create work item” open this with module scope. Do not build a module work-item form. |
| Add-existing picker | `features/issues/list/AddExistingModal.tsx` | Issues list 619 | Empty-state “Add existing work item”; on confirm the modules slice calls `POST modules/{moduleId}/issues/`. |
| Detail sidebar properties (fields, descriptions, dates, lead/members patterns) | `features/issues/detail/*` (title/description fields, save pipeline), `shared/pickers` | Issues 639 (Done), 640, 642 | The module sidebar is **its own** component in the modules feature (module fields differ from issue fields), but it uses the same field, picker and save-pipeline patterns, the shared date range and member pickers, and the kit inputs. Do not import issue-detail components that are keyed to issues. |
| Module sidebar and progress | New: `features/modules/*` (one component tree) | This area (slice L) | One sidebar used by the detail page and the peek with a read-only flag (§1.3). Progress (burndown + breakdowns) is a section of it, not a separate page. Nothing in the Issues feature computes module burndowns; reuse kit chart primitives and the server's chart data. |
| Peek panel shell | `shared/shell/PeekPanel.tsx` | Issues 646 (`shared/shell`, merged) | Modules mount the shell (close control, Escape, eyebrow/actions, body slot) and put the shared sidebar in its body. Use the read-only variant of the sidebar. The peek's URL key (`peekModule`) and param handling are the module route's. |
| Analytics scoped dialog | `features/issues/analytics/AnalyticsEntry.tsx` and the scoped dialog from NEWFRONT-738 | Analytics 738; area gate 739 | The module detail mounts the trigger with module scope; the analytics area owns the dialog. No reverse edge is needed (analytics already records the module trigger as external). |
| Archives shell, tabs, titles, badges | Archives shell from NEWFRONT-698 | Archives: 698 (shell) | The modules **tab** (`/archives/modules`) mounts inside the archives shell. Modules do not render a second tab strip or archives chrome. |
| Archived-modules screen, restored row behavior, archived peek | NEWFRONT-702 (ARCH-020–025) | Archives: 702 is the authority | **Do not fork.** MOD-026–028 and MOD-027 consume it. If the modules oracle needs a module-specific tweak, extend NEWFRONT-702's component via props; never mount a modules-owned copy. |
| Project features switch (modules) | `features/project-features/*` | Project settings 582 (PSET-046) | The modules area defines effects (§10.1), not the switch. The gate's Manage-features link targets the project features page. |
| Command palette registry, sequences, contextual entries | `shared/commands/{registry,shortcuts,store}.ts`, `shared/shell/contextual/{modules.ts,workItems.ts,hooks}.ts` | Shell 292/293/294 (Done) + the palette entity-context hook | Slice N registers “New module” + `nm` and “Open a module”; it wires the module context (status/member/copy) through the existing contextual hook. Fix the `nm` state bug there, not in a module-local key handler. |
| Sidebar favorites | `features/sidebar/favoriteLinks.ts`, `SidebarFavoritesGroup` | Shell favorites (SHELL-057) | Favorite writes go through `user-favorite-modules/`; the sidebar reads favorites from its own source. The first-ever favorite expands the group (§4.4) via the shell's favorites state, not a module-specific sidebar. |
| Toasts, clipboard, document head, access wall | `shared/shell/toaster.ts`, `shared/shell/clipboard.ts`, `shared/shell/document.tsx` (`useDocumentHead`), project access wall in the project shell | Shell (merged) | Use the shared helpers for toasts, copy, the tab title (§2.1, §8.1) and the access wall (§10.3). |
| Route validation and prefetch | `routes/$ws/_gated/projects/$projectId/modules/{index,$moduleId}.tsx`, `features/issues/filters/search.ts` (`parseIssueSearch`), `features/issues/list/prefetch.ts` | Issues list 619 + modules slices A/J | The detail host already validates the issue search and prefetches; slice J adds module data (detail, sidebar) and keeps the existing peek key handling. The list route (slice A) replaces the `TabAreaMount` and adds `peekModule` to its search schema. |
| Module user-properties record | `features/issues/prefs/entities.ts` (`scope: "module"` → `updateModuleUserProperties`), `displayPrefs.ts`, `prefsQuery.ts` | Issues prefs (merged) | Module list layout/order/filters and detail layout/display live here per §13.3. Do not add a module-only preferences store. |
| Coexistence and route ownership | `.github/newfront-path-extras.json`, `apps/web_new/migrated-routes.json`, NEWFRONT-528 | Coexistence (F-09) | The area gate adds the module path prefixes to the migrated routes when the area is green. Slices touch only their own feature/route files. |
| Contracts | `packages/api-client/src/contracts/modules.ts` (thin today: list/update) | Contracts 792 | NEWFRONT-792 extends the zod schemas and helpers for every endpoint in §13 (create/detail/issues/links/favorites/archive/archived/user-properties). Modules slices consume those helpers; they do not hand-write request shapes. |

**Module-sidebar rule (repeated because it is load-bearing).** The detail
page and the peek mount the **same** module sidebar component and the same
progress section; the peek passes a **read-only flag**. Slices must not build
a peek-only stub, a stripped “summary” panel, or a second progress chart. Any
behavior that differs between them (editability, breakdown clickability,
link actions) is a prop of the shared component.

## 13. API reference (MOD-001–043)

Base: `/api/workspaces/{ws}/projects/{projectId}`. JSON bodies; reads need
an authenticated project member. DRF error shapes: a field map for serializer
errors, or `{"error": "<sentence>"}` for explicit checks.

### 13.1 Module CRUD

| Method | Path | Body / params | Success | Errors | Rows |
| --- | --- | --- | --- | --- | --- |
| `GET` | `/modules/` | — | 200, array of module rows (non-archived only) | 403 non-member/anon | 001, 005–008, 011, 012, 019, 020 |
| `POST` | `/modules/` | `{"name", "description"?, "start_date"?, "target_date"?, "status"?, "lead_id"?, "member_ids"?}` | 201, created row | 400 duplicate name (`Module with this name already exists`), 400 start > target, 400 field errors | 003, 009, 022, 023, 037 |
| `GET` | `/modules/{moduleId}/` | — | 200, detail: base fields + `total_issues`, `completed_issues`, `cancelled_issues`, `started_issues`, `unstarted_issues`, `backlog_issues`, `total_estimate_points`, `completed_estimate_points`, `is_favorite`, `sort_order`, `lead_id`, `member_ids`, `archived_at`, `link_module`, `sub_issues`, `distribution` `{assignees, labels, completion_chart}`, `estimate_distribution` `{}` (points project: `{assignees, labels, completion_chart}`) | 404 unknown or archived (`Module not found`); 403 today for guests (NEWFRONT-334) | 019, 029, 032–035, 038, 039 |
| `PATCH` | `/modules/{moduleId}/` | any of `name`, `description`, `start_date`, `target_date`, `status`, `lead_id`, `member_ids` (partial) | 200, updated row | 400 archived module, 400 start > target, 400 duplicate name, 404 | 013–016, 024, 026, 033 |
| `PUT` | `/modules/{moduleId}/` | full row | 200 | same as PATCH | 024 (only if the edit form uses a full replace; partial PATCH is fine) |
| `DELETE` | `/modules/{moduleId}/` | — | 204 | 403 non-editor / not creator, 404 | 017, 025 |

Module row fields the client relies on: `id`, `project_id`, `name`,
`description`, `description_html`, `start_date`, `target_date`, `status`,
`lead_id`, `member_ids`, `sort_order`, `is_favorite`, `total_issues`,
`completed_issues`, `cancelled_issues`, `started_issues`, `unstarted_issues`,
`backlog_issues`, `total_estimate_points`, `completed_estimate_points`,
`created_at`, `updated_at`, `archived_at`.

### 13.2 Work items in a module

| Method | Path | Body / params | Success | Rows |
| --- | --- | --- | --- | --- |
| `GET` | `/modules/{moduleId}/issues/` | the shared issue list params (filters, cursor, layout grouping) | 200, paged issue list | 030, 031, 034 |
| `POST` | `/modules/{moduleId}/issues/` | `{"issues": ["<issueId>", ...]}` | 200/201, attached | 031 |
| `DELETE` | `/modules/{moduleId}/issues/{issueId}/` | — | 204 | 031 (removal from a module, used by the issues area) |
| `POST` | `/issues/{issueId}/modules/` | `{"modules": ["<moduleId>"], "removed_modules": ["<moduleId>"]}` | 200 `{"message": "success"}` | 037 (work-item picker) |

The issue list and work-item endpoints' full contracts are the Issues
area's; this spec only fixes the module-scoped paths and the attach body.

### 13.3 Display state (module user properties)

| Method | Path | Body | Rows |
| --- | --- | --- | --- |
| `GET` | `/modules/{moduleId}/user-properties/` | — → the record | 011, 030, 034, 039 |
| `PATCH` | `/modules/{moduleId}/user-properties/` | partial `{filters, display_filters, display_properties, rich_filters}` | 011, 030, 034 |

Display rules used here:

- The modules **list** persists layout (`display_filters.layout` =
  `list|board|timeline`), ordering key and direction, and its client filters
  in this record for the project's module context. (The old app used local
  storage keys; the new app keeps user-properties as the source of truth —
  a deliberate architecture change, not a behavior difference.)
- The **detail** persists its work-item layout/display in the same record,
  via the shared Issues prefs code (`scope: "module"`).
- Active breakdown tab: a per-module browser key (e.g.
  `module-analytics-tab-{moduleId}`), since it is view-only UI state.
- Sidebar collapsed: a per-user browser key (or the shared sidebar
  preference), persisted across reloads.
- All of these are **personal**: never write them to the project or the
  module row.

### 13.4 Favorites

| Method | Path | Body | Success | Rows |
| --- | --- | --- | --- | --- |
| `POST` | `/user-favorite-modules/` | `{"module": "<moduleId>"}` | 204 | 016 |
| `DELETE` | `/user-favorite-modules/{moduleId}/` | — | 204 | 016 |
| `GET` | `/user-favorite-modules/` | — | 200, current user's module favorites (the shell reads favorites through its own contract) | 016 |

### 13.5 Links

| Method | Path | Body | Success | Rows |
| --- | --- | --- | --- | --- |
| `GET` | `/modules/{moduleId}/module-links/` | — | 200, array `{id, url, title, created_at, created_by, …}` newest first | 035 |
| `POST` | `/modules/{moduleId}/module-links/` | `{"url", "title"?}` | 201, created link | 035 |
| `PATCH` | `/modules/{moduleId}/module-links/{linkId}/` | partial `{url, title}` | 200 | 035 |
| `DELETE` | `/modules/{moduleId}/module-links/{linkId}/` | — | 204 | 035 |

Scheme defaulting happens on the client before submit (`https?://` preserved,
otherwise prefix `http://`); the server also validates the URL. A duplicate
URL in the same module is refused by the server.

### 13.6 Archive and archived reads

| Method | Path | Body | Success | Errors | Rows |
| --- | --- | --- | --- | --- | --- |
| `POST` | `/modules/{moduleId}/archive/` | — | 200 `{"archived_at": "<iso>"}` | 400 unless completed/cancelled | 026 |
| `DELETE` | `/modules/{moduleId}/archive/` | — | 204 | 404 | 028 |
| `GET` | `/archived-modules/` | — | 200, archived module rows (favorites-first order, then created desc) | 403 | 027 |
| `GET` | `/archived-modules/{moduleId}/` | — | 200, archived detail (same detail shape) | 404 | 027, 019 (archived peek) |

### 13.7 Project flags and permissions read

| Method | Path | Use | Rows |
| --- | --- | --- | --- |
| `GET` | `/api/workspaces/{ws}/projects/{projectId}/` | project record incl. `module_view`, `cycle_view`, estimate type | 002, 030, 036 |
| `PATCH` | same | the project-settings switch flips `module_view` (owned by PSET-046) | 036 |

### 13.8 Role matrix (as the server enforces it today)

| Endpoint | Admin | Member | Guest |
| --- | --- | --- | --- |
| List modules | yes | yes | yes |
| Detail module | yes | yes | **403 today** (NEWFRONT-334) |
| Create/update/delete module | yes | yes | no |
| Module issues list | yes | yes | **403 today** (NEWFRONT-334) |
| Attach/detach issues | yes | yes | no |
| Links read | yes | yes | via project membership (read) |
| Links write | yes | yes | no |
| Favorites write | yes | yes | no (UI hides the star) |
| Archive / restore | yes | yes | no |
| Archived list/detail | yes | yes | yes (archives ARCH-028) |
| User properties | yes | yes | yes (own record) |

The client must match the intended column once NEWFRONT-334 lands (guest
read-only); until then it must not invent a workaround (§16).

## 14. Row coverage map

| Row | Sections | Oracle scenario |
| --- | --- | --- |
| MOD-001 | 2.1 | `modules/list-header.spec.ts` — list, breadcrumb, tab title |
| MOD-002 | 2.2 | `list-header.spec.ts` — gate art/explanation/Manage features, admin vs non-admin |
| MOD-003 | 2.3 | `list-header.spec.ts` — Add Module / Add, opens dialog, hidden for guests |
| MOD-004 | 2.4 | `list-header.spec.ts` — expandable search, Escape matrix |
| MOD-005 | 2.5, 6.3 | `list-header.spec.ts` — ordering keys, direction, manual; manual gates timeline reorder |
| MOD-006 | 2.6 | `list-header.spec.ts` — favorites/status/lead/members/dates, You first, expander, custom range, combined with search |
| MOD-007 | 2.7 | `list-header.spec.ts` — chips per dimension, per-value/per-dimension removal, clear-all |
| MOD-008 | 3.1 | `list-views.spec.ts` — layout-matched exclusive skeleton |
| MOD-009 | 3.2 | `list-views.spec.ts` — zero state, create opens dialog, guest disabled |
| MOD-010 | 3.3 | `list-views.spec.ts` — no-match heading/hint, clearing restores |
| MOD-011 | 3.4 | `list-views.spec.ts` — 3 layouts, persistence, compact menu list+board |
| MOD-012 | 3.5 | `list-views.spec.ts` — progress ring/check/alert, link, peek trigger; archived row opens peek |
| MOD-013 | 4.1 | `row-actions.spec.ts` — placeholders, range save, clear, card dates |
| MOD-014 | 4.2 | `row-actions.spec.ts` — six statuses/colors, inline save |
| MOD-015 | 4.3 | `row-actions.spec.ts` — avatar vs no-lead marker |
| MOD-016 | 4.4 | `row-actions.spec.ts` — star toggle, toasts, first-favorite expands sidebar, guests no star |
| MOD-017 | 4.5 | `row-actions.spec.ts` — menu per role/state, archive gate explanation, each entry |
| MOD-018 | 4.6 | `row-actions.spec.ts` — copy full URL, new tab, detail header menu |
| MOD-019 | 5 | `row-actions.spec.ts` — peek open/close, `peekModule`, shareable, board density, archived peek |
| MOD-020 | 3.6 | `list-views.spec.ts` — card identity, counts, segments, dates/star/menu, body navigates |
| MOD-021 | 6 | `row-actions.spec.ts` — timeline rows/bars/tooltips, drag/resize, manual reorder, guest no-op |
| MOD-022 | 7.1 | `dialogs-archived.spec.ts` — picker/defaults/tab order, bug 330, title validation, cancel/escape/reset |
| MOD-023 | 7.1 | `dialogs-archived.spec.ts` — success stores all fields; bug 328 failed create clears form |
| MOD-024 | 7.2 | `dialogs-archived.spec.ts` — prefill + save; bug 328 failed save clears form |
| MOD-025 | 7.5 | `dialogs-archived.spec.ts` — confirm/cancel/list delete, detail delete navigates, bug 329 peek-open, permission notice |
| MOD-026 | 7.4 | `dialogs-archived.spec.ts` — archive confirm/cancel; bug 233 failed archive reports success |
| MOD-027 | 7.3 | `dialogs-archived.spec.ts` — archive strip placement, filters, chips, row reuse, peek; zero/search/filters empties |
| MOD-028 | 7.6 | `dialogs-archived.spec.ts` — restore; bug 233 failed restore reports success |
| MOD-029 | 8.1 | `detail.spec.ts` — breadcrumb/switcher/badge/tab title, unknown id; bug 334 guest missing state |
| MOD-030 | 8.2 | `detail.spec.ts` — five layouts, display matrix, filter toggle; flag-off grouping; mobile three |
| MOD-031 | 8.3 | `detail.spec.ts` — header add scoped, empty create/add-existing, failure notice, filtered-empty |
| MOD-032 | 8.4 | `detail.spec.ts` — analytics dialog scoped, sidebar collapse persists, mobile analytics |
| MOD-033 | 9.1 | `detail.spec.ts` — fields/counts, inline edits, points gating, archived disabled, loader |
| MOD-034 | 9.2 | `detail.spec.ts` — chart/measure refetch, dates notices, tab memory + bug 337, entries filter, peek read-only |
| MOD-035 | 9.3 | `detail.spec.ts` — section states, dialog validation + scheme, bug 342 reload, rows CRUD |
| MOD-036 | 10.1 | `cross-cutting.spec.ts` — toggle both ways, member not-authorized |
| MOD-037 | 10.2 | `cross-cutting.spec.ts` — palette create gating/dialog, jump menu, work-item picker |
| MOD-038 | 10.3 | `cross-cutting.spec.ts` — copied links, unknown id missing state, outsider access wall |
| MOD-039 | 8.5 | `cross-cutting.spec.ts` — list/peek stale, own writes; bug 332 detail revalidates |
| MOD-040 | 11.1 | `cross-cutting.spec.ts` — Escape matrix, tab order, `nm`; bug 333 `nm` after palette click |
| MOD-041 | 11.2 | `cross-cutting.spec.ts` — row/card drag no-ops, no export/import/print |
| MOD-042 | 11.3 | `cross-cutting.spec.ts` — desktop same routes, allowlist, no markers, no desktop code |
| MOD-043 | 11.4 | `cross-cutting.spec.ts` — surfaces ungated, allowlist, no edition markers |

## 15. Bug-row dispositions (intended behavior, never the old bug)

Each entry names the inventory rows whose oracle scenario carries the bug,
the filed issue, what the old app does, and what the new app must do. The
new app implements the **intended** column; the oracle's bug-titled scenario
stays as evidence of the old behavior and must not be used to justify
reproducing it.

| Bug issue | Rows | Old behavior (pinned by the oracle) | Intended (new app) |
| --- | --- | --- | --- |
| **NEWFRONT-330** | MOD-022 | The create dialog's date-range control renders with no tab index, so keyboard focus reaches it **after** Submit (order `1,2,null,4,5,6,7,8`). | The date control is reachable **third** in tab order, between description and status: title → description → date → status → lead → members → cancel → submit. §7.1 |
| **NEWFRONT-328** | MOD-023, MOD-024 | A failed create resets the form to empty defaults; a failed save clears the title (and other fields) even though the dialog stays open with an error. | A failed create or save keeps **every entered field** exactly as the user left them; the dialog stays open with an error toast; nothing is stored. §7.1, §7.2 |
| **NEWFRONT-329** | MOD-025 | Deleting the module whose peek is open keeps the address on `?peekModule=` for the deleted id. | Deleting with the peek open clears `peekModule`, closes the panel and leaves the user on the plain modules list. §5, §7.5 |
| **NEWFRONT-233** (archive) | MOD-026 | A failed archive fires the success notice and navigates away as if it worked. | A failed archive shows an **error** toast, does not navigate, and the module keeps its active state. §7.4 |
| **NEWFRONT-233** (restore) | MOD-028 | A failed restore fires the success notice and keeps the row as if it worked. | A failed restore shows an **error** toast, keeps the module archived, stays on the archives screen and keeps the row. §7.6 |
| **NEWFRONT-334** | MOD-029, MOD-031, MOD-033 | Project guests get a 403 on the module detail and module-issues reads, so they land on the missing-module state (and cannot see the read-only sidebar / empty-state actions as intended). | **Backend dependency, not a client workaround.** Once the backend guest read lands, guests read the detail page, the module work-item list and the sidebar (edits disabled, no create actions, no empty-state create/add). Unknown ids still show the missing-module state, and users without project access still get the project access wall (MOD-038). §8.1, §8.3, §9.1, §16 |
| **NEWFRONT-337** | MOD-034 | The breakdown tabs show each other's data (States shows assignees, Assignees shows labels, Labels shows state groups — a one-step rotation). | Each tab shows **its own** breakdown: States → state groups, Assignees → assignees, Labels → labels. §9.2 |
| **NEWFRONT-342** | MOD-035 | The links section stays collapsed after a reload even though links exist. | The links section opens by default whenever links exist, **including after a reload**. Empty stays collapsed with the notice. §9.3 |
| **NEWFRONT-332** | MOD-039 | The module detail revalidates behind the scenes and shows other users' renames without navigation. | Detail (and list, peek, analytics) is fetch-on-mount: no background revalidation from other users' changes; only the acting user's own mutations update the view. §8.5 |
| **NEWFRONT-333** | MOD-040 | Click-activating a palette command breaks the `nm` sequence until a navigation. | The `nm` creation sequence keeps working after any palette command was clicked; click and key activation are equally safe. §10.2, §11.1 |

Count: **ten dispositions across nine bug issues** (NEWFRONT-233 is split
into its archive and restore halves), covering MOD-022, 023, 024, 025, 026,
028, 029, 031, 033, 034, 035, 039 and 040. Every row above that carries a
`bug:` or “Actual:” scenario in the inventory appears here with its intended
behavior; the slices implement the intended column and never the old bug.

## 16. Seams and open dependencies

These are **not** things to work around in module code. The slice that
touches the seam records the dependency in its own plan; the spec records it
so nobody re-decides it.

1. **Guest read on module detail and module issues (NEWFRONT-334).** The
   backend currently refuses project guests on `GET modules/{id}/` and
   `GET modules/{id}/issues/`. The intended client behavior (MOD-029, 031,
   033: guests read, edits disabled) depends on the backend change filed as
   NEWFRONT-334. Until it lands, guests see the missing-module state on the
   detail; the modules slices must not add a client-side guest fetch path,
   a proxy, or a different endpoint to fake the read. The oracle pins the
   old behavior on the old app and will assert the intended behavior on the
   new app once the backend reads land. Add the dependency to the affected
   slices (J, K, L) and to the area gate's notes.
2. **Archived-modules cold entry (NEWFRONT-228 / NEWFRONT-239).** A direct
   load or reload of `/archives/modules` hung on the skeleton in the old
   app. The archived screen must load its own data on cold entry. Ownership
   is the Archives area (NEWFRONT-702); the modules slices must not patch it
   locally, and MOD-027's archived states rely on the fixed behavior.
3. **Archived-modules screen ownership split (ARCH-020–025).** NEWFRONT-702
   is the sole authority for the archived-modules screen, its search, its
   filters/chips, its row reuse, its empty kinds and its archived peek. The
   Modules slices consume it; MOD-026–028 do **not** fork it. If the modules
   oracle later needs a module-specific behavior there, it is added to
   NEWFRONT-702's component behind a prop, or a new row is filed there.
4. **Issues layouts and entity host (NEWFRONT-619/620/621/622/623/624).**
   The module detail body is the hosted issues list. Module-specific frame
   work (slice J) and empty states (slice K) mount around it; they do not
   re-render rows, cards or bars. Any missing capability in a layout is
   fixed in that layout's slice, not forked for modules.
5. **Analytics scoped dialog (NEWFRONT-738, gate 739).** The module detail
   mounts the analytics trigger with module scope. The dialog itself is the
   analytics area's; the modules area does not build a second one. No reverse
   edge is needed.
6. **Project settings modules switch (NEWFRONT-582, PSET-046).** The switch
   and its persistence belong to the project-settings area. The modules area
   owns the gate (§2.2) and the list's reaction to the flag (§10.1).
7. **Command palette (SHELL-080–094; NEWFRONT-292/293/294 Done; contextual
   hook).** Module commands and sequences are registered through the shell
   registry; the `nm` bug lives in the shell's project-context/sequence
   state, so the fix belongs there (slice N with the shell owner), not in a
   module-local keyboard handler.
8. **Peek shell (NEWFRONT-646) and shell favorites (SHELL-057).** The module
   peek uses the shared panel shell; the favorite expansion on first
   favorite uses the shell favorites group. Extend those components if a prop
   is missing (for example the read-only variant for the sidebar content is a
   modules-side prop on the **modules** sidebar, not a shell change).
9. **Contracts (NEWFRONT-792).** Every endpoint in §13 must be in
   `@pidash/api-client` with zod schemas and contract tests before the
   consuming slice builds. The thin `modules.ts` there today (list/update)
   is not enough.
10. **Coexistence (NEWFRONT-528, `.github/newfront-path-extras.json`,
    `apps/web_new/migrated-routes.json`).** The area gate adds the module
    path prefixes to the migrated routes only when the whole area is green;
    slices must not edit those lists. The old apps keep serving modules until
    the gate flips.

## 17. Definition of done for the slices reading this spec

Each slice:

- implements exactly its rows as described here, appending **built** to the
  Status cell of each row in `parity/inventory/modules.md`;
- covers every line and branch it changed with unit tests (no stubs, no
  skipped tests, no weakened assertions);
- runs its package's typecheck, lint and tests plus `check:boundaries`,
  `check:headers`, `check:deps`, `check:coexistence` when `web_new` changed,
  the similarity check and the paths gate;
- brings up **no** parity stack (only the area gate does);
- does **not** fork the archived-modules screen, the issues layouts, the
  peek shell, the analytics dialog, the project-features switch or the
  palette; if a shared piece needs a prop, it adds it to the owner and notes
  it on that issue;
- writes copy in the product's voice except where this spec marks a label
  **pinned**, which must match literally;
- implements the **intended** behavior for every §15 disposition and never
  reproduces the old bug.
