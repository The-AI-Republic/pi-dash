# Analytics area spec — workspace screen and scoped dialog (NEWFRONT-727)

Build spec for inventory rows ANL-001–ANL-018. This document is written from
observed behavior and backend contracts, not from old code: the legacy analytics
route, components, hooks, store and service were read as reference only, as were
the six oracle scenarios under `apps/web_new/e2e/parity/analytics/` and the
existing local-file export oracle. Nothing below is copied or translated; the
new implementation is written from this prose.

This spec owns no inventory Status changes. Slices append `built` to the rows
they implement; the area gate applies `new green`.

## 1. Scope, route and shell (ANL-001, ANL-002)

The analytics screen lives at `/{workspaceSlug}/analytics/{tabId}`. It is a
workspace-level, gated route rendered inside the normal workspace shell. The
two supported tab ids are exactly `overview` and `work-items`, in that order.
`overview` is the default.

The bare address `/{workspaceSlug}/analytics` permanently redirects on the
client to `/{workspaceSlug}/analytics/overview`. Opening any other tab id is
allowed: the tab strip still renders both known tabs, the id stays verbatim in
the address, no tab is marked selected, and no tab panel renders. Clicking a
known tab then replaces the unknown segment with that tab id. Use a path
segment for the tab, not a search parameter, because the old addresses and the
oracle's expected paths use path segments; this also keeps every tab directly
shareable.

