# Exports (CSV and PDF) — build spec (EXP-001–EXP-024)

Area: Exports. Parent epic: NEWFRONT-87. Inventory:
`.ai_design/new_frontend_implementation/parity/inventory/exports.md` (EXP-001–EXP-024).
Oracle scenarios: `apps/web_new/e2e/parity/exports/workspace-exports.spec.ts`,
`local-file-exports.spec.ts`, `page-export.spec.ts`; driver contract:
`apps/web_new/e2e/parity/drivers/parity-driver.ts`. Editions: oss, cloud,
desktop, with no observed differences (EXP-024).

This is the single build spec. It replaces the three oracle-run notes
(`exports-workspace.md`, `exports-page.md`, `exports-local-file.md`), which
described behavior before the split assigned ownership. Where a prior note
disagrees with the inventory or an oracle scenario, the row/scenario wins.
Every behavioral claim below cites the old source read, the oracle scenario, or
the oracle driver read that establishes it. The old code was used as reference
only: no old code, strings, styles, or assets are copied into the new tree.

Endpoint shorthand: `HISTORY` = `POST/GET /api/workspaces/{ws}/export-issues/`;
`ACTIVITY` = `POST /api/workspaces/{ws}/user-activity/{userId}/export/`.

## 0. Ownership and seams

Six boundaries are fixed here so slices do not duplicate or fork each other.

### 0.1 Workspace settings frame and exports guide (NEWFRONT-548 / 768)

The settings-entry slice owns the `/settings/exports` frame: address, settings
sidebar entry, heading, explanatory paragraph, breadcrumb, tab title, and the
workspace admin/member gate with guest denial. The exports area owns one guide
component containing the export form and the previous-exports history. The
guide is not a second page frame and must not repeat the heading, explainer,
breadcrumb, title, or role gate.

The typed seam is `registerWorkspaceExportsGuide` in
`apps/web_new/src/features/workspace-settings/lib/exportsGuide.ts`, with
`WorkspaceExportsGuideProps = { workspaceSlug: string }`; registration returns
an unregister cleanup and the frame reads it with
`useWorkspaceExportsGuide()`. Evidence: NEWFRONT-548 as merged in
`66197a3da` (`lib/exportsGuide.ts`, `ExportsSettingsPage.tsx`, and their
tests), and workspace-settings spec §§33/41.

The merged wiring is: the exports section route renders
`SettingsSectionOutlet` and registers `ExportsSettingsPage` in the settings
page registry; the frame renders the heading, explanation and one guide slot,
and substitutes a quiet pending state while the slot is empty. Nothing imports
an exports-area registration yet, so the slot is empty in production. The
correct wiring is: the exports guide registers through
`registerWorkspaceExportsGuide` for as long as the guide is mounted, and the
section route or the frame mounts that registration (an effect is acceptable).
NEWFRONT-768 owns making that registration import real and proving it with a
route-level test; it must register into this seam, not replace or duplicate the
frame.

EXP-001/002 prove the frame through the EXP oracle while the frame code comes
from 548. The guide owns everything from the form downward: project and format
pickers, submit, history rows, polling, paging, download/expiry cells, loading
and empty states. The frame owns only page chrome and access.

### 0.2 Analytics shared CSV builder (NEWFRONT-735 → 769)

All three analytics data tables (insight table, priority-chart drill-down, and
state-breakdown table) call one local-file CSV builder. The module is
`apps/web_new/src/shared/export/csv.ts`, authored by NEWFRONT-735 as the first
cut of its table serializer; NEWFRONT-769 is blocked by 735, owns the export
rules exercised through the two oracle tables, and changes shared rules in that
same file rather than creating a second module.

Shipped API (NEWFRONT-735, merged as `8fe11330e`; NEWFRONT-769 adopts it as-is,
so this is the contract both areas build against):

```ts
export interface CsvColumn<Row> { label: string; value: (row: Row) => unknown }
export interface CsvFile { fileName: string; content: string }
export function buildCsvDocument<Row>(columns: readonly CsvColumn<Row>[], rows: readonly Row[]): string;
export function buildAnalyticsCsv<Row>(workspaceSlug: string, columns: readonly CsvColumn<Row>[], rows: readonly Row[]): CsvFile;
export function analyticsCsvColumns<Row>(columns: readonly { label: string; exportValue: (row: Row) => unknown }[]): CsvColumn<Row>[];
export function analyticsCsvFileName(workspaceSlug: string): string;
export function csvCell(value: unknown): string;
export function downloadCsvFile(file: CsvFile, environment?: CsvDownloadEnvironment): void;
export class CsvEmptyRowsError extends Error {}
```

