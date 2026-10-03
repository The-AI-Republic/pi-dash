# Issues layouts B — kanban board + gantt timeline — area spec (NEWFRONT-118)

Behavior learned by reading the old issues layouts (`issues/issue-layouts/kanban`,
`issues/issue-layouts/gantt`, the shared `gantt-chart` chart library, the kanban
and gantt view stores, and the group drag-and-drop helpers) plus the running old
app during oracle recon. Prose only: no old code, strings, styles, or assets are
reused. New code (driver extension + oracle specs) is written from this spec.
Covers inventory rows ISS-028–ISS-059.

Endpoint shorthand (backend contracts, also used by the inventory): `BASE` =
`/api/workspaces/{ws}/projects/{p}`.

## Kanban board (ISS-028–ISS-043)

### Columns and grouping (ISS-028)

- The board is one of the layouts of the project issues page. On mount (and
  whenever grouping changes) it fetches issues grouped, 30 per page, or 10 per
  page when sub-grouped.
- One fixed-width column per value of the active group-by dimension (project,
  cycle, module, state, state group, priority, labels, assignees, created-by,
  team project). Cycle, module, labels, and assignees additionally get a
  synthetic empty-value column. With no grouping there is a single column
  holding every issue.
- Each column header shows an icon for the value, the value name, and a live
  count of issues in that group. Headers sit in a sticky row; the board scrolls
  horizontally beneath it.
- Columns render lazily: only the first few columns mount immediately, the rest
  mount when scrolled into view behind skeleton placeholders.

### Swimlanes (ISS-029)

- With a sub-group-by set, the board renders a sticky row of group headers
  followed by one horizontal lane per sub-group value.
- Every lane has a sticky left header with a collapse chevron, an icon, the
  sub-group name, and the cumulative issue count across all its columns.
- The board scrolls vertically in swimlane mode. Lanes with zero issues are
  hidden when the show-empty display filter is off.
- Live recon (NEWFRONT-118 run 4): on the seeded stack, seed-created rows
  without labels never reach a labels sub-grouped board — the None lane
  renders empty (old bug NEWFRONT-158, pinned by a bug scenario); API-made
  rows behave. Oracle swimlane scenarios use scratch projects.

### Collapse and empty groups (ISS-030–ISS-032)

- In flat (non-swimlane) mode each column header carries a minimize/maximize
  control. Minimizing folds the column to a narrow vertical strip showing the
  title rotated and the count; cards are hidden.
- In swimlane mode clicking a lane header toggles that lane's cards; the
  chevron points down while collapsed and up while expanded.
- Both collapse states persist per user as part of the kanban filters (kept
  server-side with the user's issue filters, updated whenever a toggle flips).
- The show-empty display filter (default on) hides zero-count columns and
  lanes when off.

### Cards (ISS-033, ISS-034)

- A card is a real link to the work item, so it can be opened in a new tab. A
  plain click opens the peek panel on desktop and navigates full-page on
  mobile. Optimistically created cards that the server has not confirmed yet
  cannot be opened.
- Card contents: the issue identifier, quick actions revealed on hover (always
  visible on touch layouts), the issue name clamped to one line with a full
  tooltip, and the enabled display properties wrapped below. Inline editing of
  properties is gated on edit permission. Epic cards can show progress
  statistics. The currently peeked card is outlined.
- Cards render lazily like columns: roughly the first screenful mounts
  immediately, the rest behind placeholders.

### Quick-add and header create (ISS-035, ISS-036)

- Each column ends with a sticky quick-add row: a "new item" button that opens
  an inline form (project identifier, title field, hint that Enter adds
  another). Submitting creates the issue through the create endpoint with the
  column's (and lane's) grouping pre-applied plus the project's default state;
  the form stays open for the next item. It is hidden without create
  permission, in completed cycles, or when a workflow rule blocks creation.
  Recon flag: the header create entry is disabled for created-by groupings;
  whether the per-column quick-add is also suppressed there is verified
  against the live app.
- The `+` control in a group header opens the create modal prefilled with the
  column payload in project context. In cycle/module context it offers a menu
  with creating a new item and adding an existing one (adding confirms with a
  success toast).

### Drag and drop (ISS-037–ISS-041)

- Card dragging is pointer-based (press, move, release; no native HTML5 drag).
  A card can be dragged only when the active grouping is one of state,
  priority, assignees, labels, module, or cycle (both dimensions when
  sub-grouped), the card is server-confirmed, and the user may edit it.
- Reordering within a column derives a new sort position from the neighbours:
  a large step below the first card, a large step above the last, the midpoint
  between two cards, or the default step into an empty column. Reordering
  applies only when the list is in manual order; otherwise an overlay names
  the active ordering and the move is suppressed. The dropped card is briefly
  highlighted and scrolled into view.
- Dropping into another column also applies that column's group value: scalar
  fields are replaced (the empty column clears them), list fields gain the
  destination and lose the source (the empty column only removes). Cycle and
  module membership go through their dedicated add/remove calls rather than
  the plain issue update. Dropping where the grouping forbids it, or without
  permission, raises a warning toast instead of moving.