The page header carries one breadcrumb item, `Analytics`; it has no project
crumb and no trailing tab name. The document title is
`{workspace name} - Analytics` on both tabs (hyphen with surrounding spaces,
not the newer shell's middle-dot convention). The title follows a workspace
rename while the page is mounted. Guests receive the same shell, title and
breadcrumb as members.

While the project membership read is still resolving, render the analytics
shell's pending state rather than flashing the no-project view. The no-project
decision is based on the caller's joined, active, non-archived projects.

The tab strip and the project picker share one toolbar row above the panel.
The strip scrolls horizontally on narrow widths; the picker remains at the
trailing edge. There is no duration control and no additional toolbar action
(see §2).

## 2. No time-window filter

The legacy analytics store retained a duration value (defaulting to the last
30 days), its label was passed into several section subtitles, and several
call sites kept the date parameter commented out. No date or duration value is
sent to any analytics endpoint. The backend accepts `date_filter` on the
advance endpoints, but the shipped surfaces never supply it.

The new build must not invent a time-window picker or send `date_filter`.
Sections that show a duration subtitle in the oracle may omit that subtitle;
the numeric and plotted data remain all-time for the chosen project scope. If a
future issue adds a real time window, it must add a row and define the filter
across every read rather than changing only one section.

## 3. No-project empty view and creation gate (ANL-003, ANL-018)

When the caller's project list resolves to zero entries, replace the tabs,
picker and panels with a detailed empty state. It carries the heading
`No project`, a sentence explaining that work items need a project, and one
primary action labeled `Create a project`.

The action is enabled for workspace admins and members and disabled for
guests. It opens the existing project-creation dialog owned by the projects
feature; analytics does not implement a second creation form. Successful
creation closes the dialog and invalidates the project list so the analytics
page leaves the empty state and renders the normal tabs. Cancelling leaves the
empty view unchanged.

Guests can open either tab and use the picker, and the guest shell still
renders the tab strips, headings and tables. They are not offered project
creation, and the server refuses a guest project-create request. Other
workspace analytics endpoints also return 403 to guests; the UI falls back as
described in §4, §6, §7 and §9. Because the workspace chart and stats reads are
refused, a guest's analytics tables have no rows: export and search are not
exercised for guests, and the oracle only pins tabs, breadcrumb, title and
picker for them (ANL-018). The only analytics-area mutation affordance is
project creation from this empty view.

## 4. Project picker and scoping (ANL-004, ANL-014)

The toolbar picker is a multi-select over the caller's joined workspace
projects. It defaults to an empty selection, which means every joined project.
The trigger label rules are fixed, not a free count summary:

- zero selected: `All projects`;
- one selected: that project's name;
- two or three selected: the selected names joined with a comma and space;
- four or more selected: the literal `3+ projects`.

ANL-004 is settled here: implement the literal `3+ projects`. This preserves
the oracle-pinned behavior and is the intended behavior for the new build. Do
not replace it with a true count. Untoggling selections returns the label to
the appropriate name list, and clearing all selections returns `All projects`.

The picker searches against a combined string of project name and project
identifier, so either a full name fragment or the short code matches. It
supports keyboard navigation through the kit combobox, shows the project mark
and name per option, and keeps the popover open across multiple toggles so a
multi-selection does not require reopening.

Selection scopes every section on both tabs: headline totals, the overview
graph, the active-projects list, created-vs-resolved, custom insights, the
chart table, and the state-breakdown table. The selection is
serialized as a comma-separated `project_ids` query parameter; omit the
parameter entirely for the all-projects state. Selection survives tab
switching and scoping the dialog does not clear it.

The legacy app kept the selection in an in-memory store; it was not part of
the address and did not survive a reload. The oracle proves only that it
survives a tab switch (twice, back and forth). The new build deliberately
upgrades this to a route search parameter (`project_ids`) so a reload and a
shared link keep the scope; that is a minor additive divergence, not a parity
requirement, and no oracle scenario asserts it. Accept the parameter on both
tab addresses, validate it as a list of project ids, ignore ids the caller
cannot resolve, and normalize by dropping duplicates while preserving order
before it enters a request or query key. `project_ids` never appears inside the
path, so it does not change the tab segment rules in §1.

The picker only offers projects the caller has joined. When a selection
contains a project the caller can no longer resolve, drop it from the picker
label and from subsequent requests rather than displaying a blank fragment.

Opening the scoped dialog does not change the workspace page's picker
selection; the dialog has its own scope (§10). Closing the dialog restores the
workspace view exactly as it was before opening, including its picker
selection.

## 5. Overview headline totals (ANL-005)

The overview tab opens with an 11-card grid in this order:

1. `Total Users` — `total_users`
2. `Total Admins` — `total_admins`
3. `Total Members` — `total_members`
4. `Total Guests` — `total_guests`
5. `Total Projects` — `total_projects`
6. `Total Work items` — `total_work_items`
7. `Total Cycles` — `total_cycles`
8. `Total Intake` — `total_intake`
9. `Agent input tokens` — `agent_run_input_tokens`
10. `Agent output tokens` — `agent_run_output_tokens`
11. `Agent total tokens` — `agent_run_total_tokens`

Every card is driven by one totals read and has an independent placeholder in
the same cell while that read is pending. A missing field or a field without a
numeric `count` renders `0`; a failed read also settles the whole grid to zero
values without an error banner. The label remains visible during loading.

The oracle currently pins raw translation keys for the first eight labels
(`Total common.users`, `Total common.admins`, and so on), filed as
`bug: NEWFRONT-468`. Intended behavior, and what the new build implements, is
the eight human labels above. The three agent-token labels already match the
oracle. This is a declared corrected divergence, not a new copy of the bug.

At the bottom of the overview tab, render the two overview sections described
in §6 and §7.

## 6. Project radar and written summary (ANL-006)

The project-insights section pairs a radar chart with a written summary. The
graph read returns seven rows in this server order: `work_items`, `cycles`,
`modules`, `intake`, `members`, `pages`, `views`. Display names are title-cased
forms (`Work Items`, `Cycles`, `Modules`, `Intake`, `Members`, `Pages`,
`Views`), and each row carries `key`, `name` and `count`.

The radar plots one series named `Count` by dimension, using the accent color
token for stroke and fill with partial fill opacity. The written summary has a
heading for the project summary, a row identifying the current scope, and a
list that repeats the seven dimensions with their counts. The chart and list
must use the same server payload and preserve its order; the oracle asserts
that their values agree. Hovering a radar point exposes its dimension and
count.

While the read is pending, show a section-shaped skeleton covering both the
plot and its list. When the read resolves successfully with zero rows, show a
compact guidance panel: projects, work items and assignees are needed before
this section can show performance. When the read fails, keep the section
header but render neither plot, summary rows nor guidance. This is the
oracle's reachable behavior and the intended build behavior.

The old guidance branch is effectively unreachable for a successful member
read because the server always returns all seven dimensions, zeroed when
empty. Members therefore see a zeroed seven-point radar rather than an empty
panel. The new implementation must preserve all zero rows rather than filtering
them.

## 7. Active projects list (ANL-007)

The active-projects section reads per-project stats in one request. It lists
one row per returned project that the caller can resolve in the project list;
unresolvable or stale ids render nothing. Each row shows the project's mark,
its name, and a percentage badge.

For every row, percentage is:

- `0` when `total_issues` is zero or absent;
- otherwise `round(completed_issues / total_issues * 100)`.

The tone is success only when the percentage is strictly greater than 50;
50 and below use danger. The badge text is the integer percentage followed by
`%`. Project names truncate to one line in the row, and the full name is
available in a hover/focus tooltip. The list scrolls independently when it
exceeds its section height.

While the stats read is pending, show five placeholder rows. A successful
empty response shows the section header with an empty scroll area rather than
the analytics guidance panel. A failed request likewise shows no resolved
rows; do not crash or synthesize percentages.

Intentional difference from the observation: the legacy active-projects
request sends only `fields` (with the invalid legacy field names), so its list
never followed the picker. The oracle's picker scenario asserts that every
later `advance-analytics*` capture carries `project_ids`, but it does not
observe `project-stats`; its active-projects scenario runs with no picker
selection, so it only pins that unresolvable ids are skipped and that a guest
with one joined project sees that one row. ANL-004's row requires the picker to
scope every section, so the new build adds `project_ids` to this read (and to
its query key, §16.6). With no selection the list is unchanged, which is all
the oracle exercises. This is a declared additive divergence, not new
behavior attributed to the old app.

The legacy request asked for two invalid field names and consequently received
the backend's full default set. The new build must request the valid fields it
needs — `total_issues,completed_issues` — and read those exact response keys.
This request detail belongs in the contracts slice; the UI must not depend on
the old invalid-field accident.

## 8. Work-items headline totals (ANL-008)

The work-items tab opens with five cards in this order:

1. `Total Work items` — `total_work_items`
2. `Started Work items` — `started_work_items`
3. `Backlog Work items` — `backlog_work_items`
4. `Unstarted Work items` — `un_started_work_items`
5. `Completed Work items` — `completed_work_items`

Each value comes from the work-items totals read and reflects the current
project scope. The five card placeholders render while the read is pending;
a missing metric settles to `0`.

The old component had an epic-scope wording branch. No shipped surface can
reach it: there is no epic-id analytics route, the dialog's only callers pass
project scope, and the issue dialog defaults to work items. The new build
therefore uses the work-items wording everywhere reachable and does not add an
epic heading branch. If an epic producer is introduced later, it must add a row
and a scenario before adding a wording branch.

## 9. Created-vs-resolved chart (ANL-009)

The created-vs-resolved section requests the work-items chart series and
renders a smoothed area chart with two series:

- `Resolved` from `completed_issues`, green;
- `Created` from `created_issues`, blue.

The legend reads exactly `Resolved`, then `Created`, left-aligned under the
plot. Series do not show point dots and use gentle smoothing (do not join
points with straight segments). The x-axis uses the row `name` (a `YYYY-MM-DD`
bucket key), formatted as a calendar date `MMM DD, YYYY` (for example
`Oct 01, 2026`); the y-axis is the number of work items. The chart carries a
bottom legend and a hover tooltip.

The workspace read returns monthly buckets from the workspace's creation month
through the current month, including zero-count months. A successful member
read therefore always has at least one point even in an otherwise empty
workspace. A failed read shows a compact guidance panel and no plot, legend or
axis labels; the visible series arrays must be empty rather than fabricated
zeros.

While loading, show a chart-height skeleton. For the project-scoped dialog,
cycle and module scopes produce daily buckets over the cycle or module range;
if the scope has no usable start date, the server returns an empty series and
the same guidance state applies.

## 10. Custom insights — pickers and chart (ANL-010, ANL-011)

The custom-insights section sits below created-vs-resolved. It has three
controls: a metric picker, an x-axis picker, and a split picker. Opening
labels are exactly `Work item`, `Priority`, and `Add Property`.

ANL-010 is settled here. The metric picker offers only `Work item`. The
estimate metric remains absent even when the project has points estimation
enabled, and the epic metric never appears because no epic-scope producer
exists. This is intended: analytics has one supported count metric in the
reachable product surfaces. Do not add an estimate option or an epic/item swap
as part of these slices; changing that is a new product decision and requires a
row.

The x-axis picker offers every x dimension in this fixed order:

1. `State name`
2. `State group`
3. `Priority`
4. `Label`
5. `Assignee`
6. `Estimate point`
7. `Cycle`
8. `Module`
9. `Completed date`
10. `Due date`
11. `Start date`
12. `Created date`

The split picker prepends `No value`, then offers the same list with the
currently selected x dimension removed. It never offers the chosen x
dimension, and the x picker never offers the current split. Choosing `No value`
clears the split, re-queries without `group_by`, and restores the full x list.
Choosing a split narrows the x list immediately. Moving x swaps which
dimension is excluded.

Each picker change issues a fresh custom chart read with
`type=custom-work-items` and the chosen `x_axis`, `y_axis` and optional
`group_by`; values are the uppercase enum names, for example `PRIORITY`,
`STATES`, `ESTIMATE_POINTS`. The picker labels are display labels, not request
values. Picker state is local to the analytics panel or dialog and does not
need to survive a tab switch.

Rendering rules:

- With no split, render one series named `Count`, one bar per x value.
- With a split, stack one segment per `schema` key, preserving schema order
  from bottom to top. The server orders grouped data by descending total count;
  do not re-sort the schema alphabetically.
- The custom chart has no visible legend. The hover tooltip names every
  segment and shows its value.
- For a priority x-axis, use fixed hues: `Urgent` red `#ef4444`, `High`
  orange `#f97316`, `Medium` yellow `#eab308`, `Low` green `#22c55e`, and
  `None` neutral gray `#ced4da`. Match case-insensitively against server keys.
- For a state-name x-axis, use the workspace state's configured color. An
  unresolvable state id falls back deterministically into the theme palette.
- For date x-axes, convert the server value to a display label. A missing,
  null, or `none` value reads `None`. Current-year dates use `MMM D`; dates
  from another year use `MMM D, YYYY`. Invalid values remain verbatim rather
  than becoming an epoch date.
- Date splits always group by day. No day/week/month/year switcher renders on
  any shipped surface; do not add one.
- Capitalize the first letter of `Priority` and `State group` labels for
  display. For an assigned split, capitalize split labels under the same rule.
  Do not mutate the underlying keys used to look up segment values.
- In single-series mode round both ends of every bar. In stacked mode round
  only the true extremes: the lowest nonzero segment gets a rounded bottom,
  the highest nonzero segment gets a rounded top, and interior segments have
  square ends. Zero-value segments have no visible end.
- Fill stacked segments from the theme chart palette in schema order. There is
  no categorical chart palette in `@pidash/kit` today (D12 defines only the
  neutral UI tokens), so slice B owns adding one to the kit as a ten-slot
  light/dark token pair. The first ten light slots must be, in order,
  `#6172e8`, `#8b6edb`, `#e05f99`, `#29a383`, `#cb8a37`, `#3aa7c1`, `#f1b24a`,
  `#e84855`, `#50c799`, `#b35f9e`; the oracle pins exactly these. Extend
  deterministically beyond ten with generated hues rather than cycling and
  duplicating colors. Chart components read the tokens; analytics must not
  carry a second hard-coded palette.

If the custom read fails or returns no rows, show the same compact guidance
panel used by the other chart sections and no table. A successful response
with rows always renders both chart and table.

## 11. Chart table (ANL-012)

Under the custom chart, render a table that repeats the plotted rows. Its first
column header is the current x-axis display label (for example `Priority`),
and the second is `Count`. When a split is active, add one right-aligned
numeric column per schema key, using the schema display label as the header.

Rows preserve the server's chart order. The first-column values are the same
display values used on the chart, including capitalization and date
formatting. Counts equal the row's total `count`; split columns equal their
segment values.

The row-count label reads the loaded row count followed by the current x-axis
display label: `3 Priority`, `2 Priority`, and so on. The legacy label uses the
loaded dataset length and a fixed noun (it does not inflect for one row and
does not follow the search filter); the new build preserves that, while the
export still writes only the currently filtered rows (§15).

The table has an expanding search control in its toolbar and an `Export as csv`
action. Search filters only the first column, case-insensitively, and operates
on the display text. Closing the search clears the query, restores all rows,
and collapses the field. The table scrolls horizontally when split columns
exceed its width, and shows no pagination controls.

While the custom data read is pending, show a table-height skeleton and no
export button. A failed or empty read shows the section guidance rather than
an empty table with headers.

## 12. State-breakdown table (ANL-013)

Below the custom-insights section, render the state-breakdown table. In
workspace scope its first column is `Project`, followed by `Backlog`,
`Started`, `Unstarted`, `Completed`, `Cancelled`. Each row is one project with
the five group counts from the stats response.

The first cell shows the project mark and the name of the project the id
resolves to. If the id no longer resolves, the live cell renders a mark and no
name (the legacy cell reads only the resolved project); it does not fall back
to the server's `project__name`. The export path does fall back to the server
name, because the legacy export value reads `project__name` directly. The new
build mirrors both: resolved id → mark plus resolved name; unresolved id →
mark with no visible name on screen, server name in the CSV. The count columns
are right-aligned numbers.

The row-count label reads `2 Projects` or `0 Projects`, using the loaded
dataset length and the fixed noun `Projects` (the legacy label does not inflect
for a single row and does not change while search narrows the rows). The search
control expands in place, filters the first column by project name, and clears
when closed. The export action exports the currently filtered rows, not the
full dataset.

While loading, render five skeleton rows with the six real headers. A
successful empty response or a failed read renders the guidance panel plus a
zero-count label and the real headers; it must not invent rows. For a guest,
the server read returns 403 and this is the reachable UI. Rows are initially
shown in server order, which is project id order; do not sort them in the
client.

## 13. Scoped analytics dialog — shell and lifecycle (ANL-014)

The dialog is a right-hand analytics panel scoped to one project, optionally
narrowed further to one cycle or module. It is opened from surfaces owned by
other areas: the issues header, a cycle detail header, and a module detail
header. Analytics owns the dialog content, not those triggers.

Title rules:

- project scope: `Analytics for {project name}`
- cycle scope: `Analytics for {project name} in {cycle name}`
- module scope: `Analytics for {project name} in {module name}`

The oracle currently captures a double space in the module title because the
old title template concatenated an empty cycle fragment. Intended behavior is
one space; the new build implements the single-space title. This is a declared
corrected divergence.

The panel starts at a three-quarter-width desktop size and can toggle to full
width. The wide toggle is visible at medium widths and above, hidden on narrow
screens; toggling changes the panel width without losing scroll position or
re-querying. The close affordance is always present. Opening focuses the panel
and traps focus; Escape and the close control both close it.

A brief centered spinner covers the panel while the scope is applied and the
scoped reads begin. Once configured, render, in order: five work-items
headline cards, created-vs-resolved, custom insights with its chart table, and
the assignee state-breakdown table. The work-items cards keep the same labels
and values as the tab, scoped to the dialog payload.

Requests for the dialog use the project-scoped endpoint family:
`/api/workspaces/{ws}/projects/{projectId}/advance-analytics/`,
`.../advance-analytics-charts/`, and `.../advance-analytics-stats/`. Always
send `project_ids={projectId}`; add `cycle_id={cycleId}` or
`module_id={moduleId}` when opened from those surfaces. The project path id
and `project_ids` must agree. Never read the bare workspace endpoint for
dialog content.

On close, discard the dialog's project/cycle/module scope and its custom
picker state. The workspace page's picker selection must remain whatever it
was before the dialog opened. Reopening starts fresh with the newly supplied
scope.

By the time this slice shipped, both narrowing triggers existed in the new
build: the cycle header (CYC-033, NEWFRONT-712) and the module detail page
(MOD-032, NEWFRONT-802), which mounts the same entry with the module's id and
name. The dialog content's three legs — project, cycle, module — are therefore
reachable and are proven here; nothing is deferred to a later trigger slice.

Guest visibility of the dialog is governed by each trigger's owner. On desktop
issues, cycles and modules surfaces the old trigger is gated on
issue-creation rights, so guests see no trigger. Analytics does not add a
guest trigger. Scoped totals and stats reject guests (admin/member only),
while the scoped chart endpoint accepts guests at the API; the new build keeps
the trigger absent, so analytics adds no guest entry point (see §16.5).

## 14. Scoped dialog — assignee table (ANL-015)

In dialog scope, the state-breakdown table's first column becomes `Assignee`;
the remaining headers remain `Backlog`, `Started`, `Unstarted`, `Completed`,
`Cancelled`. Rows are per assignee, plus one unassigned row when the scoped
project has work items without an assignee. Do not invent rows for assignees
with no scoped work.

Each row shows the assignee's avatar when `avatar_url` resolves. Without an
image, show initials derived from `display_name` in a circular avatar. An
unassigned row has a null `display_name` and renders the text `Unassigned`
with a neutral user marker rather than an avatar or initial. The server orders
rows by `display_name`; preserve that order and place the null-name row where
the server puts it.

The dialog table has the same `Export as csv` action, expanding first-column
search, and row-count label as the tab table; the label's fixed noun is
`Assignee` (the legacy label does not inflect, so a row count of three reads
`3 Assignee`). While loading, show skeleton rows; on failure or emptiness,
show the guidance panel with a zero-count label.

Epic scope is unreachable from shipped surfaces and the scoped table has no
epic heading variant. The new build uses the work-items/assignee wording above
and does not add an epic reword. A future epic producer must add a row and
scenario first.

## 15. CSV exports (ANL-016, ANL-017)

All four analytics exports — workspace chart table, workspace state-breakdown
table, dialog chart table, dialog assignee table — use the same builder and
the file name `{workspaceSlug}-analytics.csv`.

Ownership of that builder (corrected at the exports split's review,
2026-10-20): the one module is `apps/web_new/src/shared/export/csv.ts`.
NEWFRONT-735 owns its table chrome and the first cut of the serializer,
including the dash fallback exercised by ANL-016. NEWFRONT-769 is blocked by
735, adopts that file, and owns the export rules exercised through the two
oracle tables (insight and chart drill-down, where
`analyticsExportButtonCount() === 2`). Neither creates a second module. A rule
735 shipped differently is changed by 769 in that shared file in one change.
The exports area spec (`exports.md` §0.2) records the same boundary.

The file is UTF-8 with a byte-order mark, CRLF record separators, comma field
separators, and a header row. Quote text fields when they contain a comma,
quote, CRLF or leading/trailing whitespace; escape embedded quotes by doubling.
Write numeric values bare. `Export as csv` exports the current filtered and
display-ordered rows: apply the same first-column search filter as the table
and do not include hidden or unrendered rows.

Headers use display labels, not API keys or schema keys. The four header sets
are:

- chart, ungrouped: current x-axis label, then `Count`;
- chart, grouped: current x-axis label, then `Count`, then one column per
  split label in schema order;
- workspace breakdown: `Project`, `Backlog`, `Started`, `Unstarted`,
  `Completed`, `Cancelled`;
- dialog breakdown: `Assignee`, `Backlog`, `Started`, `Unstarted`,
  `Completed`, `Cancelled`.

Cell values use the same display strings as the table, so date formatting,
capitalization and avatars' text identity agree. A cell writes `-` only when
its column's export value function yields undefined/null (or a value that
cannot be turned into a scalar). The legacy cell functions coerce an absent
source field to the empty string — a null project name exports as an empty
cell, not a dash — so the new build resolves values per column the same way and
does not blanket-convert every null source field to a dash. The live tables
currently resolve every cell, so the dash fallback does not appear in the
oracle files, but it is required behavior and must have unit coverage.