Contract: the filename is `{workspaceSlug}-analytics.csv`; the first line is the
table's export headers; null/undefined/NaN and other unreadable cells become
`-`; zero rows throw `CsvEmptyRowsError` before any Blob or save; the text
starts with a UTF-8 BOM, quotes text cells (comma, quote, CRLF or edge
whitespace), leaves finite numbers bare, and records end in CRLF. Header and
cell values come from each table's export-column metadata, not from rendered
DOM. All three analytics tables call it — the chart drill-down
(`CustomInsightsSection`), the insight/state-breakdown table
(`StateBreakdownSection`) and the scoped dialog table (ANALYTICS slice E). The
two singles are proven through the tables by `local-file-exports.spec.ts`
EXP-014/015 (`analyticsExportRowCells("chart" | "insight")`,
`analyticsExportButtonCount() === 2`). Evidence:
`apps/web/core/components/analytics/export.ts` (builder and filename) and its
call sites `insight-table/root.tsx` (filtered row model),
`work-items/workitems-insight-table.tsx` (insight columns) and
`work-items/priority-chart.tsx` (drill-down columns);
`local-file-exports.spec.ts` EXP-014/015 pins the exact headers and file name.

The analytics area owns the tables; this builder is shared infrastructure and
must not be forked per table. Row ANL-016 owns the serializer fallback in the
analytics inventory; EXP-014/015 own its behavior through these tables.

### 0.3 Pages host and page-export dialog (NEWFRONT-770 / 773)

The page-export dialog belongs to Pages: PAGE-060 owns the detail-header menu
entry that opens it and PAGE-074 records the dialog's deep behavior (the
Pages-side proof of the same dialog EXP-016–021 prove from the exports side).
Exports builds the dialog, the local document/plain-text renderers, and a
registration seam.

Preferred seam, mirroring 548: Pages' page-detail toolbar owns the menu entry
and the currently mounted editor/page facts; the exports feature exports

```ts
export interface PageExportHost {
  pageTitle: string;
  projectId: string;
  workspaceSlug: string;
  getHtml(): string;
  getMarkdown(): string;
}
export type PageExportDialogComponent = (props: {
  host: PageExportHost;
  open: boolean;
  onClose(): void;
}) => React.ReactNode;
export function registerPageExportDialog(component: PageExportDialogComponent): () => void;
```

The Pages detail mounts one host slot (or reads a registry hook) and renders the
registered dialog only when open. Exports must not mount a dialog directly in
the route or duplicate the toolbar menu. NEWFRONT-770 builds the dialog,
renderers, and this seam; Pages supplies the menu entry and host props.

Row split: NEWFRONT-770 initially owned EXP-016–021 and 023/024, but EXP-023/024
are NEWFRONT-773's rows. The spec therefore makes 770 build the keyboard-operable
controls that 773 can prove, while 773 owns proving the keyboard/focus matrix
and the edition/desktop sameness rows. 770 must not claim 023/024 built. Evidence:
inventory row ownership, NEWFRONT-770 description, and the parent split.

### 0.4 Profile activity download (NEWFRONT-573 / 771)

The profile activity tab and its shell belong to the profile area (PROF-007,
NEWFRONT-573). Exports owns the download control's fetch, save, and working
state only. The tab owns the heading, list, paging, and permission gate around
it. The download control must not render on a profile shell that refused the
tab; the workspace gate means the control is visible to admins/members and never
to guests. Evidence: `profile.md` §3 and its seam section;
`apps/web/core/components/profile/activity/download-button.tsx`;
`local-file-exports.spec.ts` EXP-022.

Filename is the oracle's rule, not the server's:
`profile-activity-<click-time-ms>.csv`, matching
`/^profile-activity-(\d+)\.csv$/`, with the stamp within 120 seconds of the
click. Save the response bytes unchanged. Ignore the server's
`Content-Disposition: attachment; filename="workspace-user-activity.csv"`; there
is no "server name wins" branch. Evidence: the old download control's local
`Date.now()` name and `local-file-exports.spec.ts` EXP-022.

