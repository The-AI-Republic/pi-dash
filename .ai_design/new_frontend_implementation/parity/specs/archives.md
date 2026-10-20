# Archives area spec — tabs, archived work items, cycles, modules, project archive (NEWFRONT-696)

Build spec for inventory rows ARCH-001–ARCH-032 (plus one proposed row,
ARCH-033, in §14). It is the single file the Archives slices build from;
the inventory
(`.ai_design/new_frontend_implementation/parity/inventory/archives.md`)
stays the capability ledger, and the five oracle specs under
`apps/web_new/e2e/parity/archives/` remain the behavior reference — already
green on the old app — that the area gate replays against `apps/web_new`.

Prose only. No code, class strings, UI text, icons, or styles are copied
from the old frontend. API paths, query parameters, field names and
storage keys are named exactly because contracts and slices build from
them. Where a row is pinned by an old bug (the oracle scenario asserts the
buggy behavior, tagged `bug:` with a linked issue), this spec states the
intended behavior the slice builds and the scenario is aligned at gate
time — never by weakening the assertion.

Endpoint shorthand, used throughout:

- `BASE` = `/api/workspaces/{ws}/projects/{projectId}`
- `WS` = `/api/workspaces/{ws}`
- `ARCHIVED_TABS` = `/archives/issues`, `/archives/cycles`, `/archives/modules`
- `DETAIL` = `/archives/issues/{archivedIssueId}`

Everything below is web-new behavior unless it is explicitly marked as an
old-app observation. The old sources were read as reference only: the
archives routes and their header, the tab strip, the archived-work-items
header and layout root, the archived cycles and modules folders (root,
header, list, modal), the issue/cycle/module archive services, the
archived-issue and archived-filter stores, the project archive/restore
modal, the auto-archive row and the archive-related row menus, empty
states and dropdowns.

---

## 1. Shell: tabs, breadcrumb, badge, titles, addresses (ARCH-001–003)

Owned by **NEWFRONT-698** (Archives slice: shell, tabs, badges, titles).

### 1.1 Route shape (ARCH-001, ARCH-029)

Every project gets three archive addresses under the project detail tree:

- `/{ws}/projects/{projectId}/archives/issues`
- `/{ws}/projects/{projectId}/archives/cycles`
- `/{ws}/projects/{projectId}/archives/modules`

The work-items tab is the canonical landing address: the project quick
actions and the sidebar project row menu in new code already navigate there
(`features/projects/components/ProjectQuickActions.tsx`,
`SidebarProjectRow.tsx`), so the shell only has to render that address.
The sidebar's "Archives" destination
(`features/sidebar/more.ts`: `segment: "projects/archives"`) is the
*archived-projects* list, not a project's archives; it is not an entry
point for this area and is left alone.
There is no `archives` index route that redirects; a bare `.../archives`
address is not a supported entry (old app had no index route either) —
the shell resolves it to `.../archives/issues` so shared links never land
on an empty frame.

Old-to-new mapping: the old app used the same three path segments. New
routes mount as a per-tab child under
`routes/{$ws}/_gated/projects/{$projectId}/archives/` with a shared layout
that owns the tab strip and header, and one route file per tab plus the
detail route (see §3.5). The workspace-level `projects/archives` route that
exists in new code is the *archived projects list* (SHELL area) and is
unrelated; the shell slice must not extend it.

### 1.2 Tab strip (ARCH-001)

The strip renders above the content of every archives screen (including
the archived-issue detail, where it is not shown — the detail is a child
of the issues tab and renders its own breadcrumb, §3.5). Order is fixed:
work items, cycles, modules. Labels are the new app's own strings; the
oracle asserts exactly `["Work items", "Cycles", "Modules"]` (the project
tab strip in `features/projects/tabs.ts` spells its own first tab "Work
Items" — the archives strip is a separate catalogue, so keep these three
labels precise and singular in casing).

Gating:

- work items: always rendered.
- cycles: rendered only when the project's `cycle_view` flag is true.
- modules: rendered only when the project's `module_view` flag is true.

The flags come from the project read the project layout already performs
(`GET BASE/` → `cycle_view`, `module_view`, the same fields
`features/projects/tabs.ts` uses for the project tab strip); the tab strip
re-renders when those flags change, and a reload re-reads them. The oracle toggles both
flags through `PATCH BASE/` and reloads, then asserts the tab list shrinks
to `["Work items", "Modules"]` when only `module_view` remains on. The
shell must therefore not cache the tab list across a full reload.

Active tab: the tab matching the current pathname is marked active
(visual emphasis plus `aria-current="page"`), the other tabs are
inactive links. Clicking the active tab changes nothing and does not
re-fetch. Clicking another tab is client-side navigation to that address;
each tab address is a plain link so middle-click / copy-address works.

If a disabled tab's address is typed directly (e.g. `archives/cycles`
while `cycle_view` is off), the shell renders the issues tab content at
the `/archives/issues` address instead of an empty frame — the same outcome
a user gets by following the navigable strip. This is an edge case the
oracle does not drive; it is stated so no slice invents a 404.

### 1.3 Header and breadcrumb (ARCH-002)

Header trail, left to right: the project switcher crumb (project name,
clicking opens the project switcher — reuse the existing project crumbs
component from the shell area), then "Archives", then the active tab label.
The "Archives" crumb links to `.../archives/issues`; on the cycles or
modules tab it still links there (there is no archives root page). The
project crumb truncates responsively; the oracle matches the project name
by containment, so truncation chrome is free.

The back control renders only at narrow viewports (≤640px in the oracle;
the shell's breakpoint constant is the single source). On desktop the
header carries the trail but no back control. The back control returns to
the previously visited screen (history back), and the oracle's narrow
flow opens live work items then archives and expects back to land on the
live work-items screen.

Archived work-item count badge (ARCH-002): on the work-items tab only, the
header shows a numeric badge with the total archived work items for the
project, computed from the first archived-issues page's total count (the
same read as §3.1; the envelope's `count`/`total_count` field, not the
length of the page). It is hidden entirely when the count is zero — no
"0" badge and no tooltip. Hovering or focusing the badge shows an
explanatory tip containing the number and the phrase "work item" (singular
for one, plural otherwise); the oracle asserts the tip contains "2" and
matches `/work items?/`. The badge updates after the first archived read
settles and after restores (a restore lowers it); it does not poll.

On the cycles and modules tabs there is no badge.

### 1.4 Browser title (ARCH-003)

`document.title` = `<project name> - Archived <section>` where section is
`work items`, `cycles`, `modules` (lower-case, matching the oracle's exact
expectations: `"<project> - Archived work items"`). The detail address
titles itself `<identifier>-<sequence> <name>` (see §3.5). Titles update on
client-side tab switches without a reload; the shell sets them per route,
not through a single effect keyed on the pathname's last segment only.

### 1.5 Loading and settling on entry (ARCH-030, ARCH-028)

The shell owns the skeleton *slot*; each tab owns its own
skeleton→content/empty/no-match switch (§2.4, §5.3). What the shell
guarantees, and what every slice must not regress, is the cold-entry
behavior called out by bugs NEWFRONT-228, 231 and 239: a direct load,
reload, shared link or hard navigation to any of the three tab addresses
settles — it shows the skeleton only while the tab's own list read is in
flight and then renders content, empty state, or no-match. It never waits
for a live screen to have been visited first. The old app's primed-only
behavior (archived selectors gated on a live fetch having set a "fetched"
flag) is not reproduced. The oracle's primed helpers
(`archivesPrimeAndEnter`, `archivesTabClick`, `archivesCyclesOpenTabViaLive`)
exist because the old app needs them; new-web must also pass direct-address
loads. The only archives scenario that asserts direct-load rendering today
is `archivesDirectLoadRenders` (a modules assertion in
`archives/modules.spec.ts`); for issues and cycles the direct load is
proven inside the primed-entry scenarios (the address, reload and
shared-link steps of ARCH-011/014/029). The area gate keeps the bug
scenarios and `archivesDirectLoadRenders` green and adds no substitute
scenario of its own.

Traffic: entering a tab issues exactly one first-page read for that tab.
Returning to the tab (client-side) refreshes it: a new read is sent. Idle
time sends no reads and no writes (no polling; see §12).

### 1.6 Shell driver ownership (NEWFRONT-698)

Single owner of the shared shell methods the oracle specs call (see §16
for the full table): `archivesOpenIssuesList`, `archivesOpenList`,
`archivesTabOpen`, `archivesTabClick`, `archivesTabNames`,
`archivesActiveTab`, `archivesActiveTabKey`, `archivesClickBack`,
`archivesBackPresent`, `archivesClientNavigate`, `archivesCountBadge`,
`archivesCountBadgeTooltip`, `archivesPageTitle`,
`archivesDirectLoadRenders`, `archivesCyclesReload`,
`archivesArmSkeletonObserver`,
`archivesSkeletonVisible`, `archivesSkeletonWasSeen`,
`archivesDelayNextListReads`, `archivesBeginTrafficSpy`,
`archivesTrafficCounts`, `archivesPrimeAndEnter`, and the tab-address
helpers (the three unused ones in §17 included). `archivesCyclesReload`
does a full page reload and is owned here, exactly once, because the
cycles scenario calls it and 701/703 must share a single implementation;
the Cycles slice does not declare it again. All other slices call these
read-only and never re-implement them.

---

## 2. Archived work items: list, filters, display, peek (ARCH-004–007)

Two slices touch this tab, deliberately:

- **NEWFRONT-699** builds the list, header (filters/display/search), the
  archived list data layer and the peek (ARCH-005, 006, 007). It builds the
  read-only rendering but does not mark ARCH-004 built, because ARCH-004's
  acceptance includes per-row menus.
- **NEWFRONT-700** finishes ARCH-004 (row menus) and owns 008–013.

No other slice may render a second archived work-items list.

### 2.1 List surface (ARCH-004)

Layout is list-only. The layout switcher does not render on this tab (the
archived store forces list); a stored board/gantt preference for the
project is ignored here and the live surfaces keep their own preference.
Rows render the same row anatomy as the live list (identifier/sequence,
title, property cells the Display control enables), but the whole screen is
read-only:

- no inline property editor opens from any cell or header property
  (the oracle probes the row title cell and expects no editor);
- no quick-add row, no create entry, no bulk-selection toolbar actions
  that write, no "edit" affordance in hover actions;
- row menus exist (see §3.2) but only carry read/restore/delete entries.

Restore/delete entries are editor-only (§11); viewers and guests still see
rows and may peek, copy links and open detail.