If the filtered row set is empty, produce no download at all; the builder
must reject or return before creating a Blob. Do not emit a headers-only file.
The export control may remain visible, but activating it with zero rows must
not fire a browser download event. No oracle scenario activates an export with
zero visible rows, so this is an intended rule rather than a pinned
observation; slice D owns making the builder reject before it creates a Blob and
unit-testing it.

The table row-count label reports the loaded dataset length, not the filtered
length, and does not change while the search narrows rows. Search targets the
first column only, matches case-insensitively, collapses the input on close,
and clears the filter at the same time. Opening the search does not change the
count, and neither does typing.

## 16. API and data layer

Endpoint shorthand below: `WS` is `/api/workspaces/{workspaceSlug}` and `P` is
`/api/workspaces/{workspaceSlug}/projects/{projectId}`.

### 16.1 Workspace totals

`GET WS/advance-analytics/?tab=overview|work-items[&project_ids=...]`

- `tab` defaults to `overview`; an unsupported tab is a 400.
- `project_ids` is a comma-separated set of project ids; omit it for all joined
  projects.
- Overview returns keys `total_users`, `total_admins`, `total_members`,
  `total_guests`, `total_projects`, `total_work_items`, `total_cycles`,
  `total_intake`, `agent_run_input_tokens`, `agent_run_output_tokens`,
  `agent_run_total_tokens`, each shaped `{ count: number }`.