### 0.5 PDF dependency, renderer, and Inter bytes (NEWFRONT-770)

The app has no PDF renderer and the allowlist has none. Use
`@react-pdf/renderer` (4.x) through a lazy `import()` in an own wrapper module,
for example `features/exports/pageExport/pdfDocument.ts` or
`shared/pdf/render.ts`; never a static import from a component or route. It
satisfies the required embedded-subset path. A standalone `pdf-lib` probe also
produced the exact media boxes, but the chosen path must be one library and must
produce decoded text.

Font source: `@expo-google-fonts/inter` (MIT AND OFL-1.1) ships desktop TTFs,
including `400Regular/Inter_400Regular.ttf`. Register that file as family
`Inter` with the renderer, subset it through the renderer's fontkit pipeline,
and use it for every PDF text style. Do not copy TTFs from `apps/web`; do not
depend on `@fontsource/inter` for the PDF face, because that package ships
WOFF/WOFF2 only. `packages/kit/src/tokens.css` only names the family; it
supplies no PDF bytes.

Both libraries (`@react-pdf/renderer` and `@expo-google-fonts/inter`) are
runtime-dependency review items on the NEWFRONT-770 slice PR: add them to
`apps/web_new/allowlist.json` through review, regenerate `pnpm-lock.yaml` with
`pnpm install`, and report gzipped size. They must be lazy/own-wrapper imports
per the Stack page. This is a build blocker for 770: if the license or package
is out of its paths, 770 files a blocker issue rather than copying old assets.

Verified probe (candidate pipeline, not production code): registering the
`@expo-google-fonts/inter` TTF and rendering a page produced
`/MediaBox [0 0 595.280029 841.890015]` for A4 and `[0 0 612 792]` for Letter,
`/BaseFont /<subset>+Inter-Regular`, and decoded title/body text with the
oracle's CMap reader. The oracle also requires every `/BaseFont` to match
`/Inter/i`; do not fall back to standard-14 fonts.

### 0.6 Bug rows, re-pinning, and the gate (EXP-018 / EXP-023)

EXP-018 and EXP-023 record intended behavior plus live bugs. The new app builds
the intended behavior: image-free output drops every picture form, full-fidelity
output embeds uploaded pictures, every picker is Tab-reachable with a visible
focus ring, menus navigate by arrow/Enter, Escape does not desync a toggle, and
every control shows a focus change.

Pinned oracle bugs: EXP-018 → NEWFRONT-356 (Markdown image syntax survives
image-free) and NEWFRONT-470 (uploaded relative-URL picture loses its raster
under full fidelity); EXP-023 → NEWFRONT-471 (pickers `tabindex=-1`, menus
ignore arrows/Enter and desync on Escape, no focus style). NEWFRONT-360 is not a
pinned assertion: it is only the oracle's load-race retry helper
`landPageContent`; record it as context, never as behavior the new app must
reproduce.

The non-bug EXP-018 scenario (absolute-URL picture embeds under Everything and
image-free drops pictures while keeping text) is the one that must be green on
`apps/web_new`. The bug-pinned scenarios must not be edited to accept the bug.

Gate semantics: the report generator counts a row green only when every
scenario tagged with it passes, so a failing `bug:` scenario keeps the row red.
When intended behavior makes such a scenario fail, the area gate files a re-pin
sub-issue (oracle-scenario paths), records the divergence row by row, sends the
epic back to In Progress and waits. Parity is not locked to the old bug where
the row records intent. The inventory carries visible `(bug: NEWFRONT-…)`
markers on EXP-018/023 so the gate can identify them without reading titles.

## 1. Workspace exports settings page — frame (EXP-001, EXP-002)

- Address `/{workspaceSlug}/settings/exports`; the settings sidebar entry is
  visible to workspace admins and members, hidden for guests. Evidence: old
  settings exports route/header and settings registry; `workspace-settings.md`
  §1/§33; `workspace-exports.spec.ts` EXP-001/002.
- Admitted roles see heading `Exports`, the explanatory sentence pinned by the
  oracle, a breadcrumb containing `Exports`, and tab title
  `{workspaceName} - Exports`. The build spec must use the oracle-pinned copy,
  not old-source strings: heading
  `Exports`; description
  `Export your project data in various formats and access your export history with download links.`
  Evidence: `workspace-exports.spec.ts` EXP-001.
