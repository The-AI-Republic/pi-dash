# Issues layouts A — area spec (NEWFRONT-117)

Oracle area spec for inventory rows ISS-001–027 (layouts shared, list,
spreadsheet, calendar) and ISS-060–074 (per-row quick actions, empty
states). Written in prose from reading the old issues layout code and from
driving the running old app against the seeded parity stack. New oracle
code (driver extension plus scenarios) is written from this spec; no old
code, styles, or assets are reused.

Old sources consulted (reference only): the per-entity layout roots, the
layout switch wrapper, the list / spreadsheet / calendar layout folders,
the row quick-action menus, the empty-state set, the header filter bar,
and the issues filter store plus user-properties endpoints. Live
observations below were confirmed on the seeded stack (one project, three
undated issues in a single state, flat list preference).

## Shared layout behavior (ISS-001–006)

- The project issues page centers on a layout root chosen per entity: each
  of project, cycle, module, saved view, archived list, and the workspace
  all-issues list has its own root that renders whichever layout the
  current user last picked for that entity.
- The header carries a segmented control with one icon-only button per
  layout, in a fixed order: list, board, calendar, spreadsheet, timeline
  (gantt). The active button is visually distinguished; clicking it again
  changes nothing. On narrow containers the same control collapses to a
  dropdown that names each layout with an icon plus a text label.
- Switching layout writes the choice to the user's per-entity display
  preferences on the server and re-renders immediately; a reload restores
  the stored layout, so the preference survives sessions.
- The preference record also holds grouping, sub-grouping, sort order, the
  show-empty-groups flag, calendar month/week mode plus weekend visibility,
  and which property columns are shown. It is fetched once when the page
  mounts and not refetched on window focus.
- While the first load is in flight each layout shows its own skeleton
  placeholder. When the project holds zero issues the page shows a
  context-specific empty state instead of the layout — except calendar,
  which always renders its grid and represents emptiness per day.
- A small floating spinner appears top-right while a mutation is saving.
  Paginating shows skeleton rows in list and spreadsheet, pulsing blocks in
  calendar, plus a thin loading bar across the top of the calendar.
- Issues created within roughly the last half minute render immediately
  even inside virtualized lists and carry a highlight; a dropped issue is
  highlighted and scrolled into view. A create that has not been confirmed
  by the server yet shows as a pulsing temporary row that reconciles to
  the real issue once the response lands.

## List layout (ISS-007–014)

- With no grouping the list shows one flat section titled for all work
  items. With a grouping chosen (project, cycle, module, state,
  state-group, priority, labels, assignees, creator, team project) the list
  shows one collapsible section per group value, with a trailing bucket for
  issues with no value where that applies. Grouping by creator disables
  per-group creation.
- Each section header shows an icon, the group name, a live count, and an
  add control. Clicking the header collapses or expands the section; the
  collapsed set is stored per user per view and shared with the board
  layout. Dropping an issue onto a collapsed section re-expands it.
- A display flag controls whether sections with zero issues render at all.
- A grouped section with more issues than fetched shows an explicit
  "load more" row; the flat list instead appends skeleton rows and loads
  the next page automatically when they scroll into view.
- Each section ends with a sticky single-field quick-add form. It opens
  from a "+ New work item" style button, inherits the section's grouping
  value (state, priority, labels, assignees, cycle, module) plus the
  project's default state, and submits with Enter — staying open for rapid
  repeat entry — while Escape or an outside click closes it. Submit shows
  a confirmation toast with follow-up actions and an optimistic row while
  saving. Guests, completed cycles, creator-grouped sections, and states
  blocked by workflow rules do not get quick-add.
- In cycle or module context the section "+" offers two entries: create a
  new work item (modal pre-set with the section value) or attach an
  existing one (search modal, then a toast). In a plain project the "+"
  opens the create modal directly.
- A row shows the issue identifier (sized to content), the title with a
  hover tooltip, a sub-issue expander that lazily loads children (nesting
  stops at three levels; deeper children open the peek panel instead), the
  enabled inline properties, and a quick-actions menu. The row is a real
  link: a normal click opens the peek panel, while middle-click or
  keyboard-modified click opens a new tab.
- The inline property strip renders only display-enabled properties:
  state, priority, start and due dates (merged or separate), assignees,
  modules and cycles (each only when the project enables that feature),
  estimate (when enabled), and read-only sub-issue, attachment, and link
  counts plus labels. Each editable property saves the moment it changes;
  an overdue due date is visually flagged depending on the issue's state
  group. Guests, archived issues, and completed cycles render read-only.

## Spreadsheet layout (ISS-015–020)

- A flat table of all issues: the first column (identifier, title, row
  actions) sticks to the left while scrolling horizontally, gaining a
  shadow once scrolled; the header row sticks to the top. Sub-issues
  expand inline with the same three-level cap as list; rows are
  virtualized.
- Columns appear only for enabled display properties. The available set
  covers assignee, creation and update timestamps, due date, estimate,
  labels, modules, cycle, link, priority, start date, state, sub-issue
  count, and attachment count. Cycle and module columns additionally
  require the matching project feature flag.
- Editable cells (state, priority, assignees, start/due dates with
  min/max constraints, estimate, labels, cycle, modules) save immediately
  and return focus to the cell. Timestamp, link, attachment-count, and
  sub-issue-count cells are read-only; activating the sub-issue count
  navigates to that issue's sub-issues. Guests, completed cycles, and
  archived issues render read-only.