- Work-items returns `total_work_items`, `started_work_items`,
  `backlog_work_items`, `un_started_work_items`, `completed_work_items`, each
  shaped `{ count: number }`.
- Permission: workspace admin or member. Guest reads return 403.

### 16.2 Workspace charts

`GET WS/advance-analytics-charts/?type=projects|work-items|custom-work-items[&project_ids=...][&x_axis=...][&group_by=...]`

- `type` defaults to `projects`.
- `project_ids` scopes every query.
- `type=projects` returns an array of seven rows in order: `work_items`,
  `cycles`, `modules`, `intake`, `members`, `pages`, `views`; each row is
  `{ key, name, count }` with display names title-cased.
- `type=work-items` returns `{ data, schema }`. `data` is monthly buckets from
  the workspace creation month through the current month; each row has `key`
  and `name` as `YYYY-MM-DD`, `count`, `created_issues`, and
  `completed_issues`. `schema` is `{ completed_issues, created_issues }`.
- `type=custom-work-items` returns `{ data, schema }`. Without `group_by`, a
  row is `{ key, name, count }` and `schema` is `{}`. With `group_by`, rows
  carry a segment-count property per schema key and `schema` maps each key to
  its display label. `data` is ordered by descending total count.
- `x_axis` defaults to `PRIORITY`; `group_by` is optional. Values are uppercase
  enum names (`STATES`, `STATE_GROUPS`, `PRIORITY`, `LABELS`, `ASSIGNEES`,
  `ESTIMATE_POINTS`, `CYCLES`, `MODULES`, `COMPLETED_AT`, `TARGET_DATE`,
  `START_DATE`, `CREATED_AT`). `y_axis` is `WORK_ITEM_COUNT`; the reachable
  client sends no other value.