- Guest: not-authorized view instead of the form, same address. Signed-in user
  with no workspace standing: workspace-not-found view, same address, because
  the layout guard fires before the page's dimmed branch (that branch is
  unreachable). Evidence: same scenario EXP-002.
- A member who may create in no project still reaches the form; the frame does
  not disable the guide because of project permissions. Evidence: EXP-002 plus
  the picker scenario EXP-003.
- The guide mounts below the frame's heading/explainer through the 548 seam
  (§0.1). The frame does not render a stub form. Loading/error for the page frame
  belongs to 548; the exports guide owns its own form/history states.

## 2. Workspace export form (EXP-003, EXP-004, EXP-005)

- Two pickers then the primary export action: project and format. No rich-filter
  control: the old form's filter block is commented out and the request sends
  only provider/project/multiple. Evidence:
  `core/components/exporter/export-form.tsx`.
- Project picker lists only projects where the signed-in user may create
  content, each option showing project key and name; search matches name or key;
  selecting collapses the button to a comma-separated key list; clearing shows
  `All projects`; empty selection means all. A member with no creatable project
  keeps an enabled picker with no options: below-member tiers never reach the
  form. Evidence: old form's `projectsWithCreatePermissions`/intersection;
  EXP-003 scenario.
- Format picker offers exactly CSV, Excel, JSON; labels map to provider ids
  `csv`, `xlsx`, `json`. The selection travels with the request and labels the
  history row. Evidence: old form/`EXPORTERS_LIST`; EXP-004 scenario.
- Submit sends `{ provider, project: string[], multiple }`, with
  `multiple = project.length > 1` and no filters. The contracts slice
  NEWFRONT-765 encodes this; do not re-derive in the component. Evidence:
  old `ExportCSVToMail`; EXP-005 scenario and captured submit bodies.
- While in flight the button reads `Exporting...` (oracle-pinned working label),
  is non-interactive, and cannot double-submit. Success toast (`Export
successful`, message naming the format); failure toast (`Error` /
  `Export was unsuccessful. Please try again.`); the button returns to `Export`
  and usable either way. Evidence: old form toast branches; EXP-005 scenario.
- On success, revalidate the history list so a fresh request appears without a
  reload. Evidence: old `mutateServices`; EXP-006.
- Backend behavior (reference, encoded by 765): POST accepts only the three
  providers (anything else 400 `{ error }`); empty project list expands to the
  caller's active, unarchived project memberships; it stores a queued export
  record and schedules the file build; response is `{ message }`. Evidence:
  `apps/api/pi_dash/app/views/exporter/base.py`, `serializers/exporter.py`.

## 3. Previous exports history (EXP-006–EXP-013)

### Contract facts (NEWFRONT-765)

GET requires both `per_page` and `cursor`; missing either returns 400
`{ error: "per_page and cursor are required" }`. `order_by` defaults to
`-created_at`. The first read is
`cursor=10:0:0&per_page=10`. Envelope keys the UI may read include `results`,
`next_cursor`, `prev_cursor`, `next_page_results`, `prev_page_results`, plus the
paginator's count fields. A record has `id`, `created_at`, `updated_at`,
`project` (string[] or null), `provider`, `status`
(`queued|processing|completed|failed`), `url` (nullable), `initiated_by`,
`initiated_by_detail`, `token`, `created_by`, `updated_by`.
`initiated_by_detail` is the user-lite shape with `id`, `first_name`,
`last_name`, `avatar`, `avatar_url`, `display_name`, `is_bot` and no `email`.
Evidence: `views/exporter/base.py`, `serializers/exporter.py`,
`serializers/user.py`; names repeated here only as the contract 765 owns.

### Rows and states

- EXP-006 — fresh request joins history without manual reload (form submission
  invalidates the history query). Evidence: old `mutateServices`; scenario.
- EXP-007 — row shows initiated-by identity (avatar, else first character of
  display name/first name; no email in the lite payload), human-formatted start
  date, project count as `N project(s)`, format label (`CSV`/`Excel`/`JSON`),
  and status marker. Evidence: old `column.tsx`; driver
  `ExportsSettingsHistoryRow`; scenario.
