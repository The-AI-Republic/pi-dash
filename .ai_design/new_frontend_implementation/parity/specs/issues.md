# Issues area spec — layouts, detail, peek, filters, bulk edit, relations, attachments (NEWFRONT-610)

Build spec for inventory rows ISS-001–238. It consolidates and completes
the three oracle-area prose specs (`issues-layouts-a.md`, rows
ISS-001–027 plus ISS-060–074; `issues-layouts-kanban-gantt.md`, rows
ISS-028–059; `issues-filters-display.md`, rows ISS-075–107), which remain
as oracle records, into the single file slices build from. Every section
below cites the row IDs it covers; every one of ISS-001–238 appears at
least once.

Prose only: no code, no copied strings, styles, or assets from the old
frontend. API paths, query parameters, field names, and storage keys are
named exactly because contracts and slices build from them. Endpoint
shorthand follows the inventory: `BASE` is
`/api/workspaces/{ws}/projects/{p}`, `WS` is `/api/workspaces/{ws}`, and
`{issues|epics}` is the service-type path segment.

Old sources consulted (reference only, never a source): the per-entity
layout roots and the layout switch wrapper; the list, spreadsheet,
calendar, kanban, and gantt layout folders; the row quick-action menus;
the empty-state set; the header filter bar with its display sections,
legacy per-dimension sections, and rich-filter row; the work-item filter
stores plus user-properties endpoints; the create/edit modal family; the
drafts page and draft store; the detail root, title pipeline, description
editor, properties sidebar, and header actions; the peek panel with its
URL sync; the sub-issues, relations, links, attachments, and run-AI
widgets; the comment composer, cards, activity feed, and reactions; the
shared dropdown and read-only component families; the label settings
screen; the cloud no-op stubs; and the issue services and detail stores
for endpoint shapes. Live behavior pinned by the oracle suite under
`apps/web_new/e2e/parity/issues/` is cited per row as `Oracle:` with the
spec file.

Intended behavior wins over pinned bugs. Where an oracle scenario pins
an old bug (marked `bug:` with a linked issue), slices build the intended
behavior stated here and the scenario is aligned at gate time — the same
convention as the shell spec's target-behavior section. Target rows whose
behavior comes from main PRs rather than the live app are restated
precisely in §22.

## 1. Layouts — shared behavior (ISS-001–006)

Each issues-bearing entity — project, cycle, module, saved view,
archived list, and the workspace all-issues list — has its own layout
root that renders whichever layout the current user last picked for that
entity (ISS-001). The header carries a layout switcher with one entry
per layout in a fixed order: list, board, calendar, spreadsheet,
timeline (gantt). On wide screens it renders as a segmented row of
icon-only buttons with the active one visually distinguished; clicking
the active entry changes nothing. On narrow containers the same control
collapses to a dropdown that names each layout with an icon plus a text
label. Archived issues always render the list layout regardless of the
stored choice, and the compact header offers only list, board, and
calendar. Switching layouts re-renders immediately and persists the
choice per user per entity, so a reload restores it.
Oracle: `layouts-a-shared.spec.ts`.

The persisted preference record (ISS-002) also holds grouping,
sub-grouping, sort order, the show-empty-groups flag, calendar
month/week mode plus weekend visibility, and which property columns are
shown. It is fetched once when the page mounts and never refetched on
window focus. Reads and writes go through per-entity user-properties
endpoints: `GET/PATCH BASE/user-properties/` for the project,
`BASE/cycles/{cid}/user-properties/` and
`BASE/modules/{mid}/user-properties/` for cycle and module contexts,
`BASE/epics-user-properties/` for epics, and `WS/user-properties/` for
the workspace scope. Oracle: `layouts-a-shared.spec.ts`.

Narrow screens get a compact header (ISS-003): a single bar with a
layout selector (list, board, calendar), a Display entry, and an
analytics entry. Cycle and module display options are disabled there
when the project has those features off. Writes go through the same
user-properties endpoints; the analytics entry opens the analytics
dialog. Oracle: `layouts-a-mobile.spec.ts`.

A layout wrapper routes loading and emptiness (ISS-004). While the
first load is in flight each layout shows its own skeleton placeholder.
When the total issue count is zero the context-specific empty state
renders instead of the layout — except calendar, which always renders
its grid and represents emptiness per day. Oracle:
`layouts-a-loaders.spec.ts`.

Ongoing loads have their own chrome (ISS-005). A small floating spinner
docked top-right appears while a filter or display refetch is in
flight; single-issue edits never raise it. Paginating shows skeleton
rows in list and spreadsheet, pulsing blocks in calendar, plus a thin
loading bar across the top of the calendar. Oracle:
`layouts-a-loaders.spec.ts`.

Freshness and optimism (ISS-006): issues created within roughly the
last half minute render immediately even inside virtualized lists and
carry a transient highlight; an unconfirmed create shows as a pulsing
temporary row keyed by a client temp id that reconciles to the real
issue once the server confirms; a dropped issue is highlighted and
scrolled into view. The oracle pins three deviations from this design
(the recency bypass never fires, fresh rows never highlight, and no
temp row renders while the create pends) as NEWFRONT-168, NEWFRONT-170,
and NEWFRONT-171. Slices build the intended behavior; the scenarios are
aligned at gate. Oracle: `layouts-a-loaders.spec.ts`.

## 2. List layout (ISS-007–014)

With no grouping the list shows one flat section holding every work
item (ISS-007). With a grouping chosen — project, cycle, module, state,
state group, priority, labels, assignees, creator, or team project —
the list shows one collapsible section per group value, with a trailing
bucket for issues with no value where that applies. Grouping by creator
disables per-group creation. Reads use `GET BASE/{issues|epics}/`
grouped, fifty per page grouped and one hundred flat. Oracle:
`sign-in-and-list.spec.ts`.

Clicking a section header collapses or expands it (ISS-008). The
collapsed set is stored per user per view and shared with the board
layout; a collapsed section hides its rows and quick-add, and dropping
an issue onto a collapsed section re-expands it. A display flag
controls whether zero-item sections render at all (ISS-009): when off,
empty group headers are dropped; when on, they still show. Oracle:
`layouts-a-list.spec.ts`.

Pagination differs by shape (ISS-010). A grouped section with more
issues than fetched shows an explicit load-more row; the flat list
instead appends skeleton rows and loads the next page automatically
when they scroll into view, via cursor paging. Oracle:
`layouts-a-list.spec.ts`.

Each section ends with a sticky single-field quick-add form (ISS-011).
It opens from a compact new-item button, inherits the section's
grouping value (state, priority, labels, assignees, cycle, module) plus
the project's default state, and submits with Enter — staying open for
rapid repeat entry — while Escape or an outside click closes it. Submit
posts to `POST BASE/{issues|epics}/` (plus a cycle or module attach
where the context needs it), shows a confirmation toast with follow-up
actions, and shows an optimistic row while saving. Guests, completed
cycles, creator-grouped sections, and states blocked by workflow rules
get no quick-add. The oracle pins the missing temp row as NEWFRONT-170
(see ISS-006); slices build the temp row. Oracle:
`layouts-a-list.spec.ts`.

In cycle or module context the section add control offers two entries
(ISS-012): create a new work item through the create modal pre-set with
the section value, or attach an existing one through a search modal
that adds it to the cycle (`POST BASE/cycles/{cid}/cycle-issues/`) or
module with a toast. In a plain project the same control opens the
create modal directly; in epic context it opens the epic modal.
Oracle: `layouts-a-row-actions.spec.ts`.

A row (ISS-013) shows the issue identifier sized to content, the title
with a hover tooltip, a sub-issue expander that lazily loads children
(`GET BASE/{issues|epics}/{id}/sub-issues/`), the enabled inline
properties, and a quick-actions menu. Inline nesting stops at three
levels; deeper children open the peek panel instead. Epics show
progress statistics rather than an expander. The row is a real link: a
normal click opens peek, while middle-click or a keyboard-modified
click opens a new tab. The oracle pins NEWFRONT-162 (nested expansion
fetches but never renders grandchildren, so the deeper-than-one paths
are unreachable); slices build expansion through the cap with peek
beyond it. Oracle: `layouts-a-list.spec.ts`.

The inline property strip (ISS-014) renders only display-enabled
properties: state, priority, start and due dates (merged or separate),
assignees, modules and cycles (each only when the project enables that
feature), estimate (when enabled), read-only sub-issue, attachment,
and link counts, plus labels. Each editable property writes
immediately through `PATCH BASE/{issues|epics}/{id}/`, with cycle and
module membership going through their bridge calls; an overdue due
date is visually flagged depending on the issue's state group. Guests,
archived issues, and completed cycles render read-only. Oracle:
`layouts-a-list.spec.ts`.

## 3. Spreadsheet layout (ISS-015–020)

The sheet is a flat table of all issues fetched with `GET
BASE/{issues|epics}/` (flat, one hundred per page) (ISS-015). The
first column (identifier, title, row actions) sticks to the left while
scrolling horizontally, gaining a shadow once scrolled; the header row
sticks to the top. Sub-issues expand inline with the same three-level
cap as the list, deeper ones opening peek; rows are virtualized. The
NEWFRONT-162 pin applies here too: slices build the cap-through-three
behavior. Oracle: `layouts-a-spreadsheet.spec.ts`.

Columns appear only for enabled display properties (ISS-016):
assignee, creation and update timestamps, due date, estimate, labels,
modules, cycle, link, priority, start date, state, sub-issue count, and
attachment count. At project level the cycle and module columns
additionally require the matching project feature flag. Oracle:
`layouts-a-spreadsheet.spec.ts`.

Editable cells (ISS-017) — state, priority, assignees, start and due
dates with min/max constraints against each other, estimate, labels,
cycle, modules — write immediately through `PATCH
BASE/{issues|epics}/{id}/` (cycle and module through their bridge
calls) and return focus to the cell. Timestamp, link, attachment-count,
and sub-issue-count cells are read-only; activating the sub-issue
count navigates to that issue's sub-issues. Guests, completed cycles,
and archived issues render read-only. Oracle:
`layouts-a-spreadsheet.spec.ts`.

Each sortable column header opens a menu offering ascending and
descending sort (ISS-018); once sorted, the menu also offers clearing
the sort, which restores creation-date order. The active sort is
remembered locally per browser and marked with an arrow on the header.
Oracle: `layouts-a-spreadsheet.spec.ts`.

A sticky bottom row reveals the same inline title form as list
quick-add — Enter repeats, Escape closes, optimistic row plus toast
(ISS-019). The table footer auto-loads further pages on scroll with
skeleton rows. Cells are keyboard-focusable: arrow keys move focus
between cells (ISS-020). Oracle:
`layouts-a-spreadsheet.spec.ts`.

## 4. Calendar layout (ISS-021–027)

Month view renders every week of the active month; week view renders
the active week only (ISS-021). Issues land on day tiles by their
target date; issues without one never appear. Today is badged, weekend
tiles are shaded, and crowded days offer a per-day load control. Reads
use `GET BASE/{issues|epics}/` with an `after`/`before` window,
`groupedBy=target_date`, four per page in month view and thirty in
week view. Oracle: `layouts-a-calendar.spec.ts`.

The header offers previous/next stepping (one month in month view, one
week in week view), a Today jump that also reselects today, and a
title button opening a month picker with year stepping that stays
disabled in week view (ISS-022). Week view shows a date-range title.
Every range change refetches the windowed query. Oracle:
`layouts-a-calendar.spec.ts`.

An Options menu (ISS-023) switches month/week mode with the active
mode checkmarked and toggles weekend columns (the grid switches
between seven and five columns). The week's first day follows the
user's profile setting. On narrow screens the menu dismisses itself
after a pick. Both settings persist through the calendar display
filter (`calendar.layout`, `calendar.show_weekends`). Oracle:
`layouts-a-calendar.spec.ts`.

Dragging an issue block onto another day re-dates it to that day
through `PATCH BASE/{issues|epics}/{id}/` with the new target date
(ISS-024). When the issue starts after the chosen day the drop is
refused with an error toast; dropping onto the same day does nothing.
The dropped block is highlighted and the grid auto-scrolls near the
edges mid-drag. Dragging is unavailable on touch layouts. Oracle:
`layouts-a-calendar.spec.ts`.

A block (ISS-025) shows a state-colored edge plus identifier and
title. Hovering reveals a preview popover of the issue; clicking opens
peek through the deep link; hover also reveals per-block quick actions
whose placement flips near the viewport edge. Unconfirmed blocks pulse
like list temp rows. Oracle: `layouts-a-calendar.spec.ts`.

Each day offers adding an issue due that day (ISS-026): either an
inline title form pre-set to the date (`POST BASE/{issues|epics}/`
with the target date pre-set), or attaching an existing issue through
the search modal (which hides issues whose start would fall after the
target, then patches the target date and attaches to the cycle or
module where the context needs it). Epic calendars only offer
creation. Oracle: `layouts-a-calendar.spec.ts`.

On narrow screens (ISS-027) tapping a day selects it and lists that
day's items beneath the grid as generic pointer rows (identifier plus
title), not links; day tiles render no issue blocks at all, and blocks
are neither draggable nor hoverable. Oracle:
`layouts-a-mobile.spec.ts`.

## 5. Board / kanban layout (ISS-028–043)

On mount and whenever grouping changes the board fetches issues
grouped, thirty per page, or ten per page when sub-grouped
(ISS-028). One fixed-width column renders per value of the active
group-by dimension (project, cycle, module, state, state group,
priority, labels, assignees, created-by, team project); cycle, module,
labels, and assignees additionally get a synthetic empty-value column.
With no grouping there is a single column holding every issue. Each
column header shows an icon for the value, the value name, and a live
count of issues in that group. Headers sit in a sticky row; the board
scrolls horizontally beneath it. Columns render lazily: only the first
few mount immediately, the rest mount when scrolled into view behind
skeleton placeholders. The header count equals the number of issues in
the group; NEWFRONT-153 fixed the old grouped header count that excluded
a pending-triage issue while its card rendered, and the scenario asserts
the intended equality. Oracle: `layouts-b-kanban.spec.ts`.