- Each sortable column header opens a menu offering ascending and
  descending sort; once sorted, the menu also offers clearing the sort,
  which restores creation-date order. The active sort is remembered
  locally per browser and marked with an arrow on the header.
- A sticky bottom row reveals the same inline title form as list
  quick-add (Enter repeats, Escape closes, optimistic row plus toast).
  The table footer auto-loads further pages on scroll with skeleton rows.
- Cells are keyboard-focusable: arrow keys move focus between cells.

## Calendar layout (ISS-021–027)

- Month view renders every week of the active month; week view renders the
  active week only. Issues land on day tiles by their due date; issues
  without a due date never appear. Today is badged, weekend tiles are
  shaded, and crowded days offer a per-day "load more".
- The header offers previous/next stepping (one month in month view, one
  week in week view), a Today jump that also reselects today, and a title
  button opening a month picker with year stepping (disabled in week
  view). Week view shows a date-range title. Every range change refetches
  the windowed issue query.
- An Options menu switches month/week mode (active mode checkmarked) and
  toggles weekend columns (grid switches between seven and five columns).
  The week's first day follows the user's profile setting. On narrow
  screens the menu dismisses itself after a pick.
- Dragging an issue block onto another day re-dates it to that day; when
  the issue starts after the chosen day the drop is refused with an error
  toast, and dropping onto the same day does nothing. The dropped block is
  highlighted and the grid auto-scrolls near the edges mid-drag. Dragging
  is unavailable on touch layouts.
- A block shows a state-colored edge plus identifier and title. Hovering
  reveals a preview popover of the issue; clicking opens peek (deep-link).
  Hover also reveals per-block quick actions whose placement flips near
  the viewport edge. Unconfirmed blocks pulse like list temp rows.
- Each day offers adding an issue due that day: either an inline title
  form pre-set to the date, or attaching an existing issue through the
  search modal (which hides issues whose start would fall after the
  target). Epic calendars only offer creation.
- On narrow screens tapping a day selects it and lists that day's issues
  beneath the grid; blocks are neither draggable nor hoverable there.

## Per-row quick actions (ISS-060–067)

- Every row exposes the same menu twice: a hover "..." trigger and the
  right-click context menu. Observed project-context entries, in order:
  edit, duplicate, open in new tab, copy link, move to another project,
  and delete. Copying the link toasts a confirmation. Edit, duplicate,
  move, and delete require edit permission and are hidden without it.
- The archive entry is gated on the issue's state group: unless the issue
  sits in a completed or cancelled state the entry is disabled with an
  explanatory note. (Live note: on a plain unstarted issue the disabled
  entry was not observed in the menu at all — the scenario must pin the
  actual rendering before asserting.)
- In a cycle's list the menu gains "remove from cycle"; in a module's
  list, "remove from module". Editing from either context pre-fills the
  current cycle/module.
- The archived list offers only restore, open in new tab, copy link, and
  delete. Restoring toasts and returns the issue to the project list.
- The workspace all-issues list offers the full project-style menu.
- Detail and peek headers offer duplicate, open in new tab, move,
  archive, restore, and delete; peek hides edit and copy-link because the
  header already carries its own copy-link button.
- The whole-list "..." menu offers copy-link (copies the list URL) and
  open in new tab, plus any extra entries the cloud edition injects.

## Empty states (ISS-068–074)

- Project list: when filters match nothing the page offers clearing all
  filters; with no filters it prompts creating the first work item. Guests
  see the same copy with disabled actions.
- Cycle list: a completed cycle shows an informational message with no
  actions; a filtered-empty list offers clearing filters; otherwise the
  page offers creating a work item and attaching an existing one.
- Module list: filtered-empty offers clearing filters; otherwise create
  plus attach-existing.
- Archived list: filtered-empty offers clearing filters; otherwise the
  page notes there is nothing archived yet and links to the project's
  automation settings (not to creation).
- Saved project view: a fixed message plus a new-work-item entry.
- Workspace all-issues: with zero projects the page offers starting the
  first project; otherwise it notes there are no views yet and offers
  adding a work item.
- Profile tabs (assigned, created, subscribed, activity): fixed
  informational copy per tab with no action; an unknown tab renders
  nothing.

## Live DOM notes (selector ground truth, seeded stack)

- Header layout switcher: a segmented container holding five icon-only
  buttons in list / board / calendar / spreadsheet / timeline order; the
  active button carries an "active" background class. No accessible names;
  the driver addresses them by container plus index.
- Group header reads "All work items" with a count; rows are anchors
  whose text holds identifier plus title; the row quick-actions trigger
  exposes an accessible "toggle quick actions" name but needs a hover
  first and a forced click (a sibling strip overlaps it under automation).
- Spreadsheet headers are plain clickable text (State, Priority,
  Assignees, Labels, Start date, Due date, Created on, Updated on, Link,
  Attachment, Sub-work item); the sort menu offers ascending/descending
  entries plus a clear-sort entry once sorted.
- Calendar header shows a month title button plus Today and Options
  buttons; the Options menu offers month/week layout picks and a weekend
  toggle. The seeded undated issues render no blocks, as specified.
- The list quick-add form input carries a "Work item title" placeholder
  and a hint that Enter adds another work item.
- The Display dropdown exposes property toggles (ID, Assignee, Start
  date, Due date, Labels, Priority, State, sub/attachment/link counts,
  Estimate) plus group-by and sort selectors.