- EXP-008 — status markers: queued/neutral (the inventory's waiting marker),
  processing/amber, completed/green, failed/red; unknown strings must render an
  unknown marker rather than crash. The server never stores `expired`; the old
  row has an unused expired pill style, so do not build a fifth status.
  Evidence: old column branches; driver; scenario.
- EXP-009 — while any row is `processing`, re-read the list every 3 seconds;
  stop polling when none is processing. A `queued` record alone does not start
  polling (old code tests only `processing`). Manual refresh is always present
  beside the section heading, reads `Refresh status` when idle and `Refreshing`
  while the re-read runs. Evidence: old `prev-exports.tsx` interval and refresh;
  EXP-009 scenario.
- EXP-010 — ten rows per page; back/forward controls above the table, disabled
  at the ends; pagination row renders only when results exist; use
  `prev_page_results`/`next_page_results` and cursors, never results.length.
  Evidence: old section; EXP-010 scenario.
- EXP-011 — completed and younger than seven days: download control is an
  anchor with a new-tab target pointing at the record's file URL. Any other
  status in that cell: plain dash. Evidence: old column; EXP-011 scenario.
- EXP-012 — expiry is client-side: add seven days to `created_at` and compare
  with now; older loses the download control and shows `Expired` in the download
  cell, in the mounted settings table. The legacy single-export card has no
  importer in the old app and is dead code; do not build it. Evidence: old
  `checkExpiry`/column, `single-export.tsx`, inventory coverage checklist;
  EXP-012 scenario.
- EXP-013 — before the first response show a skeleton placeholder; with no
  records show an illustrated empty view with title `No exports yet` and the
  oracle-pinned explanation
  `You don't have any export records right now. Once you export data, all records will appear here.`
  Evidence: old `ImportExportSettingsLoader` and empty state; EXP-013 scenario.
- Empty choice semantics: `project: null` means zero projects and must not
  crash the count; unknown provider/status render sensible unknown labels.

Error states: the histories use the list response; a failed read must not
replace the section with a raw exception. Build a retryable error treatment
consistent with the shell, keeping manual refresh usable. The oracle does not
pin a distinct history error copy; do not invent a dead-end.

## 4. Analytics local-file CSV exports (EXP-014, EXP-015)

- The work-items analytics tab renders the chart drill-down table first and the
  insight table second; each has its own collapsible search box and export
  action, so `analyticsExportButtonCount() === 2`. The state-breakdown table
  uses the same builder. Evidence: old analytics components; driver;
  EXP-014/015 scenarios.
- Search filters the first column live; export covers exactly the currently
  shown/filtered rows, never the unfiltered server set. Insight headers for the
  oracle data are `Project`, `Backlog`, `Started`, `Unstarted`, `Completed`,
  `Cancelled`; drill-down headers are `Priority`, `Count`. Evidence:
  EXP-014/015 scenarios.
- Filename `{workspaceSlug}-analytics.csv`; BOM, quoted text, bare numbers, CRLF;
  dash fallback for null/undefined; zero shown rows throws before any save so no
  download lands. Evidence: §0.2 sources and scenarios.
- Fully client-side: no export-shaped API request during the download.
  Evidence: scenarios' request capture and old builder.

## 5. Page-export dialog (EXP-016–EXP-021, 023–024)

### Open, defaults, and choices (EXP-016, EXP-017)

- Any signed-in user who can open a collaborative page detail reaches the
  dialog from the page header options menu `Export`. The dialog opens centered
  over a mounted but inert page. Evidence: old `pages/editor/toolbar/options-dropdown.tsx`
  and `export-page-modal.tsx`; EXP-016 scenario.
- Heading `Export page`. Rows: format (PDF, Markdown), content scope
  (Everything, No images), and a page-size row only for PDF. Page sizes, in
  order: `A4`, `A3`, `A2`, `Letter`, `Legal`, `Tabloid`. Fresh defaults:
  PDF / Everything / A4, size row visible. Evidence: old modal constants and
  conditional row; EXP-016 driver facts.
- Switching to Markdown hides the size row but preserves its chosen size;
  switching back restores it. Size has no effect on Markdown. Evidence:
  EXP-017 scenario.
- A `Cancel` secondary action and an `Export` primary action; while saving the
  primary reads `Exporting` and is non-interactive. Evidence: old modal;
  EXP-021 clicks the exact `Export` name and EXP-023 tabs to both footer names.

### Content scope (EXP-018)

- Intended behavior: Everything keeps all pictures; No images strips every
  picture form (HTML image elements and Markdown image references) while
  keeping text. Pictures with absolute fetchable URLs embed as image objects;
  uploaded stack-asset pictures embed their raster under Everything. Mentions
  resolve to readable display names; embedded issue cards are dropped. Evidence:
  old modal + `useParseEditorContent`; page-export spec.
- Live bug pins (do not copy into implementation): Markdown image syntax
  survives No images (NEWFRONT-356); uploaded relative-URL picture draws a frame
  but loses its raster under Everything (NEWFRONT-470). The non-bug scope
  scenario must be green; bug scenarios remain bug-pinned until re-pinned
  (§0.6). NEWFRONT-360 is context only (oracle load-race retry).

### PDF output (EXP-019)

- Confirm renders the page title plus parsed body into a white padded document
  in the chosen size, fully client-side, and saves via browser download. Evidence:
  old modal/document.tsx; EXP-019.
- Exact oracle checks (`page-export.spec.ts` EXP-019): file starts `%PDF`;
  every `/MediaBox` is exactly `0 0 595.280029 841.890015` for A4 (no
  trailing-zero normalization) and exactly `0 0 612 792` for Letter; every
  `/BaseFont` matches `/Inter/i`; decoded text contains the page title and a body
  marker; no export-shaped API traffic. Filename
  `{title-slug}-a4.pdf` / `{title-slug}-letter.pdf`.
- Title slug: lowercase, every character outside `[a-z0-9-_]` becomes `-`, runs
  of `-` collapse; size lowercased. Evidence: old modal and oracle helper.
- Renderer/font source and lazy wrapper: §0.5. The typeface must be an embedded
  subset (as verified), not a standard-14 fallback; A4/Letter numbers are
  renderer output checks, not values to hand-place in the PDF.

### Markdown output (EXP-020)

- Confirm converts the live editor body to plain-text Markdown honoring the
  scope and saves locally; filename `{title-slug}.md`. No API call. Evidence:
  old modal; EXP-020 scenario.

### Feedback, failure, reset (EXP-021)

- Success raises a confirmation (`Page exported successfully.` per oracle),
  closes, and resets to PDF / Everything / A4 for the next open. Evidence:
  old modal toast/reset; EXP-021.
- Failure keeps the dialog open, shows an error notice containing `Page could not
be exported.` (old copy `Page could not be exported. Please try again
later.`), preserves choices, and returns the primary action to its idle
  `Export` label. The oracle forces this through a sabotaged browser save
  primitive. Evidence: old catch branch; EXP-021.
- Cancel/dismiss downloads nothing and discards choices; reopening shows
  defaults. Evidence: EXP-021.

### Keyboard and focus (EXP-023) — NEWFRONT-773 proves

- Intended: every export control is Tab-reachable with a visible 2px accent
  focus ring; menus move focus/selection with arrows and select with Enter;
  Escape dismisses the dialog and closes a menu without desyncing its toggle;
  content keys outside text fields do nothing; no custom area-wide shortcuts.
- Live bug NEWFRONT-471: pickers have `tabindex=-1`, menus ignore arrows/Enter,
  Escape leaves toggle state desynced, and controls show no focus change. The
  oracle pins this and the row keeps the intended behavior. NEWFRONT-770 builds
  the keyboard-operable controls; NEWFRONT-773 proves EXP-023 and EXP-024. Do
  not claim 023 built from 770. Evidence: page-export spec EXP-023 and driver
  focus facts.

### Editions and desktop (EXP-024) — NEWFRONT-773 proves

Every export surface renders ungated in oss/cloud/desktop; traffic goes only to
the known endpoints and carries no desktop/edition/license/billing/upgrade
markers; no gate markers render. No edition-gated or desktop-only export forks
exist in old source (source sweep); the area's surfaces are identical. Evidence:
EXP-024 scenario and inventory row.

## 6. Profile activity export (EXP-022)

- Control sits beside the profile activity heading, visible only to workspace
  admins/members; guests never reach it. The profile area owns the tab/shell
  (§0.4). Evidence: `profile.md` §3; old download control; EXP-022.
- Idle label `Download today's activity`; while fetching `Downloading`; returns
  to idle after. Sends today's local date as `{ date: "yyyy-mm-dd" }` to
  `ACTIVITY`. Evidence: old control; EXP-022.
- Saves the returned CSV bytes unchanged as
  `profile-activity-<click-time-ms>.csv`; ignore the server filename (§0.4).
- The endpoint requires `date` (400 otherwise), filters that user's issue
  activities on that date, restricted to projects where the caller is active
  (up to 10,000), and returns a nine-column CSV: Actor name, Issue ID, Project,
  Created at, Updated at, Action, Field, Old value, New value. Evidence:
  `views/workspace/base.py` `ExportWorkspaceUserActivityEndpoint`; EXP-022.
- Profile contract already exists in api-client; do not duplicate it.

## 7. Contract and API summary

| Capability               | Call            | Parameters / shape                                                                                                                                                                                      |
| ------------------------ | --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Request workspace export | `POST HISTORY`  | body `{ provider: "csv"\|"xlsx"\|"json", project: string[], multiple: boolean }`; empty project = caller's active/unarchived memberships; response `{ message }`                                        |
| Read history             | `GET HISTORY`   | requires `per_page` + `cursor`; `order_by` default `-created_at`; first read `cursor=10:0:0&per_page=10`; end flags `next_page_results`/`prev_page_results`; user-lite `initiated_by_detail` (no email) |
| Analytics / drill-down   | none            | local builder in `shared/export/csv.ts`                                                                                                                                                                 |
| Page export              | none            | local PDF/Markdown; lazy renderer wrapper                                                                                                                                                               |
| Profile activity         | `POST ACTIVITY` | body `{ date: "yyyy-mm-dd" }`; response CSV bytes; server filename ignored                                                                                                                              |

NEWFRONT-765 owns zod contracts and contract tests for HISTORY; do not duplicate
them. The profile contract is already present. Analytics/page exports have no
server contract.

## 8. Scenario and gate notes

- Every scenario mints its own owner/workspace and drains seed projects; fresh
  worlds give pristine export history. Bulk/aged/stuck/failed history is
  planted server-side and labeled in the scenario. Real submissions prove the
  request path, toast, and history join. Evidence: workspace-exports spec header.
- Downloads go through the UI against the live stack, never a stubbed file.
  PDF bytes are introspected for magic, media boxes, `/BaseFont`, decoded text,
  and image objects; Markdown is compared as text. Evidence: page-export spec
  helpers and EXP-019/020.
- The exports area gate runs all three spec files against `apps/web_new` for
  every edition/desktop row. Rows stay red when any tagged scenario fails. The
  gate never edits assertions to accept a pinned bug; it files a re-pin
  sub-issue and waits (§0.6). It adds workspace routes only through the
  coexistence mechanism that supports `/{ws}/…` prefixes; if that is not
  available, it records the gap rather than inventing a prefix.
- No new row is needed for behavior omitted by EXP-001–024: the old dead-code
  modal/single-export card never mounts; the old commented-out rich filters
  never send; the `expired` status is unreachable.

## 9. Row-to-slice map

| Slice        | Rows                      | Owns                                                                   |
| ------------ | ------------------------- | ---------------------------------------------------------------------- |
| NEWFRONT-768 | EXP-001, EXP-002          | settings frame wiring through 548 seam, route-level registration proof |
| NEWFRONT-766 | EXP-003, EXP-004, EXP-005 | form, pickers, submit, feedback                                        |
| NEWFRONT-767 | EXP-006–EXP-013           | history rows, states, polling, paging, download, expiry, empty/loading |
| NEWFRONT-769 | EXP-014, EXP-015          | shared CSV builder rules and two table wirings (blocked by 735)        |
| NEWFRONT-770 | EXP-016–EXP-021           | page dialog, renderers, PDF dependency/font, Pages seam                |
| NEWFRONT-771 | EXP-022                   | profile download fetch/save/working state                              |
| NEWFRONT-773 | EXP-023, EXP-024          | keyboard/focus and edition/desktop proofs (770 builds controls)        |
| NEWFRONT-765 | contract rows for HISTORY | zod contracts + tests                                                  |
| NEWFRONT-772 | all rows                  | area gate: parity + migrated routes                                    |
