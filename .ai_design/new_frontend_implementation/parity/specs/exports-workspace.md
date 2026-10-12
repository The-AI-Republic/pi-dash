# Workspace issue exports — area spec (NEWFRONT-381)

Behavior learned by reading the old exports settings route
(`[workspaceSlug]/(settings)/settings/(workspace)/exports/` page and header),
the exporter components (`core/components/exporter/` page composition, export
form, history section, history columns, legacy single-export card), the
workspace settings registry entry, and the backend export-issues endpoint plus
the exporter history model. Prose only: no old code, strings, styles, or
assets are reused. New code (driver extension + oracle specs) is written from
this spec. Covers inventory rows EXP-001–EXP-013.

Endpoint shorthand (backend contracts, also used by the inventory): `BASE` =
`/api/workspaces/{ws}/export-issues/`.

## Page frame (EXP-001, EXP-002)

- The page lives at `/{workspaceSlug}/settings/exports` and is listed in the
  workspace settings sidebar under the Exports entry, which is itself gated to
  workspace admins and members.
- Admins and members see a settings page with a heading naming Exports, one
  paragraph of explanatory text about exporting project data in several
  formats and reaching past exports through download links, and a breadcrumb
  trail in the settings header naming the Exports section. The browser tab
  title names the current workspace followed by the Exports section.
- The permission gate is workspace-level (admin or member). A signed-in
  workspace user below that level (the view-only guest tier) sees a
  not-authorized view instead of the form: an illustration with a heading
  stating the visitor is not authorized to view the page.
- A signed-in user with no workspace membership at all still has the page
  route resolve, but the content renders dimmed and inert rather than as a
  working form. (Live recon during the oracle run pins the exact dimmed
  shape; the gate code renders the same content wrapper with a dimming
  treatment when no membership record exists.)
- The inventory's "viewers" wording maps to the workspace guest tier: the
  product has no workspace viewer role (admin / member / guest only), and
  guests are the read-only tier the gate refuses. "Outsiders" maps to guests
  plus project-level viewers without workspace standing.

## Export form (EXP-003, EXP-004, EXP-005)

- The form has two pickers followed by a primary Export action: a project
  picker ("Exporting project") and a format picker ("Format").
- The project picker lists only projects where the signed-in user may create
  content, each row pairing the short project key with the project name. The
  picker is searchable by name and key. Chosen projects collapse into a
  compact comma-separated key list on the picker button; with nothing chosen
  the button reads as all projects, and submitting with an empty choice
  exports every project the user belongs to. A user who may create in no
  project gets a disabled picker.
- The format picker offers three file kinds: comma-separated values,
  spreadsheet (Excel), and structured JSON. The choice is part of the export
  request and also labels the resulting history row (CSV / Excel / JSON).
- Submitting posts the provider id, the chosen project id list (empty means
  all), and a flag marking multi-project requests for separate files. The
  flag is true exactly when more than one project is chosen; the request
  body, not just the stored record, carries it.
- While the request is in flight the Export button shows a working state
  (Exporting...) and cannot double-submit. On success a confirmation toast
  names the chosen format; on failure an error toast prompts to retry. The
  button returns to its usable state either way.
- The backend accepts only the three known providers (anything else is a
  400), fills an empty project list with the requester's unarchived
  projects, stores the request as an export-history record in the initial
  queued state, and hands the file build to a background task.

## History list (EXP-006, EXP-007, EXP-008, EXP-009, EXP-010, EXP-013)

- Below the form sits the previous-exports section. A fresh request appears
  there without a manual reload (the submit path revalidates the list).
- Each history row shows who started the export (their avatar image, or the
  first character of their address/name when they have no avatar, beside
  their display name), when it started (human-formatted date), how many
  projects it covers ("N project(s)"), the format label matching the
  request, and the current processing state as a distinct marker.
- Processing states and their markers: queued (the initial state, neutral
  gray marker — this is the inventory's "stale"/waiting marker; the server
  never stores an `expired` status), in-flight processing (amber marker),
  finished (green marker), failed (red marker). The four read
  unambiguously at a glance. (An `expired` marker style exists in the row
  code for a status value the server never writes; expiry surfaces in the
  download column instead — see below.)
- While any export is still running, the section re-checks the list every
  few seconds on its own and settles once none are running. A manual
  refresh control is always available next to the section heading and shows
  a spinning working state (Refreshing) while the re-read is in flight.
- History pages ten records at a time with back/forward controls above the
  table. The controls disable at the ends (back on the first page, forward
  on the last) and the pagination row only renders when records exist.
- Before the first list response a skeleton placeholder shows. When the
  workspace has no exports at all, an illustrated empty view explains that
  there are no export records yet and that future exports will be listed
  there.

## Download and expiry (EXP-011, EXP-012)

- A finished export that is still fresh offers a download control in its row
  that opens the file in a new tab (anchor with a new-tab target pointing
  at the file URL). Any other cell state — still running, failed, or
  waiting — shows a plain dash placeholder instead of a control.
- Files older than seven days lose the download control in both the settings
  history table and the legacy single-export card: the cell renders an
  expired marker instead. The seven-day check is client-side (record age
  plus seven days compared against today); the server's own file cleanup
  runs on a slightly longer clock (it clears file links after eight days),
  so a record can briefly show the client-side expired marker while its
  link still exists. Oracle scenarios manufacture old records with a
  server-side backdate and assert the table cell; the assertion text names
  the cell, not the cleanup task.
- The legacy single-export card (`single-export.tsx`) has no importer
  anywhere in the old app (verified by search, as the inventory's coverage
  checklist records), so it never mounts; its format/status/expiry cells
  duplicate the table's. The oracle expiry scenario asserts the mounted
  settings table, which is the only surface a user can reach.

## Scenario data strategy

- Every scenario mints its own owner and workspace, drains the backend seed
  projects, and creates exactly the projects it needs, following the
  established harness pattern. Export history is workspace-scoped, so fresh
  worlds give each scenario a pristine history.
- Real submissions go through the UI against the live stack (proving the
  request path, the toast, and the history join). Bulk history (pagination
  past ten) and aged records (expiry) are manufactured server-side through
  the Django shell helper and labeled as such in the scenario; the one
  record each of those scenarios asserts through the UI is still created
  (or completed) through the real path where the row requires it.
- A planted `processing` record with no background task behind it stays
  running forever, which makes the auto re-poll deterministic: the section
  must keep re-reading while it is present and settle once it is flipped
  to a terminal state.