With a sub-group-by set, the board renders a sticky row of group
headers followed by one horizontal lane per sub-group value (ISS-029).
Every lane has a sticky left header with a collapse chevron, an icon,
the sub-group name, and the cumulative issue count across all its
columns. The board scrolls vertically in swimlane mode. The oracle pins
NEWFRONT-158 (deleting a label ghosts its formerly labeled rows out of
later labels-grouped reads — the rows exist and are unlabeled, yet
render in no lane) on a scratch project; intended, the issue renders
under the synthetic empty lane, and slices build that. Oracle:
`layouts-b-kanban.spec.ts`.

In flat mode each column header carries a minimize/maximize control
(ISS-030): minimizing folds the column to a narrow vertical strip
showing the title rotated and the count, hiding the cards. In
swimlane mode clicking a lane header toggles that lane's cards, with
the chevron pointing down while collapsed and up while expanded
(ISS-031). Both collapse states persist per user alongside the kanban
filters locally and are restored on return. A grouped read only carries buckets for labels, modules, cycles and
assignees that issues use, and a sub-grouped read only carries lanes for
values some issue holds. With empty groups visible (the default), the
board therefore fills those axes from the project's own lists (labels,
modules, cycles, members) and draws a zero-total column or lane for each
unused value, keeping the server's bucket order and the synthetic empty
value last. With the filter off, only values that hold work remain. The
show-empty display filter hides zero-count columns and lanes when off
(ISS-032). Oracle: `layouts-b-kanban.spec.ts`.

A card is a real link to the work item, so it can be opened in a new
tab (ISS-033). A plain click opens the peek panel on desktop and
navigates full-page on mobile. Optimistically created cards that the
server has not confirmed yet cannot be opened. Card contents (ISS-034):
the issue identifier, quick actions revealed on hover (always visible
on touch layouts), the issue name clamped to one line with a full
tooltip, and the enabled display properties wrapped below; inline
editing of properties is gated on edit permission. Epic cards can show
progress statistics, and the currently peeked card is outlined. Cards
render lazily like columns: roughly the first screenful mounts
immediately, the rest behind placeholders. Oracle:
`layouts-b-kanban.spec.ts`.

Each column ends with a sticky quick-add row (ISS-035): a compact
new-item button opening an inline form with the project identifier, a
title field, and a hint that Enter adds another. Submitting creates
the issue through the create endpoint with the column's (and lane's)
grouping pre-applied plus the project's default state; the form stays
open for the next item. It hides without create permission, in
completed cycles, or when a workflow rule blocks creation, and the
header create entry stays disabled for created-by groupings. The group
header add control (ISS-036) opens the create modal prefilled with the
column payload in project context; in cycle or module context it
offers a menu with creating a new item and adding an existing one,
with adding confirmed by a success toast. Oracle:
`layouts-b-kanban.spec.ts`, `layouts-a-row-actions.spec.ts`.

Card dragging is pointer-based — press, move, release, no native HTML5
drag (ISS-037). A card can drag only when the active grouping is one
of state, priority, assignees, labels, module, or cycle (both
dimensions when sub-grouped), the card is server-confirmed, and the
user may edit it. Reordering within a column derives a new sort
position from the neighbours — a large step below the first card, a
large step above the last, the midpoint between two cards, or the
default step into an empty column — and persists it with `PATCH
BASE/{issues|epics}/{id}/` carrying the sort order. Reordering applies
only in manual order; otherwise an overlay names the active ordering
and the move is suppressed. The dropped card is briefly highlighted
and scrolled into view. The oracle pins NEWFRONT-154 (under a
non-manual order the drop silently persists a new sort order with no
overlay); slices build suppression plus the overlay. Oracle:
`layouts-b-kanban.spec.ts`.

Dropping into another column also applies that column's group value
(ISS-038): scalar fields are replaced (the empty column clears them),
list fields gain the destination and lose the source (the empty column
only removes). Cycle and module membership go through their dedicated
add/remove calls rather than the plain issue update. Dropping where
the grouping forbids it, or without permission, raises a warning toast
instead of moving. The oracle pins NEWFRONT-160 (when the moving user
has no eligible runner, moves into In Progress/In Review/In Test bounce
to Backlog though the card animates as moved — the platform's
no-eligible-runner bounce, not a state-lookup bug); whether an explicit
human move should skip the bounce is an open platform decision
(NEWFRONT-248). Across swimlanes
(ISS-039) both the group and sub-group values update, provided both
dimensions are draggable; otherwise the move is refused. The oracle
pins NEWFRONT-159 (sub-grouping by target date rendering a blank
board through a client crash); intended, one swimlane renders per
target-date value with its issues as cards. Oracle:
`layouts-b-kanban.spec.ts`.

While any card drag is in flight a delete zone is pinned top-center,
announced as a drop-to-delete target (ISS-040). Dropping a card on it
opens the delete-confirmation modal; confirming deletes the issue
through the delete endpoint. A feedback overlay covers a column
dragged over while it cannot accept the card (ISS-041): a notice
naming the active ordering when not manually sorted, a notice refusing
completed cycles, and (cloud edition) a notice when a workflow rule
blocks the target state; a truly disabled drop is ignored with a
warning toast. The NEWFRONT-154 pin applies to the ordering overlay as
in ISS-037. Oracle: `layouts-b-kanban.spec.ts`.

Each column paginates independently (ISS-042). Flat columns auto-load
the next page through a scroll sentinel with skeleton loaders;
sub-grouped columns show an explicit load-more entry instead.
Concurrent loads for one column are suppressed. Columns and cards
outside the viewport are replaced by height-estimated placeholders
until scrolled near (ISS-043); while dragging near an edge the board
auto-scrolls horizontally and the hovered column vertically. Oracle:
`layouts-b-kanban.spec.ts`.

## 6. Gantt / timeline layout (ISS-044–059)

The timeline pairs a sticky left sidebar list with a scrollable chart
on the right (ISS-044). Issues are fetched flat, one hundred per page.
The header shows a live item count, the Week/Month/Quarter switcher, a
Today button, and the fullscreen toggle. Bars are positioned by
start/target dates; issues without dates still appear in the sidebar
with an add-block affordance on their empty row. Oracle:
`layouts-b-gantt.spec.ts`.

Exactly three zoom levels exist (ISS-045) — Week, Month, Quarter —
each with its own day width (week widest, quarter narrowest). The zoom
is session-local: neither persisted nor deep-linked. Switching levels
re-centers the chart on today. Weekend columns are tinted; the week's
start day comes from the user profile. Week is the initial level.
Today (ISS-046) re-renders the current zoom and scrolls today into
view, landing just right of center; the current-date column is
highlighted in every zoom, and mounting the chart auto-centers today.
NEWFRONT-163 fixed the old app's Month bug (its today-week highlight
vanished on the week's last day because the week flag compared full
timestamps against a midnight end date): the week containing today stays
highlighted every day on `web_new`, and the scenario asserts that while
the oracle leg still pins the old day-aware behavior. The expand control
moves the chart into a full-screen portal overlay and the shrink control
returns it inline
(ISS-047). Scrolling near either horizontal edge appends the adjacent
date range and grows the chart without bound (ISS-048); prepending on
the left preserves the scroll position. Oracle:
`layouts-b-gantt.spec.ts`.

Each sidebar row (ISS-049) links identifier plus name (peek on
desktop, deep-link; disabled for unconfirmed cards) and, for fully
dated issues, a duration in days. Rows are virtualized; peeked and
hovered rows are visually distinguished. Rows drag to reorder with a
drop indicator (ISS-050); the new sort position derives from the
neighbours (a step below the first row, above the last, midpoint
between). Reordering is a separate drag instance from card dragging
and applies only in manual sort order; outside manual sort the drag
instance is disabled and the drop is a silent no-op. Oracle:
`layouts-b-gantt.spec.ts`.

All three bar gestures are mouse-driven with day snapping and persist
on release through the batch issue-dates endpoint (`POST
BASE/issue-dates/` carrying the updates); a failed persist toasts an
error (ISS-051). Moving drags the bar body horizontally and shifts
start and target together, preserving duration; only fully dated bars
move (ISS-052). Resizing drags the edge handles — left edge moves the
start date, right edge the target — never narrower than one day, with
a floating label previewing the date under the handle; either handle
can fill in its missing date on a half-dated bar (ISS-053). Hovering
an undated issue's empty row reveals an add control that follows the
cursor with a date tooltip; activating it plants a short block at that
day (a week-long block in quarter zoom) via the issue update endpoint
(ISS-054). A sticky quick-add at the chart bottom creates issues
pre-dated today through tomorrow so the new bar appears immediately;
it hides without create permission or in completed cycles (ISS-055).
Oracle: `layouts-b-gantt.spec.ts`.

Bars are tinted by their state color, with a fading mask when only one
date is set (ISS-056). The issue name stays pinned visible while the
chart scrolls horizontally; epic bars show progress once the bar spans
at least two days. Hovering a bar opens a preview popover; clicking
opens peek through the deep link. When a bar lies outside the viewport
its row shows a sticky arrow pointing toward it; activating the arrow
scrolls (extending the date range when needed) to bring the bar into
view (ISS-057). Moving, resizing, reordering, and planting blocks
require project admin or member standing; reordering additionally
requires manual sort (ISS-058). Guests see the chart but none of the
drag, resize, or add affordances, and completed cycles suppress
quick-add. While loading, the layout-level loader covers the fetch
while the chart mounts with data; an empty project shows the first-run
state with no chart, and the load-more sentinel renders as a pulsing
placeholder (ISS-059). Oracle: `layouts-b-gantt.spec.ts`.

## 7. Per-row quick actions (ISS-060–067)

Every row exposes the same menu twice: a hover trigger and the
right-click context menu (ISS-060). The project-context entries, in
order: edit, duplicate, open in new tab, copy link, move to another
project, archive, and delete. Copying the link toasts a confirmation.
Edit, duplicate, move, archive, and delete require edit permission and
hide without it. Duplicate in this build goes through the create modal
prefilled from the source issue (native duplication is cloud-only).
Oracle: `layouts-a-row-actions.spec.ts`.

Edit opens the same work-item dialog as creation, in edit mode: the
form arrives prefilled from the row and its heading says the form
edits an existing record rather than creating one. The heading's exact
wording is each app's own copy, not a parity property — the rebuild
says "Update work item", the old app says "Update", and the oracle
scenario pins the intent (an edit-mode heading naming an update) so it
passes on both (NEWFRONT-934). The same single heading serves every
entry point that opens the edit form — the row menu, the detail and
peek panels, the relations widget's row edit, and drafts; only the
intake accept form carries its own heading naming the specific action
it performs (the editTitle override, INT-020).

The archive entry is gated on the issue's state group (ISS-061):
unless the issue sits in a completed or cancelled state the entry
renders disabled with an explanatory note that only finished work
items can be archived. In a cycle's list the menu gains a remove-from-
cycle entry that deletes the cycle bridge (`DELETE
BASE/cycles/{cid}/cycle-issues/{bridgeId}/`); editing from that
context pre-fills the current cycle (ISS-062). A module's list
likewise gains remove-from-module through the module bridge delete,
with edit pre-filling the module (ISS-063). Oracle:
`layouts-a-row-actions.spec.ts`.

The archived list offers only restore, open in new tab, copy link, and
delete — no edit, archive, or move (ISS-064). Restore deletes the
archive record (`DELETE BASE/{issues|epics}/{id}/archive/`), toasts,
and returns the issue to the project list. The oracle pins
NEWFRONT-165 (the read-only wiring hiding the delete entry, so only
three entries render); intended, the delete entry is present. The
workspace all-issues list offers the full project-style menu (ISS-065).
Oracle: `layouts-a-row-actions.spec.ts`.

Detail and peek headers offer duplicate, open in new tab, move,
archive, restore, and delete (ISS-066); peek hides the edit and
copy-link entries because the header already carries its own copy-link
button. The whole-list ellipsis menu (ISS-067) offers copying the list
URL plus opening it in a new tab, with cloud-provided entries appended
in the cloud edition. Oracle: `layouts-a-row-actions.spec.ts`.

## 8. Empty states (ISS-068–074)

Project list (ISS-068): when filters match nothing the page offers
clearing all filters; with no filters it prompts creating the first
work item. Guests see the same copy with disabled actions. Cycle list
(ISS-069): a completed cycle shows an informational message with no
actions; a filtered-empty list offers clearing filters; otherwise the
page offers creating a work item and attaching an existing one. Module
list (ISS-070): filtered-empty offers clearing filters; otherwise
create plus attach-existing. Oracle: `layouts-a-empty-states.spec.ts`.

Archived list (ISS-071): filtered-empty offers clearing filters;
otherwise the page notes there is nothing archived yet and links to
the project's automation settings rather than to creation. Saved
project view (ISS-072): a fixed message plus a new-work-item entry.
Workspace all-issues (ISS-073): with zero projects the page offers
starting the first project; otherwise it notes there are no views yet
and offers adding a work item. Profile tabs — assigned, created,
subscribed, activity (ISS-074): fixed informational copy per tab with
no action; an unknown tab renders nothing. Oracle:
`layouts-a-empty-states.spec.ts`.

Filtered-empty on every list keys off the stored rich filter
expression (the `rich_filters` user-properties value); the legacy
filters object alone does not drive the card. A stored expression
filters the rows and raises the filter pill, but on project, module,
and archived lists the fresh card stays until a condition is added
through the filter-row UI — only then does the no-results card render.
Clearing restores the unfiltered list. The archived page loads filters
from window-local storage only, never the server (NEWFRONT-167, pinned
by the oracle).

