# Project settings — features, estimates, repository, automations, schedulers: area spec (NEWFRONT-566)

Rows PSET-043–061. Written from reading the old feature tabs and
combined list, the automations rows, the scheduler bindings
components, the repository tab, and the settings estimates entry
with its embedded shared component — plus the inventory Acceptance
column and the features, automations, schedulers and
estimates-github oracle scenarios. No old code, strings, styles or
assets reused. Oracle scenarios are written from this spec and proven
against the running old app.

Target rows (inventory describes the post-main-PR target,
NEWFRONT-156 rule): PSET-058 normalizes stale runner-pool selections
back to the project default (target per main#561); PSET-059 badges a
finished finite series as Completed once the API reports
series_exhausted (needs main#559), and disabled or uninstalled
bindings stop firing (target per main#561). Neither marker exists in
the old checkout; the new surface must add them.

## Dedicated feature tabs (PSET-043–047)

Each of the five tabs — saved views (PSET-043), timeboxing periods
(PSET-044), the triage inbox (PSET-045), sub-project grouping
(PSET-046) and free-form documents (PSET-047) — renders
the same reusable switch row with its own heading and short
explainer. The switch reflects one boolean project flag read from
the already-loaded project record, with no per-tab fetch or loader
of its own. Toggling sends
`PATCH /api/workspaces/{ws}/projects/{pid}/` with a single-key
payload flipping that flag, wrapped in pending/success/failure
notices, and the corresponding product area appears or disappears
project-wide. Non-admin members never see the switch: the page
wrapper returns a not-authorized view without admin standing.

## Combined feature list (PSET-048)

The five-entry combined card list (a marker plus title and explainer
per feature, no per-feature child settings) exists in the tree but
mounts only inside the post-project-create sheet, which pairs it
with a footer offering close and open-project exits. Toggling any
entry writes the matching single flag through the same project
update. The settings sidebar never renders this combined list — it
renders navigation groupings only. Creation closes the dialog with a
success notice and no feature step ever appears in the modal flow,
as the oracle pins; spec and test text must not place the combined
list in Settings.

## Estimates entry (PSET-049)

The settings estimates page is a thin shell — title, admin gate,
header — embedding the shared estimates component with the
workspace, project and admin standing. The page owns only the shell;
everything below reuses the shared component.

Cross-epic seam: the loading screen, the illustrated empty state
with its creation action, the enable switch, the active-system row,
the archived-systems list and the create/edit/delete dialogs are
all owned by the estimates area (EST-014 contract): this area reuses
them and builds no local copies. Observed behavior of the reused
component: while the project-estimates fetch is pending a dedicated
loader shows; with no active system the empty state offers creation,
which posts `POST .../projects/{pid}/estimates/` with the estimate
object (name, type, marked latest-used) plus its points, then
confirms or notices; the single active system renders as an editable
row with edit and delete callbacks; superseded systems render in a
separate read-only section with no row buttons; the enable switch
writes the project's active-estimate reference to the store's
current active id (or null to disable) with enabled/disabled notices,
reading switch state from the stored reference rather than list
position. Row actions render only when admin standing and editability
both hold.

Seam caveat the oracle pins: in this build the active row exposes
only deletion — the edit dialog is wired in the shared root but
unreachable here, since the row-actions component renders the delete
affordance alone. Spec and test text must not describe an
estimate-row edit control as reachable in Settings; at gate time
this area proves empty-state creation, active-system delete and the
enable switch, and treats row editing as owned by the estimates
area.

Estimate writes and point sub-paths used above belong to the
contracts issues (estimates, repository, deploy and scheduler
endpoints).

## Repository tab (PSET-050, PSET-051, PSET-052)

The tab admits project admins and members; outsiders get the denial
view. It fetches binding state with
`GET /api/workspaces/{ws}/projects/{pid}/repository/` and shows a
skeleton placeholder while unresolved.

The bound card (PSET-050) shows a provider-family label derived from
the provider value, the repository full name as an external link to
its web address, a mirroring-contract line (mirrored title,
description and synced comments read-only while workflow fields stay
editable), a fixed four-hour cadence line with the last-success
timestamp appended when present, and a failure line only when a
latest error exists. The sync toggle reflects the enabled flag and is
admin-only.

The sync toggle (PSET-051) writes
`PATCH .../projects/{pid}/repository/` with the enabled flag, then
revalidates the binding. Unbinding deletes the same repository path
with no confirmation dialog, then revalidates plus a success notice
noting mirrored issues stay editable afterwards — and a previously
mirrored issue does remain server-editable after unbind. Both
controls disable while a request is in flight and for non-admins.

The unbound guidance card (PSET-052) explains that binding happens in
General settings next to the repository URL field — kept there
deliberately so the stored URL and the binding cannot drift — after
which the tab toggle works. No branch picker and no direction
selector exist on this tab. The tab itself never calls bind; binding
happens solely through the General row (PSET-008).

## Automations tab (PSET-053, PSET-054)

Each automation is a row with a marker, title, explainer and switch,
reading its delay field from the loaded project (undefined counts as
off, any nonzero value as on) behind the admin gate. Enabling
auto-archive (PSET-053) writes a one-month default; disabling writes
zero and collapses the delay picker. When on, a delay selector shows
the current month count with singular/plural wording, offering preset
counts of one, three, six, nine and twelve months plus a custom-range
action opening a dialog. The dialog takes whole months one to twelve
with required/minimum/maximum validation; empty input shows the
range message, valid input writes the field and closes, and the
dialog resets on close. Out-of-range typed values never reach app
validation — the native number bounds stop them first (the dialog
stays open with no error text and the server value unchanged), so
spec and test text must present the range message as the empty-input
case only.

Auto-close (PSET-054) shares the structure over the close-delay plus
default-state fields: enabling writes a one-month default plus the
first cancelled-group state id (or null when no cancelled state
exists); disabling clears both delay and state and collapses both
pickers. The state picker lists only cancelled-group states with
their markers and names, writes the chosen id, and disables itself
when fewer than two cancelled states give no real choice. Enabling
with zero cancelled states in the project turns the feature on with
a null default state and shows a placeholder rather than a state
name — no guard prevents this. Non-admin gating matches the archive
row.

Pinned divergence NEWFRONT-397: the row skeletons exist in the tree
but are unreachable on fresh open — the tab settles on rows with no
loader phase. Slices must preserve the observed no-loader behavior
as pinned.

## Scheduler installs table (PSET-055)

The tab renders one row per scheduler install bound to the project,
read from
`GET .../projects/{pid}/scheduler-bindings/` for rows plus
`GET /api/workspaces/{ws}/schedulers/` for the definition catalog.
Each row shows the scheduler identity (color chip, definition name,
handle), recurrence as a plain-language sentence derived from the
stored rule anchored at the start time (raw rule kept as hover text;
a blank rule reads as a one-time firing), next and last run times
with a never fallback, an optimistic enabled switch with a text
label, the last-updated time, and edit/uninstall actions. The switch
writes `PATCH .../scheduler-bindings/{bid}/` with the enabled flag
through an optimistic update with rollback plus success/failure
notices, locking while its own flip is in flight. Row clicks
deep-link to that install's detail page (PSET-059); action cells stop
propagation. The settings tab is project-admin only — members are
not routed here from settings; their read-only installs view lives
on the project-level schedulers page outside Settings. The shared
panel's read-only rendering serves that project-surface view.

## Install versus author dialog (PSET-056, PSET-057)

The creation dialog offers install-existing versus create-new paths;
the author path hides unless the viewer holds the workspace-admin
grant. The installable set is catalog entries enabled workspace-wide
minus already-bound ids, preselecting the first eligible entry with
a note that installed ones are excluded. Install submits
`POST .../projects/{pid}/scheduler-bindings/` with the scheduler,
project, start, timezone, rule, extra context, enabled flag,
findings-handling mode and runner pool. The dead end appears only
when nothing is installable and the viewer cannot author: a compact
dialog names the empty state and advises asking a workspace admin,
offering no author action. When the viewer can author, the empty
case instead defaults to the create path.

The author path (PSET-057, workspace-admin only) takes a name with
an auto-derived handle (hand-edit latches the handle like the
project identifier in PSET-013), description, prompt and color. One
submit performs two sequential calls: definition creation with
`POST /api/workspaces/{ws}/schedulers/` carrying handle, name,
description, prompt, color and enabled, then the binding post above.
Duplicate handles surface inline on the handle field from the
server's field error after submit rather than as a notice. On binding
failure after a successful definition create, the dialog reports
the split outcome, refreshes the catalog and flips to
install-existing with the fresh definition preselected, so retry
cannot orphan the definition.

## Install configuration (PSET-058)

Shared by install and edit flows: a start datetime (local picker
converted to UTC on submit; required, with empty submit flagging the
field), a timezone selector over the runtime platform zone list
(which carries no UTC entry in modern runtimes; UTC survives only in
the curated fallback for runtimes without zone support, and empty
submits default to it), recurrence through a rule builder —
repeat-every count, weekday chips, monthly variants, Never/On/After
ends, plus an Advanced raw textarea — with a live plain-language
preview (a blank rule fires once at start), optional project-context
notes appended to the base prompt at run time, an enabled flag noting
disabled installs do not fire, a three-way findings-handling choice
(file one item per finding; fix and open for review without merging;
file plus delegate the fix to the coding agent into review), and a
runner-pool override defaulting to the project default with late
binding at fire time. All fields persist and round-trip; the preview
tracks the builder. The builder serializes valid rules, and a stored
rule the builder cannot express opens in the raw textarea and submits
byte-identical unless edited. Install defaults are tomorrow-morning
start, browser zone or UTC, daily recurrence, enabled on. (The old
checkout predates the builder and shows a raw rule textarea with
preview; the builder is the target behavior above.)

Target restatement (per main#561): when the loaded pool list lacks
the form's selected pool id, the field normalizes to the
project-default empty value, matching the dispatcher fire-time
fallback — a no-op save therefore cannot fail on a deleted pool.
Stale selections reset to default; slices must build this even
though the old checkout predates it.

## Install detail page (PSET-059)

The detail page loads `GET .../scheduler-bindings/{bid}/` plus paged
run history from `GET .../scheduler-bindings/{bid}/runs/` (page
parameter, optional page size), polling every fifteen seconds. It
shows a back-to-list link, name/color/handle/source markers, a
workspace-disabled badge when the definition is off workspace-wide,
a workspace-definition link for workspace admins only, edit and
uninstall actions for project admins, the latest-error banner when
present, a configuration grid (schedule, starts-at, timezone with
UTC display default, next run, findings mode, pool name or default
label, extra/skip dates when non-empty, installer identity, enabled
switch), an optional project-context block, and a collapsible
composed-prompt view collapsed by default. Run-history columns are
started (with a queued fallback), ended, status badge, human
duration, pool name and result excerpt (failure text for failed runs,
otherwise the done-summary string); rows link to the existing
run-detail route, empty states distinguish disabled versus
scheduled-but-never-run versus plain empty, and a footer paginates
with page and total. A missing install shows a not-available state
with a back link.

Target restatements: a finished finite series badges Completed once
the API reports series_exhausted (needs main#559) — absent from the
old checkout, slices must add it; disabled or uninstalled bindings
stop firing (target per main#561) — the old view asserts this in
copy but owns no firing logic, so slices carry the badge plus the
enabled-state plumbing and treat actual firing as the
schedulers-area dispatcher's concern.

Cross-epic seam: the scheduler definition catalog and the run-detail
views are owned by the agents/schedulers areas; this area links to
them and reuses their row/status presentation. At gate time this area
proves the table, dialog, configuration round-trip, detail page and
history polling, and treats definition authoring surfaces beyond the
creation dialog and run-detail pages as outbound integrations owned
elsewhere.

## Edit and uninstall (PSET-060)

The edit dialog opens prefilled by converting the stored UTC start
to local input form and copying timezone (or UTC), rule, context,
enabled flag, findings mode and pool (empty for default). Submit
writes the binding update with trimmed text fields and null pool for
default; field-level server errors surface as notices. Uninstall is
a confirmation stressing the workspace definition survives for later
reinstall, deleting `.../scheduler-bindings/{bid}/` with no other
payload; detail-page uninstall navigates back to the list.
Uninstalling removes the binding only, keeping the definition
reusable.

## Empty and loading states (PSET-061)

Feature switches read the already-loaded project with no loaders of
their own. Automations settles on rows with no loader phase
(NEWFRONT-397). Estimates shows its dedicated loader while the list
fetch is held, then the illustrated empty state with its creation
action. The repository tab shows its skeleton while the binding
fetch is unresolved, then the bound or guidance card. Schedulers
shows an in-table creation prompt when empty, the dead-end message
when nothing is installable, and field-level messages for missing
start, bad month range, missing scheduler or findings choice, and
duplicate handles.

## Reuse map for the feature, estimates, repository, automations and
scheduler slices (NEWFRONT-582, NEWFRONT-602, NEWFRONT-604)

Exists in apps/web_new and reusable as-is: the project update and
create mutations, the estimate list read, and the settings-adjacent
project-surface scheduler views. New for these slices: the five
feature tabs plus the hidden post-create sheet wiring, the
estimates page shell reusing the shared component, the repository
tab with sync toggle and confirm-free unbind, the two automation
rows with pickers and custom-month dialog, the scheduler table,
two-path dialog, configuration forms, detail page with polled
history, and the write contracts, which belong to the contracts
issues (estimates, repository, deploy and scheduler endpoints).
