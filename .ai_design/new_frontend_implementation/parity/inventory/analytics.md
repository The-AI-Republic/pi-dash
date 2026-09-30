# Inventory: Analytics

- Area: Analytics (Phase 3)
- ID prefix: `ANL-`
- Editions: oss, cloud, desktop
- Status convention: `not started` / `oracle green` / `new green`
- Parity test column stays empty until oracle scenarios exist (NEWFRONT-56).

All behavior below was learned from reading the old frontend sources and is
described in fresh prose. Nothing here is copied from the old codebase.

## Rows

| ID | Capability | Who | Edition | Old entry point | API | Acceptance | Parity test | Status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ANL-001 | Workspace analytics page offers two tabs (a workspace overview and a work-item breakdown); the active tab is part of the page address, switching tabs moves to the new address, and opening the page without a tab (or with an unknown one) lands on the first tab | any workspace member incl. guests | all | workspace analytics route `analytics/[tabId]` | none (client-side tab state synced to the URL) | each tab has a shareable address; switching updates the address; a missing or unknown tab id shows the first tab |  | not started |
| ANL-002 | Analytics pages carry a section breadcrumb in the app header and a browser title that names the current workspace | any workspace member incl. guests | all | analytics route layout + header | none (client-side) | header shows the analytics trail; the document title mentions the workspace name |  | not started |
| ANL-003 | With no projects in the workspace, the analytics page shows a no-project empty view whose project-creation action is limited to admins and members | empty view for all; creation action admin/member only | all | workspace analytics page | project list membership (already loaded) | empty view renders when the workspace has no projects; guests cannot trigger creation from it |  | not started |
| ANL-004 | A project picker in the analytics header scopes every section to a subset of the member's projects; it searches by name and identifier, defaults to everything, and compresses long selections into a count summary | any workspace member incl. guests | all | analytics header filter actions | all analytics reads take an optional project-ids filter | picking projects narrows all sections; clearing restores the full workspace; search matches names and short codes |  | not started |
| ANL-005 | The overview tab opens with a grid of headline totals covering people (overall, per role), projects, work items, cycles, intake, and agent token usage; each card shows a loading placeholder while fetching and falls back to zero when its metric is absent | any workspace member incl. guests | all | analytics overview tab | GET workspace advance-analytics totals for the overview tab | all headline cards render with values; loading placeholders show first; missing metrics read as zero |  | not started |
| ANL-006 | The overview tab shows a per-project radar chart beside a written summary that lists each project dimension with its work-item count; a loading skeleton covers the fetch and a guidance empty view appears when there is nothing to plot | any workspace member incl. guests | all | analytics overview tab, project-insights section | GET advance-analytics charts for the projects graph | chart and summary list agree on dimensions and counts; skeleton shows during load; the empty view renders on empty data |  | not started |
| ANL-007 | The overview tab lists active projects with a per-project completion badge whose tone flips at the halfway mark; long names are shortened with their full form on hover, and entries that no longer resolve are skipped | any workspace member incl. guests | all | analytics overview tab, active-projects section | GET project stats (total + completed work-item fields) | every resolvable project shows its badge with the right tone; loading rows show first; stale entries render nothing |  | not started |
| ANL-008 | The work-items tab opens with headline totals split by workflow progress (overall, started, backlog, unstarted, completed); wording adapts when viewed in epic scope | any workspace member incl. guests | all | analytics work-items tab; scoped modal (epic wording) | GET workspace advance-analytics totals for the work-items tab | all five cards render with values matching the current scope; epic scope rewords the labels |  | not started |
| ANL-009 | The work-items tab plots created versus resolved items over time as two smoothed area series with calendar-formatted axis labels; a loader covers the fetch and a guidance empty view appears when there is no series data | any workspace member incl. guests | all | analytics work-items tab, created-vs-resolved section | GET advance-analytics charts for the work-items graph | both series render with readable date labels and a legend; loader shows first; empty data shows the guidance view |  | not started |
| ANL-010 | The custom-insights section lets the viewer choose what runs along each axis and an optional second split; the two property pickers never offer the same dimension twice, the metric picker hides the estimate option unless points-based estimation is active and swaps the epic/item metric by scope | any workspace member incl. guests | all | analytics work-items tab, custom-insights selectors | GET advance-analytics charts for the custom graph (axis params) | changing any picker re-queries and re-renders; mutually chosen dimensions stay exclusive; unavailable metrics are not offered |  | not started |
| ANL-011 | The custom bar chart stacks segments when a second split is chosen and stays single-series otherwise; priority splits use fixed per-priority hues, state splits reuse workspace state colors, date splits group by day/week/month/year with year-aware labels, and extra segments extend the theme palette; bar ends round only on true segment edges | any workspace member incl. guests | all | analytics work-items tab, custom-insights chart | same custom-graph read as ANL-010 | grouped data stacks with one legend entry per segment; priority/state/date splits color as described; single-series mode shows one set of bars |  | not started |
| ANL-012 | A data table under the custom chart repeats the plotted values with a live search box and a comma-separated-file export action; the export carries the visible columns and row values | any workspace member incl. guests | all | analytics work-items tab, custom-insights table | same custom-graph read as ANL-010 (client-side table/export) | table rows match the chart; typing filters the table; exporting downloads a file with the table's columns and values |  | not started |
| ANL-013 | A state-breakdown table lists per-project counts across the workflow stages (project scope) with a live search box, a row-count label, skeleton rows while loading, a guidance empty view on no data, and a comma-separated-file export of the filtered rows | any workspace member incl. guests | all | analytics work-items tab, insights table | GET advance-analytics stats for the work-items tab | counts match the server per project and stage; search narrows rows; export contains exactly the filtered rows |  | not started |
| ANL-014 | Work-item analytics can be opened as a side dialog scoped to a project, cycle, or module from issue, cycle, and module surfaces; the dialog titles the scope, offers a wide-screen toggle on larger screens, shows a brief spinner while the scope applies, and clears the scope on close so the workspace view is unaffected | any project member incl. guests | all | issue list filter bar; cycle detail header; module detail header (incl. small-screen headers) | project-scoped advance-analytics reads (totals, charts, stats) | opening from each surface shows only that scope's data; toggling wide mode resizes; closing restores the unscoped store |  | not started |
| ANL-015 | Inside the scoped dialog the breakdown table swaps its first column from project to assignee, showing avatars (or an initial, or an unassigned marker when there is no one), and epic scope rewords the table headings | any project member incl. guests | all | scoped analytics dialog, insights table | project-scoped advance-analytics stats read | assignee rows render with avatar or fallback marker; unassigned rows are labeled; epic scope rewords headings |  | not started |
| ANL-016 | Every analytics export downloads a comma-separated file named for the workspace, maps each column to its display label, and writes a dash for values that cannot be read | any workspace member incl. guests | all | insights table export action; custom-chart table export action | none (client-side file generation) | the downloaded file name carries the workspace slug; headers are human labels; unreadable cells read as a dash |  | not started |
| ANL-017 | Analytics tables pair a row-count label with an expanding search box that filters the first column; closing the box clears the filter | any workspace member incl. guests | all | insights table; custom-chart table | none (client-side) | the label counts the current rows; opening, typing, and closing the box filters then resets the first column |  | not started |
| ANL-018 | Analytics are readable by every role including guests; the only gated action in the area is project creation from the no-project empty view | guests/viewers (negative) | all | all analytics surfaces | none (client-side gating; server enforces) | guests can browse, filter, and export but are offered no working mutation affordance |  | not started |