## 9. Filters and display options (ISS-075–107)

Every issues surface pairs two independent systems: a display and
arrangement control (grouping, ordering, which fields show) stored per
user per entity on the server, and a row filter control (which work
items match) in two generations — the newer condition-row builder on
all main issues headers, and the older per-dimension dropdown family
surviving on the sub-issue widget (see ISS-094 in §22). Both are
personal: changing them never rewrites the URL, and returning to the
page restores the stored set (ISS-106). The only portable filter set
is a saved view. Oracle: `filters-display.spec.ts`.

The header Display popover (ISS-075) is a scrollable panel with
Display Properties, Group by, Sub-group by, Order by, and Options
sections, each collapsible, rendered in that fixed order. Only the
sections enabled for the active layout and page appear: a table-like
layout offers ordering plus the sub-item switch only, while
calendar-style layouts offer almost nothing. Every change persists to
the per-user per-entity record and survives reload; the board and list
regroup without a page navigation. Oracle:
`filters-display.spec.ts`.

Grouping (ISS-076) is a single choice among the dimensions the layout
supports; picking one regroups immediately. On the board layout the
dimension already used for sub-grouping hides here, and cycle/module
options vanish when the project disables those features. Sub-grouping
(ISS-077) is board-only, visible only once a grouping is chosen, and
mirrors the grouping control with the chosen grouping hidden. Ordering
(ISS-078) is a single choice among the layout's sort orders with a
stable newest-first default, hidden where the layout fixes its own
order. The sub-items switch (ISS-079) toggles whether sub-issues
appear inline; it defaults on, the table layout always behaves as
off, and it renders only where the layout enables it. The empty-groups
switch (ISS-080) is purely local: hiding drops zero-item sections
from list and board. Display properties (ISS-081) render as one pill
per field — identifier, people, dates, labels, priority, state, counts
of sub-items, links, and attachments, estimate, cycle, module, and
more — intersected with the layout's allowed set (calendar and gantt
allow barely any); cycle and module pills vanish when the project
disables those features, and epics relabel the sub-issue count as a
work-item count. Oracle: `filters-display.spec.ts`.

The per-dimension filters (ISS-082–093) have two shipped surfaces.
On main issues pages they are rich-filter dimensions: the condition
row's property picker offers State, State Group, Priority, Assignees,
Created by, Mentions, Label, Cycle, Module, Start date and Target date;
the personal work-items page adds Project and Subscriber. Each
dimension's value picker is a search box over an uncapped option list
with check marks — no section title, applied count, collapse, or
five-row cap — while the date dimensions use date-picker value inputs
for a single day or a range. Picking joins the condition to the stored
expression with AND and narrows the list; the expression lives in
user-properties (`rich_filters`), with the workspace record on personal
surfaces. Cycle and Module are hidden when the project disables those
features, and a project picks no Project dimension. The sub-issue
widget's own Filters dropdown is the second surface: it composes the
legacy collapsible sections for State, Priority, Assignees, Start date,
Target date, and issue types only — state group and project are
allowlisted out — each with its own title count, collapse, search, and
five-row cap (priority renders every row in fixed order instead),
applied local to the loaded page. Dimensions: workflow state with group
icons (ISS-082); state group as all seven groups in one picker
(ISS-083); priority as a fixed five with icons (ISS-084); assignee and
creator as member pickers with avatars (ISS-085, ISS-086); mentions as
a member picker over mentioned items (ISS-087); labels with colored
dots (ISS-088); cycle with lifecycle status icons including completed
cycles (ISS-089); module with icons (ISS-090); project with logo or
emoji for cross-project views (ISS-091); start and target dates as
date-picker values on main pages, and as preset relative windows plus
a Custom row opening the date modal in the widget, where custom entries
are detected by the dash in the stored value and cleared by unticking
the custom row, and presets and custom combine (ISS-092); and
subscriber as a catalogue-only dimension on the personal work-items
page, with no oracle scenario behind it (ISS-093). The
collapsible-section chrome (ISS-094) renders on issues surfaces only
inside that widget dropdown; the rebuild's shared section control for
the rich value lists, the display dropdown, and the widget is §22
target behavior, not an oracle property. No page-level legacy Filters
dropdown exists. Oracle: `filters-dimensions.spec.ts`.

On issues surfaces an applied condition is removed through its own
condition pill's remove control (ISS-095); removing one condition
patches the expression immediately and leaves the siblings intact.
No applied-filters chip bar is composed above any issues list. The
Applied*Filters chip components belong to views, cycles, modules,
pages and projects, not issues. Oracle: `filters-rich.spec.ts`.

The condition-row builder header toggle (ISS-096) doubles as the entry
point: with no conditions and nothing unsaved it reads as an add
control opening the field picker; otherwise it shows or hides the
builder row, switching to an accented icon while conditions exist or
the view has unsaved edits. The dev-server oracle pins NEWFRONT-146:
there the toggle is inert in both directions, because the old filter
HOC registers its per-entity instance in a side-effectful memo and an
unmount cleanup deletes it, so StrictMode's simulated unmount empties
the registry for good. The production oracle, which is the default,
has no strict double-effects and the toggle works, so the row states
are proven there with seeded expressions (a non-empty expression
shows the row on load and clearing the last condition hides it) and
the same scenario holds on both builds. Oracle:
`filters-rich.spec.ts`.

Each condition (ISS-097) is a three-part pill: the field (clicking
re-picks it and re-derives operator and value), the operator
(searchable; locked when only one applies), and the value slot, plus a
remove control. Unknown fields (deleted property, lost access) render
as an error pill with a tooltip and remove-only behavior. Conditions
persist via expression change into user-properties or the view patch.
Adding a field (ISS-098) happens through a searchable picker listing
only not-yet-used fields; the new condition joins with AND using that
field's default operator (negated when the default is a negation).
When every field is used the picker shows a single disabled exhaustion
row; a misconfigured field reports an error toast and adds nothing.
Operators come in plain and negated pairs (ISS-099): every enum
property (state, state group, priority, assignees, mentions,
created-by, labels) offers `is` and `is not`, and a negated pick is
stored as the base operator plus a negation flag (`{ not: {
field__operator: value } }`). Date conditions keep both lookup
families — `is` / `is not` for one day and `between` / `not between`
for a range. In the old OSS app the enum controls render locked to
their positive form because `allowNegative` is hard-coded false
(bug: NEWFRONT-149, pinned); the rebuild follows this intended pair.
Oracle: `filters-rich.spec.ts`.

Value slots adapt to the field kind. Searchable single pick
(ISS-100): lazy-loaded options, re-picking the current value clears
it, opens by itself when empty, dash placeholder when unset, selected
icon plus label otherwise. Searchable multi pick (ISS-101): first two
picks as chips with the overflow counted, same empty behavior. Single
date (ISS-102): bounded picker, always set, never clearable. Date
range (ISS-103): both ends required before it commits, visibly warned
while incomplete. Oracle: `filters-rich.spec.ts`.

The row's right side (ISS-104) offers clear-all plus view actions:
clearing drops every condition; saving prefills a create-view dialog
with the current expression and display settings; updating appears
only for the owner of an unlocked, changed view. Creation targets the
workspace or project view collections (`POST WS/views/` or the
project views endpoint); updates patch the view record. Project saving
requires the project's views feature; workspace default views refuse
updates. The seam with the views epic is drawn in §21. Builder
instances (ISS-105) are either persisted (bound to an entity id) or
temporary (ephemeral ids that die with the component); their per-field
configurations register on mount and release on unmount. No chip bar
renders on issues surfaces; removal runs through each condition's own
control (NEWFRONT-148, pinned). Oracle: `filters-rich.spec.ts`.

The issues header offers an analytics entry (ISS-107) that opens a
work-items analytics dialog scoped to the current project, with the
epic flag passed through. It hides from people who cannot create work
items. The seam with the analytics epic is drawn in §21. Oracle:
`filters-display.spec.ts`.

## 10. Multi-select and bulk operations (ISS-108–116)

Selection mechanics are client state. Clicking a row's control
toggles it (ISS-108); shift-click selects the contiguous range from
the last selection in both directions, flattened across groups; a
group header is tri-state and toggles the whole group. Only
same-project items are selectable, with a tooltip explaining the
restriction; selection auto-prunes entities that leave the list on
filter or regroup. Arrow keys move a highlighted active row and scroll
it into view, with Shift plus arrows extending the selection to the
previous or next entity; all of it disables when selection is disabled
(ISS-109). While items are selected, a browser reload or leave
confirmation is armed; confirming clears the selection (ISS-110).
NEWFRONT-129 records an old-app bug: the OSS `apps/web` build hard-disables
bulk operations, so its oracle pins the absence (no selection checkboxes,
arrow keys driving no selection, no leave confirmation, no banner).
`apps/web_new` implements the intended mechanics instead, and its ISS-108–111
scenarios prove selection, keyboard selection, the leave guard and the
banner. Oracle: `bulk-selection.spec.ts`.

In this build a sticky bottom bar appears once a selection is active
(ISS-111): it promotes bulk editing with an upgrade link, since the
functional bulk toolbar is cloud-only. It renders nothing when no
selection is active. The old OSS app never reaches it because selection is
disabled (`bug: NEWFRONT-129`); `apps/web_new` builds the banner as intended.
The cloud toolbar replaces it in the cloud edition. Oracle:
`bulk-selection.spec.ts`.

The cloud bulk toolbar applies one request across the selection.
Bulk update (ISS-112) posts `POST BASE/bulk-operation-issues/` with
the issue ids and a properties object; the bulk-editable set is
exactly state, priority, label ids, assignee ids, start date, target
date, module ids, cycle id, and estimate point. Array-valued
properties are appended and unioned rather than replaced, scalars
replace, and lists regroup after. Bulk delete (ISS-113) removes all
selected issues permanently in one request (`DELETE
BASE/bulk-delete-issues/`) after confirmation. Bulk archive (ISS-114)
archives them all at once (`POST BASE/bulk-archive-issues/`),
recoverable from archives. Bulk subscribe (ISS-115) subscribes the
current user to notifications for all selected issues (`POST
BASE/bulk-subscribe-issues/`). What the OSS build renders for each of
these rows is absence: no toolbar, no bulk entries — the oracle pins
that absence, and §23 defines the cloud proof. Oracle:
`bulk-selection.spec.ts`.

In the timeline, when bulk operations are enabled, the sidebar rows
plus header expose selection checkboxes revealing the bulk toolbar;
they hide otherwise and always for epics (ISS-116). Oracle:
`bulk-selection.spec.ts`.

## 11. Create and edit work-item modal (ISS-117–136)

The modal is the shared work-item creation entry point (see the shell
seam in §21): headers, quick actions, empty states, the palette
command, and sub-issue lists all open this same form, and the issues
slice owns the dialog plus the submit flow. The core form (ISS-117)
carries a project selector, a title, a rich description, and a strip
of state, runs-on target, priority, assignees, labels, start and due
dates, cycle (when the project enables cycles), modules (when
enabled), estimate (when enabled), and parent. The title auto-focuses.
Required means project plus title (unless AI title generation fills
it); the modal refuses to render when the user has no create-
permission project. On submit it posts `POST BASE/{issues|epics}/`,
shows a success toast with action items, then closes — or, with
Create-more on, resets. Description assets finalize after the create,
and cycle/module membership is reconciled after the create when the
payload differs from the context. Oracle: `create-modal.spec.ts`.

The title (ISS-118) is required (non-whitespace) and capped at 255
characters with inline errors. When an assistant is configured and
this is a brand-new item, a blank title triggers AI generation from
the description (needing a non-empty description, with timeout and
empty-result handling); edits and drafts always keep the title
required. Title generation posts the description to the assistant
title endpoint. The Create-more toggle (ISS-119) lives in the footer:
when on, a successful create keeps the modal open, resets it (keeping
project, type, the route's cycle/module context, and the description
default), and re-focuses the title; when off, the modal closes. It hides when editing. Oracle:
`create-modal.spec.ts`.

Closing or escaping a new item with unsaved changes offers saving a
draft (ISS-120): a dialog offering discard, cancel, or save-to-drafts.
Changes are sanitized first (empty values, empty description, and the
project id stripped; cycle and module ids seeded from the route are
context, so an untouched form opened from a cycle or module page closes
silently too), and if nothing meaningful remains the modal just closes. Saving posts `POST WS/draft-issues/` with an untitled
fallback. Editing never prompts. The seam with the drafts epic is
drawn in §21. In the cloud edition, typing debounces a duplicate
search (ISS-121); matches show a count button opening a side-by-side
duplicate panel. In this build the hook returns none, so the button
and panel are effectively absent; §23 defines the cloud proof.
Oracle: `create-modal.spec.ts`.

Parent handling (ISS-122) runs through an add-parent chip opening a
project-scoped search, debounced at half a second, excluding the item
itself (or epics when searching epics) via `GET BASE/search-issues/`
with the parent flag. Selecting sets the parent id and shows a parent
tag with state dot, identifier, name, and a remove control; each
result carries an open-in-new-tab link. The project selector (ISS-123)
lists only projects the user can create in (or an explicit allow
list); changing the project resets dependent fields, clears an applied
template plus the editor, refetches cycles, and recomputes the default
type. It disables when editing or moving. Oracle: `modal-actions.spec.ts`,
`create-modal.spec.ts`.

Opening the modal from a cycle or module page pre-seeds the new
issue's cycle and module ids from the route (ISS-124). On save, when
the payload differs from the context, the issue is created first and
then explicitly added to the cycle or module; edits reconcile
diff-wise with the add and remove calls run sequentially to avoid
store races. Enter in the editor submits; Escape and the Discard
button close only when the editor reports ready-to-discard, otherwise
the close is refused with an error toast (ISS-125). Enter in the title
submits; Enter in the description belongs to the editor and starts a
new line, leaving the form open. Fields carry an explicit tab order.
Oracle: `create-modal.spec.ts`.

