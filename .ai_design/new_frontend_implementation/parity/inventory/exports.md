# Feature inventory: Exports (CSV, PDF)

Area: Exports (CSV, PDF) (Phase 3). ID prefix `EXP-`.
Editions: oss, cloud, desktop (no differences found; every capability below behaves the same in all three).
Source behavior learned by reading the old frontend (routes, components, services listed in the coverage
checklist below). The old app was not run in this session; the oracle run (NEWFRONT-60) verifies each row
against the live app later. Written from behavior notes; no old code, strings, or assets copied.

Row format per the Parity page: ID, capability, who, edition, old entry point, API, acceptance,
parity test (empty for now), status (`not started`).

| ID | Capability | Who | Edition | Old entry point | API | Acceptance | Parity test | Status |
|----|------------|-----|---------|-----------------|-----|------------|-------------|--------|
| EXP-001 | Open the workspace exports settings page | Workspace admin or member | all | Workspace settings sidebar, exports entry; direct URL under the workspace settings section | session/membership reads | Admins and members see the page with its heading, explanatory text and breadcrumb; the browser tab title names the current workspace |  | not started |
| EXP-002 | Blocked exports page for viewers and outsiders | Guest, viewer, or non-member | all | Same settings URL as EXP-001 | membership reads | Users without member-level workspace rights see a not-authorized view instead of the form; signed-in users with no rights at all see the content dimmed and inert |  | not started |
| EXP-003 | Pick which projects to export | Workspace admin or member | all | Export form, project picker | project list already loaded client-side | The picker lists only projects where the user may create content, searchable by name and key; chosen projects show as a compact key list, an empty choice means every project, and people who may create nowhere get a disabled picker |  | not started |
| EXP-004 | Choose the export file format | Workspace admin or member | all | Export form, format dropdown | none client-side | One of three formats (comma-separated values, spreadsheet, structured JSON) can be selected; the choice travels with the export request and labels the history row |  | not started |
| EXP-005 | Request a workspace issue export | Workspace admin or member | all | Export form, export action | `POST /api/workspaces/{ws}/export-issues/` | Submitting sends the chosen projects (empty means all; several projects are flagged for separate files) and shows a working state on the button; success raises a confirmation naming the format and failure raises a retry prompt, with the button usable again either way |  | not started |
| EXP-006 | New export joins the history | Workspace admin or member | all | Previous-exports section after EXP-005 | `GET /api/workspaces/{ws}/export-issues` (paged) | A fresh request appears in the history list without a manual reload |  | not started |
| EXP-007 | History table content | Workspace admin or member | all | Previous-exports table | same list call as EXP-006 | Each row shows who started the export (with avatar or initial), when it was started, how many projects it covers, the format, and its current processing state |  | not started |
| EXP-008 | Export processing states | Workspace admin or member | all | Status marker in each history row | same list call as EXP-006 | In-flight, finished, failed and stale exports each carry a distinct marker so the user can tell at a glance which files are ready |  | not started |
| EXP-009 | Live refresh while an export runs | Workspace admin or member | all | Refresh control above the history | same list call as EXP-006, re-polled | The list re-checks itself every few seconds while any export is still running and settles once none are; a manual refresh control with a spinning indicator is always available |  | not started |
| EXP-010 | Page through export history | Workspace admin or member | all | Previous/next controls above the history | same list call as EXP-006 with page cursors | Ten records show per page with working back/forward controls that disable at the ends |  | not started |
| EXP-011 | Download a finished export file | Workspace admin or member | all | Download control in a finished history row | file URL returned on the history record | A finished, still-fresh export offers a control that opens the file in a new tab; anything else in that column shows a placeholder dash |  | not started |
| EXP-012 | Export file expiry after a week | Workspace admin or member | all | Same download column as EXP-011 | none (client-side date check) | Files older than seven days lose their download control and show an expired marker instead, in both the settings table and the legacy single-export card |  | not started |
| EXP-013 | Empty and loading states of the history | Workspace admin or member | all | Previous-exports section | same list call as EXP-006 | Before the first response a skeleton placeholder shows; when no exports exist an illustrated empty message explains that future exports will be listed there |  | not started |
| EXP-014 | Export a filtered analytics table as a file | Workspace admin or member viewing analytics | all | Export action in an analytics insight table header | none (built locally in the browser) | The downloaded file contains exactly the currently filtered rows with the table's export columns; missing cell values fall back to a dash placeholder and the filename carries the workspace name |  | not started |
| EXP-015 | Export a chart drill-down table as a file | Workspace admin or member viewing analytics | all | Export action in a chart drill-down data table | none (built locally in the browser) | Same local-file behavior as EXP-014 over the drill-down rows and columns, including the workspace-named file |  | not started |
| EXP-016 | Open the page-export dialog | Anyone who can open a collaborative page | all | Page editor toolbar, export option | none | The dialog opens over the page offering format, content-scope and (for documents) page-size choices, pre-set to sensible defaults |  | not started |
| EXP-017 | Page export format choice with conditional page size | Anyone who can open a collaborative page | all | Export dialog, format and page-size pickers | none | Document and plain-text formats are offered; the page-size picker (common office paper sizes) is visible only for the document format and hides for plain text |  | not started |
| EXP-018 | Page content scope (with or without images) | Anyone who can open a collaborative page | all | Export dialog, content-scope picker | project-aware content resolution for embedded items | Full-fidelity and image-free variants are offered; the image-free variant strips pictures from both document and plain-text output |  | not started |
| EXP-019 | Save a page as a paginated document | Anyone who can open a collaborative page | all | Export dialog, confirm while document format is selected | none (rendered locally) | The page title plus body render into a paginated document in the chosen paper size using the product typeface; the file saves locally with a name derived from the page title plus the paper size |  | not started |
| EXP-020 | Save a page as plain text | Anyone who can open a collaborative page | all | Export dialog, confirm while plain-text format is selected | none (converted locally) | The page body converts to plain-text markup honoring the content-scope choice and saves locally with a name derived from the page title |  | not started |
| EXP-021 | Page-export feedback and dialog reset | Anyone who can open a collaborative page | all | Export dialog, confirm and close controls | none | Success closes the dialog with a confirmation and resets the choices for next time; failure keeps the dialog open with an error message; cancelling discards the choices without exporting |  | not started |
| EXP-022 | Download today's profile activity as a file | Workspace admin or member on a profile activity tab | all | Download control on the profile activity page | `POST /api/workspaces/{ws}/user-activity/{userId}/export/` | The control is visible only to admins and members, shows a working state while fetching, then saves the returned data as a locally generated file stamped with the current time |  | not started |
| EXP-023 | Keyboard and focus behavior (no custom shortcuts) | Everyone using keyboard | all | Export form, history controls, analytics export actions, page-export dialog | none | All export controls are reachable and operable by keyboard with visible focus; no screen in this area requires custom key bindings |  | not started |
| EXP-024 | Uniform behavior across editions and desktop | Everyone | all | Every entry point above, in each build | all APIs above | Export capabilities, permissions, formats and file handling are identical in the community, cloud and desktop builds; there is no edition-gated or desktop-only export behavior |  | not started |