- Permission: workspace admin or member. Guest reads return 403.

### 16.3 Workspace stats

`GET WS/advance-analytics-stats/?type=work-items[&project_ids=...]`

Returns an array of per-project rows: `project_id`, `project__name`,
`cancelled_work_items`, `completed_work_items`, `backlog_work_items`,
`un_started_work_items`, `started_work_items`; ordered by `project_id`.
`type` defaults to `work-items`; any other type is a 400. Permission: workspace
admin or member; guests receive 403.

### 16.4 Project stats

`GET WS/project-stats/?fields=total_issues,completed_issues[&project_ids=...]`

The endpoint accepts `project_ids`; the legacy list never sent it, while the
new build does whenever the picker has a selection (§7). Returns an array of
rows shaped `{ id, total_issues, completed_issues }` (the
server may include more fields if requested; its valid set also includes
`total_members`, `total_cycles`, `total_modules`). Invalid requested fields
are ignored and the server falls back to the full valid set. Permission:
admin, member or guest. The new client requests only the two fields it uses.

### 16.5 Scoped variants

- `GET P/advance-analytics/?tab=work-items[&project_ids={projectId}][&cycle_id=...|&module_id=...]`
- `GET P/advance-analytics-charts/?type=work-items|custom-work-items[&project_ids={projectId}][&cycle_id=...|&module_id=...][&x_axis=...][&group_by=...]`
- `GET P/advance-analytics-stats/?type=work-items[&project_ids={projectId}][&cycle_id=...|&module_id=...]`

Scoped totals use the same five work-items keys. The scoped work-items chart
uses monthly buckets for project scope and daily buckets inside a cycle or
module range; a cycle/module with no usable start date returns empty `data` and
`schema`. Scoped stats rows carry `display_name`, `assignee_id`, `avatar_url`,
`cancelled_work_items`, `completed_work_items`, `backlog_work_items`,
`un_started_work_items`, `started_work_items`; the unassigned row has a null
`display_name` and `assignee_id`. Project-scoped totals and stats are
admin/member only, so guests receive 403. Project-scoped charts also permit
guests at the API (the same permission set as the workspace chart endpoint),
but no shipped guest surface opens the dialog or requests dialog content; the
new build keeps the trigger absence in §13 rather than adding a guest-only
chart path.

### 16.6 Query keys and fetch policy

Use TanStack Query with keys rooted at `["ws", workspaceSlug, "analytics", ...]`.
Recommended shapes:

- `["ws", ws, "analytics", "totals", tab, normalizedProjectIds, cycleId, moduleId]`
- `["ws", ws, "analytics", "charts", kind, normalizedProjectIds, cycleId, moduleId, xAxis, yAxis, groupBy]`
- `["ws", ws, "analytics", "stats", "work-items", normalizedProjectIds, cycleId, moduleId, scope]`
- `["ws", ws, "analytics", "project-stats", normalizedProjectIds]`

`kind` distinguishes `projects`, `work-items`, and `custom-work-items`. Include
the custom axes only in the custom key. Include cycle/module ids only in the
dialog query. Normalize project ids (dedupe, preserve picker order) before
putting them in a key or request so two equivalent selections share a cache
entry.

All analytics reads are ordinary on-mount reads. No polling and no fixed
refetch interval. Focus refetch may follow the app-wide TanStack Query default
(the old app globally revalidated on focus); do not add a special analytics
override that refetches more aggressively. 403s are terminal empty states for
the affected section, not retries. Do not retry 4xx responses.

Consumers should read the `count` field, not the whole totals object. A totals
object missing a requested key is a zero card, not a crash. Treat a missing
`data` or `schema` as an empty result.

## 17. Reuse map for the new build

This is a survey of `apps/web_new` and `@pidash/kit`, not a plan to copy legacy
components. Everything here is either reused as-is, extended with new props, or
newly built. Shared-component extensions become a `blocked_by` edge rather
than a fork.

Summary of the reuse decision:

- Reused as-is: `ProjectPicker`'s option/search machinery and
  `usePickerProjects`, `projectsQueryOptions`, `ProjectMark`, `ProjectLogo`,
  `CreateProjectDialog`, `usePermissions` and the session role constants, kit
  `Combobox`, `Button`, `IconButton`, `Input`, `Tooltip`, `Skeleton`,
  `Spinner`, `EmptyState` and `Avatar`, and the existing Issues
  `AnalyticsEntry` trigger shell.
