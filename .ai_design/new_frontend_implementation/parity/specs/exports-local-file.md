# Analytics and profile local-file exports — area spec (NEWFRONT-382)

Behavior learned by reading the old analytics export builder, the
insight-table and chart components that call it, the profile activity page
plus its download control, the user-activity export endpoint, and the chart
data parser. Prose only: no old code, strings, styles, or assets are
reused. New code (driver extension + oracle specs) is written from this
spec. Covers inventory rows EXP-014, EXP-015, EXP-022.

Endpoint shorthand (backend contracts, also used by the inventory): `BASE` =
`/api/workspaces/{ws}/user-activity/{userId}/export/`.

## Analytics tables and their export actions (EXP-014, EXP-015)

- The work-items analytics tab renders, from top to bottom: totals, a
  created-vs-resolved chart, a customized-insights section (a priority chart
  with its drill-down data table beneath it), and the insight table (project
  or assignee rows with state counts). Both data tables pair a collapsible
  search box with an export action, so the tab carries two identically
  labeled export actions whenever analytics data exists: the drill-down
  table's first, the insight table's second.
- Each table's search box starts collapsed behind an icon-only button and
  expands into a text input on click. Typing filters the table's first
  column live (project names in the insight table, breakdown labels in the
  drill-down table). The count text beside the box names the row count and
  the grouping ("N Projects", "N Priority").
- The insight table's export covers exactly the currently shown (filtered)
  rows with the table's export columns: the grouping name plus one numeric
  column per work-item state. The drill-down export covers exactly the
  shown breakdown rows with its export columns: the breakdown label plus
  the count (and one column per breakdown series when the chart groups by
  a second dimension).
- Missing cell values fall back to a dash placeholder in the file builder.
  Neither table's value functions can produce a missing value through the
  UI (they coalesce to empty text or zero before the builder runs), so the
  fallback is defensive: the oracle scenarios pin exact cell equality on
  complete data rather than manufacturing a missing cell.
- Exporting with zero shown rows throws inside the file builder before any
  save, so no file lands (verified live: filtering to no match and
  exporting produces no download event).
- The file is built and saved entirely in the browser: no API call is
  involved. The suggested file name carries the workspace slug followed by
  the analytics suffix with a CSV extension. Content is comma-separated
  with a byte-order mark, quoted text cells, bare numbers, and
  carriage-return line endings; the first row holds the export column
  headers.

## Profile activity download (EXP-022)

- The profile activity page (`/{workspaceSlug}/profile/{userId}/activity`)
  lists one user's workspace activity under a "Recent activity" heading
  with a download control beside it. The control is permission-gated at the
  workspace tier: admins and members see it on any user's activity page,
  while below-member tiers never reach the activity view at all (the layout
  guard renders its no-results view instead).
- Clicking the control posts the current day (yyyy-mm-dd payload date) to
  the user-activity export endpoint and shows a working state on the
  button while the fetch runs, returning to its idle label afterwards.
- The endpoint requires the date, collects that user's issue activities
  created that day (up to ten thousand rows), and returns them as CSV text:
  a nine-column header (actor, issue reference, project, timestamps,
  action, field, old and new values) followed by one quoted row per
  activity.
- The page saves the returned bytes as a locally generated file whose name
  stamps the click time in milliseconds with a CSV extension. The saved
  file carries the server's bytes unchanged.

## Scenario data strategy

- Every scenario mints its own owner and workspace, drains the backend
  seed projects, and creates exactly the projects and work items it needs,
  following the established harness pattern. Fresh worlds with distinct
  priorities give the chart a multi-row breakdown and the insight table one
  row per project.
- Analytics downloads go through the UI against the live stack and assert
  the download event (file name plus parsed cell content), never a stubbed
  file. The profile download goes through the UI with its request stalled
  once to prove the working label, then released; the saved bytes are
  compared for equality with the same endpoint called directly, and the
  request body is read off the live request.
- Below-member gating is proven by opening the same activity page as a
  workspace guest and asserting the control never renders; member access is
  proven the same way with a workspace member who holds no project
  membership, showing the gate is workspace tier rather than self-only.