- Across swimlanes both the group and sub-group values update, provided both
  dimensions are draggable; otherwise the move is refused.
- While any card drag is in flight a delete zone is pinned top-center
  (announced as a drop-to-delete target, turning red on hover). Dropping a
  card on it opens the delete-confirmation modal; confirming deletes the
  issue through the delete endpoint.
- A feedback overlay covers a column dragged over while it cannot accept the
  card: a notice naming the active ordering when not manually sorted, a notice
  refusing completed cycles, and (cloud edition) a notice when a workflow rule
  blocks the target state.

### Pagination and virtualization (ISS-042, ISS-043)

- Each column paginates independently. Flat columns auto-load the next page
  through a scroll sentinel with skeleton loaders; sub-grouped columns show
  an explicit load-more entry instead. Concurrent loads for one column are
  suppressed.
- Columns and cards outside the viewport are replaced by height-estimated
  placeholders until scrolled near. Live recon (NEWFRONT-118 run 4)
  confirms the mechanism: loaded issues mount anchor shells for every
  row, but only the visible window (about six cards) renders card
  content — the rest stay 100px placeholder shells, so card-name readers
  must tolerate empty shells and drags must hold from the window middle.
  While dragging near an edge the board auto-scrolls horizontally and
  the hovered column vertically.

## Gantt timeline (ISS-044–ISS-059)

### Layout and header (ISS-044)

- The timeline pairs a sticky left sidebar list with a scrollable chart on the
  right. Issues are fetched flat, 100 per page.
- The header shows a live item count, the Week/Month/Quarter switcher, a Today
  button, and the fullscreen toggle.
- Bars are positioned by start/target dates. Issues without dates still appear
  in the sidebar with an add-block affordance on their empty row.

### Zoom, today, fullscreen, infinite scroll (ISS-045–ISS-048)

- Exactly three zoom levels (Week, Month, Quarter), each with its own day
  width (week widest, quarter narrowest). The zoom is session-local: it is
  neither persisted nor deep-linked. Switching levels re-centers the chart on
  today. Weekend columns are tinted; the week's start day comes from the user
  profile. Month is the initial level.
- Today re-renders the current zoom and scrolls so today is centered. The
  current-date column is highlighted in every zoom. Mounting the chart
  auto-centers today.
- The expand control moves the chart into a full-screen portal overlay; the
  shrink control returns it inline.
- Scrolling near either horizontal edge appends the adjacent date range and
  grows the chart, so the timeline extends without bound. Prepending on the
  left preserves the scroll position.

### Sidebar rows (ISS-049, ISS-050, ISS-059)

- Each row links identifier plus name (peek on desktop, deep-link; disabled
  for unconfirmed cards) and, for fully dated issues, a duration in days. Rows
  are virtualized; peeked and hovered rows are visually distinguished.
- Rows drag to reorder with a drop indicator; the new sort position derives
  from the neighbours (a step below the first row, above the last, midpoint
  between). Reordering is a separate drag instance from card dragging and
  applies only in manual sort order. Recon flag: the not-manually-sorted
  warning toast is verified against the live app (its trigger may be dead).
- While loading, the sidebar shows skeleton rows and the header a loading
  label; the load-more sentinel renders as a pulsing placeholder.

### Bars: move, resize, add (ISS-051–ISS-055)

- All three gestures are mouse-driven with day snapping and persist on
  release through the batch issue-dates endpoint; a failed persist toasts an
  error.
- Moving drags the bar body horizontally and shifts start and target together,
  preserving duration. Only fully dated bars move.
- Resizing drags the edge handles (left edge moves the start date, right edge
  the target date), never narrower than one day. A floating label previews
  the date under the handle. Either handle can fill in its missing date on a
  half-dated bar.
- Hovering an undated issue's empty row reveals a `+` control that follows
  the cursor with a date tooltip; clicking plants a short block at that day
  (a week-long block in quarter zoom) via the issue update endpoint.
- A sticky quick-add at the chart bottom creates issues pre-dated today
  through tomorrow so the new bar appears immediately. It hides without
  create permission or in completed cycles.

### Bar presentation and navigation (ISS-056, ISS-057)

- Bars are tinted by their state color, with a fading mask when only one date
  is set. The issue name stays pinned visible while the chart scrolls
  horizontally. Epic bars show progress once the bar spans at least two days.
  Hovering a bar opens a preview popover; clicking opens peek (deep-links).
- When a bar lies outside the viewport its row shows a sticky arrow pointing
  toward it; activating the arrow scrolls (extending the date range when
  needed) to bring the bar into view.

### Permissions (ISS-058)

- Moving, resizing, reordering, and planting blocks require project
  ADMIN or MEMBER; reordering additionally requires manual sort. Guests see
  the chart but none of the drag, resize, or add affordances. Completed
  cycles suppress quick-add.

## Cross-cutting recon notes

- Switching layouts persists the grouping choice (group-by state survives a
  move to the board and back); specs reset grouping via the API before and
  after each scenario so layout state never leaks between scenarios or into
  the shared example spec.
- The seed holds one member user, one guest, one project, and three issues:
  scenarios needing cycles, modules, labels, extra states, or assignees
  create them through the API inside the scenario.