- Extended, never forked: `ProjectPicker`'s existing `trigger` prop is reused;
  the analytics caller supplies a trigger whose multi-select summary implements
  `3+ projects` (the default summary reads a true count and must not be used);
  kit `Dialog` with a right-panel/wide presentation, or a new kit `Sheet`
  primitive if that variant would distort `Dialog`; the existing token-backup
  download pattern extracted into a shared CSV download helper.
- New and owned by analytics: the route and tab strip, headline cards, radar
  and active-projects sections, created-vs-resolved and custom-insight
  sections, the analytics table shell and its search/row-count chrome, the
  analytics feature query options, and the pure chart/export formatting
  helpers.
- Not to be used: the legacy analytics components, store, service, chart
  utilities and export helpers. No analytics feature may import from
  `@pi-dash/*` or `apps/web/**`, and no slice may copy an old component and
  edit it.

### Parity driver contract

The driver interface in `apps/web_new/e2e/parity/drivers/parity-driver.ts`
already declares the analytics methods the oracle calls. Implementations in
`drivers/web-new.ts` are still `todo()` stubs; each slice implements the
subset it owns. Do not change method semantics or weaken assertions when
filling them in.

- Shell/scoping: `analyticsOpenTab`, `analyticsOpenRoot`,
  `analyticsCurrentPath`, `analyticsTabIds`, `analyticsSelectedTab`,
  `analyticsSelectTab`, `analyticsPanelVisible`, `analyticsBreadcrumb`,
  `analyticsPageTitle`, `analyticsEmptyFacts`, `analyticsEmptyCreateProject`,
  `analyticsCreateAffordances`, `analyticsPickerLabel`, `analyticsPickerOpen`,
  `analyticsPickerClose`, `analyticsPickerOptions`, `analyticsPickerSearch`,
  `analyticsPickerToggle`, `analyticsPickerChecked`,
  `analyticsStartRequestSpy`, `analyticsStopRequestSpy`,
  `analyticsSpyRequests`.
- Overview and work-items: `analyticsOverviewCards`,
  `analyticsWorkItemsCards`, `analyticsHeadlinePlaceholders`,
  `analyticsTotalsStall`, `analyticsTotalsRelease`,
  `analyticsChartsStall`, `analyticsChartsRelease`,
  `analyticsProjectStatsStall`, `analyticsProjectStatsRelease`,
  `analyticsRadarSummary`, `analyticsRadarFacts`, `analyticsRadarLoading`,
  `analyticsActiveProjects`, `analyticsActiveProjectsLoading`,
  `analyticsActiveProjectTooltip`, `analyticsCreatedResolvedFacts`,
  `analyticsCreatedResolvedLoading`, `analyticsSectionGuidance`.
- Custom insights and exports: `analyticsCustomPickerLabels`,
  `analyticsCustomPickerOptions`, `analyticsCustomPickerSelect`,
  `analyticsCustomChartFacts`, `analyticsCustomTooltipSegments`,
  `analyticsExportOpen`, `analyticsExportButtonCount`,
  `analyticsExportRowCount`, `analyticsExportRowCells`,
  `analyticsExportHeaders`, `analyticsExportCountLabel`,
  `analyticsExportSearchExpanded`, `analyticsExportSearch`,
  `analyticsExportClearSearch`, `analyticsExportSearchClose`,
  `analyticsExportEmptyText`, `analyticsExportDownload`,
  `analyticsExportYieldsNoDownload`, `analyticsInsightTableStall`,
  `analyticsInsightTableRelease`, `analyticsInsightSkeletonCells`.
- Scoped dialog: `analyticsDialogOpenFromIssues`,
  `analyticsDialogOpenFromCycle`, `analyticsDialogOpenFromModule`,
  `analyticsSurfaceTriggerCount`, `analyticsDialogTitle`,
  `analyticsDialogSpinnerSeen`, `analyticsDialogWideToggleVisible`,
  `analyticsDialogToggleWide`, `analyticsDialogWidth`,
  `analyticsDialogHeadlineCards`, `analyticsDialogRowCount`,
  `analyticsDialogTableHeaders`, `analyticsDialogTableCells`,
  `analyticsDialogAssigneeFacts`, `analyticsDialogExportDownload`.

The stall/release and request-spy methods are test hooks, not product
behavior; they may simulate a slow read at the feature's fetch boundary, but
must not alter the production request or response shape.

### Slice A — route, tabs, picker, empty view, totals (NEWFRONT-731)

- Route stub exists at `apps/web_new/src/routes/$ws/_gated/analytics/index.tsx`
  and currently renders `TabAreaMount` (SHELL-054). Replace it with the real
  route tree: the index redirect to `overview`, a path child for `$tabId`, and
  the feature component. Do not create a parallel route outside `_gated`.
- `apps/web_new/src/shared/pickers/ProjectPicker.tsx` already supports
  multi-select, lazy project loading, search over name+identifier
  (`projects.ts`), selected-first sorting and a custom trigger. Reuse it as-is
  for options/search; supply a custom trigger that implements the four label
  cases in §4. Do not fork the picker.
- `projectsQueryOptions`, `usePickerProjects`, `ProjectLogo`/`ProjectMark`, and
  `CreateProjectDialog` already exist under `features/projects`. Reuse them for
  the picker and no-project creation. Use the kit `EmptyState` detailed
  variant for the no-project view.
- `src/core/session/permissions.ts` and `usePermissions` provide
  `ROLE_ADMIN`, `ROLE_MEMBER`, `ROLE_GUEST`, `canEdit` and `isAdmin`. Use
  `canEdit` for the creation action; do not compare numeric roles ad hoc.
- No kit `Tabs` component is exported. Build an analytics-local tab strip using
  kit `Button`/`IconButton` styling and ARIA tab roles, or add `Tabs` to the kit
  as a general primitive. Do not import a tab component from another area or
  the old app.
- Totals cards are new analytics-local presentation using kit `Skeleton` for
  placeholders and ordinary type/spacing tokens.

### Slice B — shared chart layer, radar, active projects (NEWFRONT-732)

- There is no chart dependency or shared chart wrapper in `apps/web_new` yet.
  Add Recharts 3 per decision D15 through a lazy `import()` boundary, wrapped
  in new `shared/charts` components that accept typed data/series and read
  kit color tokens. This slice also adds the ten-slot categorical chart palette
  to kit tokens described in §10; the light values are oracle-pinned and the
  kit currently has no chart palette. This slice owns the catalog, dependency
  allowlist, kit token change and lockfile update.