Data: `GET BASE/archived-issues/` with the list query (§2.2). The server
defaults `order_by=-created_at` when none is sent (newest created first) and
passes it through `order_issue_queryset`, so a read without a stored
preference is newest-first. The app's display default is manual order
(`sort_order`, the Display panel's "Manual"), which the read carries once a
preference exists; before one does, the screen shows the server's
newest-first order. Grouping defaults to none, and
the display choices persist locally per project (see §2.3).
Each row links to its detail address `DETAIL`.

### 2.2 Filters and search (ARCH-005)

The header carries the shared rich-filter entry used by the live list
(NEWFRONT-626–629): the filter control opens the condition builder, the
applied conditions render as the shared filter row with one removable pill
per condition, a clear-all control, and an active indicator on the filter
button. The archived list reuses this component read-only — it must not fork
it. What differs from the live list:

- The condition set available is the work-item set the oracle seeds and
  asserts: priority, state group, state, cycle, module, assignee, creator,
  label, start date, target date. Saved views are not offered (no Save
  entry on this tab), and view tabs do not render.
- Applying, removing or clearing a condition re-runs the archived list
  read with the serialized expression in `filters` (the same
  `{and: [{priority__in: "urgent"}]}`-style JSON the live list sends).
  The oracle asserts the outgoing request URL contains `/archived-issues/`
  and its params contain `filters` with `urgent`, then that removing the
  pill sends a request whose params no longer contain `urgent`, and that
  clear-all does the same for a stored `low` condition.
- Stored expressions restore: the archived filter state is held locally
  per project — browser storage key `issue_local_filters`, one entry per
  view keyed by the archived store key (`"ARCHIVED"`), workspace slug,
  project id as `viewId`, and `userId` undefined — and read once on entry.
  The entry's `filters.richFilters` member holds the expression (the
  camel-case read the oracle's `archivesSeedStoredExpression` driver
  seeds). **The new app must read and write exactly this old shape** — the
  driver seeds it directly, and nothing in the oracle goes through a new
  app writer first. A seeded expression must render its pills and narrow the
  very first list read. This is the NEWFRONT-242 fix: the old entry was inert
  and seeded expressions never re-applied because the store read a
  different key/casing than the writers write.
- The archived filters live **only** in that local record; the archived
  screen does not read or write the project's server user-properties
  record (`/user-properties/`) for filters. Display options (§2.3) follow
  the same local-only rule.
- The filter rows/pills use the shared condition-pill component, so the
  oracle's `isRichFilterRowVisible`, `richFilterRowText` and
  `richConditionCount` resolve without any archived-specific markup.

The metadata needed to render pickers (states, labels, members, cycles,
modules) comes from the project context providers the project layout
already mounts; the archived screen must not issue its own
filter-metadata endpoint. No dedicated filter-metadata read is required by
the oracle, and this spec does not introduce one — if a slice finds a
missing dataset, it extends the shared project-context provider rather
than adding an archives-only fetch (and names the endpoint in the contracts
issue if it is genuinely new).

Search: the archived work-items header does not render a name search
(the old tab had none; searching is what filters are for). The oracle does
not drive search on this tab.

### 2.3 Display options (ARCH-006)

The header's display control reuses the live Display panel (NEWFRONT-618),
restricted to the options the archived layout can honor:

- Group by: state, cycle, module, priority, labels, assignees, creator,
  none. Grouping is client-side over the fetched page(s); the grouped
  headings are the same heading component as the live list. The oracle
  sets state grouping and expects rows split under headings, then "None"
  and expects the headings gone and rows reunited.
- Order by: manual order (`sort_order`), last created (`-created_at`),
  last updated (`-updated_at`), start date (`start_date`), priority
  (`-priority`) — the `archived_issues` catalogue's exact list. Manual
  ordering on this tab is the archived rows' stored order — reordering is
  not offered (no drag; §12), only the ordering key.
- Display properties: the live list's column toggles (identifier, priority,
  state, assignees, start/target dates, labels, cycle, module, etc.).
  Toggling a column re-renders rows and changes their text (the oracle
  toggles one column and expects the row text to change).
- Empty groups: the live show/hide-empty-groups toggle applies to the
  grouped view.
- Layout and calendar/board filters are not shown. **Item type / sub-issue
  filters are not shown either:** the ARCH-006 row's "item type" wording has
  no counterpart on the old archived display panel
  (`ISSUE_DISPLAY_FILTERS_BY_PAGE.archived_issues` exposes display
  properties, group by, order by and the show-empty-groups extra only). The
  row text is satisfied by the archived-display contract in this section:
  the panel offers grouping, ordering and display properties and no type
  control. The new app must not invent a work-item-type picker on this tab;
  if a type filter is ever offered, the row gets its own inventory entry and
  oracle scenario first.

Persistence: display filters and properties are stored in the same local
storage record as §2.2, per project, and applied on entry. Ordering changes
that need a server re-read (last created/updated, start date, priority)
send the read again with `order_by` (and `group_by` where the backend
groups); pure client-side regrouping re-renders without a read. The oracle
only asserts the visible outcome (row order, headings, row text), so the
implementation may choose the cheaper of the two as long as a reload
restores the chosen display.

### 2.4 Skeleton and empty states (ARCH-030, ARCH-033)

First load shows the list skeleton until the archived read settles. Then:

- rows and no active filters/search → list.
- rows present but the filter row hides everything → the shared
  filtered-empty state whose primary action clears the filters (restores
  the unfiltered list). Clearing filters is a local read-state reset with no
  server permission behind it, so the action is enabled for every role that
  can apply a filter. The old app additionally disabled it for guests; the
  new app enables it — an additive difference that removes no capability.
- zero archived rows and no filters → the archived "nothing archived yet"
  state: an explanation plus a control that opens the project's automations
  settings. The canonical address is
  `/{ws}/settings/projects/{projectId}/automations` (the project-settings
  route that hosts the auto-archive row, NEWFRONT-600); there is **no**
  `/{ws}/projects/{projectId}/automations` alias in the new router — that
  address is named nowhere in this spec. This control is *not* a
  create-work-item action.
  Role: the control is **present but disabled for roles that cannot open
  the automations page** (guests and plain members; the page itself is
  admin-gated, AGT-037, and refuses them). The old empty state renders the
  same affordance with a disabled action for non-editors, so keeping it
  present-but-disabled preserves the observable affordance without
  producing a navigation that lands on the not-authorized view. Admins get
  an enabled control. The explanation text renders for every role.

This empty state is inventory gap G-1; see §14 for the row decision and
owner.

Failures: if the archived read fails (403/500), the tab renders an error
state with a retry control — not a skeleton forever. This is the
NEWFRONT-228/231/239 guard on this tab: whatever the status, the screen
settles into content, empty/no-match, or an explicit error.

### 2.5 Side peek (ARCH-007)

Selecting a row opens the side peek beside the list, using the shared
peek shell (NEWFRONT-646) with the archived restriction set:

- title and description render in their read-only display form (no editing
  surface, no autosave);
- every property picker (state, priority, assignee, dates, labels, cycle,
  module, estimate, etc.) is disabled and cannot open;
- the activity/comments area is read-only (no composer; reaction and
  activity editing disabled);
- quick actions inside the peek are restricted to the archived set
  (copy link; restore for editors), never live writes.

Address: the peeked item is carried in the `issue` query param on the
archives list address (plus `project`/`nesting` cleared to null for this
context). The oracle reads the URL params and asserts `issue` equals the
archived issue id while `project` and `nesting` are null; reload reopens
the same peek; closing the peek removes all three params. Clicking a
different row replaces the param. Escape closes the peek and clears the
param.

The peek body is read from the archived list row plus the single-item
read (§18).

### 2.6 Reuse and ownership (issues tab)

Reused read-only from the Issues area: the list layout and row components
(NEWFRONT-619), display panel (NEWFRONT-618), rich filters and filter row
(NEWFRONT-626–629), peek shell (NEWFRONT-646) with detail composition
(NEWFRONT-639/640/642), row menus and the archive modal (NEWFRONT-636/632),
empty states (NEWFRONT-637 / ISS-071), prefs and pickers from `shared/`.
Built by the Archives slices: the archived list data layer and read-only
wiring (699), the archived row menu wiring and detail/dialog (700), the
empty-state composition and automations route constant (700). If a needed
change lives in one of the Issues components, the slice relates that
Issues slice instead of forking the component.

---

## 3. Archived work-item item actions and detail (ARCH-004, 008–013)

Owned by **NEWFRONT-700**.

### 3.1 Restore from the row menu (ARCH-008)

Editors (project admin or member) see a Restore entry in each row's menu.
Choosing it:

- sends `DELETE BASE/issues/{issueId}/archive/` (204);
- on success shows the shared success toast and removes the row without a
  full reload; the live work-items list shows the item again on its next
  read (and the count badge drops);
- on failure shows the shared error toast (with retry semantics of the
  shared mutation layer) and the row stays;
- the row menu closes; no navigation happens.

Viewers and guests never see Restore. The oracle also proves failure by
failing the next archive-scoped mutation and asserting an error toast plus
the item still archived on the server.

### 3.2 Delete from the row menu (ARCH-009)

The intended archived row menu has four entries: Restore, Open in new tab,
Copy link, Delete. Delete is editor-only and opens the shared delete
confirmation dialog (the same dialog the live row/detail Delete use,
NEWFRONT-636/632), naming the item by identifier and sequence. Confirming
sends `DELETE BASE/issues/{issueId}/`, shows the success toast, and removes
the row; failure shows the error toast and keeps the row. Cancel closes the
dialog and changes nothing.

Bug: NEWFRONT-165 and NEWFRONT-234 — the old read-only wiring hid the
row-menu Delete, so deletion only existed on the detail screen. The oracle
scenarios assert the three-entry menu; the intended four-entry menu above
is what the slice builds, and the gate aligns those scenarios.

### 3.3 Copy link / open in new tab (ARCH-010)

Both entries must produce the archived detail address
`/{ws}/projects/{projectId}/archives/issues/{issueId}` — the absolute URL
built from the current origin. Copy link writes it to the clipboard and
shows the shared "link copied" success toast; open in new tab opens that
URL in a new tab. Neither is editor-only.

Bug: NEWFRONT-235 and NEWFRONT-243 — the old rows built a live browse
address from a missing identifier, yielding `/browse/undefined-<seq>/`. The
oracle scenarios assert that broken shape; the slice builds the archived
address above and the gate aligns them. The capability-level copy
assertion in the permissions matrix (URL-shaped payload, success toast)
already holds for that shape and keeps working.

### 3.4 Detail address, banner, loader, not-found (ARCH-011)

The detail route `/{ws}/projects/{projectId}/archives/issues/{archivedIssueId}`
renders:

