# Project settings, part F — area spec (PSET-043–061)

Oracle scope of NEWFRONT-393: feature-flag tabs, the estimates settings
entry, the github sync tab, the automations tab, and the scheduler
installs area. Written in prose from reading the old app
(`apps/web`); the Playwright scenarios in
`apps/web_new/e2e/parity/project-settings/` are coded from this text,
never from the old sources.

## Feature tabs (PSET-043–047)

Each feature has a dedicated tab at
`/{workspace}/settings/projects/{project}/features/{views,cycles,intake,modules,pages}`.
Every tab is project-admin only: a viewer without project admin rights
gets the shared not-authorized view instead of the tab. Each tab shows a
heading with a one-line explainer and a single boxed switch
("Enable views", "Enable cycles", "Enable intake", "Enable modules",
"Enable pages"). Flipping a switch PATCHes one boolean on the project
(`issue_views_view`, `cycle_view`, `inbox_view`, `module_view`,
`page_view`), shows a pending toast ("Updating project feature...") and
then a success toast ("Project feature updated successfully.") or an
error toast. The switch reads the already-loaded project, so the tabs
render with no loaders of their own. Turning a feature off hides its
area product-wide (saved views, planning periods, the intake area,
sub-project grouping, free-form documents); turning it back on
restores the area.

## Combined feature list (PSET-048)

The combined list renders five cards (Cycles, Modules, Views, Pages,
Intake), each with an icon, a title, a one-line explainer, and a single
switch — no per-feature child settings. Each switch PATCHes its own
flag with the same pending/success/error feedback as the dedicated
tabs. The same list is reused on the post-create sheet, where it always
renders in admin mode. Non-admins see the switches disabled.

Recorded absence (not a bug): the post-create feature step was
intentionally hidden upstream (bd4f157e — MVP starts work-items-only,
the toggle UI remains in project settings), and the combined list
served only that step — so the PSET-048 surface does not exist in this
checkout and creating a project only ever closes the modal with a
success notice. The paragraph above describes the removed surface for
context; the oracle scenario proves the absence.

## Estimates entry (PSET-049)

The estimates tab at `.../settings/projects/{project}/estimates` is
project-admin only (non-admins get the not-authorized view) and embeds
the shared estimates component: a loader first, then an illustrated
empty state with a creation action when no estimation system exists, or
the active system shown with deletion only through a confirmation
dialog (no edit affordance in this OSS build, per the estimates area's
EST-014 contract) with superseded systems listed read-only. The enable
switch sets or clears the project's active-estimate reference;
flipping it on links the current active system id with a success
notice, flipping it off unlinks it.

## Github tab (PSET-050–052)

The github tab at `.../settings/projects/{project}/github` is visible
to project members and admins (viewers below member get the
not-authorized view). It fetches the repository binding and shows a
skeleton while the binding is unresolved. A bound project shows a card
naming the provider family (GitHub, GitLab, or a generic Git provider),
linking out to the repository by its full name, stating the mirroring
contract (mirrored issues are read-only on title, description, and
synced comment bodies; workflow fields stay editable), and showing the
four-hour sync cadence with the last successful sync time plus a plain
"Last error:" line when a failure is recorded. The sync switch flips
the enabled flag with failure toasts on error; the Unbind button
(admin-only, disabled while a request is in flight) unbinds with no
confirmation dialog and a success notice stating mirrored issues remain
editable. With no repository linked, the tab shows a guidance card
pointing at General settings, where linking happens via the repository
URL field and Bind action — so the stored URL and the binding cannot
drift. There is no branch picker and no direction selector on this tab.

## Automations tab (PSET-053–054)

The automations tab at `.../settings/projects/{project}/automations`
is project-admin only and holds two built-in rows plus an empty custom
slot that renders nothing in this checkout. The rows carry a skeleton
for the not-yet-loaded project, but it never renders on a fresh open:
the route wrapper blanks the content until the project retrieve
resolves, and that retrieve fills the shared map the rows read — so
the tab settles directly on the rows with no loader phase (old bug
NEWFRONT-397; intended behavior is the skeleton until loaded).
The auto-archive row has a switch that applies
a one-month default when enabled (`archive_in` 0 to 1) and expands to a
month picker: presets plus a "Customize time range" action opening a
dialog whose number input carries native min/max, so out-of-range
values never submit (no request, the dialog stays open, no app
message), while an empty submit shows "Select a month between 1 and
12." Valid whole months from 1 to 12 persist and close the dialog.
The auto-close row has
a switch that applies a one-month default plus a default cancelled
state when enabled (`close_in` 1 with `default_state`), clearing both
on disable; it expands to a delay picker and a state picker limited to
cancelled-group states, where the state picker is disabled when there
is only one choice. Failed saves toast an error. Both rows disable
their controls for non-admins.

## Scheduler installs (PSET-055–060)

The schedulers tab at `.../settings/projects/{project}/schedulers` is
project-admin only (members are not routed here from settings; their
read-only view lives on the project-level schedulers page). It lists
one row per install: scheduler identity, recurrence as a plain-language
sentence, next and last run times, an optimistic enabled switch that
rolls back on error, last-updated time, and edit/uninstall actions.
Clicking a row deep-links to the install's detail page. The creation
button opens a two-path dialog: install an already-defined workspace
scheduler that is enabled and not yet bound here, or author a new
workspace scheduler definition and install it in one submit (name with
an auto-derived slug, description, prompt, color). The author path
requires workspace admin and flags duplicate slugs inline; when nothing
is installable and the viewer cannot author, a dead-end message
explains why instead of offering a phantom action. Install
configuration covers a start datetime, a timezone picked from the
platform zone list (the runtime's Intl list, which carries no UTC
entry in modern browsers; UTC survives only in the curated fallback
for runtimes without Intl zone support, and empty submits default to
it), recurrence through
a rule builder (repeat-every, weekday chips, monthly variants,
Never/On/After ends, raw advanced text; a blank rule fires once) with
a live plain-language preview, project-specific context appended to the
base prompt at run time, an enabled flag, a three-way findings-handling
choice (file per finding; fix and open for review without merging; file
plus delegate the fix to the coding agent into review), and a
runner-pool override defaulting to the project default with stale
selections normalized back. The detail page shows the full
configuration, the latest error banner when present, a collapsible view
of the exact composed prompt, a workspace-definition link for workspace
admins, and a paged run history (start, end, status, duration, pool,
result excerpt) whose rows link to run detail and which refreshes on a
fifteen-second poll; missing installs show a not-available state with a
back link. Editing happens through the prefilled dialog; uninstalling
asks for confirmation and removes only the binding — the workspace
definition survives for later reinstall.

## Empty and loading states (PSET-061)

Feature switches read the already-loaded project and show no loaders;
automations shows a loader until the project loads; schedulers shows an
in-table creation prompt when empty, a dead-end message when nothing is
installable, and field-level messages for a missing start, a bad month
range, a missing scheduler choice, and duplicate slugs. Server errors
surface as toasts, per field where available.