## Coverage checklist

Every route file, top-level component folder, and API endpoint in the assigned sources, mapped to rows.
Endpoint paths below are backend contracts as observed from the old frontend's call sites.

### Route files (`apps/web/app/(all)/…`)

| Source | Covering rows |
|--------|---------------|
| `[workspaceSlug]/(settings)/settings/(workspace)/exports/page.tsx` | EXP-001, EXP-002 |
| `[workspaceSlug]/(settings)/settings/(workspace)/exports/header.tsx` | EXP-001 |
| `[workspaceSlug]/(projects)/profile/[userId]/activity/page.tsx` (download control host; rest of page is profile-area scope) | EXP-022 |

### Top-level component folders

| Source | Covering rows |
|--------|---------------|
| `core/components/exporter/` — `guide.tsx` (page composition) | EXP-005, EXP-006 |
| `core/components/exporter/` — `export-form.tsx` (project picker, format picker, submit) | EXP-003, EXP-004, EXP-005 |
| `core/components/exporter/` — `prev-exports.tsx` (history section: refresh, pagination, empty/loading states) | EXP-006, EXP-009, EXP-010, EXP-013 |
| `core/components/exporter/` — `column.tsx` (history table columns, state markers, download/expiry cells) | EXP-007, EXP-008, EXP-011, EXP-012 |
| `core/components/exporter/` — `export-modal.tsx`, `single-export.tsx` | Dead code: no importer anywhere in `apps/web` (verified by search); the modal duplicates the settings form (project picker, separate-files toggle, same create call plus navigation to the history page) and the card duplicates the history download/expiry cell. No rows; behaviors already covered by EXP-003 through EXP-012 |
| `core/components/analytics/` — `export.ts` (local file builder), `insight-table/root.tsx` (header export action), `work-items/workitems-insight-table.tsx` + `work-items/priority-chart.tsx` (call sites) | EXP-014, EXP-015 |
| `core/components/pages/modals/export-page-modal.tsx` (dialog, format/scope/size choices, local save, feedback) | EXP-016, EXP-017, EXP-018, EXP-019, EXP-020, EXP-021 |
| `core/components/editor/pdf/document.tsx` (paginated document renderer: typeface registration, paper size, page furniture) | EXP-019 |
| `core/components/profile/activity/download-button.tsx` (permission-gated download control, local file save) | EXP-022 |

### API endpoints observed from these sources

| Endpoint | Covering rows |
|----------|---------------|
| `POST /api/workspaces/{ws}/export-issues/` | EXP-005 |
| `GET /api/workspaces/{ws}/export-issues` (cursor-paged) | EXP-006, EXP-007, EXP-008, EXP-009, EXP-010, EXP-013 |
| `POST /api/workspaces/{ws}/user-activity/{userId}/export/` | EXP-022 |

No other source mapped to zero rows. No `bug:` rows: nothing observed contradicted its evident intent; the oracle run
marks any such scenario if the live app disagrees. Easy-to-miss behaviors called out as their own rows:
EXP-002, EXP-008, EXP-009, EXP-012, EXP-013, EXP-022, EXP-023, EXP-024.