- header breadcrumb: project, Archives, Work items, then
  `{project.identifier}-{issue.sequence_id}` (and the item title as the
  document title). The identifier + sequence resolves only once the
  record is loaded; before that the crumb is empty (the oracle's
  held-fetch probe expects the reference absent during the hold).
- an archived notice banner stating the item is archived, with a control
  that returns to the archived list address `.../archives/issues`.
- while the record read is in flight: the shared detail skeleton / loader.
  A cold entry holds the record read; the probe expects the banner, the
  activity section and the identifier reference all absent during the
  hold, then all present after release. The old app rendered a blank
  content area (bug NEWFRONT-245); the slice builds the loader.
- a missing/forbidden record: the shared not-found state with a control
  back to the live work-items list of the project. The old app rendered a
  blank page with no error and no way out (bug NEWFRONT-236). Holding the
  URL is fine; the oracle's grace window after chrome paint must show the
  not-found control.
- read-only content: the detail main column (title, description) renders
  in display form; the properties sidebar is disabled; activity is
  read-only; the subscribe control is not rendered; reaction control is
  disabled.

Data: the archived single-item read `GET BASE/issues/{issueId}/archive/`
returns the archived record including `archived_at`; it is the read the
detail and peek body use (contract in §18). The live detail read also
resolves archived ids today, but the archives screens must not depend on
it: the archived scope is the one the old archive service calls, and using
one read keeps the peek and the detail consistent. The project read
supplies the identifier.

The held-fetch probe in the oracle intercepts a `GET` whose path contains
`/projects/{projectId}/issues/{issueId}`; both the archive-scoped read and
the detail read match it, so the probe works regardless — but the app must
issue exactly one such read on entry (not a lite read followed by a second
full read) so the sample is deterministic.

### 3.5 Detail quick actions (ARCH-012)

The detail header quick actions for editors: Restore, Delete, Copy link.
Restore sends `DELETE BASE/issues/{issueId}/archive/`, toasts success and
navigates to the item's live address: the short-link route
`/{ws}/projects/{projectId}/issues/{issueId}`, which the shell resolves to
the canonical `/{ws}/browse/{identifier}-{sequence}` address (the oracle
asserts the path contains `/browse/`). The live address itself is the
Issues/shell area's business; Archives only has to land there. Delete opens
the shared delete confirmation, sends `DELETE BASE/issues/{issueId}/`,
toasts success and returns to `.../archives/issues`. Both failures toast an
error and keep the user and the archived state. Viewers see the detail and
copy link but no Restore/Delete.

### 3.6 Archive a live work item (ARCH-013)

Entry point: the live Issues surfaces — the row/layout quick actions menu
(NEWFRONT-636, ISS-061) and the live detail header actions
(NEWFRONT-644) — open the shared archive dialog owned by NEWFRONT-632
(ISS-134). The Archives slice consumes that dialog; it does not build a
second one. Where the archived entry point needs different copy or gating,
it extends the Issues component through the blocked_by edge.

Behavior:

- the menu entry renders only for editors (project admin/member) on live,
  non-archived work items; it is disabled with an explanatory tooltip
  until the item's state group is `completed` or `cancelled`; on the
  completed/cancelled item it is enabled;
- the dialog heading names the item by kind, project identifier and
  sequence (the oracle reads the heading and expects the identifier and
  sequence as separate words: `Archive Work item IDENT SEQ`; the new app
  writes its own casing but must include both tokens);
  the body says the item moves to the project's archive and can be
  restored later (the oracle matches `/restor/i`);
- confirming sends `POST BASE/issues/{issueId}/archive/` (200
  `{archived_at}`), shows the success toast, closes the dialog and removes
  the item from live lists; it appears under the archives tab. The oracle
  then checks the server's archived list contains it and the archive
  address shows it;
- cancelling closes without a write (the server state is unchanged);
- a failed POST shows the error toast and keeps the item live and the
  dialog state recoverable. The old app's success-only path is not
  reproduced;
- later a restore returns it to live lists.

Guests and viewers never see the entry (server also enforces).

---

## 4. Archived cycles (ARCH-014–019)

Owned by **NEWFRONT-701**, which also owns the read-only archived-cycle
peek body reused by NEWFRONT-702 and the `archivesCycles*` driver family.

Seam with the Cycles epic: CYC-027 (NEWFRONT-711) owns the live cycle
list, its search/filters chrome and the archived-cycles *list* as it
appears from the Cycles area; CYC-024–026 (NEWFRONT-711) own the live
archive entry and restore entries on the live surfaces. NEWFRONT-701 does
not build a second archived-cycles screen or a second archive dialog. It
builds the Archives-tab composition (mounted in the NEWFRONT-698 shell),
the address-driven peek, the exact search/filter interactions, the three
empty/no-match states, the cold-entry settling, and the archived-specific
gating/copy/error handling the oracle checks. Anything that belongs in a
CYC component is extended there through the edge.

### 4.1 List and peek (ARCH-014)

Data: `GET BASE/archived-cycles/` returns the archived cycles for the
project as a flat JSON array (no pagination envelope). Server order is
`-is_favorite, -created_at`: favorites first, and within each group newest
created first. The list must not promise "creation order"; it is newest
first, and the order is the server's, not a client sort. The row fields the list and peek draw
from are pinned in §18: `id`, `project_id`, `name`, `description`,
`start_date`, `end_date`, `owned_by_id`, `status` (`CURRENT`, `UPCOMING`,
`COMPLETED`, `DRAFT`), `archived_at`, `is_favorite`, `total_issues`,
`completed_issues`, `started_issues`, `unstarted_issues`,
`cancelled_issues`, `backlog_issues`, `assignee_ids`.

The tab renders a plain flat list of cycle rows — unlike the live screen,
there is no "Upcoming/Completed" grouping and no group headings; the
oracle asserts group headings are empty on this tab while the live screen
groups. Each row shows cycle name, the dates, and progress. Rows are not
draggable (§12).

Selecting a row opens the cycle side peek with the archived cycle record
read from `GET BASE/archived-cycles/{cycleId}/`; the panel is read-only
(§5.6). The peeked cycle is carried as `peekCycle={cycleId}` on the tab
address; reload or a shared link reopens it, and closing removes the
param. The oracle:
- opens the tab, sees only the archived cycle in the list, and that the
  live screen groups instead (both server-side membership checks agree);
- peeks a row, asserts the param, closes, asserts the param gone; compares
  the peek body with the archived detail read.

Cold entry must settle (bugs NEWFRONT-231/239); the old app pinned a
skeleton-forever on fresh loads and dropped an open peek on reload.

### 4.2 Search (ARCH-015)

The header search is expandable: a magnifier button expands the box and
focuses the input. Typing filters the rendered rows client-side by cycle
name (case-insensitive substring); no server read is issued. Escape clears
the text on the first press (box stays open, focused) and collapses on the
second; the clear control empties the text and collapses; clicking outside
collapses only when the text is empty, never when it has content. Searching
never writes. The oracle asserts each of these, plus that server state is
untouched (no new reads/writes during typing).

### 4.3 Filters and chips (ARCH-016)

The filter menu offers only date ranges: Start date and Due date,
each with the shared relative-date options (e.g. "1 week from now",
"1 month from now", past variants). It does **not** offer status (the old
archived root hides the status section); module/lead/member sections belong
to the modules tab. Applied filters render as removable chips in a chip row;
a clear-all control drops every chip at once; removing or clearing
re-renders the list. Filtering is client-side; the oracle asserts no new
server reads/writes.

The filter button must visibly indicate that filters are active (marker
or equivalent) — the visible marker the old app lacked (bug NEWFRONT-229).
The oracle's bug scenario asserts the absence on the old app; the
intended scenario (`archivesModuleFiltersActive`-equivalent on cycles, and
the modules variants) is aligned at gate time.