- Build radar and summary new. Reuse `ProjectMark` for project identity and
  kit `Tooltip` for long names. Reuse `projectsQueryOptions`/the project store
  to resolve active-project ids; do not copy project display logic.
- The active-projects query is new, but its response type belongs in
  `@pidash/api-client` (contracts issue NEWFRONT-728). The percentage and tone
  helpers can be small pure functions with unit tests.
- Skeleton shapes can use kit `Skeleton`; no legacy loader is imported.

### Slice C — created-vs-resolved, custom pickers, stacked bar (NEWFRONT-733)

- Build the area and bar charts on the shared chart layer introduced by slice
  B; do not create a second chart wrapper or a second color engine.
- Use kit `Combobox` or `Menu` for the three pickers and kit `Button` for the
  trigger. The picker option constants live in the analytics feature, not in a
  copied legacy constants package.
- Axis display formatting, date grouping, palette extension and stacked-edge
  rounding are new pure helpers in the analytics feature or shared chart
  layer. Unit-test them directly.
- No kit legend primitive is needed because the custom chart has no legend;
  the area chart uses Recharts' legend with styled labels.

### Slice D — table foundation, breakdown, exports (NEWFRONT-735)

- No kit `Table` component is exported. Existing feature tables (for example
  the workspace members roster) are feature-local and not searchable in the
  analytics sense; do not import one of them. Build one analytics table shell
  (header, rows, right-aligned numeric cells, horizontal scroll) and reuse it
  for the chart table, workspace breakdown and dialog breakdown. Do not fork a
  second table per section.
- Search/row-count chrome is new for analytics; use kit `Input`, `IconButton`,
  `Button` and existing focus styles. Do not import the old searchable table.
- CSV generation is new at `apps/web_new/src/shared/export/csv.ts`. Reuse the
  browser-download pattern in
  `apps/web_new/src/features/workspace-settings/tokens/download.ts`
  (Blob, object URL, anchor, revoke) by extracting or calling a shared helper;
  do not duplicate the anchor plumbing. The builder must be injectable for unit
  tests and must implement BOM, CRLF, quoting, dash fallback and the zero-row
  no-download rule. NEWFRONT-769 owns the later export-rule changes in this
  same file; it must not replace it with a second implementation.
- Skeleton rows use kit `Skeleton`. Empty guidance uses kit `EmptyState`
  section variant.

### Slice E — scoped dialog, assignee column, guest read-only (NEWFRONT-738)

- Reuse the existing Issues entry at
  `apps/web_new/src/features/issues/analytics/AnalyticsEntry.tsx` (ISS-107)
  for the trigger and role gate. Replace its placeholder body with the real
  dialog content. If NEWFRONT-619 introduces a shared `AnalyticsDialog` leaf,
  extend that component; do not fork it.
- Kit `Dialog` is centered and does not expose the legacy right-panel/wide
  behavior. Add a documented right-panel variant (position and width props)
  to `Dialog`, or build a new accessible `Sheet` primitive in the kit. Do not
  create an analytics-only modal trap. The wide toggle needs a medium-and-up
  breakpoint and preserved focus/scroll.
- Kit `Avatar` already supports `name`, `src` and `initial`, and falls back
  to initials. Reuse it for assignee cells. Use plain text plus a neutral user
  mark for `Unassigned`.
- Reuse the slice D table shell and CSV builder; pass an assignee-first column
  definition and an assignee count label. Do not fork the table.
- Reuse `usePermissions`/`useProjectRoles` for any trigger guard analytics
  itself owns. Cycle and module triggers stay in their areas: NEWFRONT-712
  (CYC-033) for cycles and NEWFRONT-802 (Modules slice J, MOD-032) for modules.
  Neither needs a reverse edge to slice E; those slices mount the trigger and
  consume this dialog.

### Contracts (NEWFRONT-728)

The contracts slice owns zod request/response schemas and contract tests in
`@pidash/api-client` for all endpoints in §16. It must encode:

- the totals response as a record of metric keys each shaped `{ count }`, or a
  strict explicit object with those keys;
- chart responses as a discriminated union of the project array and the
  `{ data, schema }` shapes;
- stats rows for both project scope and assignee scope, including null
  `display_name`/`assignee_id` and nullable `avatar_url`;
- `project-stats` rows with the valid field names (`total_issues`,
  `completed_issues`), not the invalid legacy field names;
- query parameter typing for `project_ids`, `cycle_id`, `module_id`, `x_axis`,
  `y_axis`, `group_by`, `tab` and `type`.

## 18. Intended vs observed