Editing an existing work item (ISS-126) sends only changed fields
(plus identity, project, type, and description) via `PATCH
BASE/{issues|epics}/{id}/`, with drafts patching `WS/draft-issues/
{id}/` and cycle/module changes going through their add/remove calls.
Success toasts, optionally with view and copy actions; Create-more
and the discard-draft prompt stay suppressed; the existing description
is fetched on open. The description editor (ISS-127) is a full rich-
text surface with mention lookup through workspace search and
image/file upload whose assets finalize on save (the entity type
differs for drafts versus issues). When AI is configured and a title
exists, an auto-generate entry drafts a description from the title
(with empty-title and monthly rate-limit handling), and a GPT popover
inserts responses at the cursor. Oracle: `modal-actions.spec.ts`,
`create-modal.spec.ts`.

The Advanced disclosure (ISS-128) holds an optional git work-branch
field, collapsed by default (auto-expanding when a branch already
exists), sent as the branch field on create and update. It validates
a 128-character cap plus an allowed charset; empty means a new branch
will be created. Intended, a saved branch round-trips into the edit
modal; the oracle pins NEWFRONT-140 (the server stores the value but
no read returns it, so the edit modal always shows empty). Slices
build the round-trip against the fixed read. The cloud-only trio
(ISS-129) — issue type picker driving type-specific custom properties,
template picker prefilling the form plus the editor, and a validated
additional-properties block — is absent in this build (stubs); §23
defines the cloud proof. Oracle: `create-modal.spec.ts`.

A draft in the modal publishes through a publish entry (ISS-130):
it requires a project (else an error toast), persists additional
property values, then moves the draft into the project via `POST
WS/draft-to-issue/{id}/` with a success toast. The create success
toast (ISS-131) carries a view link (new tab) and a copy-link action
that copies the URL and confirms briefly; draft creation shows no
such toast. Oracle: `create-modal.spec.ts`.

The modal label picker (ISS-132) is a searchable multi-select of
project labels grouped by parent and child, lazily fetched. Admins
can type a new name plus Enter to create (random color) and select it,
with deduplication and a duplicate-name error; non-admins see an
empty-result note instead of a create affordance. Hovering a work item
anywhere it appears as a reference (relations, links, calendar, and
the like) shows a preview card (ISS-133): identifier, state with group
icon plus name, title, priority icon, and a start/due line (a range
when both are set, overdue highlighted, no date row when neither is
set). Oracle: `modal-actions.spec.ts`.

Archiving runs through a centered confirmation (ISS-134) naming the
issue and noting archives are recoverable; confirming posts `POST
BASE/{issues|epics}/{id}/archive/`, toasts, and closes (restore
deletes the same record). Deleting runs through an alert (ISS-135)
warning the deletion is permanent and naming the item with the right
noun for its kind. Before submit it checks creator-or-admin standing:
unauthorized attempts get a permission toast and no delete, and server
permission errors map to the same message. Moving (ISS-136) lists
joined same-workspace projects excluding the current one, searchable,
through `POST BASE/work-items/{id}/move/`; cross-workspace moves are
unsupported. Selecting moves the issue, toasts, and redirects to the
new URL since the identifier changes; with no other projects an empty
note renders instead. Oracle: `modal-actions.spec.ts`.

## 12. Draft work items (ISS-137–141)

Drafts live at the workspace drafts route and are workspace-scoped,
never project list items (ISS-137). The list fetches `GET
WS/draft-issues/` cursor-paginated at fifty per page, without
refetch-on-focus; a load-more row with a loader paginates, and each
draft renders as a block. The seam with the drafts epic is drawn in
§21: the issues slice builds this page end to end. Oracle:
`drafts.spec.ts`.

A draft block (ISS-138) shows a type identifier, the project
identifier (click-to-copy), the name, quick actions, and inline
state, priority, labels, start and due dates (overdue flagged), and
assignees, plus modules, cycles, and estimates conditional on the
project features; edits patch `PATCH WS/draft-issues/{id}/`.
Double-clicking a block or its quick-action menu edits through the
modal pre-filled from the draft store (ISS-139); copying opens a
duplicated draft payload; moving to the project promotes the draft
into a project issue via `POST WS/draft-to-issue/{id}/` and adjusts
the count. Deleting (ISS-140) asks a confirm alert warning the delete
cannot be undone; only the draft creator or a project admin may
delete (the server enforces too), success toasts, and permission
errors read distinctly from generic failures. With no drafts the page
invites creating the first one, disabled unless the viewer is a
workspace member or better; with zero projects it instead prompts
starting a first project (ISS-141). Oracle: `drafts.spec.ts`.

## 13. Issue detail (ISS-142–164)

Opening a work item full-page (ISS-142) runs through the browse
route `/{ws}/browse/{IDENT-seq}` in two phases: a lite fetch by
identifier (`WS/work-items/{IDENT-seq}/`) resolving id and project,
then a full hydrate (`GET BASE/issues/{id}/`). A skeleton shows while
loading; the page title combines identifier and name; intake items
redirect to the project intake tab; the sidebar auto-collapses under
768 pixels. The legacy short link (ISS-143) at
`/{ws}/projects/{p}/issues/{id}` is a loader only: it fetches `GET
BASE/issues/{id}/meta/` and redirects to the browse route, showing a
spinner while resolving. Missing, deleted, or no-access targets land
on a generic empty state linking to all-issues, with no distinct
forbidden versus not-found rendering. Oracle:
`issue-detail-core.spec.ts`.

The detail root (ISS-144) pairs main content on the left with a
collapsible properties sidebar on the right. A missing issue id
renders an empty state linking to other work items; the sidebar
honors a global collapse flag and shows a skeleton while metadata
hydrates; under 768 pixels the sidebar content renders inline under
the main content. Oracle: `issue-detail-core.spec.ts`.

The title (ISS-145) is an inline autosaving field: it saves about a
second and a half after typing stops, on blur (trimmed), and on
unmount, via `PATCH BASE/issues/{id}/` carrying the name.
Empty or whitespace input is rejected and reverts, never sent, with a
required indicator; the 255 cap carries a live counter; GitHub-synced
issues render the field disabled. A save-status indicator (ISS-146)
next to the title reflects the title-plus-description submit
lifecycle, moving from a saving state to a saved state that fades
out; navigation while a save is in flight raises a leave warning. The
description (ISS-147) is a rich-text editor below the title autosaving
through the same status pipeline; synced issues never patch. Version
history lists prior versions with timestamps and authors and restores
chosen HTML back through the update (disabled for archived and synced
items); reads use the description-versions endpoints. In the cloud
edition a debounced duplicate detector surfaces a de-dupe popover as
you type (see ISS-231 in §20). Oracle:
`issue-detail-core.spec.ts`.

The sidebar rows each patch one property and render read-only for
guests. State (ISS-148) updates immediately through the issue patch
with the state id, and the overdue highlight follows the state group.
Assignees (ISS-149) multi-selects project members with avatars once
more than one is set, showing an add placeholder when empty. The
runs-on row (ISS-150) selects where the AI agent runs — cloud agent,
managed or local runner, or a specific pod — constrained by the
project's enabled agent options, with desktop availability rules from
ISS-218 (§17). Priority (ISS-151) updates immediately. Created-by
(ISS-152) shows the creator avatar plus name, never editable, with
intake-bot creators masked under the product name. Start and due
dates (ISS-153) constrain each other (start at or before due, due at
or after start); overdue due dates render flagged by state group;
clearing writes null. The cloud-only overdue badge stays a stub here
(see ISS-236). Estimate (ISS-154) hides entirely unless the project
has estimates enabled; the seam with the estimates epic is drawn in
§21. Modules (ISS-155) hide unless the project module view is on;
selection diffs into add/remove sets through the module bridge
endpoints, with the control disabled mid-request. Cycle (ISS-156)
hides unless the project cycle view is on; changing adds through
`POST BASE/cycles/{cid}/cycle-issues/`, clearing removes through the
bridge delete, unchanged selection no-ops, and promise toasts confirm.
Parent (ISS-157) opens a searchable picker modal, sets the parent id,
then refreshes and fetches the parent's sub-issues; inline remove
clears the parent through the sub-issue removal path. Labels (ISS-159)
combine chips with a searchable multi-select combobox, lazily
fetched; removing a chip patches the label ids, and admins can create
inline by typing a name (random color) with a duplicate-name error,
while non-admins see an empty-result note. Oracle:
`issue-detail-sidebar.spec.ts`.

A parent banner atop the main content (ISS-158) shows the parent
state dot, identifier, and name; clicking navigates to the parent
(epics full-page, otherwise peek). An ellipsis menu lazily lists
siblings excluding the item itself, with loading and empty notes,
each linking to that sibling, plus a remove-parent entry clearing the
parent id. Oracle: `issue-detail-sidebar.spec.ts`.

The detail header carries a copy-link button (ISS-160) copying the
absolute work-item URL with a success toast, and a subscribe toggle
(ISS-161) reading and writing `BASE/issues/{id}/subscribe/` (status
read, post to subscribe, delete to unsubscribe). The copy toast's
exact wording is each app's own copy, not a parity property — the
rebuild says "Copied the work item link.", the old app its own
Link-copied notice, and the oracle scenario pins the intent (a notice
saying the link was copied) so it passes on both (NEWFRONT-945). The toggle toasts on
change, shows a loader until the state is known, hides on archived
items, and disables while in flight. The header ellipsis (ISS-162)
holds the destructive and navigation entries with role gating:
delete redirects to the issues list (archives for archived items)
with a toast, archive redirects with a toast, restore toasts and
navigates to the item. The delete/archive confirmation headings'
exact wording is each app's own copy, not a parity property — the
rebuild says "Delete work item" and "Archive work item IDENT SEQ",
the old app says "Delete Work item" and "Archive Work item IDENT
SEQ", and the oracle scenario pins the intent (a confirmation
heading naming the verb and the record) so it passes on both
(NEWFRONT-944). The identifier (ISS-163) renders the short
code plus sequence, click-to-copy with toast and tooltip, hidden when
the key display property is off; work-item type switching and the
type badge are cloud-only, so this build shows only the copyable id
(see ISS-235). The agent status panel (ISS-164) is restated precisely
in §22; the oracle pins only its absent-without-runs state. Oracle:
`issue-detail-core.spec.ts`.

## 14. Peek panel (ISS-165–172)

Clicking a row or card in any layout opens the work item in the peek
panel (ISS-165), rendered through a portal; the shell owns the panel
shell and the issues slice owns the data fetching (see §21). The
panel fetches the full item (`GET BASE/{issues|epics}/{id}/`) with a
skeleton while loading; editability is computed per role; archived
items render read-only; nested sub-issue peeks track a nesting level.
The oracle pins NEWFRONT-139 for the nested path (unreachable on the
dev stack where widget rows never render; asserted on the production
stack); slices build the nested peek per the main#533 target in §22.
Oracle: `issue-detail-peek.spec.ts`.

The header layout toggle offers three display modes (ISS-166):
side-peek docked right (default), centered modal, and full-screen
portal. Side and modal modes use a single-column layout; full-screen
uses two columns. The mode is component-local state — never persisted
across reopen or reload and never in the URL. URL sync (ISS-167)
two-way binds the peeked issue id, the peeked project id (omitted
when it equals the route project), and the nesting level; a URL
carrying a peeked issue id auto-opens the peek. Store-to-URL writes
use history replacement, so browser Back goes to the previous page
rather than closing the peek. Oracle:
`issue-detail-peek.spec.ts`.

Closing (ISS-168) clears the peek and strips the URL param. Escape
closes only when no modal, active input, or editor is open, then
refocuses the originating list row; outside-click closes only when
the peek is not embedded and nothing else is open. Embed mode
disables both. The peek header actions (ISS-169) mirror detail:
copy-link to clipboard with a toast, open-full-screen (which clears
the peek then defers navigation to the full page; modified clicks
open a new tab without tearing down the peek; the link stays disabled
until the work-item link resolves), subscribe, and the quick-action
entries. Delete, archive, and restore close the peek appropriately,
and opening a quick-action modal suppresses Escape and outside-click
closing. While fetching, a full-height skeleton mimics header, title,
description, sub-issues, attachments, and the property grid, with
only the Close button interactive (ISS-170). When the fetch throws,
a generic empty state renders with only a Close action — no retry and
no auto-redirect — collapsing every failure (not found, forbidden,
network) into the one message (ISS-171). The peek Properties section
(ISS-172) carries the same property set as the full sidebar in
peek-view form, with intake-bot creators masked, due dates bounded by
start and overdue-highlighted, and each mutation refetching activity;
it is also reused inline on the mobile full-page detail. Oracle:
`issue-detail-peek.spec.ts`.

## 15. Detail widgets (ISS-173–193, ISS-238)

The widget root (ISS-173) opens with an action row — add
sub-work-item, add relation, add link, attach, plus Run AI, which
always shows — followed by collapsibles in a fixed order:
Sub-work-items, Relations, Links, Attachments. Each collapsible
renders only when it has content and carries a count; open/closed
state persists per widget. Guests see disabled add buttons and no
section add entries. The oracle pins NEWFRONT-139 across the
sub-issue widgets (on the dev stack the rows never render through a
StrictMode double-toggle while header counts stay correct; asserted
on the production stack); slices build the intended rendering per the
main#533 target in §22. Oracle:
`issue-detail-widgets-subissues.spec.ts`.

