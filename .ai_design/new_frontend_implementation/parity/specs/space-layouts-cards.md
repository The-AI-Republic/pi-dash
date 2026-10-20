> **Superseded.** This note was folded into [`space.md`](space.md) by NEWFRONT-751; that file is the authoritative build spec for the area. Kept for history (the oracle research notes it was written from).

# Area spec: space board layouts & cards (SPC-020–033)

Learned by reading the old space frontend (board routes, navbar, layout
trees, card blocks, publish/issue/filter stores listed in
`parity/inventory/space.md`) and by driving the old app live against the
seeded parity stack. No old code copied; this prose is the only thing the
new implementation and the oracle scenarios build from.

## Layout offering and switching (SPC-020, SPC-021, SPC-022)

- The navbar carries a small segmented switcher with one icon button per
  offered layout. Hovering a button reveals its layout name. The offered
  set is the intersection of two things: the layouts the client knows how
  to render (exactly two exist — the stacked list and the sideways board),
  and the ones the publisher enabled in the board's view settings. Views
  the publisher enabled beyond those two kinds have no switcher entry at
  all. A board with a single enabled layout shows a one-button switcher,
  i.e. effectively no choice.
- Clicking a switcher button flips the layout immediately and records the
  new view in the address bar's view parameter, preserving any open-issue
  and filter parameters. The flip reuses the already loaded issues: no
  issue fetch fires, so switching back and forth never touches the
  network. Because the address carries the view, pasting it elsewhere
  opens the board directly in that layout.
- On arrival the view parameter is validated against the enabled set. A
  missing or unrecognised value settles on the first enabled layout (the
  list when enabled, else the board) and the address is rewritten to name
  it, so the settled address is always shareable. The switcher is the
  source of truth afterwards: only offered layouts can be picked.

## List layout (SPC-023)

- Issues group under one collapsible section per workflow state, in state
  order. Each section header shows the state's marker, its name, and the
  issue count reported by the server for that group. Sections with no
  issues still render their header (with a zero count) rather than
  disappearing. Clicking a header collapses or expands its rows.
- Rows show the public key (project short code plus sequence number), the
  title as a link, and the property markers described below.
- Known server miscount (NEWFRONT-454, pinned by `bug:` scenarios): the
  per-group total the server reports can run short of the rows it
  returns, because the total query filters out declined-triage issues
  that the rows query still includes. The header faithfully renders the
  short total, so a header can read one fewer than the rows beneath it.
  Intended behavior is header count equals rendered rows.

## Board layout (SPC-024)

- States render as fixed-width columns in a sideways-scrolling strip, one
  column per workflow state. Each column header shows the same
  marker-plus-name-plus-count treatment as list headers. Empty states
  render an empty column rather than disappearing.
- The same NEWFRONT-454 miscount applies to column headers: they render
  the server's short group total.

## Empty board (SPC-025)

- When the board holds zero published issues in total, neither layout
  renders: groups, skeletons, and column shells are all replaced by a
  single centered notice announcing that no work items were found. The
  navbar (switcher included) stays live above it.

## Per-group paging (SPC-026)

- The first fetch pulls fifty issues per group. Each group pages on its
  own: loading more in one group never touches the others, and empty
  groups stay put while a sibling grows.
- List groups page through an explicit manual control pinned under the
  group's rows; while the next page is in flight the control is replaced
  by pulsing placeholder rows, then the new rows append and the control
  retires once the group is exhausted.
- Board columns page by scrolling: placeholder cards plus an invisible
  tripwire sit at the column's end, and scrolling them into view loads
  the next page. There is no manual control on plain state columns (the
  manual variant exists only for a subgrouping the public board never
  uses). The column's load trigger is debounced, so a burst of rapid
  scrolls fires exactly one fetch and never duplicates cards.

## Cards: identity and markers (SPC-027, SPC-028)

- Every card shows the public key (project short code plus sequence
  number) and the title, truncated to one line; hovering the title
  reveals the full text in a tooltip. Keys share one project prefix
  across the board and track the server's sequence numbers.
- Beneath (list) or inside (board) each card sits a fixed marker strip:
  the state marker with its name, the priority marker (icon only, no
  text), label chips by name, and the due-date marker in short-month
  day-year form. Issues without labels or dates simply omit those
  markers; the priority marker is always present (the unset level shows
  its own icon). The start date never renders on board cards.
- Resolved live, inventory confirmation (1): the cycle and module fetches
  have no visible surface anywhere on board cards. An issue assigned to
  both still shows no cycle or module marker; the display set is fixed
  to key, state, labels, priority, and due date.

## Card footers (SPC-029)

- Board cards carry an inline footer with the vote control (up/down with
  counts) and the emoji-reaction control, gated separately by the
  publisher's vote and reaction flags: each half renders only when its
  flag is on, and when both flags are off the footer renders nothing at
  all, leaving the card compact.
- Resolved live, inventory confirmation (4): list cards never render a
  vote/reaction footer in any flag combination — the footer exists only
  on board cards.

## Opening cards and highlight (SPC-030, SPC-031)

- Clicking a card opens the issue overlay, records the issue in the
  address's open-issue parameter, and preserves the current view. In
  list layout the open row additionally gains the active highlight.
- Exactly one row is ever highlighted: opening another issue moves the
  highlight to it, and closing the overlay clears the highlight
  entirely. Closing also drops the open-issue parameter while keeping
  view and filter parameters.

## Read-only board (SPC-032)

- Nothing on the board can be moved or edited: cards carry no draggable
  marking, no drag handles, and no inline inputs or editable regions in
  either layout. Attempting to drag a card leaves the order untouched,
  and board interactions never call a mutation endpoint.

## No live refresh (SPC-033)

- Refocusing the tab never refetches the issue list and never changes
  the order: issues published on the server while the visitor looks
  away stay invisible until the visitor navigates, and the rendered
  order is byte-identical before and after.
- Reconnecting deviates (NEWFRONT-455, pinned by a `bug:` scenario): the
  list fetch opts out of focus revalidation but leaves reconnect
  revalidation at the data layer's default, so an offline/online cycle
  refetches and newly published issues appear unprompted. Intended
  behavior is reconnect behaving like refocus — no fetch, loaded data
  stays.