Storage: cycle archived filters and search live locally per project (the
cycle filter store's "archived" slot), restored on entry; they do not
persist to the server and do not leak into the live cycle tab.

### 4.4 Empty, no-match, skeleton (ARCH-017)

- first load: cycle skeleton until the read settles;
- zero archived cycles: an illustrated empty state explaining that
  archiving completed cycles is what fills the screen;
- cycles exist but an excluding filter hides all: a filters no-match hint
  (distinct wording/illustration);
- cycles exist but the search text matches nothing: a search no-match
  hint, distinct from the filter hint;
- the oracle asserts zero-state, both hints and their difference, that the
  two hint texts differ, that the skeleton appears while a delayed read is
  in flight, and that server state is untouched.

As with issues, a failed read settles into an error state, never a
skeleton forever (bugs 231/239).

### 4.5 Archive a live cycle (ARCH-018)

Entry point: the live cycle row menu on the live cycles screen. The single
owner is **NEWFRONT-711** (Cycles slice E, rows CYC-024 archive and CYC-028
row menus), built on the rows from NEWFRONT-708 (CYC-001..005). The
Archives slice (NEWFRONT-701) proves the dialog and its gating; it does not
place a second entry anywhere, and no other Archives slice may add one.

- Entry renders only for editors and only on non-archived cycles; it is
  disabled with an explanation until the cycle has finished (the cycle
  `status` is `COMPLETED`, which the server derives from `end_date` in the
  past; `DRAFT`, `CURRENT` and `UPCOMING` stay disabled); it hides
  entirely for archived cycles.
- The dialog names the cycle and notes it can be restored later.
- Confirm sends `POST BASE/cycles/{cycleId}/archive/` (200
  `{archived_at}`), toasts success and navigates to the live cycles
  screen; the server's archived list then contains it (oracle asserts).
- Cancel writes nothing.
- On failure: error toast, no success toast, no navigation; the cycle
  stays live. Bug: NEWFRONT-244 — the old store swallowed write errors,
  so failures reported success and navigated away. The oracle bug
  scenario pins that; the gate aligns it to the intended behavior.
- Project members (role member) see and can use the entry; guests see no
  entry and the server refuses the write.

### 4.6 Restore an archived cycle (ARCH-019)

From the archived-cycle row menu, editors see Restore alongside Copy link
and Open in new tab; Edit and Delete are not offered for archived cycles.
Restore sends `DELETE BASE/cycles/{cycleId}/archive/` (204; the probed
live path — the `archived-cycles/{cycleId}/unarchive/` alias 404s on
`/api` and must not be contracted), toasts success and keeps the user on
the archived cycles tab (the row disappears, the list re-renders).
**CYC-026's wording "sends the user to the archives list" is superseded
here by the oracle:** `archives/cycles.spec.ts` asserts the address stays
on `/archives/cycles` after a restore, and the new app stays on the tab.
The Cycles slice must not "fix" this toward a navigation. On
failure: error toast, the row stays archived, the user stays put (bug
NEWFRONT-244's intended behavior). Guests see no entry and the server
refuses.

### 4.7 Cycles driver ownership

NEWFRONT-701 implements the whole `archivesCycles*` family — except
`archivesCyclesReload`, which is the shell's single full-reload helper
(§1.6, §16) — plus `archivesOpenCyclesTab` and the cycle-peek family
(`archivesOpenCyclePeek`, `archivesCycleRowNames`, `archivesCyclePeekReadOnly`,
`archivesCloseCyclePeek`) reused read-only by NEWFRONT-702. It reuses the
shell (698) and shared peek-entry (699) methods read-only. CYC rows
(NEWFRONT-709/711) do not add archives driver methods; their own drivers
stay in the cycles family.

---

## 5. Archived modules (ARCH-020–025)

Owned by **NEWFRONT-702**.

Seam with the Modules epic (NEWFRONT-82): MOD-026 (archive), MOD-027
(archived-modules screen from the Modules area) and MOD-028 (restore) are
owned by **NEWFRONT-799** ("Modules slice G: archive/restore conversations
and archived-modules screen (MOD-026-028)"), the single owner of the live
module archive entry, the Modules-area archived screen and the restore.
NEWFRONT-702 is a **consumer**: it composes the archived-modules content
inside the NEWFRONT-698 shell (the project Archives tab), reuses
NEWFRONT-799's live entry/menus read-only, and extends a NEWFRONT-799
component only through the edge — it never forks one and never adds a
second live entry or a second archived list. The edge is
`NEWFRONT-702 --blocked-by NEWFRONT-799`. NEWFRONT-702 closes the parity
gaps listed below.

### 5.1 List, sort and peek (ARCH-020)

Data: `GET BASE/archived-modules/` (flat array; server order
`-is_favorite, -created_at`, i.e. favorites first and newest created first
within each group) with `id`, `workspace_id`, `project_id`, `name`,
`description`, `description_text`, `description_html`, `start_date`,
`target_date`, `status` (`backlog`, `planned`, `in-progress`, `paused`,
`completed`, `cancelled`; archiving is offered only when the status is
`completed` or `cancelled`), `lead_id`, `member_ids`, `total_issues`,
`completed_issues`, `started_issues`, `unstarted_issues`,
`cancelled_issues`, `backlog_issues`, `created_at`, `updated_at`,
`archived_at` (pinned by contracts NEWFRONT-697). The peek body reads
`GET BASE/archived-modules/{moduleId}/`.

Rows show module name, status, dates, lead/member avatars and progress.
A sort control is always visible in the header and shows the current key
(default Name) and direction (default Ascending). Options, in this order:
Name (`name`), Progress (`progress`), Number of work items
(`issues_length`), Due date (`target_date`), Created date (`created_at`),
Manual (`sort_order`); plus Ascending/Descending. Changing the key or direction reorders rows
immediately. The oracle: default name-ascending; created-date follows
creation order for a deliberately non-alphabetical creation sequence;
descending reverses. Sorting is display-only (local preference, per
project), never a server write, and rows do not reorder by dragging.

Selecting a row opens the module side peek (read-only, §5.6), with
`peekModule={moduleId}` on the tab address; reload reopens; close clears.
Cold entry settles (bug NEWFRONT-228).

### 5.2 Search and filters (ARCH-021)

Search behaves exactly as §4.2 (expand + focus, live client filter,
Escape clears then collapses, clear empties + collapses, outside-click
collapses only when empty).

Filters menu sections, in order: Lead, Members, Start date, Due date.
Status and favorite are hidden for archived modules (the old archived root
passes an `isArchived` flag that removes them) — the oracle asserts exactly
this four-section list. A lead filter applies client-side and raises a
chip whose key is `lead`; removing the chip restores the rows; clear-all
drops every applied filter and chip. The filter button indicates active
filters visibly (bug NEWFRONT-229). Filters and search persist locally per
project and never touch the live module filters.

The Display refinements the oracle expects on this tab (ordering as in
§5.1) are part of this header, not a separate menu.

### 5.3 Empty, no-match, skeleton (ARCH-022)

Same four outcomes as cycles with module wording: skeleton while fetching;
zero archived modules → illustrated empty state explaining that archiving
completed or cancelled modules fills the screen; filters-hide-all → filters
no-match; search-hides-all → search no-match; the three row lists/hints
differ. Failure settles on an error state.

### 5.4 Archive a live module (ARCH-023)

Entry point: the live module row menu on the live modules screen, owned by
**NEWFRONT-799** (MOD-026 archive; MOD-027 archived screen; MOD-028
restore). Behavior mirrors §4.5 with module specifics; NEWFRONT-702 proves
it and adds no entry of its own:

- editor-only, hidden on archived modules;
- disabled with an explanation until the module status is `completed` or
  `cancelled` (the oracle checks an unfinished module's disabled entry
  and a completed module's enabled entry);
- dialog names the module and notes restorability;
- confirm `POST BASE/modules/{moduleId}/archive/` (200 `{archived_at}`),
  success toast, navigate to the live modules screen; cancel writes
  nothing;
- guest sees no entry and the server refuses;
- failure: error toast, no success, no navigation (bug NEWFRONT-233's
  intended behavior; the old store reported success).

### 5.5 Restore an archived module (ARCH-024)

From the archived-module row menu, editors see Restore with Copy link and
Open in new tab; Edit and Delete are absent. Restore sends
`DELETE BASE/modules/{moduleId}/archive/` (204, the probed live path — do
not contract the `unarchive/` alias), toasts success, and keeps the user on
the archives modules tab with the module live. As with cycles (§4.6), the
oracle (`archives/modules.spec.ts`, ARCH-024) asserts the user stays on the
tab after restore; that is the intended behavior and it overrides any
wording in MOD-028 that implies navigation back to the live modules
screen. Failure: error toast, row
stays (bug NEWFRONT-233). Guests see no entry and the server refuses.

### 5.6 Read-only cycle and module panels (ARCH-025)

Both peek panels render the archived record's details (name, status,
dates, lead/members, progress counts/summary) and the same analytics
summaries the live panels show, but every editing affordance the live panel
offers is absent or disabled: no status picker, no add-link/attach control,
no lead/member edit, no date edit, no description edit. The oracle compares
the archived panel with its live twin: the archived panel asserts the
status is locked and no add-link affordance is offered, while the live twin
offers them. The archived-cycle half of this row is the read-only peek body
NEWFRONT-701 owns; NEWFRONT-702 reads it, does not fork it.

### 5.7 Modules driver ownership

NEWFRONT-702 implements the `archivesModule*` / archived-module menu
family and the live-module helpers (`archivesOpenModulesTab`,
`archivesOpenLiveModules`, `archivesLiveModuleMenuEntries`,
`archivesChooseLiveModuleMenuEntry`, `archivesArchivedModuleMenuEntries`,
`archivesChooseArchivedModuleMenuEntry`, `archivesAwaitModuleRow`,
`archivesFailNextModuleWrite`), and reuses the cycle-peek family (701),
shell (698) and shared peek entry (699) read-only. **NEWFRONT-799 does not
implement any `archives*` driver method:** its live-module rows are proven
by the Modules area's own drivers, and every archived-modules driver belongs
to 702 exactly once. Likewise CYC-027 (NEWFRONT-711) adds no `archives*`
method; the archived-cycles tab's drivers are 701's.

---

## 6. Project archive and restore (ARCH-026)

Owned by **NEWFRONT-703** for the archive-side proof; the dialog and
mutations already exist in new code and are **not rebuilt**:

- `features/projects/api/mutations.ts` exports `useArchiveProject` /
  `useRestoreProject` (NEWFRONT-284), invalidating the workspace subtree and
  removing the project's favorites row;
- `features/projects/components/ProjectArchiveDialog.tsx` owns the dialog,
  direction copy (`selectArchiveBody`), busy confirm and failure handling.
  It is shipped by **NEWFRONT-284** (SHELL-037, "Shell slice:
  archive/restore/delete + project create (SHELL-037-042)") and mounted from
  the **projects-list card menu**.

Entry points (all owned elsewhere; Archives proves them read-only and adds
none):

- **NEWFRONT-284**: the projects-list card menu (`cardMenu.tsx`), archive
  for a project admin on an active card, restore/delete for a project admin
  on an archived card (SHELL-037..039). Those are rows of NEWFRONT-284;
  there is no separate "SHELL-037 slice".
- **NEWFRONT-572** (Project-settings slice: General tab, PSET-005–009): the
  settings danger zone, which mounts the same dialog. In new code this slot
  is still a placeholder in `ProjectSettingsGeneralMount.tsx`, so the
  settings entry is not live yet; NEWFRONT-703 depends on NEWFRONT-572 for
  it and must not build it.
- The project quick actions (`ProjectQuickActions.tsx`) and the sidebar
  project row (`SidebarProjectRow.tsx`) carry **no** project archive/restore
  action in new code; their "Archives" entry is the project's work-items
  archive tab. Neither is an ARCH-026 entry point.

What the archives slice must prove and close:

- Archive direction: the dialog warns that the project — its work items,
  cycles, modules and pages — is archived, that it leaves the projects
  list and its work items leave search, and that a project admin can
  restore it later. The warning must name work items, cycles, modules,
  and pages. The current `selectArchiveBody("archive", …)` string names
  only work items and search, so this is a required **cross-slice change to
  NEWFRONT-284's `ProjectArchiveDialog`**. Owner: NEWFRONT-284 edits
  `selectArchiveBody` (and its unit test); NEWFRONT-703 proves the oracle
  sentence. Record the edge `NEWFRONT-703 --relates-to NEWFRONT-284` (the
  shell slice is already merged, so this is a follow-up change, not a
  blocker) and comment on NEWFRONT-284 that ARCH-026 needs the widened
  warning. NEWFRONT-703 must not fork the dialog to carry different copy.
- Confirm: `POST BASE/archive/` (200 `{archived_at}`), the confirm control
  shows a busy/progress state while the write is in flight, on success a
  success toast whose body states the project was archived (the oracle
  asserts the phrase "has been archived successfully", already shipped by
  the shell mutation in NEWFRONT-284) and a return to `/{ws}/projects`;
  the project disappears from active lists and its favorite entries are
  removed server-side. Favorites cleanup is server behavior; the app only
  re-reads favorites.
- Restore direction: dialog explains the project returns to the projects
  list and becomes visible to its project members again. Restore is
  **project-admin only**; non-admins never get an enabled restore action
  (the card/row entry for archived projects is admin-gated). Confirm
  `DELETE BASE/archive/` (204), success toast, project back in the list. The
  restore success toast body the oracle asserts is "in your projects"
  (`archives/project-crosscutting.spec.ts`, "confirming restores"), already
  shipped by the shell mutation in NEWFRONT-284; keep it.
- Failures (either direction): error toast, dialog stays open with prior
  state intact, project state unchanged on the server.
- Closing the dialog after a failure or a cancel resets it: reopening shows
  the idle confirm with no busy spinner (the oracle checks this).

Cross-epic note (also §15): NEWFRONT-284 already implements the write path
and the dialog (SHELL-037). NEWFRONT-703 must not create a second dialog or
a second pair of mutations; the only code change it may request is the
widened warning copy in `selectArchiveBody`, owned by NEWFRONT-284 through
the edge above. Everything else is proof from the archives side.

---

## 7. Auto-archive setting (ARCH-027)

Owned by **NEWFRONT-703** for the archives-side proof; the surface is
Project-settings/Agents owned and already exists:

- route `/{ws}/settings/projects/{projectId}/automations` (the only
  canonical address; no `/{ws}/projects/{projectId}/automations` alias
  exists), page `AutomationsSettings` / `AutomationRows` in
  `features/automations`,
  owned by NEWFRONT-600 / NEWFRONT-582 (AGT-034..037);
- the archive row is `ArchiveRow` in `AutomationRows.tsx` with the delay
  picker (`DelayPicker`) and `CustomMonthDialog`; `archiveTogglePatch`
  toggles between the stored window and 0;
- the write is `PATCH BASE/` with `{archive_in: <months>}` (0 disables).

Behavior proved from the archives side:

- disabled (`archive_in === 0` or unset): the row shows the switch off and
  no window picker; the switch on writes `archive_in: 1` (one month
  default) and persists;
- enabled: the window picker is visible and shows the stored months;
  choosing a preset writes that number; the custom entry opens the month
  dialog and writes the chosen number; the value survives a reload;
- switching off writes `archive_in: 0` and hides the picker;
- non-admins are **refused the automations page** (AGT-037 renders the
  not-authorized view), so no disabled controls render for them. This is
  the documented gap: the row's "non-admins see disabled controls"
  acceptance is unreachable; the oracle asserts the refusal. New-web keeps
  the refusal (it matches the intended access rule: auto-archive is
  admin-only) and the inventory row carries the gap note. Do not build a
  disabled read-only variant just to satisfy the text.
- The chosen window is server-side behavior: closed work items older than
  the window archive themselves. The screen only proves the stored value
  and that it drives the project representation; it does not prove the
  scheduler's timing (no clock control in the parity stack).

Cross-epic note (§15): nothing here is archived-area code. NEWFRONT-703
proves it, links the empty-state control (§14) to this route, and does not
re-implement the row.

---

## 8. Addresses and reload (ARCH-029)

Each tab address (`ARCHIVED_TABS`), the detail address (`DETAIL`) and the
peek params are shareable and reload-safe:

- `.../archives/issues` reloads onto work items with rows; the
  `issue=<id>` param reopens the same peek after reload; closing clears
  `issue`, `project`, `nesting`.
- `.../archives/cycles` reloads onto the active cycles tab and its shell,
  and settles to rows/empty (bugs 231/239 not reproduced);
  `peekCycle=<id>` reopens the peek after reload; close clears it.
- `.../archives/modules` the same with `peekModule=<id>`.
- `DETAIL` reloads to banner plus detail (§3.4) and survives a shared link.
- The tab strip and the "Archives" crumb preserve the current tab semantics
  described in §1.3; there is no state in component memory that a reload
  loses (filters/search/display come from local storage, peeks from the
  address).

The oracle drives all of this except the cycles/modules content assertions
that the old app cannot pass; those are the `bug:` scenarios 231/239 and
the gate aligns them.

---

## 9. Loading placeholders (ARCH-030)

One rule across the area: the first list/detail read for a surface shows
its skeleton (list skeleton on issues/cycles/modules, detail skeleton on
the archived item) and then settles to content, empty, no-match, or an
explicit error. Skeletons never outlive a settled request (success or
failure), and a delayed read leaves the skeleton visible only for its
duration. The oracle delays the next list read by 3s and asserts
skeleton → content/empty on each tab, and holds the detail record read and
asserts the loader (NEWFRONT-245 intended). The shell owns the skeleton
slot (§1.5); each tab owns its switch.

---

## 10. Confirmations and progress (ARCH-031)

Every archive, restore and delete in the area ends in exactly one visible
outcome:

- success: the shared success toast, followed by the navigation/stay rule
  of its row (§3, §4.5/4.6, §5.4/5.5);
- failure: the shared error toast, no success toast, no navigation away,
  and the server state stays as before (row remains archived/live).

Dialog progress: the archive/restore confirm controls show a busy state
while the write is in flight and cannot be double-submitted. Project
archive/restore (§6) shows the busy confirm; item-level actions use the
shared mutation busy state. The oracle proves project success + progress
+ dialog reset, project failure keeping the project archived, item restore
failure keeping the item archived. This is the scenario set behind bugs
NEWFRONT-233/244: the intended behavior is error-first, never the
swallowed-error success path.

---

## 11. Permissions matrix (ARCH-028)

Roles in new code are read from the **project** membership:
`ROLE_ADMIN` (project admin, 20), `ROLE_MEMBER` (15), `ROLE_GUEST` (5) —
`apps/web_new/src/core/session/permissions.ts` and `canEditInProject`. The
matrix is per project `member_role`; a workspace-level admin who is not a
project admin is **not** treated as one by the gate (the project read is the
only source). Editions: oss, cloud, desktop — the archives area has no
edition split (§11); the matrix below applies identically.

| Capability | Guest | Member (viewer of archived data, editor of live) | Admin |
|---|---|---|---|
| Open any enabled archives tab | yes | yes | yes |
| List/read archived work items, cycles, modules | yes | yes | yes |
| Filter, search (cycles/modules), display options (issues), sort (modules) | yes | yes | yes |
| Open issue/cycle/module peek, open archived issue detail, copy link | yes | yes | yes |
| Restore/delete archived work item (row + detail) | no | yes | yes |
| Restore archived cycle / module | no | yes | yes |
| Archive a live work item/cycle/module | no | yes | yes |
| Archive/restore the whole project | no | no | yes (admin only: archive on active projects, restore on archived ones) |
| Auto-archive setting (view/edit) | no | no | yes (non-admins are refused the automations page, AGT-037) |
| Empty-state "set automation" control | disabled | disabled | enabled |

Rendering rule: mutation entries are absent, not disabled, for roles that
cannot use them. Guests and viewers can use every read affordance. The one
exception is the empty-state automation control, which is **present but
disabled** for guests and plain members (§2.4, ARCH-033): it is an
affordance, not a mutation entry, and the page it opens is admin-gated, so
the control must not navigate a refused user. The project archive/restore
action in new code is admin-only on every surface (the projects-list card
menu gates both directions on project admin), so it is absent for members
and guests.

Guest-403 backend gap (NEWFRONT-240, in the PDASHOSS01 project): the old
server refuses guest reads of
`archived-issues/` and `archived-cycles/` (lists) and the archived issue
detail with 403 — while `archived-modules/` answers 200. The guest oracle
scenario asserts those 403s and the stuck skeletons/blank detail they cause.
**The oracle can never go new green without a PDASHOSS01 backend change**:
the scenario's assertion is about the server status codes, not UI. The area
gate must therefore, on the first run that reaches it:

1. file one `PDASHOSS01` (backend) issue to allow guest/project-member
   reads of archived issues, archived cycles and the archived issue
   detail (note in it that `PDASHOSS01-310` covers the adjacent live
   cycle-issues gap but not this one, and that NEWFRONT-240 is the
   frontend-side record), and
2. relate it to NEWFRONT-703 (`--blocked-by`) and, through the epic, note
   it on NEWFRONT-79, and
3. leave the guest scenario unweakened. Once the backend allows the reads,
   the scenario's expected statuses flip to 200 and its UI assertions
   (guests see every tab, filter, peek, copy, no mutation entries) become
   the new-green proof.

Until then the guest row stays oracle green / not new green, and the gate
reports it honestly rather than passing a weakened scenario. This is a
product/backend decision outside the archives code; the new frontend is
already written to render the read surfaces for guests.

Cold entry (NEWFRONT-239) and the direct-load bugs (228/231) are on the
**frontend** side and must be fixed by the new slices (§1.5, §4.1, §5.1) —
they are not excused by the backend gap.

---

## 12. Explicit absences (ARCH-032)

The archives area deliberately has none of the following, in any edition
or on desktop, and each is proven by behavior, not by absence of markup
where shared components could add chrome:

- **No keyboard shortcuts.** No single-key action archives, restores,
  deletes or opens a peek/menu from the archive screens. Pressing `a`,
  `r`, `Delete`, `Enter` on a focused archive tab sends no write, opens no
  dialog, opens no peek and opens no row menu. (Typing into the search box
  is the only keyboard input that filters; Escape closes the peek/search
  as specified.)
- **No drag-and-drop.** Issue rows do not reorder by dragging (a real drag
  leaves order and server state unchanged); the oracle also asserts
  cycle/module rows are not draggable at all. The shared list row may carry
  a draggable DOM attribute from live usage; the proof is behavioral
  (order unchanged, zero writes) plus no drag affordance.
- **No exports/imports.** No export/import control renders on any archive
  screen, and no export endpoint is called.
- **No live updates.** Entering a tab loads once; idle time produces no
  reads and no writes (no polling, no websocket-driven re-reads); a
  re-entry refreshes. Search/filter/sort changes are client-side and issue
  no reads.
- **No edition splits.** The OSS, cloud and desktop builds render the same
  screens, the same entries and the same endpoints; there are no
  cloud-only or desktop-only archive capabilities, and no upgrade prompts
  in this area. The oracle checks the served app exposes the same tabs and
  endpoints the oracle build does; the gate repeats the area suite per
  edition it can serve.

---

## 13. Cross-cutting behavior summary (ARCH-029–032)

The cross-cutting rows are not a screen: they are invariants every slice
must satisfy, proven by NEWFRONT-703 from the archives side on top of the
surfaces built by 698–702. The owning per-row map is in §21; the
driver-method partition is in §16–§17.

---

## 14. Inventory gap: archived work-items empty state (proposed ARCH-033)

**Decision: yes, a new row is needed.** The archived-empty-state behavior
exists in the oracle (`issues/layouts-a-empty-states.spec.ts`, ISS-071) but
no ARCH row names it; the Archives inventory's coverage table lists only
the archived list's loading branch, and the slice descriptions carry it
implicitly under NEWFRONT-700. Leaving it rowless means the area gate
cannot count it as covered and a regressing slice could drop it silently.

Proposed row, appended — **do not renumber existing rows**:

> **ARCH-033** — See the archived work-items empty state: with no archived
> items at all, an explanation plus a control that opens the project's
> automations settings (auto-archive); when items exist but filters hide
> them all, a clear-filters action instead.
>
> - Who: any project role sees the explanation. The automations control is
>   **enabled for project admins and present-but-disabled for members and
>   guests** (the automations page is admin-gated, AGT-037); the
>   clear-filters action on the filtered-empty branch is enabled for every
>   role. Edition: all.
> - Old entry point: archived work-items tab with zero visible rows.
> - API: `GET BASE/archived-issues/` (zero results); no write; the control
>   navigates to `/{ws}/settings/projects/{projectId}/automations`.
> - Acceptance: zero-archived → the "nothing archived yet" explanation
>   plus a control to the project's automations settings (the canonical
>   route, not a create action); the control is enabled only for project
>   admins and is rendered disabled for members and guests.
>   Filtered-empty → when archived rows exist and one or more applied
>   conditions hide them all, a "no matching results" state offering a
>   clear-filters action that restores the rows (enabled for every role
>   that can filter; the old app disabled it for guests, an additive
>   difference only). The two branches are mutually exclusive (no
>   explanation when a filter is active; no clear-filters action when
>   nothing is archived) and appear only after the skeleton settles.
> - Parity test: to be written as an ARCH-tagged oracle scenario under
>   `apps/web_new/e2e/parity/archives/` (the ISS-071 scenario is
>   Issues-row tagged and cannot join the ARCH tag set without a new
>   scenario). NEWFRONT-703 owns writing it; the area gate's fix sub-issue
>   is the fallback if 703 lands before the row exists in the inventory.

Owner: **NEWFRONT-700** (its description already says it composes the
empty state and owns the automations route constant). NEWFRONT-703 proves
it from the archives side together with ARCH-030. The two slices split the
behavior cleanly: 700 builds both branches and the control's role state;
703 owns the parity proof (the ARCH-tagged scenario) and the inventory row
follow-through, and never renders the empty state itself. Before NEWFRONT-700
starts, the row must be added to
`parity/inventory/archives.md` (append after ARCH-032, Status
`not started`) and an ARCH-tagged oracle scenario added under
`apps/web_new/e2e/parity/archives/`; if the row is not added by then,
NEWFRONT-700 builds the same behavior and the gate files the row + scenario
as a fix sub-issue rather than passing the row as covered. This spec is the
authority for the acceptance text above.

---

## 15. Cross-epic rows: what is already implemented and what remains

These rows cross into other epics. New code already ships part of them;
the archives slices must reuse, not rebuild.

### ARCH-026 — project archive/restore
Already implemented: `features/projects/api/mutations.ts`
(`useArchiveProject`/`useRestoreProject`, favorites cleanup client refresh) —
NEWFRONT-284; `ProjectArchiveDialog.tsx` + `selectArchiveBody` and its
busy/failure handling — NEWFRONT-284 (SHELL-037). Remaining for parity: the
dialog warning must name cycles, modules and pages in addition to work
items and search (a change NEWFRONT-284 makes to `selectArchiveBody`); prove
success → projects list, favorites removal, the "in your projects" restore
toast and both failure paths from the archives side. The settings danger
zone entry is NEWFRONT-572's; NEWFRONT-284's card menu covers the list
entry. **Do not rebuild** the dialog, the card menu or the mutations.

### ARCH-027 — auto-archive
Already implemented: `features/automations` (`AutomationRows`,
`ArchiveRow`, `DelayPicker`, `CustomMonthDialog`, `archiveTogglePatch`,
PATCH `archive_in`) — NEWFRONT-600 / NEWFRONT-582, AGT-034..037; route
`settings/projects/{projectId}/automations`. Remaining: nothing to build;
NEWFRONT-703 proves toggle default/preset/custom/zero persistence, the
non-admin refusal, and links the ARCH-033 empty-state control to this
route (`/{ws}/settings/projects/{projectId}/automations`). **Do not
rebuild** the row or the settings page.

### Live-surface entry ownership (ARCH-013, 018, 023, 026, 027)

Each of these rows lives, at the user-visible entry, on a surface another
epic owns. The table names the screen, the owning inventory rows and the
single slice that owns the entry. The Archives slice named in the last
column proves the dialog/gating from the archives side and never adds a
second entry, dialog or write path.

| Row | Entry screen (live surface) | Owning rows | Owning slice | Archives slice |
|---|---|---|---|---|
| ARCH-013 archive a work item | live work-items list row/layout quick actions and the live work-item detail header actions, in the Issues area | ISS-061 (layout quick actions), ISS-134 (archive modal), ISS-162 (detail "…" quick actions) | NEWFRONT-636 (row menu, ISS-061), NEWFRONT-632 (modal, ISS-134), NEWFRONT-644 (detail actions, ISS-162) | NEWFRONT-700 consumes; extends copy/gating only through those edges |
| ARCH-018 archive a cycle | live cycles list row menu (`/{ws}/projects/{id}/cycles`) | CYC-024 (archive), CYC-028 (row menus), CYC-001..005 (rows) | NEWFRONT-711 (CYC-024/028), NEWFRONT-708 (rows) | NEWFRONT-701 proves dialog, gating, error handling; does not add an entry |
| ARCH-023 archive a module | live modules list row menu (`/{ws}/projects/{id}/modules`) | MOD-026 (archive entry), MOD-027 (archived-modules screen), MOD-028 (restore) | NEWFRONT-799 (Modules slice G, MOD-026-028) | NEWFRONT-702 is a consumer: composes the archived tab and proves the dialog; extends the NEWFRONT-799 component through the edge (`NEWFRONT-702 --blocked-by NEWFRONT-799`) |
| ARCH-026 archive/restore a project | projects-list card menu; project settings danger zone (not yet mounted in new code) | SHELL-037..039 (archive/restore dialog and list entries; rows of NEWFRONT-284), PSET-005..009 (settings General tab) | NEWFRONT-284 (mutations + dialog + card menu); NEWFRONT-572 (settings danger zone) | NEWFRONT-703 requests the widened warning copy from NEWFRONT-284 (`--relates-to`) and proves the flow; waits on NEWFRONT-572 for the settings entry; no second dialog |
| ARCH-027 auto-archive | project automations settings (`/{ws}/settings/projects/{id}/automations`) | AGT-034..036 (archive/close rows, delay picker, custom month); AGT-037 (admin gate) | NEWFRONT-600 (agents automations), NEWFRONT-582 (project-settings automations tab) | NEWFRONT-703 proves persistence and the admin refusal; links the ARCH-033 empty-state control; no second row |

### ARCH-028 — permissions
Already implemented partially: `permissions.ts` roles and the project access
gate; mutation entries gated by `canEditInProject` in
`shared/shell/contextual`. Remaining: the per-surface entry gating of §11
(archives row menus, cycle/module menus, detail actions, project
archive/restore, automations refusal), the guest read surfaces, and the
NEWFRONT-240 backend blocker. NEWFRONT-703 closes the UI half and files
the backend issue.

---

## 16. Reuse map against the new shell and Issues area

Surveyed in `apps/web_new` (not the old app). For each seam: what the
Archives slices **build**, what they **reuse read-only**, and which
non-archives slice owns the entry point.

### F-06 shell
- Reused read-only: project crumbs/project switcher and header chrome,
  `ProjectTabStrip` (archives is not a project tab — the strip is not
  extended), the project access gate in
  `routes/{$ws}/_gated/projects/{$projectId}/route.tsx` (reads project,
  role, flags), the page-title mechanism, the shared toast/notification
  helpers (`notifySuccess`, `notifyError`), and the project quick actions /
  sidebar project row that already link to `archives/issues`.
- Built by NEWFRONT-698: the archives layout and per-tab routes, tab strip,
  header trail + badge + back control, titles, skeleton slot, and the
  addresses for detail.
- Shell-owned entries: project archive/restore dialog and mutations
  (NEWFRONT-284, SHELL-037..039) — §6; project tab strip/access gate —
  reused; sidebar "Archives" (archived projects list) — unrelated, reused
  as-is; project quick actions and sidebar project row navigation — reused
  as-is.

### Issues area
- NEWFRONT-619 (list layout/rows/quick actions chrome): reused for the
  archived list, grouping headings and row anatomy. NEWFRONT-699 builds the
  archived data layer and read-only wiring; NEWFRONT-700 builds the row
  menu configuration on top of the Issues row-menu component.
- NEWFRONT-618 (display options): reused for ARCH-006; NEWFRONT-699 passes
  the archived option subset (§2.3).
- NEWFRONT-626–629 (rich filters, filter row, condition pills): reused for
  ARCH-005; NEWFRONT-699 supplies the archived store and local storage.
- NEWFRONT-636 (row + layout quick actions, archived restricted menu,
  ISS-064): owns the live entry point for ARCH-013 (ISS-061) and the row
  menu component; NEWFRONT-700 configures the archived entries.
- NEWFRONT-632 (archive modal, ISS-134): owns the archive dialog for
  ARCH-013; NEWFRONT-700 consumes it, extending copy/gating only through
  the edge.
- NEWFRONT-637 (empty states, ISS-071): owns the empty-state components;
  NEWFRONT-700 composes them for ARCH-004/033.
- NEWFRONT-639 (detail core), 640/642 (sidebar properties), 644 (detail
  header actions), 646 (peek panel): reused for ARCH-007/011/012/025 read-
  only; NEWFRONT-699 wires the peek lock, NEWFRONT-700 the detail and its
  actions.
- No second list/detail/peek/menu/modal is built anywhere in Archives.

### Cycles epic (NEWFRONT-81)
- NEWFRONT-711 (Cycles slice E, rows CYC-022..028) is the owner of
  CYC-024 (archive a finished cycle), CYC-025/CYC-026 (restore), CYC-027
  (archived cycles list) and CYC-028 (row menus), plus the intended
  NEWFRONT-244 failure behavior. NEWFRONT-701 composes the archives tab and
  closes the ARCH-014..019 gaps (§4); it does not add a second live entry,
  a second dialog or a second archived list.
- Reused read-only by NEWFRONT-701: the cycle rows and list groups
  (NEWFRONT-708, CYC-001..005), lifecycle/date filters and row menus
  (NEWFRONT-709, CYC-006..008/028), and the narrow/touch and guest
  read-only surfaces (NEWFRONT-715, CYC-048). The `archivesCycles*` driver
  family belongs to NEWFRONT-701 (§16); the Cycles area's own drivers stay
  in its slices.
- Specific entry owners (each exactly one slice):
  live cycle archive entry → CYC-024, NEWFRONT-711;
  live cycle restore → CYC-025/026, NEWFRONT-711;
  archived-cycle row in the *Cycles* area → CYC-027, NEWFRONT-711;
  archived-cycles content of the project Archives tab → NEWFRONT-701.
- Archives tree consequence: relate **NEWFRONT-701 --blocked-by
  NEWFRONT-711** (a specific slice edge), not just an epic-level edge to
  NEWFRONT-81.
- Archives tree consequence: the Archives epic relates **NEWFRONT-701
  --blocked-by NEWFRONT-711** (specific slice edge), not just an epic-level
  edge to NEWFRONT-81.

### Modules epic (NEWFRONT-82)
- MOD-026 (archive entry), MOD-027 (archived-modules screen from the
  Modules area) and MOD-028 (restore) are owned by **NEWFRONT-799**
  ("Modules slice G: archive/restore conversations and archived-modules
  screen (MOD-026-028)"). NEWFRONT-799 is a single slice for all three
  rows, so the live entry, the Modules-area archived screen and the restore
  each have exactly one owner.
- NEWFRONT-702 is a **consumer** of 799: it composes the archived-modules
  content of the project's Archives tab, reuses 799's live entry/rows
  read-only, and extends a 799 component only through the edge.
- Specific entry owners (each exactly one slice):
  live module archive entry → MOD-026, NEWFRONT-799;
  live module restore → MOD-028, NEWFRONT-799;
  archived-module row in the *Modules* area → MOD-027, NEWFRONT-799;
  archived-modules content of the project Archives tab → NEWFRONT-702.
- Archives tree consequence: relate **NEWFRONT-702 --blocked-by
  NEWFRONT-799** (a specific slice edge). The epic-level edge to NEWFRONT-82
  is not enough.
- NEWFRONT-799 implements no `archives*` driver method (§16): the
  Modules-area rows are proven by that area's own drivers.

### Project settings / Agents
- Auto-archive row and automations page: NEWFRONT-600 (AGT-034..037) and
  NEWFRONT-582 (PSET feature flags + automations); route constant owned by
  NEWFRONT-700 for the ARCH-033 control. Reused read-only; not rebuilt.

### `e2e/parity/drivers/web-new.ts` shared driving surface

The archives driver methods are shared by ARCH, CYC and MOD rows: the tab
strip, row menus, peek, skeleton and traffic spy. To stop CYC-027
(NEWFRONT-711) and MOD-027/028 from each building their own archived tab,
ownership is by **family** with one owner and one implementation each:

| Family | Owner | Count (declared / exercised) | Read-only consumers |
|---|---|---|---|
| Shell | 698 | 26 / 23 | 699–703 |
| Issues list, filters, display, peek entry | 699 | 20 / 20 | 700, 703 |
| Issues row menus, mutations, detail, dialogs | 700 | 26 / 26 | 703 |
| Cycles (incl. the cycle-peek family) | 701 | 45 / 43 | 702, 703 |
| Modules | 702 | 32 / 32 | 703 |
| Cross-cutting | 703 | 8 / 8 | — |
| **Total** | | **157 / 152** | |

Every method below appears in exactly one family, and the six lists sum to
**157** — the declared total. CYC-027 (NEWFRONT-711) and the Modules slices
(NEWFRONT-799) do not add `archives*` methods for the archived tab; their
own drivers stay in their areas' families. Relate the edges as the spec
says so a CYC or MOD slice is not tempted to build a second archived tab
against a second driver set. The gate proves only this surface.

**Shell — NEWFRONT-698.** Tab strip and navigation, addresses, titles,
badge, back control, skeleton slot, traffic spy, priming, and the three
interface methods reserved for riding slices:

  `archivesActiveTab`, `archivesActiveTabKey`, `archivesArmSkeletonObserver`,
  `archivesBackPresent`, `archivesBeginTrafficSpy`, `archivesClickBack`,
  `archivesClientNavigate`, `archivesCountBadge`, `archivesCountBadgeTooltip`,
  `archivesCyclesReload`, `archivesDelayNextListReads`, `archivesDirectLoadRenders`,
  `archivesGoToProjectArchives`, `archivesOpenIssuesList`, `archivesOpenList`, `archivesOpenTab`,
  `archivesPageTextPresent`, `archivesPageTitle`, `archivesPrimeAndEnter`,
  `archivesSelectArchivesTab`, `archivesSkeletonVisible`, `archivesSkeletonWasSeen`,
  `archivesTabClick`, `archivesTabNames`, `archivesTabOpen`, `archivesTrafficCounts`

Declared but unused by the five archives specs, exactly matching §17's
list — these three stay in the interface for shell/riding slices and must
not be deleted: `archivesGoToProjectArchives`, `archivesPageTextPresent`,
`archivesSelectArchivesTab`. `archivesCyclesReload` is the reload helper the
cycles spec calls (`archives/cycles.spec.ts`); it is family-owned here so
701 and 703 share one implementation, and it is counted in this family, not
in Cycles. 703's address scenario uses the generic `reloadPage`.

**Issues list, filters, display, peek entry — NEWFRONT-699.** The archived
work-items list surface and the shared peek entry that 701/702/703 reuse:

  `archivesCollapseSearchOutside`, `archivesFilterControlVisible`, `archivesGroupHeadings`,
  `archivesInlineEditorOpens`, `archivesPeekActivityEditable`, `archivesPeekClose`,
  `archivesPeekDescriptionEditable`, `archivesPeekDescriptionText`, `archivesPeekOpenFirst`,
  `archivesPeekQueryParams`, `archivesPeekTitle`, `archivesPeekTitleLocked`,
  `archivesPeekVisible`, `archivesRowPresent`, `archivesRowText`, `archivesSearchText`,
  `archivesSearchType`, `archivesSeedStoredExpression`, `archivesVisibleIssueNames`,
  `archivesWaitForListQuery`

**Issues row menus, mutations, detail, dialogs — NEWFRONT-700.** Row menus
on the issues tab, failure shaping, the archived detail, the archive
dialog and delete confirmation:

  `archivesArchiveDialog`, `archivesArchiveModalBody`, `archivesArchiveModalCancel`,
  `archivesArchiveModalTitle`, `archivesCancelArchiveDialog`, `archivesClearMutationFailure`,
  `archivesCloseMenus`, `archivesConfirmArchiveDialog`, `archivesDetailBannerBack`,
  `archivesDetailBannerText`, `archivesDetailBreadcrumbText`, `archivesDetailComposerVisible`,
  `archivesDetailMenuChoose`, `archivesDetailNotFoundVisible`,
  `archivesDetailPendingStateOnHeldFetch`, `archivesDetailReactionControlEnabled`,
  `archivesFailNextMutation`, `archivesItemFailNextWrite`, `archivesMutationDetailBannerText`,
  `archivesMutationVisibleIssueNames`, `archivesOpenDetailRaw`, `archivesOpenRowMenu`,
  `archivesRowMenuClick`, `archivesRowMenuEntries`, `archivesRowMenuEntryTitles`,
  `archivesRowMenuOpenFirst`

**Cycles — NEWFRONT-701.** The `archivesCycles*` family (minus the
shell-owned `archivesCyclesReload`) plus the cycle-peek family reused by
702:

  `archivesCloseCyclePeek`, `archivesCyclePeekName`, `archivesCyclePeekReadOnly`,
  `archivesCycleRowNames`, `archivesCyclesArchiveDialogCancel`,
  `archivesCyclesArchiveDialogConfirm`, `archivesCyclesArchiveDialogText`,
  `archivesCyclesClickAway`, `archivesCyclesClosePeek`, `archivesCyclesEmptyHeading`,
  `archivesCyclesFilterChipTexts`, `archivesCyclesFilterOptionNames`, `archivesCyclesFilterPick`,
  `archivesCyclesFilterRemoveChip`, `archivesCyclesFilterSections`,
  `archivesCyclesFiltersActive`, `archivesCyclesFiltersClearAll`, `archivesCyclesFiltersClose`,
  `archivesCyclesFiltersOpen`, `archivesCyclesGroupHeadings`, `archivesCyclesLiveGroupOpen`,
  `archivesCyclesMenuEntries`, `archivesCyclesMenuPick`, `archivesCyclesNoMatchHint`,
  `archivesCyclesOpenLive`, `archivesCyclesOpenPeek`, `archivesCyclesOpenPeekLink`,
  `archivesCyclesOpenRowMenu`, `archivesCyclesOpenTabRaw`, `archivesCyclesOpenTabViaLive`,
  `archivesCyclesPeekName`, `archivesCyclesPeekParam`,
  `archivesCyclesSearchClear`, `archivesCyclesSearchEscape`, `archivesCyclesSearchExpanded`,
  `archivesCyclesSearchFill`, `archivesCyclesSearchFocused`, `archivesCyclesSearchOpen`,
  `archivesCyclesSearchText`, `archivesCyclesSkeletonShownOnSlowFetchViaLive`,
  `archivesCyclesSkeletonVisible`, `archivesCyclesVisibleNames`, `archivesOpenCyclePeek`,
  `archivesOpenCyclesTab`, `archivesOpenLiveCycles`

Two methods in this family are declared and unused by the archives specs —
matching §17's list — and are kept: `archivesCyclePeekName` (peek-name
helper for 702's shared peek checks) and `archivesOpenLiveCycles` (live-cycle
navigation helper for the Cycles area). They are not "reserved for the
Cycles epic" as a second driver set; they live here, one implementation.

**Modules — NEWFRONT-702.** Modules list, sort, search, filters, peek,
live module menus, archive/restore dialog and lead filtering:

  `archivesArchivedModuleMenuEntries`, `archivesAwaitModuleRow`,
  `archivesChooseArchivedModuleMenuEntry`, `archivesChooseLiveModuleMenuEntry`,
  `archivesClearModuleFilters`, `archivesClearModuleSearch`, `archivesCloseModuleFilters`,
  `archivesCloseModulePeek`, `archivesEscapeModuleSearch`, `archivesFailNextModuleWrite`,
  `archivesLiveModuleMenuEntries`, `archivesModuleChips`, `archivesModuleFilterGroups`,
  `archivesModuleFiltersActive`, `archivesModulePeekName`, `archivesModulePeekReadOnly`,
  `archivesModuleRowNames`, `archivesModuleSearchText`, `archivesModuleSearchVisible`,
  `archivesModuleSortLabel`, `archivesModulesEmptyKind`, `archivesModulesShowsSkeleton`,
  `archivesOpenLiveModulePeek`, `archivesOpenLiveModules`, `archivesOpenModuleFilters`,
  `archivesOpenModulePeek`, `archivesOpenModuleSearch`, `archivesOpenModulesTab`,
  `archivesRemoveModuleChip`, `archivesSetModuleSort`, `archivesToggleLeadFilter`,
  `archivesTypeModuleSearch`

**Cross-cutting — NEWFRONT-703.** Project dialog confirms and write
shaping, and the absence probes:

  `archivesDragReorders`, `archivesExportControlVisible`, `archivesPressKey`,
  `archivesProjectConfirmBusy`, `archivesProjectConfirmLabel`, `archivesProjectDelayNextWrite`,
  `archivesProjectFailNextWrite`, `archivesRowDraggable`

---

## 17. Driver surface inventory

Measured on head `cf72f6a06` by parsing
`apps/web_new/e2e/parity/drivers/parity-driver.ts` and the five archives
specs:

- **157** `archives*` methods are declared in the interface;
- **152** are exercised by at least one archives spec;
- **5** are declared but unused, and this is the authoritative list:
  `archivesCyclePeekName`, `archivesGoToProjectArchives`,
  `archivesOpenLiveCycles`, `archivesPageTextPresent`,
  `archivesSelectArchivesTab`. Where §16 mentions unused methods it names a
  subset of these five; there is no other unused list.

This corrects the issue text's "157 today": it is 157 declared / 152
exercised. All 157 declarations are `todo()` in `drivers/web-new.ts` (every
one calls `todo(...)` directly from its body), so no archived driver
behavior exists yet and every family above is built by its owner.

Per spec file, the exercised method counts are: `work-items-list` 24,
`work-items-mutations` 16, `cycles` 39, `modules` 43,
`project-crosscutting` 31 — their union is 152, because shell and peek
methods are shared across files. The families in §16 partition the union
plus the interface-only methods; a slice implements its family once and the
other specs reuse it.

The specs also call generic, non-archives drivers read-only — the list
layout, display, rich-filter, row-menu and toast drivers
(`layoutsRowMenuItems`, `layoutsRowMenuChoose`, `layoutsRowMenuOpenNewTabUrl`,
`layoutsDeleteModalVisible`, `layoutsDeleteModalConfirm`,
`layoutsDetailMenuItems`, `subscribeToggle`, `openDisplayOptions`,
`setDisplayGroupBy`, `setDisplayOrderBy`, `toggleDisplayProperty`,
`isDisplayOptionChecked`, `isRichFilterRowVisible`, `richFilterRowText`,
`richConditionCount`, `propertyPickerDisabled`, `pickerOpen`,
`openArchiveProjectDialog`, `archiveDialogBodyText`, `confirmArchive`,
`confirmRestore`, `openArchivedProjects`, `awaitProjectCard`,
`clickCardRestore`, `rulesLastToast`, `isToastVisible`, `sawToast`,
`readClipboard`, `currentUrlPath`, `reloadPage`, `goToPath`,
`openAuthenticated`, `rulesEnsureSignedIn`) and the API/seed helpers
(`archivesArchivedIssues`, `archivesArchivedCycles`, `archivesArchivedModules`,
`archivesArchiveIssue`, `archivesArchiveCycle`, `archivesArchiveModule`,
`archivesProjectArchivedAt`, `archivesCycleArchivedAt`,
`archivesModuleArchivedAt`, `archivesPatchModule`,
`archivesArchivedListStatus`, `archivesArchivedIssueStatus`). Those belong
to the Issues/shell driver families or to `helpers/api.ts`; the archives
slices consume them and do not fork them. `layoutsSeedArchivedLocalFilter`
is the Issues-owned variant for the ISS-071 scenario; the archives spec
uses `archivesSeedStoredExpression`, which 699 owns.

## 18. Contracts gap (feeds NEWFRONT-697)

Existing coverage in `packages/api-client/src/contracts` (verify, do not
redo):

| Endpoint | Contract | State |
|---|---|---|
| `POST/DELETE BASE/issues/{id}/archive/` | `issue-lifecycle.ts` (`ArchiveStamp`, `archiveIssue`, `restoreIssue`) | covered |
| `DELETE BASE/issues/{id}/` | `issue-actions.ts` | covered |
| `POST/DELETE BASE/archive/`, `PATCH BASE/ {archive_in}` | `project-management.ts` (`archiveProject`, `unarchiveProject`, `ProjectPatch.archive_in`) | covered |
| `GET BASE/archived-issues/` | `issues.ts` `listArchivedIssues` | covered **without query params** |

Gaps to contract in NEWFRONT-697:

| Endpoint | Needed shape |
|---|---|
| `GET BASE/archived-issues/` | extend `listArchivedIssues` with the list query the screen sends: `per_page`, `cursor`, `order_by` (`sort_order`, `-created_at`, `-updated_at`, `start_date`, `-priority`), `group_by`/`sub_group_by` (when grouping), and `filters` (the serialized expression parameter the `ComplexFilterBackend` reads; the reader posts it as a JSON string). Same `IssueListResponse` envelope; `total_count`/`count` feed the header badge |
| `GET BASE/issues/{issueId}/archive/` | archived single-item read for the detail and peek body, serialized with the detail shape (`id`, `sequence_id`, `name`, `description`, `state`, `archived_at`, and the properties the detail renders). **This is the read the app calls** — the live detail route also works for archived ids, but the archived tab/detail must not depend on live rows, so contract this one; do not add a second live-detail path |
| `GET BASE/archived-cycles/` | flat array with `id`, `workspace_id`, `project_id`, `name`, `description`, `start_date`, `end_date`, `owned_by_id`, `status`, `archived_at`, `is_favorite`, `total_issues`, `completed_issues`, `started_issues`, `unstarted_issues`, `cancelled_issues`, `backlog_issues`, `assignee_ids` (the backend's selected columns) |
| `GET BASE/archived-cycles/{cycleId}/` | archived cycle detail for the peek body: the same fields plus the progress/estimate summaries the peek renders |
| `GET BASE/archived-modules/` | flat array with `id`, `workspace_id`, `project_id`, `name`, `description`, `description_text`, `description_html`, `start_date`, `target_date`, `status`, `lead_id`, `member_ids`, `total_issues`, `completed_issues`, `started_issues`, `unstarted_issues`, `cancelled_issues`, `backlog_issues`, `created_at`, `updated_at`, `archived_at` |
| `GET BASE/archived-modules/{moduleId}/` | archived module detail for the peek body: the same fields plus the sub-issue/label/estimate summaries the peek renders |
| `POST/DELETE BASE/cycles/{cycleId}/archive/` | archive `200 {archived_at}`, restore `204` — extend `contracts/cycles.ts` (`CycleArchiveStamp`), reusing the existing `CycleListItem`-style row for lists where compatible |
| `POST/DELETE BASE/modules/{moduleId}/archive/` | archive `200 {archived_at}`, restore `204` — extend `contracts/modules.ts` (`ModuleArchiveStamp`) |
| filter metadata | **none added**: the archived list reuses the project-context datasets (§2.2); NEWFRONT-697 does not contract an archives-only metadata read unless a slice proves one is needed, in which case it names the endpoint here first |

Do not contract the `archived-cycles|modules/{id}/unarchive/` alias: it
404s on `/api` and is not the app's path.

---

## 19. Pinned bugs → intended behavior

The oracle specs are ground truth for observable behavior. Where a
scenario pins an old bug, the slice builds the intended behavior below and
the gate aligns that scenario; no scenario is weakened or skipped.

| Issue | Pinned old behavior (oracle) | Intended (build this) | Rows |
|---|---|---|---|
| NEWFRONT-165, NEWFRONT-234 | Archived work-item row menu has no Delete | Four entries: Restore, Open in new tab, Copy link, Delete (editor); Delete confirms then deletes | 004, 009 |
| NEWFRONT-228 | Direct load of archived modules tab never renders | Cold load settles: skeleton → rows/empty | 020, 022, 021 |
| NEWFRONT-231 | Fresh archived-cycles load sticks on skeleton; open peek lost on reload | Cold load settles; `peekCycle` survives reload | 014, 017 |
| NEWFRONT-239 | Cold entry to cycles/modules tabs sticks on skeleton for all roles | Direct address, reload and shared link settle | 028, 029, 030 |
| NEWFRONT-229 | Filters button shows no active indication | Visible marker whenever any filter is applied (cycles + modules) | 016, 021 |
| NEWFRONT-233 | Failed module archive/restore reports success | Error toast, no success, no navigation; state unchanged | 023, 024 |
| NEWFRONT-244 | Failed cycle archive/restore reports success | Error toast, no success, no navigation; state unchanged | 018, 019 |
| NEWFRONT-235, NEWFRONT-243 | Copy/open build `/browse/undefined-N/` | Copy/open build `DETAIL` (`/archives/issues/{id}`) | 010 |
| NEWFRONT-236 | Missing archived item renders blank | Shared not-found with control to live work items | 011 |
| NEWFRONT-242 | Filter entry inert; stored expressions never restore | Filter entry works; seeded expression restores and narrows the first read | 005 |
| NEWFRONT-245 | Archived detail shows no loader while fetching | Detail skeleton/loader until the record lands | 011, 030 |
| NEWFRONT-240 | Guests get 403 on archived issues/cycles list + issue detail; tabs stick on skeletons | Guests read every tab; **requires a PDASHOSS01 backend change** (§11) | 028 |

---

## 20. Edition and platform matrix

oss / cloud / desktop: identical archive screens, entries, endpoints and
rules. The desktop build serves the same web background (no
desktop-overlay archive surface exists), so no row has a desktop-only
variant and ARCH-032 proves there is no edition split. The area gate runs
the suite per edition it can serve; a row tagged "all" must be green in
each.

---

## 21. Row → slice map

| Row | Slice |
|---|---|
| 001, 002, 003 | 698 |
| 005, 006, 007 | 699 |
| 004, 008, 009, 010, 011, 012, 013 | 700 |
| 014, 015, 016, 017, 018, 019 | 701 |
| 020, 021, 022, 023, 024, 025 | 702 |
| 026, 027, 028, 029, 030, 031, 032 | 703 |
| 033 (proposed) | 700 (composed), proven by 703 |

Every row 001–032 appears exactly once; 033 is the proposed addition from
§14. Read-only reuse across slices (699's list for 700's menus, 701's peek
body for 702, 698's shell for everyone) is described per row above and is
not a second ownership.