The sub-issues title (ISS-174) shows a circular progress gauge plus a
done-over-total count computed from the state distribution, with the
total taken from the paginated total count rather than the loaded
page length; for epics the widget reads as work items. Each row shows
identifier, name, inline properties, and a chevron lazily expanding
its own children recursively. The rollup covers the full child set
and stays identical on every page (target per main#496; restated in
§22). Creating (ISS-175) offers two entries: create-new opens the
create modal with the project locked and the parent preset, while
add-existing opens a project-scoped multi-select modal (no workspace
toggle) posting the chosen ids to the sub-issues endpoint; both toast
on success and failure. Removing (ISS-176) unlinks immediately with
no confirm, patching the child parent id to null and dropping the
row; deleting requires the confirm modal and then destroys the child.
Row labels differ for epics, and the row shows a loading state while
either runs. Inline per-row edits (ISS-177) patch one field per
control — state, priority, dates (the merged range only when both
ends are set and both display properties are on), assignees — with
overdue highlighting; visible columns follow the section display
properties; clicking the row opens the child in peek. The title
actions (ISS-178) hold a filters dropdown (priority, state group,
state, project, type, assignees, start/due) and display filters
(group-by, order-by, display properties); grouped rendering carries
counts, an empty filtered result offers clearing filters, and the
state is per parent. Pagination (ISS-238) loads at most fifty
children on the first page; a load-more control fetches the next
cursor page per parent (nested parents keep their own cursor) with a
loading row while fetching, hiding when exhausted; small parents load
in one request with no control. The endpoint returns the page under
the sub-issues key with the full-set state distribution plus total
count and cursors alongside (target per main#496; restated in §22).
Oracle: `issue-detail-widgets-subissues.spec.ts`,
`issue-detail-widgets-subissue-rows.spec.ts`.

Implementation note (NEWFRONT-647): the slice built the widget root and
the sub-work-items section. The action row renders all five entries and
only the sub-work-item one is live; relation, link, attachment and Run
AI stay disabled until their owning slices (NEWFRONT-649, NEWFRONT-650,
the run-AI slice) mount their entries through the same row. Sections
after the first arrive through a slot so the fixed order holds. The
filters dropdown offers priority, state group, state, project, assignees
and both dates; the project section appears only when a loaded child
sits in another project, and the issue-type section is the ISS-234 stub
the old OSS build renders — nothing — because types are cloud-only and
owned by NEWFRONT-657; the cloud slice wires the same section in. The
gauge and count read the first page's full-set distribution and total
and draw for both parent kinds, with only the label switching for epics;
while grouping, ordering and filtering run over the loaded pages, and
load-more carries its own cursor per parent (nested parents fetch their
own pages). Clicking a row opens the child at its browse key; the
in-place edit mode belongs to NEWFRONT-631 and peek to NEWFRONT-646.
The dev-only StrictMode double-toggle (NEWFRONT-139) is not reproduced:
the widget renders the intended rows.

Implementation note (NEWFRONT-647, review pass): Epic parents read and attach their children through
`epics/<id>/issues/` (the issues collection stays
`issues/<id>/sub-issues/`), which is what the old issue service swaps and
what the epics contracts mirror; the cache key stays one entry per
parent. The custom date half stores a bare day for "On" and the matcher
reads a bare day as exactly that day, while `;after` stays the lower
bound and `;before` the upper one on either dimension. The header and
the add menu draw for a childless parent too, and the create,
add-existing and delete dialogs stay mounted whether the section is
open, closed, empty or still loading, so the first child of a new issue
can be added. The read runs whether or not the section is open: the
collapsed title needs its counts and a remounted parent must render its
toggle. Rows resolve each child's own project code for the identifier
and link (a cross-project child is not the parent's key) and draw
assignees from the project-member directory, falling back to a neutral
"Member" for an unknown id. The row menu offers Copy link to everyone
and remove/delete to members; the full edit modal belongs to
NEWFRONT-631 and is not offered here, and open-in-new-tab is not in the
old menu.

Relations group by type (ISS-179), each a default-open inner
collapsible with a colored header; only present types with at least
one item show, and the widget count sums all types. Each row shows an
identifier badge, the name, and inline state, priority, and assignees;
clicking opens peek (epics open in a new tab). Adding (ISS-180) picks
one of the four relation types, then opens the shared existing-issues
modal with a workspace-level search toggle; multiple items can be
added at once through `POST BASE/issues/{id}/issue-relation/` with
the type and items, while zero selection toasts an error. Edges stay
reciprocal (ISS-181): adding records the reverse on the other side
(blocking both ways, duplicate and relates-to symmetric), and
removing removes the reverse too, optimistically with rollback on
failure. Row actions (ISS-182) remove (detaching both ways through
`POST BASE/issues/{id}/remove-relation/` with the type and related
issue, then refetching activity), delete (unrelating then destroying
the related item through the confirm modal), edit (opening the
create/update or epic modal), and copy-link. The four types (ISS-183)
each carry their own icon, color, placeholder, and activity strings,
with the blocked direction styled danger, the blocking direction
styled warning, and relates-to plus duplicate neutral. Oracle:
`issue-detail-widgets-relations.spec.ts`.

Links (ISS-184) count from the link count; each shows a URL-derived
favicon, the title falling back to the URL, and an added-by line with
relative time and author (bots labelled). Clicking a link card copies
its URL with a toast. One modal serves add and edit (ISS-185): a
required URL auto-prefixed when the scheme is missing (malformed
strings are otherwise accepted) plus an optional title, posting to
the links collection or patching the link id, with toasts. Opening
(ISS-186) launches the URL in a new tab; removing deletes immediately
with no confirm and a toast; row controls hide without permission.
Oracle: `issue-detail-widgets-links.spec.ts`.

Attachments (ISS-187) list with a file-type icon, filename, human
size, uploader avatar with an uploaded-on tooltip, and (permitting) a
delete menu per row; clicking opens the file in a new tab, and an
alternate card-grid presentation also exists. Uploads (ISS-188) run
from the section add entry and from a dropzone covering the list in a
three-step signed flow: post the metadata, put the bytes to the
signed URL, then patch the attachment record complete. Only one file
per upload is accepted; oversize files are rejected; a promise toast
plus an in-progress row with debounced live percent track the
upload; activities refetch after; no file-type restriction applies.
Opening (ISS-189) uses the served asset URL in a new browser tab —
there is no in-app viewer. Deleting (ISS-190) asks a confirmation
alert naming the file and warning permanence, then deletes with a
toast. The size limit (ISS-191) comes from the instance config with a
built-in fallback; the dropzone enforces it and echoes it in rejection
toasts. Oracle:
`issue-detail-widgets-attachments.spec.ts`.

Run AI (ISS-192) dispatches an extra agent run through `POST
/api/runners/runs/` with the workspace, work item, and the run-ai
trigger; the prompt is server-rendered from the issue's phase
template with no client prompt body. When a run is already active the
response reports queued (info toast), else success; the issue is
refetched afterward so the status card updates. Comment-and-run
(ISS-193) sits in the comment composer toolbar: it posts the composer
comment first and only dispatches a continuation run when a comment
was actually posted (the comment becomes the server-side prompt),
with the same queued/started toasts; an empty comment short-circuits
the run. Oracle: `issue-detail-widgets-runai.spec.ts`.

## 16. Comments, activity, and reactions (ISS-194–206)

The Activity section merges activities and comments into one timeline
ordered by creation time (ISS-194). Each row shows the actor, a
paraphrased sentence, and a relative timestamp with a full-date
tooltip; every field's activity type renders (name, state, assignees,
priority, estimate, parent, relations, dates, cycles, modules,
labels, link, attachment, archived, agent wait/wake attributed to the
product, intake verbs, and more). Reads use `GET
BASE/issues/{id}/history/` for activities with comments from the
comment store. A three-row skeleton shows while loading; an empty
feed renders nothing. The seam with the comments epic is drawn in
§21: the issues slice builds this feed end to end. Oracle:
`activity-feed.spec.ts`.

The Activity header filter popover (ISS-195) holds four toggles —
updates, comments, state, assignee — all on by default and persisted
per browser; deselecting the last remaining toggle is a no-op, and an
accent dot marks the button while not all are selected. Creation and
deletion entries always show. The sort control (ISS-196) flips
ascending/descending by creation time, persisted per browser with
ascending default; in descending mode the composer renders above the
feed, in ascending below. Oracle: `activity-feed.spec.ts`.

The composer (ISS-197) submits on Enter (modifier-plus-Enter inserts
a newline) when non-empty, posting `POST
BASE/issues/{id}/comments/`; success toasts, resets the editor, and
confirms assets, with double-submit guarded. It supports file and
image upload plus an internal/external access specifier defaulting to
internal; the composer hides when disabled or archived. Editing
(ISS-198) is author-only — even admins cannot edit others' comments —
through an inline pre-filled editor with save (check or Enter, empty
blocked) and cancel restoring; edited comments carry an edited
marker, patching `BASE/issues/{id}/comments/{cid}/`. Deleting
(ISS-199) is likewise author-only, removes the comment with a toast,
and asks no confirmation. Oracle: `comment-crud.spec.ts`.

Access toggling (ISS-200) flips internal versus external on the
comment, with the icon shown when the access specifier applies; it
appears only for published/anchored projects and is not author-gated.
The seam with the public-boards epic is drawn in §21. Folding
(ISS-201) persists in the comment's labels; folded comments hide body
plus reactions behind an expand toggle, used mainly to fold noisy
agent comments; not author-gated. Copying a comment link (ISS-202)
copies a deep link with the comment hash plus a toast; opening such a
URL scrolls to and highlights that comment for several seconds.
Available to anyone on non-intake issues. Oracle:
`comment-card-actions.spec.ts`.

Reactions on comments (ISS-203) toggle: clicking an existing reaction
removes it. Each shows a count, highlights when the current user
reacted, and tooltips reactor names; the footer renders only when
reactions exist, and folded comments hide reactions. Endpoints post
to and delete from the comment reactions collection. Reactions on the
issue itself (ISS-204) follow the same toggle model on the issue
reaction row, with the add button always shown when not disabled, and
the row disables on archived items. Oracle: `reactions.spec.ts`.

Code-review links (ISS-205) live in a collapsible above Activity that
stays hidden until at least one review is linked. Each shows a status
badge (merged, draft, open, closed) plus a title, opens in a new tab,
and detaches on hover; attaching pastes a PR/MR URL (trimmed, cleared
on success, server errors surfaced). Reads and writes use the
code-reviews collection under the issue. The worklog (ISS-206) is
cloud-only: in this build every worklog surface renders empty — no
button, rows, or summary, not even a call to action — and the activity
filter keeps no worklog category; in cloud,
eligible members log time against an issue and it appears in the feed
and sidebar. §23 defines the cloud proof. Oracle:
`code-reviews.spec.ts`, `worklog-absent.spec.ts`.

## 17. Shared property dropdowns and read-only components (ISS-207–220)

Every property dropdown shares one behavior family (ISS-207) across
rows, detail, peek, and spreadsheet: several button variants plus an
icon-only form and custom-node triggers; keyboard support where Enter
toggles, Escape closes, Tab closes, and arrows navigate; the search
input auto-focuses on open except on mobile, where Escape inside it
clears the query; outside-click closes; disabled state shows a
not-allowed cursor and suppresses clearing; empty and loading states
render distinctly. Each dropdown lazily fetches its options on open.
Oracle: `dropdown-common.spec.ts`.

The member picker (ISS-208) types to search over project members
(falling back to workspace members without a project scope), in
single or multi mode with multi staying open across picks. Avatars
and avatar groups render; the current user reads as you; sorting puts
the current user then selected members first. Suspended users show a
suspended marker and are not selectable. The state picker (ISS-209)
single-selects over project states with search, group icon plus name,
and a spinner while initializing; there is no clear row since a state
is always set, and the workflow-aware option is a no-op in this
build (see ISS-233). The priority picker (ISS-210) single-selects the
fixed five over a static enum with search and per-level icons; the
empty level is a real value rather than a separate clear. The project
picker (ISS-211) searches joined projects by name with logo plus name
in single or multi mode, selected-first sorting, and an excludable
current project; the oracle notes all live callers are single-select
with no exclusion, so multi plus exclusion stay unexercised here.
Oracle: `member-dropdown.spec.ts`, `state-dropdown.spec.ts`,
`priority-dropdown.spec.ts`, `project-dropdown.spec.ts`.

The cycle picker (ISS-212) single-selects project cycles with search,
group icon plus name, prepending a no-cycle clear row when allowed;
completed cycles and the current cycle filter out, and there is no
inline cycle creation. The seam with the cycles epic is drawn in §21.
The module picker (ISS-213) searches project modules in single mode
(with a no-module row) or multi mode (removable chips or a counted
label), with icon plus name and selected-first sorting; no inline
module creation. The seam with the modules epic is drawn in §21. The
date picker (ISS-214) is a single-date calendar, clearable to null,
with min/max disabling out-of-range days so start stays at or before
due in both directions; month/year caption, week-start from the
profile, portal rendering. The range picker (ISS-215) renders merged
(one smart label) or split (two spans joined by an arrow) modes, with
min/max, optional portal, and clearable behavior (default off)
resetting both ends. Intended, clearing a merged cell clears both
ends in one update; the oracle pins NEWFRONT-150 (the clear firing
two concurrent single-field patches of which exactly one survives a
server lost-update). Slices build the single-update clear. Oracle:
`cycle-dropdown.spec.ts`, `module-dropdown.spec.ts`,
`date-dropdown.spec.ts`, `date-range.spec.ts`.

The estimate picker (ISS-216) single-selects project estimates with
search, prepending a no-estimate row, with icon plus point value;
time-system estimates render as hours and minutes. With no active
estimate system a single non-interactive empty row renders; the
project needs an explicit estimate patch before the row appears. The
seam with the estimates epic is drawn in §21. The intake-state picker
(ISS-217) single-selects project intake states with search and an
intake-group icon defaulting to triage, a spinner while initializing,
no clear row, and fallback to the first state; the oracle notes
exactly one intake state per project. The seam with the intake epic
is drawn in §21. The execution-target picker (ISS-218) searches pods
plus cloud and managed agent rows in a single-select; on create it
auto-selects the project default pod, and the server auto-pins fresh
issues to the project default pod while one exists, so no unpinned
state shows. Cloud and managed rows appear only when offered and
disable with a human-readable reason when unavailable (missing LLM
config, desktop BYOK limits, disabled managed runner, disconnected
desktop); the default pod carries a default tag, and the label reads
as runs-on when a cloud agent is offered. Desktop specifics are drawn
in §21. The layout picker (ISS-219) selects a layout type through a
search-disabled dropdown with icon plus label and a checkmark on the
selected entry; an exclusion list can hide options, though the oracle
notes the only caller never passes it, so all layouts are always
offered. Oracle: `estimate-dropdown.spec.ts`, `intake-state.spec.ts`,
`pod-dropdown.spec.ts`, `layout-dropdown.spec.ts`.

Read-only property views (ISS-220) show a non-interactive value with
an icon and an empty fallback across state, members, labels (count
plus a tooltip of names), date, estimate, cycle, module, and
priority. The oracle notes the dedicated read-only components are
dead code with zero usages: archived issues render the normal sidebar
with all dropdowns disabled, except the Labels row which renders with
no trigger at all. Slices build the observed pattern — disabled
normal dropdowns, triggerless labels — rather than the dead
components. Oracle: `readonly-archived.spec.ts`.

## 18. Cross-cutting behavior (ISS-221–225)

There is no socket, event stream, or poll in the issue layer
(ISS-221): another user's new issue or edit never appears without a
reload or refetch. The only refresh-without-reload is self-initiated
after a mutation (parent stats plus the activity feed) alongside
in-memory optimistic list and count updates; the agent-status
panel's fifteen-second poll (ISS-164) is the one exception. Oracle:
`cross-cutting-realtime.spec.ts`.

Permissions (ISS-222): project admins and members can create, edit,
quick-add, inline-edit, drag, and bulk-operate; guests and lower are
read-only — no quick-add, no inline edit, no drag, hidden or disabled
add controls, read-only property components. Label creation is
admin-only; deletion is creator-or-admin; comment edit and delete are
author-only; archived and GitHub-synced items force read-only. The
oracle notes the list quick-add trigger is absent for guests and that
fresh members land on onboarding in-browser. Oracle:
`cross-cutting-permissions.spec.ts`.

The workspace all-issues view (ISS-223) reads arbitrary query params
from the URL into its route filters on load — a deep-link path
distinct from the per-user persisted filters of ISS-106. The oracle
notes the route filters are write-only in this build: the URL keeps
its params but the list ignores them (plausibly an edition seam).
Slices keep the params-reading contract so the deep link works once a
consumer reads them. Cycle transfer (ISS-224) moves remaining issues
to another cycle through `POST
BASE/cycles/{cid}/transfer-issues/` with the new cycle id, then
refetches. The oracle notes the prompt renders only for completed
cycles (status is date-driven) despite active-cycle wording, and that
transfer freezes source progress into its snapshot, so the source
keeps its counts by design and the button stays up. Slices build the
observed completed-cycle prompt with the snapshot semantics. Oracle:
`global-view-url-params.spec.ts`, `cycle-transfer.spec.ts`.

Every inline and detail property edit applies optimistically
(ISS-225): the change lands in the in-memory list or map and
re-buckets or re-sorts immediately, covering property edits,
cycle/module changes, relation add/remove, date drags, and
subscription toggles. When the server rejects, the change visibly
snaps back to the prior value. Oracle:
`optimistic-rollback.spec.ts`.

## 19. Label management (ISS-226–230)

Labels are managed in project settings and drive every issue label
picker. Creating (ISS-226, admin-only) uses an inline form with a
color-swatch picker over a preset palette (new labels seed a random
color) and a required name capped at 255 with inline errors,
posting `POST BASE/issue-labels/`; duplicate names error; fresh
labels take large incremental sort orders and immediately reach all
pickers. Editing (ISS-227) reuses the same inline form in update
mode, pre-filled, patching the label id with an optimistic store
update plus rollback on failure; renames land on every issue carrying
the label, and a missing label on update toasts a fixed error.
Deleting (ISS-228) warns that the label leaves all work items and
views filtering on it, then deletes on confirm with errors toasted;
deletion cascades the label off issues. Oracle:
`label-settings-crud.spec.ts`.

Grouping (ISS-229) builds a two-level tree by drag: dropping onto
another label nests it as a child, dropping above or below reorders
with fractional sort orders, patching the parent or sort order on the
label id. Rules prevent self and own-parent drops plus anything
deeper than two levels (group drops force reorder-above, child drops
land as siblings); a remove-from-group entry clears the parent. The
drag wiring mounts only when editable. Loaded-and-empty renders an
empty note plus a first-label entry for admins; a four-row skeleton
shows while either label read is pending (ISS-230). Oracle:
`label-grouping.spec.ts`, `label-empty-loading.spec.ts`.

## 20. Edition-only features (ISS-231–237)

In this build every row below renders nothing — pure no-op stubs, not
even a call to action — and the oracle pins that absence. The cloud
edition fills each with the real behavior; §23 defines what each
affected slice builds for cloud and how the gate proves it. Oracle
for all seven: `edition-stubs.spec.ts`.

De-dupe (ISS-231): no duplicate UI in the create modal or issue
detail here; cloud provides similar-issue detection while typing in
the create modal (ISS-121) and a description-editor popover (ISS-147).
Epics (ISS-232): the epic create/edit modal is an empty stub here —
epics cannot be created or edited — though the epic branch exists
inertly throughout relations and sub-issue code; epics are a cloud
feature. Workflow (ISS-233): all workflow components and hooks are
no-ops here — no disabled overlays or messages, drops never blocked,
creation never blocked, plain state options — so this build imposes
no transition rules and every state change or drag is allowed; cloud
enforces configurable transitions with disabled overlays.
Team-project and issue-type filters (ISS-234): nothing renders here;
cloud provides a team-project scoping filter and an issue-type filter
plus applied chips. The oracle notes the stubs mount in the sub-issue
filter dropdown, the project-list rich-filters toggle never creates
its filter instance, and the team-project plus applied-type
components have zero callers. Custom types, templates, and additional
properties (ISS-235): the type switcher shows only the copyable
identifier here, with additional-property widgets and type/property
activity rows empty stubs; cloud adds work-item types, per-type
custom properties, templates (ISS-129), and their activity entries.
Cloud-only detail badges (ISS-236): the overdue badge next to the due
date and the transfer-hop info note on the cycle row render nothing
here; cloud shows both. Timeline dependencies (ISS-237): the
dependency layer renders nothing here even though the core chart
accepts a dependency flag and block connection points; cloud draws
dependency arrows between blocks and offers drag handles to link
them. Cloud-only acceptance throughout is described from the seams
and call sites visible here and is re-verified against the running
cloud app at gate time.

## 21. Cross-epic seams

Each seam below is drawn so the issues rows are provable at gate time
without depending on Backlog epics. The rule in every case: the issues
slice builds the issues-side chrome plus the entry point and owns the
mutation endpoints its rows name, while the owning epic registers
content, owns its destination routes, or owns its configuration
surfaces. Gate fixtures seed through the API, never through another
epic's UI.

Shell inputs (binding, from the finished shell spec §10). The shared
peek side panel built by NEWFRONT-280 in the shared kit (SHELL-016)
is reused by the issues peek slice (ISS-165–172) for full peek: the
shell owns the panel shell with its editable title field, and issues
owns issue-data fetching semantics, reading through its own area
hooks and never duplicating them. The shared work-item creation entry
point (SHELL-086/047) routes the sidebar button and the palette
command through one form: issues owns the create/edit dialog plus the
submit flow (ISS-117–136) and registers its creation commands with
the command registry; the shell owns the registry and the entry
points it can prove. Palette contextual edits (SHELL-090/091) call
issues-owned endpoints — work-item update, subscription, labels, and
members — through the contextual actions interface; the palette never
calls entity endpoints directly, and issues owns those contracts.

Comments epic NEWFRONT-72 (ISS-194–204 versus CMT-\*). The issues
slices build the issue-scoped feed end to end: timeline merge,
filter, sort, composer, cards, access toggle, fold, deep links, and
both reaction rows, exactly as §16 states, and the issues gate proves
them with the issues oracle specs. The comments epic owns
cross-surface comment concerns (mentions everywhere, comment rendering
inside notifications and other areas) and any shared primitives it
rows; it reuses the issues comment endpoints and must keep them
compatible. If the comments epic later extracts shared components,
it refactors issues' usage without changing observable behavior; the
issues gate never depends on comments-epic code landing first.

Drafts epic NEWFRONT-73 (ISS-120, ISS-130, ISS-137–141). The issues
slices build the whole workspace-drafts surface: the drafts page with
its blocks, inline properties, quick actions, delete flow, and empty
states, plus the modal save-as-draft confirm and the publish entry,
with the draft endpoints contracted by the issues contracts. The
drafts epic owns drafts for other surfaces (if it rows any) and
reuses these contracts rather than duplicating them. The issues gate
proves every drafts row with seeded drafts and no drafts-epic code.

Views epic NEWFRONT-78 (ISS-104 save/update view). The issues slice
builds the filter-row triggers: clear-all, the save entry prefilling
a create-view dialog with the current expression plus display
filters and properties, and the update entry with its owner/unlocked/
changed visibility rules, including the endpoint calls. The views
epic owns the view record, the view routes and lists, and the
dialog's view-specific chrome. At issues-gate time a minimal
issues-owned save form proves the flow (prefill payload plus the
create/update calls); once views lands, the triggers delegate to the
views dialog without changing the payload contract.

Archives epic NEWFRONT-79 (ISS-061, ISS-064, ISS-071). The issues
slices build the archive mutations with their gating (archive only
from finished states, creator-or-admin delete), the archived-issues
list as a full layout root with its restricted row menu, and the
archived empty states. The archives epic owns archive retention and
automation configuration surfaces. The automation link in the
archived empty state navigates to a route constant the archives epic
owns; at gate time a stub destination proves the navigation.

Intake epic NEWFRONT-80 (ISS-217). The issues slice builds the
intake-state picker with its reads plus the disabled triage-row
rendering, proven at gate with seeded intake states. The intake epic
owns the intake and triage screens, the pending-count semantics, and
all intake actions; the issues rows never navigate into them.

Cycles NEWFRONT-81 and modules NEWFRONT-82 (ISS-012, ISS-036,
ISS-062, ISS-063, ISS-069, ISS-070, ISS-212, ISS-213, ISS-224). The
issues slices build every cycle/module context inside issues
surfaces: group-header create/add-existing, header create menus, row
remove entries, cycle/module empty states, the cycle and module
pickers, the sidebar rows with their bridge calls, and the transfer
prompt with its snapshot semantics. The cycles and modules epics own
the cycle/module pages, planning UX, progress, and snapshots. The
issues gate proves all of it with API-seeded cycles and modules; no
cycle or module UI is needed.

Estimates epic NEWFRONT-84 (ISS-154, ISS-216). The issues slices
build the estimate sidebar row and picker plus the estimate patch,
conditional on the project estimate configuration, proven at gate
with a seeded estimate system. The estimates epic owns estimate
system configuration in project settings. The enabled check reads
project config both epics share.

Analytics epic NEWFRONT-85 (ISS-107). The issues slice builds the
header entry with its create-permission visibility rule plus the
dialog launch scoped to the current project. The analytics epic owns
the dialog contents. The gate proves entry visibility and launch with
stub contents.

Exports epic NEWFRONT-87 (ISS-067 layout-level actions). The issues
slice builds the list ellipsis with copy-link and open-in-tab. The
exports epic registers cloud-provided export entries into that menu
through a documented extension point; the gate proves the base
entries and the extension contract with stub entries.

Desktop specifics (ISS-150, ISS-218). The issues slices build the
runs-on row and the execution-target picker with availability reasons
carried as data from the pod list plus project config. Desktop
capability detection (connected desktop, BYOK support, managed
runner state) is owned by the desktop platform layer and arrives as
flags; the gate proves every reason string with stubbed flags.

Public boards NEWFRONT-99 (ISS-200). The issues slice builds the
comment access toggle for published/anchored projects — endpoint,
icons, and visibility rule — and the gate proves the toggle plus
persistence. The public-boards epic owns the published-board
surfaces that consume the access flag; no boards UI is needed here.

## 22. Target-behavior restatements

ISS-094 — decision: correct the row and build the chrome where it
renders. The NEWFRONT-119 oracle finding stands for main issues
pages: the legacy per-dimension dropdown renders on no main issues
surface there — the condition-row builder plus the Display sections
are the only filter UI, and the condition row's value pickers are a
search box over an uncapped option list, not collapsible sections.
The section chrome has exactly one live surface on issues surfaces in
the old app: the sub-issue widget filter dropdown, which composes the
legacy sections for state, priority, assignees, the two dates, and
issue types. So the row is neither rebuilt as a page-level family nor
retired. The inventory correction (completed by NEWFRONT-148) scopes
the oracle entry point to that widget dropdown and reads the acceptance
as its chrome — title plus chevron collapse, checkbox (multi) or radio
(single) options with optional icon or pulse, applied count, shared
search box, view-all past five (priority renders every row instead),
and empty placeholder. The rebuild target, owned by NEWFRONT-629 with
the widget adoption in NEWFRONT-647, is one shared section control
behind the rich value lists, the display dropdown, and the widget —
target behavior recorded here, not an oracle property.

Implementation note (NEWFRONT-629): the slice built the chrome as the
shared section control under the issues filters and adopted it in the
surfaces that exist today — the condition-row builder's rich value
lists, the display dropdown, and (through NEWFRONT-647) the sub-issue
widget. They read title, applied count, chevron collapse, the search
box, the five-row cap with view all/less, the no-matches placeholder,
and icon and pulse marks from the one control.

One refusal is deliberate rather than a gap: the display dropdown
keeps `aria-pressed` rows over checkbox/radio semantics and does not
cap its fixed, short option sets. Both are recorded here so a later
pass does not read them as the chrome going unused. Every date
section also runs on the same control — title with its applied count
(presets plus a single Custom entry), chevron collapse, the search box
over the relative windows, the five-row cap and the no-matches
placeholder — with the Custom row and calendar below the rows
(ISS-094 now holds for the date value lists too). The control has two
parts: `FilterSection` for pickable rows, and `FilterSectionFrame`
(title bar plus collapsible body) for a section whose body is not a
row list, which is what the display dropdown's property pills and
switches use; those two keep the frame's own collapse rather than a
no-op toggle, and the frame gives them the same title bar as every
other section.

The date dimensions keep both lookup families. "is" / "is not" and
"between" / "not between" are offered, because the JSON filter set
accepts `__exact` and `__range` leaves under `not`; a negated date
leaf is never lifted into the bare parameters, which only read
positive bounds (`<day>;after` is a lower bound, `<day>;before` an
upper one). The custom half keeps the old dialog's three one-day
shapes and its pair: On stores the day exactly (`2026-02-10`), After
and Before store one open bound (`2026-02-10;after`,
`2026-02-10;before`), and Range under "between" stores two plain days
(`a,b`). A stored value reopens in its own shape. Presets and the
custom half combine, and unticking Custom removes only the custom
half.

Encoding follows what the list endpoint can resolve truthfully. A
forward relative window (`<n>_<unit>s;after;fromnow`) and an open
custom bound travel as the bare `start_date` / `target_date` entries,
joined with commas; any leftover custom day or pair stays in the JSON
tree as `__exact` / `__range`, and a backward relative window stays
in the JSON tree because the bare parameter cannot carry its
direction. Completed cycles list with the completed glyph, and a date
section's Custom row opens and focuses the calendar when ticked with
no custom day stored.

Project (ISS-091) is a cross-project dimension: the old issues
filters never offer it inside one project's issue list, so the
catalogue exposes it only on the personal work-items surface, where
it lists every joined project (applied first, then alphabetical, five
rows with view all) and persists `project_id`. Project rows draw the
stored mark in this order: the picked emoji, else the configured
icon, else the short code's initial. Icon names come from the old
icon set; the new frontend maps each name onto its closest Lucide
glyph and keeps the stored color, and a name outside that map falls
back to the initial rather than an empty tile.

The sub-issue widget does not exist in the new frontend yet; its
filter dropdown adopts the same control when that widget is built
(NEWFRONT-647), which is the only outstanding place ISS-094 renders.
ISS-093 is registered the same way: the personal work-items
catalogue offers the `subscriber_id` dimension, and the my-work-items
page that renders the catalogue belongs to the profile pages
(NEWFRONT-573).

ISS-165 carries the same StrictMode note (nested peek unreachable on
the dev stack) and is covered by the main#533 target below.

ISS-164 — agent status panel (target per main#446). The panel renders
only when the issue has an agent run or ticker. It shows a status
title, detail line, and badge for every run plus ticker state, and a
stats grid with run count, executor, next tick (a queued entry reads
as queued rather than a time), tick budget as used-of-max runs, and a
tokens tile. Re-tick renders when allowed; a not-granted reply
surfaces an info toast. Abort renders a confirm modal and drives the
run to cancelled through the cancel-requested state. The tokens tile
shows the issue's cumulative token total abbreviated like the runner
panel: verbatim below one thousand, then one decimal with a
thousands or millions suffix (so mid-five-figures reads in thousands
and near-a-million rounds up to millions). The total folds in live
in-flight usage without double-counting the row's own contribution:
the stored server sum plus only what the live state reports beyond
the active row's figure (the whole live figure when the row is
unwritten, just the delta when stale, nothing once caught up). It
shows zero, never blank, when nothing was ever reported, carries a
tooltip explaining the figure is a floor rather than a billing total
because runs that died before reporting count as nothing, and hides
only when the payload has no token figure at all (neither a stored
total nor live usage). Refresh reads through the issue retrieve
(including the agent-status token total); the panel polls every
fifteen seconds while a run is active plus a scheduled refresh at the
next tick. Endpoints: issue retrieve for refresh, `POST
/api/runners/re-tick/` for re-tick, `POST
/api/runners/runs/{runId}/cancel/` for abort. Oracle:
`issue-detail-core.spec.ts` (today pins only the absent-without-runs
state; the gate aligns the presence scenarios to this target).

ISS-174 — sub-issues view with progress rollup (target per main#496
for paging plus main#533 for StrictMode). The title shows a circular
gauge plus a done-over-total count computed from the state
distribution over the full child set, with the total taken from the
paginated total count rather than the loaded page length; the rollup
stays identical on every page. Each row shows identifier, name,
inline properties, and a chevron lazily expanding its own children
recursively; for epics the widget reads as work items. Rows render on
every stack: the fetch path uses idempotent visibility helpers plus
an in-flight guard so a double-invoked mount effect neither refires
the fetch nor flips the flag back off, while the chevron
expand/collapse path keeps its toggle semantics. Oracle:
`issue-detail-widgets-subissues.spec.ts` (aligned at gate).

ISS-238 — sub-issue pagination (target per main#496). The first page
loads at most fifty children (the endpoint default page size, max one
hundred). A load-more control fetches the next cursor page per parent
— nested parents keep their own cursor — with a loading row while
fetching, hiding when exhausted. Small parents load in one request
with no control. The response carries the page under the sub-issues
key with the full-set state distribution plus total count and cursors
alongside; grouping applies to the returned page only. Local count
sync prefers the paginated total over the loaded list length, and
follow-up pages append through a cursor carried per parent. Oracle:
`issue-detail-widgets-subissues.spec.ts` (aligned at gate).

## 23. Cloud-presence proof

The oracle pinned OSS-observable behavior only for the cloud rows:
each asserts absence or unavailability on the OSS seeded stack. Every
affected slice builds both sides behind the edition flags — the OSS
absence (or prompt) and the cloud behavior — and the area gate proves
both. Proof comes in two shapes. Extended scenarios reuse the same
spec file with an edition flag: the OSS leg asserts the OSS behavior,
the cloud leg asserts the replacement behavior on the same surface.
Edition-gated proof adds cloud-only scenarios for surfaces that have
no OSS counterpart at all. A row turns new green only when its OSS
leg and its cloud leg both pass.

Bulk selection and actions (NEWFRONT-633 owns ISS-108–115, NEWFRONT-634
owns ISS-116; extended scenarios in `bulk-selection.spec.ts`). OSS
builds the selection mechanics plus the sticky upgrade banner and no
toolbar; cloud replaces the banner with the functional toolbar
driving the four bulk endpoints (update with the exact editable set
and append-union array semantics, delete after confirmation, archive,
subscribe) plus the timeline checkboxes. The OSS leg asserts banner
and no toolbar; the cloud leg selects issues and asserts each bulk
outcome through the endpoints.

De-dupe (ISS-121 owned by NEWFRONT-630, ISS-231 owned by NEWFRONT-657;
extended scenarios in `create-modal.spec.ts` and
`edition-stubs.spec.ts`). OSS builds the hook returning no matches
with button and panel absent. Cloud builds the debounced duplicate
search while typing, the count button, the side-by-side panel, and
the description-editor popover. The cloud legs seed near-duplicate
issues and assert the button, panel, and popover appear with the
matches.

Types, templates, and additional properties (ISS-129 owned by
NEWFRONT-631, ISS-235 owned by NEWFRONT-657; extended scenarios in
`create-modal.spec.ts` and `edition-stubs.spec.ts`). OSS builds the
stubs: no type or template dropdowns, no additional-properties block,
identifier-only type area, empty property widgets and activity rows.
Cloud builds the type picker driving type-specific custom properties,
template application prefilling form plus editor, validated custom
properties in modal and detail, and their activity entries. The cloud
legs pick a type, apply a template, fill a custom property, and
assert each renders and persists.

Worklog (ISS-206 owned by NEWFRONT-650; edition-gated proof in
`worklog-absent.spec.ts` plus a cloud worklog spec). OSS builds
empty surfaces with no worklog button, rows, summary, or call to
action, and the activity filter keeps no worklog category. Cloud
builds the log entry, rows, sidebar summary, and feed entries for
eligible members. The cloud scenarios log time and assert it appears
in the feed and sidebar.

The cloud worklog implementation is currently blocked: the OSS
backend has no worklog model, serializer, route, or contract, and the
cloud overlay is not part of this repository (NEWFRONT-612 recorded
the same gap). NEWFRONT-650 therefore builds the OSS absence plus the
`issue-worklog` edition flag in
`apps/web_new/src/features/issues/detail/widgets/worklog/`, and
NEWFRONT-672 carries the cloud backend, contracts, cloud surface, and
cloud scenario. ISS-206 turns fully new green only when NEWFRONT-672
lands and the area gate proves both editions.

Epics (ISS-232 owned by NEWFRONT-657; edition-gated proof in
`edition-stubs.spec.ts` plus cloud epic scenarios). OSS builds the
empty modal stub with the inert epic branches. Cloud builds epic
create and edit through the modal from relations and sub-issue flows.
The cloud scenarios create and edit an epic and assert the branches
activate.

Workflow (ISS-233 owned by NEWFRONT-657; extended scenarios in
`edition-stubs.spec.ts` plus the board specs' cloud legs). OSS builds
the no-op hooks: every state change and drag allowed, plain state
options. Cloud builds configurable transitions with disabled overlays
on board drops, state pickers, and creation. The cloud legs configure
a blocked transition and assert the overlay plus the refused move.

Team-project and issue-type filters (ISS-234 owned by NEWFRONT-657;
edition-gated proof in `edition-stubs.spec.ts`). OSS builds the
empty stubs. Cloud builds the scoping filter, the type filter, and
their applied chips. The cloud scenarios apply each filter and assert
the list and chips respond.

Detail badges (ISS-236 owned by NEWFRONT-657; extended scenarios in
`edition-stubs.spec.ts` and the sidebar spec's cloud legs). OSS
builds the empty stubs. Cloud builds the overdue badge next to the
due date and the transfer-hop info note on the cycle row. The cloud
legs seed an overdue issue and a transferred cycle and assert both
accents render.

Timeline dependencies (ISS-237 owned by NEWFRONT-657; edition-gated
proof in `edition-stubs.spec.ts` plus cloud timeline scenarios).
OSS builds the chart with the dependency layer off. Cloud builds
dependency arrows between blocks plus drag handles to link them. The
cloud scenarios link two blocks and assert the arrow persists.

## 24. F-06 reuse map (NEWFRONT-17 survey)

NEWFRONT-17 plus the shell slices built a working foundation in
`apps/web_new`: the app shell with sidebar, title bar, and workspace
switching; sign-in; toasts; the command registry with per-screen
registration; a read-only virtualized issue list on the project
issues route; the shared peek panel shell; the shared creation dialog
shell with its draft persistence and submit flow; the browse key
resolution; palette creation entries and the contextual work-item
entries; kit primitives (button, dialog, input, menu, popover,
tooltip, badge, avatar, virtual list, skeleton, spinner, switch,
empty state, context menu, key caps, toasts); and thin API contracts
(issue list, detail, archived list, update, delete, create,
cycle/module membership, subscription, plus state, label, member,
cycle, module, and estimate list reads). Reference-data staleness,
workspace-scoped query keys, and the mutation-plus-invalidation
pattern are established. Per slice, what exists versus what must be
added or reworked:

- Layout switch, prefs, loaders (ISS-001–006, NEWFRONT-614): reuse
  the issues route shell, the validated search-param pattern, query
  keys, and the skeleton plus spinner primitives; the layout
  switcher, per-entity user-properties reads and writes, the
  freshness/highlight/optimistic-temp machinery, and the floating
  mutation spinner are new (the list query currently hardcodes paging
  and ordering).
- Dropdown primitives A and B (ISS-207–219, NEWFRONT-615/616): reuse
  kit menu, popover, input, avatar, and badge primitives plus the
  lazy option-list pattern; all thirteen pickers with their shared
  keyboard, search, and empty/loading behaviors are new, as are the
  uncontracted dropdown reads.
- Labels settings plus shared picker (ISS-226–230, NEWFRONT-617):
  reuse the label list reads, menu plus dialog primitives, and the
  optimistic mutation pattern; the settings screen with inline
  create/edit/delete, the two-level drag grouping, the skeleton and
  empty states, and the shared picker plus label-write contracts are
  new.
- Display dropdown (ISS-075–081, ISS-106, NEWFRONT-618): reuse the
  popover plus pill primitives and the user-properties patch
  pattern; the per-layout section matrix with all five sections and
  the persistence scope are new.
- List layout (ISS-007–014, NEWFRONT-619): reuse the virtualized list,
  row link pattern, and group query options; grouping sections with
  collapse state, pagination modes, quick-add with its toast actions,
  the cycle/module header menu, sub-issue expansion, and the inline
  property strip are new or rework the read-only rows in place.
- Spreadsheet layout (ISS-015–020, NEWFRONT-620): reuse virtualization
  and the cell dropdown primitives from the picker slices; the sticky
  grid, display-driven column set, inline cell editing with refocus,
  header sort menus with local persistence, the sticky quick-add, and
  keyboard navigation are new.
- Calendar layout (ISS-021–027, NEWFRONT-621): reuse the windowed query
  pattern and popover plus menu primitives; month/week views with day
  bucketing, navigation plus picker, options menu, date drag with
  rejection rules, blocks with preview and actions, per-day add, and
  the mobile day-detail view are new.
- Kanban structure plus cards (ISS-028–035, NEWFRONT-622): reuse the
  grouped query, lazy column placeholders, and the row link plus
  inline-property patterns; columns, swimlanes, collapse states,
  cards, and both quick-add paths are new.
- Kanban actions plus drag-drop (ISS-036–043, NEWFRONT-623): reuse the
  toast and optimistic-update helpers; pointer dragging with sort
  derivation, cross-column and cross-lane value application, the
  delete zone, feedback overlays, and per-column pagination with
  virtualization are new.
- Timeline core (ISS-044–049, NEWFRONT-624): reuse the header plus
  switcher patterns and virtualization; the sidebar-plus-chart
  layout, zoom levels, today behavior, fullscreen portal, infinite
  date extension, and sidebar rows are new.
- Timeline drag plus add (ISS-050–055, NEWFRONT-625): reuse the batch
  update contract pattern and toast errors; row reorder, bar move and
  resize with day snapping, block planting, and the pre-dated
  quick-add are new.
- Rich filters core (ISS-095–099, NEWFRONT-626): reuse menu plus
  popover primitives and the user-properties patch pattern; the chip
  bar, toggle, condition pills, field picker, and negation storage
  are new.
- Rich filter inputs plus save view (ISS-100–105, ISS-107,
  NEWFRONT-627): reuse picker and date primitives; the four value
  inputs, clear-all, the view save/update triggers with the minimal
  save form, temporary instances, and the analytics entry are new.
- Legacy filter dimensions A and B plus chrome (ISS-082–094,
  NEWFRONT-628/629): reuse the dropdown shell and member/state/label
  reads; the thirteen dimension sections, the subscriber dimension,
  and the shared section chrome in the widget dropdowns (per §22)
  are new.
- Create modal core plus project/parent (ISS-117–123, NEWFRONT-630):
  reuse and extend the shared creation dialog shell, its draft
  persistence, and its submit flow in place (title plus project exist
  today); the full field strip, AI title generation, Create-more, the
  draft-save confirm, the cloud de-dupe hook point, parent search,
  and project-change resets are new.
- Modal context, edit, description, advanced (ISS-124–130,
  NEWFRONT-631): reuse the dialog shell and editor primitives; cycle
  and module pre-seed with reconciliation, keyboard and discard
  guards, changed-field diffing, the rich description with mentions
  and AI entries, the git branch field, the cloud type/template
  stubs, and publish are new.
- Modal extras plus archive/delete/move (ISS-131–136, NEWFRONT-632):
  reuse dialog, toast, clipboard, and menu primitives; toast action
  items, the modal label picker with inline create, the hover preview
  card, and the archive/delete/move confirmations are new.
- Bulk selection plus actions (ISS-108–115, NEWFRONT-633): reuse the
  selection-state pattern and toast plus dialog primitives; checkbox,
  range, keyboard, and leave-guard mechanics, the OSS banner, and
  the cloud toolbar with its four bulk endpoints are new.
- Timeline chrome plus permissions plus timeline bulk (ISS-056–059,
  ISS-116, NEWFRONT-634): reuse bar and row components from the
  timeline slices; bar presentation with preview and off-viewport
  arrows, the permission gating, loading and empty chrome, and the
  bulk checkboxes are new.
- Row plus layout quick actions (ISS-060–067, NEWFRONT-636): reuse the
  context-menu primitive, clipboard helper, and the archive/delete/
  move endpoints; the seven context menus with their entries, gating,
  and archive rules are new.
- Empty states (ISS-068–074, NEWFRONT-637): reuse the empty-state kit
  primitive and the creation dialog launch; the per-context
  first-run, filtered-empty, and guest-disabled variants with their
  navigation targets are new.
- Detail core (ISS-142–147, NEWFRONT-639): reuse the browse
  resolution, route gate, and editor primitives; the two-phase load,
  short-link redirect, root layout with responsive sidebar, title
  pipeline with save indicator, and description editor with version
  history are new.
- Sidebar properties A and B (ISS-148–159, NEWFRONT-640/642): reuse
  the picker slices' dropdowns and the issue patch contract; the
  twelve sidebar rows with their conditionals, bridge calls, parent
  banner with siblings, and label combobox are new.
- Drafts (ISS-137–141, NEWFRONT-643): reuse the paginated list
  pattern and modal prefill; the drafts route, blocks with inline
  properties, edit/copy/promote actions, delete flow, and empty
  states are new.
- Detail header actions plus agent/run controls (ISS-160–164,
  ISS-192–193, NEWFRONT-644): reuse clipboard, toast, and dialog
  primitives plus the subscription contracts; copy-link, subscribe,
  the header menu with its redirects, the agent panel with polling
  and token tile, and both run dispatches are new.
- Peek panel (ISS-165–172, NEWFRONT-646): reuse the shared peek panel
  shell (portal, editable title, close handling) and the detail
  components; issue-data fetching, display modes, URL sync, close
  rules with refocus, header actions, loading and error states, and
  the properties section are new.
- Sub-issues widget (ISS-173–178, ISS-238, NEWFRONT-647): reuse the
  collapsible plus menu primitives and the create modal; the action
  row, gauge rollup, recursive rows with inline edits, filters and
  display controls, create/add-existing, remove/delete, and paged
  loading are new.
- Relations plus links widgets (ISS-179–186, NEWFRONT-649): reuse the
  existing-issues modal pattern and dialog primitives; grouped
  relations with reciprocal edges, row actions, the type map, and
  the links collection with its modal are new.
- Attachments plus code reviews plus worklog (ISS-187–191, ISS-205–206,
  NEWFRONT-650): reuse the file-upload primitives and collapsible
  pattern; the attachment list with signed three-step upload,
  open/delete, size-limit enforcement, code-review links, and the
  OSS-empty worklog with its cloud build are new.
- Comments (ISS-197–202, NEWFRONT-651): reuse editor primitives and
  the comment contracts; composer with uploads and access specifier,
  author-gated edit and delete, access toggle, fold, and deep links
  are new.
- Activity feed plus reactions (ISS-194–196, ISS-203–204,
  NEWFRONT-654): reuse popover primitives and browser persistence;
  the merged timeline with its activity sentence coverage, filter
  and sort controls, and both reaction rows are new.
- Cross-cutting (ISS-220–225, NEWFRONT-656): reuse the permission
  hook, optimistic helpers, and query invalidation; the read-only
  rendering pattern, no-live-update posture, global-view param
  reading, cycle transfer prompt, and rollback behavior are new or
  hardened in place.
- Edition-only behaviors (ISS-231–237, NEWFRONT-657): reuse the
  edition flag interface; the OSS stubs plus all seven cloud builds
  (de-dupe surfaces, epic modal, workflow overlays, team/type
  filters, custom types and properties, detail badges, dependency
  layer) are new.

Biggest gaps overall: the four richer layouts (board, spreadsheet,
calendar, timeline) with their drag systems; the display and filter
stack; the full modal field set with editor and AI entries; detail
with sidebar, peek, and widgets; comments with feed and reactions;
bulk with its cloud toolbar; and every cloud build behind the
edition flags.

## 25. API calls in scope

`BASE` is `/api/workspaces/{ws}/projects/{p}`; `WS` is
`/api/workspaces/{ws}`. Ownership in parentheses: A is NEWFRONT-611,
B is NEWFRONT-612, C is NEWFRONT-613, covered means already
contracted. Slices call through contracts only, never raw fetch.

Issues core (A unless covered). List `GET BASE/{issues|epics}/`
with group, sub-group, order, filter, and cursor params (covered
thinly for the flat list; A owns grouped, filtered, and global
variants). Detail `GET BASE/{issues|epics}/{id}/` (covered). Create
`POST BASE/{issues|epics}/` (A, shared with sub-issue create).
Full update `PATCH BASE/{issues|epics}/{id}/` across every writable
field — name, description, state, priority, assignees, labels, dates
including the target date, estimate, parent, cycle, modules,
sort order, git branch (A; thin update covered). Delete `DELETE
BASE/{issues|epics}/{id}/` (covered). Archive `POST
BASE/{issues|epics}/{id}/archive/` and restore via delete on the
same record (A). Move `POST BASE/work-items/{id}/move/` (A).
Duplicate has no dedicated endpoint in the old services: it goes
through the create modal prefilled from the source issue (A confirms
against Django; if a dedicated endpoint exists, A contracts it).
Short-link meta `GET BASE/issues/{id}/meta/` (C). Lite browse `GET
WS/work-items/{IDENT-seq}/` (covered via search plus detail; C owns
any gap). Description versions `GET .../description-versions/`
plus version detail (B). Issue search `GET BASE/search-issues/` with
the parent flag (B for parent search; C confirms shared use).
Batch dates `POST BASE/issue-dates/` with the updates array (A).

User properties and views (A thin slice; project plus workspace
covered). Reads and writes `GET/PATCH BASE/user-properties/`,
`BASE/cycles/{cid}/user-properties/`,
`BASE/modules/{mid}/user-properties/`,
`BASE/epics-user-properties/`, and the saved-view and global
variants (A). View create `POST WS/views/` or the project views
endpoint plus view update (A thin slice; full ownership in views).

Bulk, cycle transfer (A). `POST BASE/bulk-operation-issues/` with
issue ids plus properties, `DELETE BASE/bulk-delete-issues/`,
`POST BASE/bulk-archive-issues/`, `POST
BASE/bulk-subscribe-issues/`, and `POST
BASE/cycles/{cid}/transfer-issues/` with the new cycle id — or, if
Django shows the app looping single-item calls, A contracts that
loop and says so.

Sub-issues, relations, links (B). Paged children `GET
BASE/{issues|epics}/{id}/sub-issues/` with page size and cursor
(epics use the issues segment); add-existing posts the child ids to
the same collection; detach patches the child parent id to null
while delete destroys the child. Relations `GET/POST
BASE/issues/{id}/issue-relation/` plus remove via `POST
BASE/issues/{id}/remove-relation/` with type and related issue.
Links under the issue-links (or links for epics) collection with
post, patch, and delete on the link id.

Attachments, drafts, reviews, worklog (B). Attachments on the
assets host: post metadata, put bytes to the signed URL, patch the
attachment record complete, get to list, delete on the asset id;
the size limit reads from instance config. Drafts `GET/POST
WS/draft-issues/`, item get/patch/delete on the draft id, promote
`POST WS/draft-to-issue/{id}/`. Code reviews on the issue
code-reviews collection with attach post and detach delete. Worklog
log and view endpoints (cloud; B contracts from Django).

Comments, activity, reactions, labels (C). Comments post to and
patch/delete on `BASE/issues/{id}/comments/` with the comment id,
plus access toggle and fold-label writes. Activity `GET
BASE/issues/{id}/history/`. Reactions post/delete on the issue and
comment reactions collections. Labels: list covered; create
(post name plus color), rename/recolor (patch), delete, and
group/reorder (patch parent plus sort order) are C.

Dropdown reads and agent thin slice. Intake states, execution-target
pods per project, and estimate reads (C; full estimate, cycle, and
module ownership stays in their epics). Agent status refresh through
the issue retrieve including the token total, re-tick `POST
/api/runners/re-tick/`, abort `POST
/api/runners/runs/{runId}/cancel/`, dispatches `POST
/api/runners/runs/` with the run-ai and comment-and-run triggers
(B thin slice; full ownership in runners).

Subscription status, subscribe, and unsubscribe on
`BASE/issues/{id}/subscribe/` are covered; the palette and header
toggles reuse them.

## 26. Hybrid issue assignment (ISS-239–242)

An issue runs either on its pod (the squad) or on one dedicated
runner inside that pod. The contract is settled by the hybrid
assignment design (HA-1, `.ai_design/dedicated_runner_assignment`)
and is new-app-first: neither the old web UI nor the backend offered
any of it when the slices below were built, so the rows below carry
no oracle result yet and the scenarios named run once the old-app
picker (PDASHOSS01-299) and the backend (PDASHOSS01-297) land. Until
then the frontend builds against the intended contract and degrades
gracefully: every new field is optional, runners without eligibility
answers stay pickable with status lines from the live status plus
heartbeat, and the picker surfaces are the create dialog and the
detail sidebar — the new list, board and table layouts show no pod
today, so there is nothing to extend there.

The execution-target picker (ISS-241) keeps the ISS-218 shape —
search, single-select, cloud/managed peer rows with availability
reasons, the DEFAULT tag — and nests one runner level under each
pod: the pod row reads as the squad choice, followed by that pod's
runners, each with a status line (online and idle, busy with a queue
depth, offline with the last heartbeat), its owner, and — when the
server rules it out — disabled with the reason for its code. Picking
a busy or queued runner warns how long the wait is without blocking
the save. The collapsed trigger names the runner with its pod; while
the run waits it also carries the waiting reason as a badge. A
runner pick writes the pod, the runner and the local executor in one
issue PATCH and survives a reload; an agent pick clears the pod and
the runner (ISS-242). A rejected write shows the API's validation
message in place next to the picker instead of a toast.

While the issue's active run is queued for its runner (ISS-239), the
detail names the runner and the reason — offline with its last
heartbeat, busy with the runs ahead, not permitted, or dispatching.
The new detail has no agent-status area yet, so the banner sits at
the top of the detail's main column, visible to everyone who can see
the issue, with its actions disabled for read-only viewers. The
banner offers the three ways out (ISS-240): run once on any runner
in the pod (disabled unless the server marks the run releasable,
which is the creator-or-assignee rule), change the issue's runner
through the picker, and cancel the run. The release is run-scoped:
the issue keeps its dedicated assignment afterwards.