## Explicitly out of scope for this area (owned elsewhere)

- Opening the scoped analytics dialog (header buttons on issue, cycle, and module surfaces): Issues / Cycles / Modules areas; only the dialog content itself is rowed here (ANL-014, ANL-015).
- Epic-mode label swaps driven by the issues store scope: Issues area (epic views); the analytics-side wording change is ANL-008/ANL-015.
- No keyboard shortcuts, drag-and-drop, imports, real-time updates, edition-gated tabs, or desktop-only analytics behavior were found in the scoped sources.

## Coverage checklist

Route files (each mapped to covering rows):

- `analytics/[tabId]/page.tsx` (tab state synced to the address, no-project empty view with gated creation, read access for all roles) -> ANL-001, ANL-003, ANL-018
- `analytics/[tabId]/layout.tsx` (header + content shell) -> ANL-002
- `analytics/[tabId]/header.tsx` (section breadcrumb) -> ANL-002

Top-level component folders (each mapped to covering rows):

- `core/components/analytics/` root files (`analytics-wrapper`, `analytics-section-wrapper`, `analytics-filter-actions`, `total-insights`, `insight-card`, `loaders`, `export`) -> ANL-004, ANL-005, ANL-008, ANL-013, ANL-016, ANL-017
- `core/components/analytics/overview/` (overview mount, project radar + summary, active-projects list + item) -> ANL-005, ANL-006, ANL-007
- `core/components/analytics/select/` (`project`, `analytics-params`, `select-x-axis`, `select-y-axis`) -> ANL-004, ANL-010
- `core/components/analytics/select/duration.tsx`: date-range picker exists but is not mounted anywhere (the header slot that would hold it is commented out) — dead code, no row.
- `core/components/analytics/insight-table/` (table shell, searchable data table, skeleton loader) -> ANL-012, ANL-013, ANL-017
- `core/components/analytics/work-items/` root files (`root`, `created-vs-resolved`, `customized-insights`, `priority-chart`, `workitems-insight-table`, `utils`) -> ANL-008, ANL-009, ANL-010, ANL-011, ANL-012, ANL-013, ANL-015
- `core/components/analytics/work-items/modal/` (dialog shell, scoped content, header with wide-screen toggle) -> ANL-014, ANL-015
- `core/components/chart/utils.ts` (date grouping, label cleanup, palette extension) -> ANL-011
- `core/components/analytics/trend-piece.tsx`: trend badge widget exists but is not rendered by any analytics surface (its only call site is commented out) — dead code, no row.
- `core/components/analytics/empty-state.tsx` (themed generic empty panel): not referenced by any analytics surface in the scoped sources — dead code, no row.
- `ce/components/analytics/` (tab definitions + tab hook: overview and work-items tabs, both enabled) -> ANL-001

API endpoints (each mapped to covering rows):

- `GET /api/workspaces/{slug}/advance-analytics` (headline totals per tab) -> ANL-005, ANL-008
- `GET /api/workspaces/{slug}/advance-analytics-charts` (projects graph, work-items graph, custom graph) -> ANL-006, ANL-009, ANL-010, ANL-011
- `GET /api/workspaces/{slug}/advance-analytics-stats` (state-breakdown table) -> ANL-013
- `GET /api/workspaces/{slug}/projects/{id}/advance-analytics[-charts|-stats]` (peek/scoped variants used by the dialog) -> ANL-014, ANL-015
- `GET /api/workspaces/{slug}/project-stats/` (per-project totals for the active-projects list) -> ANL-007
- Note: the analytics store holds a date-range value and the service call sites carry a commented-out date-filter parameter, so no time-window filtering reaches the server today — no row; revisit if the picker in `select/duration.tsx` is ever mounted.