| Row     | Oracle observation                                                                                                                                                                                                | Intended new-build behavior                                                                                                                                                                                                       |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ANL-001 | Two tabs, `overview` first; bare address redirects; unknown id retained with no selected panel.                                                                                                                   | Same. Path-segment tab ids, `overview` default, unknown id preserved.                                                                                                                                                             |
| ANL-002 | Breadcrumb `Analytics`; title `{workspace} - Analytics`.                                                                                                                                                          | Same exact wording.                                                                                                                                                                                                               |
| ANL-003 | No-project empty view; creation enabled for admin/member, disabled for guest; create leaves empty view.                                                                                                           | Same, reusing the shared project-create dialog.                                                                                                                                                                                   |
| ANL-004 | Default `All projects`; name+identifier search; four picks show literal `3+ projects`; one pick scopes every read and survives tab change.                                                                        | Same literal `3+ projects`, as settled above. Picker also scopes `project-stats` (§7; additive divergence, not observed). Picker lives in a validated `project_ids` search param (additive divergence: reload/share persistence). |
| ANL-005 | 11 cards with correct keys/order and zeros on failure; labels render raw i18n keys (bug NEWFRONT-468).                                                                                                            | 11 cards with intended human labels in §5; values unchanged. Declared corrected divergence.                                                                                                                                       |
| ANL-006 | Radar and summary agree on seven dimensions; skeleton; zero dimensions for empty member; failed guest read has headers only, no guidance.                                                                         | Same; keep zero dimensions and headers-only failure.                                                                                                                                                                              |
| ANL-007 | Percentage `round(completed/total*100)`, success only `>50`; long name truncates with tooltip; unresolved project rows skipped; five loading rows.                                                                | Same. Request and consume `total_issues,completed_issues` rather than legacy invalid fields. Picker scope added (declared divergence).                                                                                            |
| ANL-008 | Five work-items cards with exact labels/order; dialog keeps same labels; epic branch unreachable.                                                                                                                 | Same; no epic wording branch.                                                                                                                                                                                                     |
| ANL-009 | Two smoothed areas, legend `Resolved`,`Created`, `MMM DD, YYYY` labels, loader, guidance on failed read.                                                                                                          | Same. Monthly workspace buckets and daily scoped buckets.                                                                                                                                                                         |
| ANL-010 | Metric only `Work item` even with points estimation; fixed x list; split excludes x; every re-pick re-queries.                                                                                                    | Same. Settled as intended, not a bug.                                                                                                                                                                                             |
| ANL-011 | Priority and state colors; date labels current-year `MMM D`, other-year `MMM D, YYYY`, missing `None`; no legend; stacked schema order; only extreme ends rounded; first ten palette slots fixed; 11th generated. | Same. Slice B adds the fixed ten-slot chart palette to kit tokens; day-only grouping.                                                                                                                                             |
| ANL-012 | `Priority`,`Count`; label `3 Priority`; rows repeat chart values; search and filtered export.                                                                                                                     | Same, generalized to the current x-axis label and optional split columns. Label uses loaded length, fixed noun.                                                                                                                   |
| ANL-013 | `Project`,`Backlog`,`Started`,`Unstarted`,`Completed`,`Cancelled`; `2 Projects`; five skeleton rows; guidance with zero count on failed read.                                                                     | Same. Unresolved project id shows mark without a name on screen; CSV falls back to the server name. Fixed noun, loaded length.                                                                                                    |
| ANL-014 | Project/cycle titles; module oracle has double space; spinner; wide toggle md+; scoped requests; close restores `All projects`; guest has no trigger.                                                             | Same, except the module title uses one space (declared corrected divergence). Scope is dialog-local; workspace picker unchanged.                                                                                                  |
| ANL-015 | `Assignee` table with avatar/initial/unassigned; scoped counts; epic reword unreachable.                                                                                                                          | Same; no epic reword branch; fixed `Assignee` noun in the count label.                                                                                                                                                            |
| ANL-016 | All four exports use `{ws}-analytics.csv`, display headers and no dash cells.                                                                                                                                     | Same filename and headers; dash only for an undefined/null column value (legacy coerces null fields to empty); no download for zero filtered rows.                                                                                |
| ANL-017 | Count label plus expanding first-column search; close clears filter and collapses.                                                                                                                                | Same. Label is the loaded length with a fixed noun and does not follow search.                                                                                                                                                    |
| ANL-018 | Guests browse tabs and picker; no creation; server refuses guest create and workspace analytics reads; UI zeros/guidance.                                                                                         | Same. Guest tables have no rows, so export/search are not exercised; only creation is gated.                                                                                                                                      |

## 19. Permissions and editions

Analytics itself has no edition-specific surface. The OSS, cloud and desktop
builds render the same tabs, picker, charts, tables, exports and dialog; the
cloud overlay does not add an analytics-only endpoint or UI. The area gate can
prove the OSS/desktop checkout behavior above. It cannot prove a private cloud
override unless such an override exists; none was found in the scoped sources.
If a cloud-only analytics variant is introduced later, add rows rather than
silently branching in this area.

Workspace role rules:

- admin and member: full analytics read access on both tabs and in the dialog;
- guest: read-only workspace analytics UI and picker; workspace advance reads
  return 403 and settle to zero/guidance, the guest's tables therefore have no
  rows (no search/export to exercise); project-stats is permitted, so the
  active-projects list shows the guest's resolvable joined projects; no project
  creation;
- analytics does not offer issue editing, comments or any other mutation.

Project creation from the no-project view uses the permission helpers in
`src/core/session/permissions.ts`; analytics must not duplicate the role logic
or expose an enabled affordance that the server would refuse.

## 20. Issues-area dependency conclusion

Analytics is overwhelmingly read-only and consumes very few Issues-area
components. The named Issues slices were reviewed as potential dependencies:

- NEWFRONT-619 (list layout) — needed only as the owner of the existing Issues
  analytics trigger/dialog leaf. Slice E already has this `blocked_by` edge.
- NEWFRONT-627 (rich filter inputs + save view, ISS-107) — the analytics entry
  is already present in `apps/web_new/src/features/issues/analytics/`. Slice E
  needs the leaf, not the whole rich-filter stack; rely on the existing entry
  and its owner, and do not add a new blocker on the full filter implementation
  unless that slice replaces the leaf.
- NEWFRONT-615/616 (dropdown primitives), 622/623 (kanban), 626 (rich filter
  core), 628/629 (legacy filter dimensions), 630 (create modal), 633 (bulk
  actions), 639/640/642 (detail/sidebar), 646 (peek) — not needed. Analytics
  does not render issue rows, filter controls, kanban cards, bulk actions,
  issue detail, or peek. Do not create edges to these.
- Cycle trigger owner is NEWFRONT-712 (CYC-033); it already depends on slice E
  and must not gain a reverse edge. Module trigger owner is NEWFRONT-802
  (Modules slice J, MOD-032, under NEWFRONT-82) and likewise needs no reverse
  edge to slice E: it mounts the trigger and consumes this dialog. Analytics
  records that both triggers are external and does not build either itself.

The only genuinely shared pieces in this area are the project picker, project
creation dialog, project marks and avatars, kit dialog/combobox/table/chart
primitives, and the CSV download plumbing. Extending the picker's trigger
label or the kit dialog's panel variant is an extension of the owning
component, not a fork; if a slice cannot extend it, that slice becomes the
blocked_by edge.

## 21. Row coverage index

- ANL-001 — §1.
- ANL-002 — §1.
- ANL-003 — §3.
- ANL-004 — §4, §18.
- ANL-005 — §5, §18.
- ANL-006 — §6, §18.
- ANL-007 — §7, §18.
- ANL-008 — §8, §18.
- ANL-009 — §9, §18.
- ANL-010 — §10, §18.
- ANL-011 — §10, §18.
- ANL-012 — §11, §15, §18.
- ANL-013 — §12, §15, §18.
- ANL-014 — §13, §18.
- ANL-015 — §14, §18.
- ANL-016 — §15, §18.
- ANL-017 — §11, §12, §15, §18.
- ANL-018 — §3, §13, §18, §19.

Rows owned by this spec: none. Do not change the inventory Status column in
this issue.
