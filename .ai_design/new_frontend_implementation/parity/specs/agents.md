# Agents area spec — Schedulers, prompts, assistant (AGT-001–065)

Parent epic NEWFRONT-75. Inventory
`.ai_design/new_frontend_implementation/parity/inventory/agents.md` (rows
AGT-001–065; 63 oracle green, AGT-064/065 not started). Oracle scenarios
`apps/web_new/e2e/parity/agents/` (69 spec files across assistant-core,
assistant-plus, prompts, schedulers-catalog and schedulers-project, green on
`apps/web`).

Method: the old frontend was read as a behavior reference only — the
schedulers, prompts and assistant routes, the project schedulers and
automations settings routes, the schedulers, scheduler-bindings, automation,
assistant and chat component trees, the scheduler and prompt-section stores,
the scheduler, prompt-section, assistant, auto-pm and AI services, the
desktop session and event-source adapters, and the desktop overlay agent
runtime. Nothing is imported, copied or translated from those sources; this
spec restates what was learned in prose. The only code-like tokens below are
API paths, query parameter names, payload field names, event kind names and
error codes, which the contracts issues (NEWFRONT-568, NEWFRONT-577) turn
into zod contracts. New code is written from this spec, the wiki pages
(Reference not source; Parity; Architecture; Data layer; UI kit and editor)
and the kit — never with old files open.

Own-code exception: much of this area was written by AI Republic after the
Plane import, and those paths may be ported, not only referenced (wiki
Reference, not source; decision D16). §16 names the exact own-code module to
slice mapping this spec verified against the tree. For ported modules this
spec records the behavior and the inventory row mapping without re-describing
logic the slices carry over; anything those modules use from Plane code
stays reference-only and is rewritten. Two corrections to the candidate list
in the issue body came out of that verification: the project install,
calendar and automation components, the editor AI helper and its services,
and the dictation hook are Plane code (reference-only), and the assistant
settings page chrome belongs to the profile area.

Target-behavior rule (NEWFRONT-156): the inventory describes target
behavior, not today's main. Two rows (AGT-064 per main#586, AGT-065 per
main#561) are still open PRs while the live old app shows pre-PR behavior.
Those rows are restated precisely in §14; their slices (NEWFRONT-608,
NEWFRONT-587) implement the target text and the area gate aligns the
scenarios. Smaller main-PR targets nested inside oracle-green rows (the
Completed badge per main#559 inside AGT-013, receipt anchors per main#579
inside AGT-028) are noted where they belong.

Status convention: this spec owns no inventory Status changes. Slices append
`built` to the rows they implement.

Where the running old app contradicts the inventory row text, the oracle
pinned the observed behavior and filed a bug issue. Those deviations are
recorded in §18. Slices implement the target stated in each section below;
the gate proves the target.

## 1. Scheduler catalog CRUD (AGT-001–005, slice NEWFRONT-581)

The workspace catalog lives at the workspace schedulers address
(`/{workspaceSlug}/schedulers`) and renders as a table of every reusable
scheduler definition. Each row shows the display name, the URL handle, an
origin mark (only the built-in origin is provable — no API creates
manifest-source definitions), a live install count, an enabled mark and the
last update time. An empty catalog shows a guidance row instead of a blank
table, and the tab title carries the workspace name. Workspace admins,
members and guests all read the list. The single read is `GET
/api/workspaces/{slug}/schedulers/`. Oracle:
`schedulers-catalog/catalog.spec.ts`. (AGT-001)

Creation opens from the catalog header action into a definition dialog with
name, handle, description, base prompt, color and enabled flag. A valid
submit persists through `POST /api/workspaces/{slug}/schedulers/`, refreshes
the list and shows a success notice. Invalid input is rejected inline, a
backend failure surfaces its field detail, and the dialog stays open on
failure so nothing is lost. The create control renders for workspace admins
only. Oracle: `schedulers-catalog/create.spec.ts`. (AGT-002)

Editing opens the same dialog in edit mode from the per-row edit action. It
pre-fills the stored values and saves through `PATCH
/api/workspaces/{slug}/schedulers/{id}/` with the handle omitted — the
handle is immutable after creation and the field locks in edit mode. The
list refreshes on save, and later firings follow the updated definition.
Workspace admin only. Oracle: `schedulers-catalog/edit.spec.ts`. (AGT-003)

Deletion opens a confirmation dialog from the per-row delete action. The
dialog explains that removal stops project installs from firing and frees
the handle for reuse. Confirming issues `DELETE
/api/workspaces/{slug}/schedulers/{id}/`, removes the row and refreshes the
list; cancelling and in-flight submits are guarded, and a failure shows an
error notice with the dialog left safe. Workspace admin only. Oracle:
`schedulers-catalog/delete.spec.ts`. (AGT-004)

The handle field derives from the display name until the user edits it by
hand: typing a name proposes a lowercase dashed handle, hand-editing stops
the derivation, and a blank derivation forces manual entry. Malformed
handles are rejected inline, and edit mode locks the field (see AGT-003).
Pure client behavior, no API. Oracle:
`schedulers-catalog/handle.spec.ts`. (AGT-005)

## 2. Project installs, bulk, single, author (AGT-006–009, slice NEWFRONT-584)

One definition can be installed onto many projects at once from the catalog
per-row install action, which is offered on enabled definitions only. The
bulk dialog shows a project picker listing joined projects with search and
select-all, marking already-installed targets locked. An empty selection is
refused. Submitting issues one `POST
/api/workspaces/{slug}/projects/{id}/scheduler-bindings/` per target with a
shared schedule, then partitions the per-target results: successes close
out with a notice while failures stay selected for retry, and install
counts refresh. Reaching the dialog needs workspace admin; each target
additionally needs project admin, so partial success is normal. Oracle:
`schedulers-catalog/bulk-install.spec.ts`. (AGT-006)

Each project has an install list (the project schedulers list tab, plus a
settings-embedded variant — see AGT-021) reading `GET
/api/workspaces/{slug}/projects/{id}/scheduler-bindings/`. Rows show a color
mark, name, handle, plain-language schedule, next and last run, an enable
switch and the update time. A row opens the install detail; the management
cells never navigate. Non-admins get a read-only list while project admins
see the management controls, and the empty state invites creation or
install. Oracle: `schedulers-project/install-list.spec.ts`. (AGT-007)

The project creation dialog offers an install path that lists only eligible
definitions — enabled and not yet bound to this project. Submitting needs a
definition and a start, posts `POST
.../projects/{id}/scheduler-bindings/`, and refreshes the list on success;
backend field errors surface without closing the dialog. Project admin to
submit. Gap carried from inventory: the viewer-browsing half
is unreachable — the creation control is project-admin-only, so the dialog
has no non-admin entry point. Note also that the dialog's initial path
follows data that may still be loading on a cold open. Oracle:
`schedulers-project/install.spec.ts`. (AGT-008)

The same dialog offers a create path that authors a new definition and
installs it in one submit: `POST /api/workspaces/{slug}/schedulers/` then
`POST .../scheduler-bindings/`. It opens on the create path automatically
when nothing is installable. Handle-uniqueness conflicts map inline onto
the handle field, and a definition-ok/install-failed outcome flips to the
install path with the new entry preselected so nothing is orphaned. Required
fields on the hidden path never block the visible path. Needs workspace
admin (definition) plus project admin (install); the tab hides otherwise.
Oracle: `schedulers-project/author-install.spec.ts`. (AGT-009)

## 3. Binding edit, toggle, uninstall (AGT-010–012, slice NEWFRONT-587)

An install is edited from the list row, the detail page or the calendar
drawer through one shared dialog. The dialog pre-fills start (converted to
the local input), timezone, recurrence, project context, enabled flag,
finding-handling mode and execution pod, and saves through `PATCH
/api/workspaces/{slug}/projects/{id}/scheduler-bindings/{bindingId}/`; the
caller (list, detail or calendar) refreshes and backend field errors surface
in a notice. An empty pod value persists as the project default. The
recurrence field reconstructs builder state from the stored rule and opens
in raw mode for inexpressible rules, submitting untouched rules byte-identical
— that half is the main#561 target restated in §14 (AGT-065); the live old
app still shows a plain textarea there. Project admin only. Oracle:
`schedulers-project/edit-install.spec.ts`. (AGT-010)

The enable switch on list rows and the detail header flips with optimistic
feedback: it applies immediately through an enabled-only `PATCH
.../scheduler-bindings/{bindingId}/`, rolls back with an error notice on
failure, and blocks double submits. Firing stops while off. Note carried
from inventory: disabling keeps the stale next-run timestamp server-side;
the stop is proven by exclusion from the occurrences read and the vanishing
calendar blocks. Project admin only; the switch is inert otherwise. Oracle:
`schedulers-project/toggle.spec.ts`. (AGT-011)

Uninstalling opens a confirmation dialog from the per-row or detail-page
action. The dialog explains that firing stops on this project while the
workspace definition survives and stays reinstallable. Confirming issues
`DELETE .../scheduler-bindings/{bindingId}/` and removes the install; a
detail-page uninstall returns to the install list, and a failure shows an
error notice. Project admin only. Oracle:
`schedulers-project/uninstall.spec.ts`. (AGT-012)

This slice also owns the RRULE builder widget (AGT-065): the shared
schedule-fields piece used by the new, edit and bulk dialogs. The target is
restated in §14; the slice builds from that text, not from the old
textarea.

## 4. Install detail, fields, tabs, gates (AGT-013, AGT-014, AGT-018–022, slice NEWFRONT-590)

The install detail route (`/{ws}/projects/{id}/schedulers/{bindingId}`)
reads `GET .../scheduler-bindings/{bindingId}/` and shows a configuration
grid — readable schedule, start, timezone, next run, finding-handling mode,
pod or default mark, extra and skip dates, installer and the enable switch
— with the composed prompt hidden behind a reveal toggle and a last-error
panel when an error is present. Distinct empty states cover awaiting-first-run
versus disabled installs. A finished finite series badges Completed once the
API reports the exhausted-series signal (main#559 target; no such signal
exists in this checkout, so the badge is unproven and the gate aligns it).
Run rows link to run detail (runners area owns the destination; main#561
target). Viewers read, project admins manage, and workspace admins get a
catalog link. Gaps carried from inventory: the last-error panel and run-row
links need live firings the seeded stack never produces, so the panel is
asserted absent. Oracle: `schedulers-project/install-detail.spec.ts`.
(AGT-013)

The run-history table inside the detail reads `GET
.../scheduler-bindings/{bindingId}/runs/?page=&per_page=` and re-fetches on
its own roughly every quarter minute without a reload (measured at about
15 seconds between consecutive refetches), with page controls and a position
indicator. A fetch failure explains the install may have been removed and
offers a way back. Gap carried: paging needs more runs on one install than
the seeded stack can provision, so the no-pager state is asserted instead.
Oracle: `schedulers-project/run-history.spec.ts`. (AGT-014)

The finding-handling mode is chosen per install inside the install dialogs:
filing issues (the default), applying fixes for review, or filing plus
delegating — one single choice whose help text updates per option. One
definition may behave differently across projects. The mode persists on the
binding create and update payloads (`outcome_mode`). Whoever may submit the
hosting dialog sets it (project admin; bulk needs catalog rights too).
Oracle: `schedulers-project/outcome-mode.spec.ts`. (AGT-018)

The execution pod is chosen per install or left on the project default,
which means late binding at fire time. The selector reads the project-scoped
pod list (`GET /api/runners/pods/?project=`, active pods only, readable by
any workspace member), persists the chosen pod on the binding, resets a
stale saved pod to default instead of failing the save, and disables while
pods load. Present in single-project dialogs, absent from bulk; project
admin. Oracle: `schedulers-project/pod.spec.ts`. (AGT-019)

Recurrence rules render as plain-language sentences everywhere — list rows,
detail, dialog previews and the calendar drawer. Blank means a single firing
at the start. Builder dialogs keep the humanizer line live under the widget
as confirmation, with a fix-before-submit hint for invalid raw-mode input
(main#561 target); the stored value remains the tooltip fallback. Client-side
rendering, no API. Notes carried from inventory: invalid input echoes raw in
the humanizer, invalid submits keep the raw input and surface the backend
field error, and the single-shot sentence differs between list/detail and
dialog wordings — this spec prefers the dialog's sentence for the new app.
Oracle: `schedulers-project/recurrence-text.spec.ts`. (AGT-020)

The project schedulers section has list and calendar tabs with the calendar
as the bare-path landing: opening `/{ws}/projects/{id}/schedulers` lands on
the calendar (`.../schedulers/calendar`), and the tabs stay unhighlighted on
install-detail routes. The project-settings variant
(`/{ws}/settings/projects/{id}/schedulers`) reuses the installs panel but
shows a not-authorized view to non-admins, while the project page shows them
a read-only list. Same binding-list read. Oracle:
`schedulers-project/section-tabs.spec.ts`. (AGT-021)

The catalog and prompts route guards admit workspace roles (admin, member,
guest) inside the normal workspace chrome and refuse outsiders with a
not-authorized panel, never mounting a duplicate sidebar. No API — route
guards. Gap carried: the layout-level refusal is unreachable while the
workspace wrapper guard stands, so outsiders meet the workspace not-found
surface first. Oracle: `schedulers-catalog/route-gate.spec.ts`. (AGT-022)

## 5. Firing calendar, rail, occurrence panel (AGT-015–017, AGT-062, slice NEWFRONT-593)

The calendar tab (the section's default landing) renders occurrences from
`GET .../scheduler-bindings/occurrences/?from=&to=` in month-grid and
24-hour week-grid views — opening on the week view — with period stepping,
jump-to-today, a today mark and a locally ticking time line. Past and future
blocks are visually distinct, overcrowded days collapse behind an overflow
control, and the surface is read-only: no drag to create or reschedule.
Viewers read; edit-from-calendar reuses the edit dialog and needs project
admin. Gaps carried: past occurrences need run rows the seeded stack never
produces, so the past/future distinction is unprovable and every block is
asserted future-styled. The occurrences read caps at 5000 with a truncation
hint (`has_more` plus `next_window_start`) and the window clamps to 90 days.
Oracle: `schedulers-project/calendar.spec.ts`. (AGT-015)

A per-definition visibility rail beside the calendar (wide screens only,
hidden automatically on narrow ones) offers one toggle row per installed
definition with color marks plus show-all/hide-all. The hidden set is a
local-only preference persisted per project across tabs; it is load-time
state — a second tab loads with it applied, but open tabs do not follow
each other live. No API. Oracle:
`schedulers-project/visibility-rail.spec.ts`. (AGT-016)

Clicking a calendar block opens an occurrence side panel with timing,
timezone and status plus recurrence and context for future firings or a run
link for past ones; it always links the parent install, closes explicitly,
and renders nothing with no selection. Same occurrences read; run links land
in the runners area. Gap carried: the past-run halves need run rows the
seeded stack never produces, so the future halves carry the row. Oracle:
`schedulers-project/occurrence-panel.spec.ts`. (AGT-017)

Negative row: the calendar has no drag-to-create, drag-to-reschedule or
export affordances. Blocks are click-to-inspect only, run history navigates
to run detail instead of exporting, and nothing downloads, prints or shares
from these surfaces. The absence must hold in the new app. Gap carried: the
run-history navigation half needs a clickable run row, so the export absence
on both surfaces is asserted instead. Oracle:
`schedulers-project/no-drag-export.spec.ts`. (AGT-062)

## 6. Prompt sections browser and edits (AGT-023–027, AGT-033, slice NEWFRONT-597)

The prompts route (`/{ws}/prompts`) opens on a sections browser: one card
per prompt section with title, registry key, provenance mark (registry
default, workspace override or personal override), lock and governance
marks, kind marks for the three agent kinds (coding task, review,
scheduler), and a stale-override warning where the default moved on. A side
nav jumps to each section anchor. Sections load per kind and scope through
`GET /api/workspaces/{slug}/prompt-sections?kind=&scope=`; member lists
always load while the workspace baseline loads for admins. Admin, member
and guest all read. Note carried: the baseline split is UI-gated — the API
serves workspace scope to members while the UI hides their workspace
editing. Oracle: `prompts/sections-browser.spec.ts`. (AGT-023)

Workspace default text is editable where the section allows it, for
workspace admins only. The edit control appears only when the section
permits, the viewer is admin and the baseline loaded; the editor seeds from
the existing override or the baseline, and saving activates only when the
draft differs. Saves go through `PUT
/api/workspaces/{slug}/prompt-sections/{key}` with workspace scope in the
body. Intended target: the save persists, confirms and refreshes the card —
the live old app rejects saves, recorded as bug NEWFRONT-193 in §18, and
the new app implements the intended flow. Oracle:
`prompts/workspace-edit.spec.ts`. (AGT-024)

Personal overrides are editable where the section allows it, for any
workspace member. The same editor seeds from the personal override, the
workspace baseline or the registry default; locked or admin-managed sections
offer no personal editing. Saves use the same upsert endpoint with personal
scope, and the intended target (persist, confirm, mark the card personal)
likewise stands behind bug NEWFRONT-193. Oracle:
`prompts/personal-edit.spec.ts`. (AGT-025)

The section editor compares the draft against the pristine default side by
side in two columns. Inline errors show on save failure, cancelling
discards, and a revert affordance appears only when an override exists at
that scope. No reads beyond the section lists. Oracle:
`prompts/compare.spec.ts`. (AGT-026)

Reverting an override to the default requires an explicit confirmation. The
dialog explains the revert with scope-specific wording and an
irreversibility warning; confirming issues `DELETE
/api/workspaces/{slug}/prompt-sections/{key}?scope=`, deactivates the
override and refreshes the lists, while cancelling keeps it. Whoever owns
the override (personal) or the workspace admin (baseline) may revert. The
intended target stands behind bug NEWFRONT-193; note the revert path reports
only its generic message, dropping the server detail the save path shows.
Oracle: `prompts/revert.spec.ts`. (AGT-027)

Locked and admin-managed sections refuse edits at the right tier: locked
sections offer no editing to anyone, admin-managed sections offer no
personal editing, and the marks explain who controls the section. Tier
enforcement lives in the section upsert endpoint. Gap carried: the live
registry ships no workspace-tier section, so the admin-managed half is
unprovable — the scenario asserts the absence of that badge on every card.
Oracle: `prompts/tiers.spec.ts`. (AGT-033)

## 7. Prompt receipt, previews, states (AGT-028–032, slice NEWFRONT-599)

The receipt view on the prompts route shows the assembled template per agent
kind through `GET /api/workspaces/{slug}/prompts/{kind}/compiled?scope=`.
Each receipt card numbers its sections with provenance marks, shows a
rendered section-count badge, expands to the full assembled text and
collapses again; a side nav lists kinds with rendered section counts and
anchors (main#579 target). The intended target renders the count in badges
and nav — the live old app renders the raw count template, recorded as bug
NEWFRONT-194 in §18. Viewers read; admin-only extras below. Oracle:
`prompts/receipt.spec.ts`. (AGT-028)

When the viewer customized a section, the expanded receipt card contrasts
two assemblies: the personal template and the workspace-only variant that
automatic runs use. Without overrides only one block shows. Same compiled
read, which includes the workspace-only variant when overrides exist.
Oracle: `prompts/receipt-variant.spec.ts`. (AGT-029)

Saved-prompt preview renders a template against a real issue or install
(workspace default), for workspace admins only. Each receipt card carries a
render-against-real-data form with a single target field — an issue for the
coding and review kinds, an install for the scheduler kind — posting `POST
/api/workspaces/{slug}/prompts/{kind}/preview` with the target id. The
returned prompt text renders, or an inline error shows. The intended target
stands behind bug NEWFRONT-193. Note: empty targets disable the submit, so
the inline empty-target message is unreachable. Oracle:
`prompts/saved-preview.spec.ts`. (AGT-030)

Unsaved-draft preview renders the in-progress edit against a real issue or
install, for whoever may edit that scope. The draft-preview panel under the
section editor posts the same preview endpoint plus the draft scope, section
key and body. A kind switcher appears when the section belongs to several
kinds, and the scheduler kind selects the install-target payload shape. Bad
targets and render failures show inline. The intended target stands behind
bug NEWFRONT-193. Gaps carried: no overridable section spans several kinds,
so the switcher never renders, and the scheduler recipe is all locked, so
the install-target draft shape is proven by AGT-030 instead. Oracle:
`prompts/draft-preview.spec.ts`. (AGT-031)

Loading and failure states guide instead of blanking: a loading line shows
until the required lists resolve, a member-list failure shows an error
banner, and a workspace-baseline failure shows an admin-only warning that
hides (not silently drops) the editing controls. Same section and compiled
reads. Oracle: `prompts/loading-failure.spec.ts`. (AGT-032)

## 8. Project automations (AGT-034–037, slice NEWFRONT-600)

The automations settings page
(`/{ws}/settings/projects/{id}/automations`) hosts the idle automations.
The auto-archive row toggles idle archiving: enabling applies a one-month
default delay and reveals a duration picker with presets plus a custom
entry, disabling resets to zero. Persistence is `PATCH
/api/workspaces/{slug}/projects/{id}/` with the archive delay field.
Project admin to change. Gap carried: the viewer read-only half is
unreachable — the page gates non-admins out (AGT-037), so no read-only row
renders for them. Oracle: `prompts/auto-archive.spec.ts`. (AGT-034)

The auto-close row toggles idle closing with a delay plus a cancelled-state
target: enabling applies a one-month default plus the first cancelled-group
state, disabling resets to zero with no state. A state picker limited to
cancelled-group states (disabled unless several exist) shows a placeholder
when no state resolves. Same project update with the close delay and default
state fields. Project admin to change; same unreachable viewer half as
above. Oracle: `prompts/auto-close.spec.ts`. (AGT-035)

The custom month dialog behind either duration picker accepts only whole
months in a bounded range: the numeric input requires an in-range value with
an out-of-range message, cancelling resets, and submitting forwards to the
row handler and closes. No API — it feeds the project update. Notes carried:
fractional input never submits (native step validation blocks it with the
dialog left open) and the dialog seeds its input statically rather than from
the current value. Oracle: `prompts/custom-month.spec.ts`. (AGT-036)

The automations page gates on project admin — non-admins see a
not-authorized view with dimmed content — funnels every row change through
one handler, and hosts an empty extension slot below the built-in rows in
the community build; edition-gated slot contributions need the cloud build.
Same project update. Oracle: `prompts/automations-gate.spec.ts`. (AGT-037)

Built in `apps/web_new` (NEWFRONT-600): `features/automations` owns the
screen, its queries and the rules; `routes/$ws/_gated/settings/projects/
$projectId/automations.tsx` composes it. Reads: the workspace's
project-details collection (the only read carrying the delays and the
state target) and the project's state list; writes go through the
project update and fold the answered row back into the collection
cache, so a refused write leaves the stored values on screen. The page
waits for the project collection before painting the rows. The
extension slot is a mount point with no community contributions;
edition contributions still need the cloud build to prove. Selectors
the oracle probes: the section heading "Automations", level-4 row
headings, role=switch toggles, a delay trigger named by its row lead
and reading "N month(s)", role=option presets, a dialog titled
"Customize time range" with an "Enter Months" field, and the refusal
heading "Oops! You are not authorized to view this page". The dialog
turns off native form validation so zero and over-range whole counts
still reach its handler and raise the range message, while a
fractional count is refused silently and never forwarded.

## 9. Assistant landing, setup, composer gates (AGT-038–041, AGT-048, slice NEWFRONT-603)

The assistant landing (`/{ws}/assistant`) greets a keyed member with a
headline, a caption and a standalone composer. Submitting creates an
untitled thread through `POST
/api/workspaces/{slug}/ai-assistant/threads/` and navigates into it carrying
the first message; the composer locks while starting so one submit creates
exactly one thread, blank submits never leave the page, and a failed start
surfaces a notice and creates nothing. Workspace member and above. Oracle:
`assistant-core/landing-start.spec.ts`. (AGT-038)

When no provider key is stored, a setup card replaces the chat entry on the
landing and in the thread empty state. The card explains the user supplies
their own key and the assistant acts with exactly their permissions, with a
button to the provider settings page (profile area owns the destination —
see §13). Gating reads `GET /api/users/me/ai-assistant/config/`; no thread
can start until the key exists. Oracle:
`assistant-core/setup-card.spec.ts`. (AGT-039)

With setup incomplete the composer locks down in both the thread view and
the landing: inputs disable with an explanatory line, and sends are blocked
with an error notice instead of creating threads. Same config read. Note
carried: the chat root's key-required notice needs a mid-session key removal
plus a config revalidation with no deterministic UI path driving it; the
dashboard card proves the same notice (AGT-048). Oracle:
`assistant-core/composer-lockdown.spec.ts`. (AGT-040)

Negative row: guests are excluded from the assistant everywhere. The
dashboard shows them no assistant card, and guest navigation to assistant
surfaces is refused. Members and above proceed normally. Note carried:
refusal is backend-driven (a forbidden response lands as a notice); no route
guard exists, and the refused thread list leaves the sidebar with neither
rows nor the placeholder. Oracle:
`assistant-core/guest-exclusion.spec.ts`. (AGT-041)

The dashboard assistant card (card shell owned by NEWFRONT-70 per SHELL-007;
this slice owns the quick-entry behavior and the seam contract in §13)
offers a single-line input with a submit action plus suggestion chips that
fill the draft without sending, and up to five recent conversations
deep-linking into the assistant view, newest first. Asking starts a thread
from the card. An unconfigured key blocks sends with an error notice. Member
and admin only; guests see nothing. Oracle:
`assistant-core/dashboard-card.spec.ts`. (AGT-048)

## 10. Thread view, streaming, send, titles (AGT-042, AGT-043, AGT-047, AGT-049, AGT-053, AGT-054, slice NEWFRONT-605)

The thread view (`/{ws}/assistant/{threadId}`) streams answers live. The
transcript pages in through `GET .../threads/{id}/messages/` (offset and
limit parameters), token deltas arrive over the event stream `GET
.../threads/{id}/events/` (with an offset parameter for replay) with
sequence-number dedupe on reconnect, and a one-second poll backs up the
stream while a turn runs. Sending swaps to a stop action during a turn;
stopping posts `POST .../threads/{id}/cancel/` and keeps the composer locked
until the server event or the poll confirms. Send and turn failures show
inline with detail. Intended target: stop-then-server-event-confirm with the
inline error line — the live old app drops every live event (bytes
corruption), recorded as bug NEWFRONT-197 in §18, so completion currently
runs through the poll. Delta and dedupe halves run at the transport contract
with stubbed frames. Oracle: `assistant-core/streaming-stop.spec.ts`.
(AGT-042)

Blank messages never send: blank drafts are ignored, the send button shows
a posting state, success clears the draft and refreshes thread ordering, and
failure shows inline without losing the transcript. Sends post `POST
.../threads/{id}/messages/` with the content. The provider-failure and
conflict-line halves stand behind bug NEWFRONT-197. Oracle:
`assistant-core/send-rules.spec.ts`. (AGT-043)

Messages render by role: user turns align to one side, assistant turns to
the other with markdown, streaming with no content yet shows a typing
placeholder, error turns render as tinted boxes, and the list auto-scrolls
on new messages with an empty placeholder when there are none. Transcript
read only. Notes carried: the markdown, placeholder and auto-scroll halves
run at the transport contract with stubbed frames; the user, error and empty
halves run against real endpoints. Oracle:
`assistant-core/message-styles.spec.ts`. (AGT-047)

The composer keyboard contract: unmodified Enter submits from both the
multiline composer and the single-line widget input, Shift+Enter inserts a
newline, blank drafts never send, and no global hotkeys or modified variants
exist on this surface. Intended target: only unmodified Enter submits — the
live old app also submits on Control+Enter, recorded as bug NEWFRONT-195 in
§18. Client behavior, no API; this stays a local composer contract, not an
entry in the shared command registry. Oracle:
`assistant-core/keyboard.spec.ts`. (AGT-049)

The first message from the landing is delivered exactly once per thread:
the draft travels as one-shot navigation state and the state clears after
the first send, so refresh or back never resends or duplicates the message.
Same thread and message writes. Oracle:
`assistant-core/first-message-once.spec.ts`. (AGT-053)

New threads gain server titles from the first message: the untitled thread
is titled after the first exchange, and list order and titles refresh after
sends without manual reload. Titles truncate to 60 characters. Note carried:
the title-helper endpoint (`POST .../generate-title/`) serves only the
create-issue dialog — chat titles come from the send path. Oracle:
`assistant-core/server-titles.spec.ts`. (AGT-054)

## 11. Sidebar, deeplinks, tools, paging (AGT-044–046, AGT-055, AGT-056, AGT-058, AGT-061, slice NEWFRONT-607)

The conversation sidebar inside the assistant layout lists threads from
`GET /api/workspaces/{slug}/ai-assistant/threads/`: each thread links to
its own route with its title or a fallback label, the open thread
highlights, no conversations shows a placeholder line, archived threads
never appear, and new threads prepend optimistically. Thread owners,
member and above. Oracle: `assistant-core/thread-sidebar.spec.ts`. (AGT-044)

Per-thread deep links are shareable entry points: opening
`/{ws}/assistant/{threadId}` loads that transcript in a centered column,
missing parameters render nothing, and tool-result and widget links land on
the right thread. Same thread and message reads. Notes carried: tool-result
links run at the transport contract with a stubbed frame, and widget-link
landing is proven in AGT-048. Oracle:
`assistant-core/thread-deeplinks.spec.ts`. (AGT-045)

Tool activity renders inline: tool calls and results become activity rows
with deep links, and dropped tool servers render one line per server with a
plain-language reason plus a special whole-outage phrasing — a per-turn
notice, never a toast. Notices are client-derived per turn and reappear
correctly on reload. Transcript plus the `tool_call`, `tool_result` and
`tool_servers_skipped` stream events. The mid-turn halves stand behind bug
NEWFRONT-197 (live events dropped; reload replay renders them); mapped
reasons, whole-outage and tool-row halves run at the transport contract
with stubbed frames. Oracle: `assistant-core/tool-activity.spec.ts`.
(AGT-046)

Long transcripts page incrementally: older pages append above through the
cursor-and-limit message read without losing scroll position, and stream
events merge with fetched pages without duplicates. Gap carried: the
beyond-100 multi-fetch loop needs more sends in one thread than the hourly
send throttle allows, so cursor paging plus merge-without-duplicates is
proven instead. Oracle: `assistant-core/transcript-paging.spec.ts`.
(AGT-055)

Turn completion is detected even when the stream drops, via transcript
polling: when no streamed rows arrive and the newest real message is
terminal, the turn completes through the poll path and the composer unlocks.
Notices never count as terminal messages. Note carried: the poll path clears
the busy state without setting the event-driven error line by design — that
line only ever comes from a turn-failed event. Oracle:
`assistant-core/poll-completion.spec.ts`. (AGT-056)

Negative row: no rename, archive or delete controls exist for threads in
the chat surface. Threads can be started, viewed and streamed; the update
and delete thread endpoints exist server-side but have no buttons here, and
the service methods stay reserved for a future surface. Oracle:
`assistant-plus/no-thread-management.spec.ts`. (AGT-058)

Structural negative row: the shared history-panel component is not the
assistant sidebar. The assistant layout renders its own inline link rail;
the reusable history panel stays available for runner chat (rowed under
runners) and no duplicate or dead sidebar may render. Oracle:
`assistant-plus/sidebar-identity.spec.ts`. (AGT-061)

## 12. Keys, voice, registry, desktop, editor (AGT-050–052, AGT-057, AGT-059, AGT-060, AGT-063, slice NEWFRONT-608)

Push-to-talk voice dictation transcribes into the draft: press-and-hold
records with a live timer and indicator, releasing transcribes through
`POST /api/users/me/ai-assistant/transcribe/` (multipart, upload field
`file`) and appends with spacing fixed. Denied, failed and unconfigured
states each explain the next step. Dictation additionally needs the speech
key and the dictation flag on (see AGT-057 and the §14 kill-switch target);
a dictation-disabled response re-reads config to hide the mic. Notes
carried: transcription runs stubbed (no speech backend) and denial mapping
runs against the rejection name. Oracle: `assistant-plus/dictation.spec.ts`
plus `dictation-denied.spec.ts` and `dictation-states.spec.ts`. (AGT-050)

Without a speech key, pressing the microphone control routes to dictation
settings instead of recording; with a key the press stays. The check reads
`GET /api/users/me/ai-assistant/stt-config/`. The flag-off half (control
hidden, no routing) is the main#586 target in §14; the settings surface
itself belongs to the profile area. Oracle:
`assistant-plus/setup-redirect.spec.ts`. (AGT-051)

Provider and speech keys are managed per user with a connectivity check
each: each key can be read, saved, removed and tested when enabled, and the
assistant and dictation gates react to the stored state. Endpoints are the
config get/put/delete plus test-post pairs under both `config/` and
`stt-config/`. The flag-off write refusals are the main#586 target in §14;
the page chrome itself is rowed under profile/settings. Note carried: the
backend strips the trailing slash when saving the base address. Oracle:
`assistant-plus/key-lifecycle.spec.ts`. (AGT-052)

Model key, speech key and the dictation flag are separate prerequisites
with separate gates: chat needs the provider key, dictation additionally
needs the speech key and the flag on (default off), and each missing
prerequisite blocks only its own surface with its own guidance. Config and
speech-config reads, with the flag travelling on the speech-config get. The
flag thirds are the main#586 target; only the provider/speech separation is
proven in this checkout. Oracle: `assistant-core/key-gates.spec.ts`.
(AGT-057)

The bundled-engine profile and token endpoints stay desktop-only: web chat
never calls `GET /api/users/me/ai-assistant/agent-profile/` or `POST
.../agent-token/`, and only desktop-gated sessions may use them. Chat,
config and transcription endpoints require member-or-higher workspace role
on every edition. Note: the inventory coverage checklist names the token
read as GET, but the desktop runtime calls it as POST with an empty body
and reads back a token plus expiry — the contracts issue follows the POST
shape. Oracle: `assistant-plus/desktop-only.spec.ts`. (AGT-059)

Tool-server registry management lives in settings, not in chat: chat
surfaces never create, edit or remove tool servers, the skipped-server
notice links nowhere and only reports, and the registry chrome is rowed
under profile/settings. The registry endpoints (`GET/POST
.../mcp-servers/`, `PATCH/DELETE .../mcp-servers/{id}/`) are contracted so
the gates stay testable. Oracle:
`assistant-plus/registry-in-settings.spec.ts`. (AGT-060)

The embedded AI writing helper inside rich editors takes a typed request
plus the surrounding draft, shows the generated markup for review, and
inserts it on confirmation. It enters from the issue description editor and
the page editor AI menu through a task popover; empty returns are marked
invalid, quota rejections and other failures confirm distinctly, and
closing discards the draft request. Service calls are the issue-modal task
create and editor-task calls (`POST .../ai-assistant/`, `POST
.../rephrase-grammar/`). Bug NEWFRONT-206 in §18: the page half has its AI
extension disabled so no entry renders, the ask-submit is unwired, and no
rephrase backend exists. Notes carried: the issue-modal caller passes no
prompt (the wire carries the typed task only), answers run at the transport
contract stubbed, and the same scenario locks the keyless-entry-hidden
half. Oracle: `assistant-plus/editor-ai.spec.ts` plus
`assistant-plus/page-ai-menu.spec.ts`. (AGT-063)

This slice also owns the dictation kill switch (AGT-064). The target is
restated in §14; the slice builds from that text, since the live old app
still renders dictation unflagged.

## 13. Cross-epic seams (resolved)

SHELL-007 assistant-card links (card shell owned by NEWFRONT-70, this epic
owns the assistant view). The deep-link contract the shell card relies on:
the landing lives at `/{workspaceSlug}/assistant` and each thread at
`/{workspaceSlug}/assistant/{threadId}`. The shell card reads its threads
through the existing `listAssistantThreads` contract (`GET
/api/workspaces/{slug}/ai-assistant/threads/`, newest first, automation
threads excluded) and its setup state through `getLlmConfig` (`GET
/api/users/me/ai-assistant/config/`, blank default when absent); this area
must honor those addresses and that thread shape (identifier, title,
archived flag, active-turn flag, created and updated timestamps) — titles
with a fallback label for untitled threads, archived threads never shown,
at most five recents newest first. Suggestion picks and the setup reminder
lead to the landing; recent rows link to thread addresses. Both sides link
through the F-09 coexistence helper, so the links flip from anchor to
router navigation automatically when the area gate registers the assistant
prefixes. Gate-provable on both sides with stub thread rows.

Dashboard and sidebar surfaces adjacent to SHELL-048 that link schedulers
or prompts. The shell renders the entries; this area owns every
destination. Concretely: the sidebar More disclosure lists the workspace
Prompts and Schedulers entries (auto-opening with the row active), and the
project tab strip lists the Schedulers tab entry; all three currently mount
thin placeholder screens owned by shell slices, which this area's slices
replace with the real catalog, prompts and project-sections screens (§§1,
2, 4–6). A survey of the dashboard quick links found no scheduler, prompt
or assistant entries there — no seam. The intake pending-count rule and the
tab permission checks stay shell-side; destination readiness never gates
the shell.

Route ownership. This area registers and owns content for: the workspace
`/{ws}/schedulers` catalog; the workspace `/{ws}/prompts` sections and
receipt views; the assistant `/{ws}/assistant` landing and
`/{ws}/assistant/{threadId}` thread view (no placeholder exists yet — this
area registers both); the project `/{ws}/projects/{id}/schedulers` section
with its list, calendar and `{bindingId}` detail routes; the
`/{ws}/settings/projects/{id}/schedulers` settings variant panel; and the
`/{ws}/settings/projects/{id}/automations` page content with its extension
slot. Other epics own: the sidebar and tab-strip entry points (shell,
NEWFRONT-70); the settings shells those two settings pages mount in
(project-settings area); the profile and provider-settings page chrome
behind the setup-card, dictation-setup and registry links (profile area),
including the dictation deep-link anchor inside the assistant settings
page; the dashboard widget stack around the assistant card (shell); the
issue and page editor chrome hosting the AI writing helper (issues and
pages areas); the run-detail destinations linked from run history and tool
results (runners area); and the reusable history panel itself (runners
area, shared with runner chat). The area gate adds this area's path
prefixes to the migrated-route list (F-09).

## 14. Target-behavior rows (NEWFRONT-156 restatement)

AGT-064 — Dictation kill switch, target per main#586. An operator flag
(default off, environment-sourced locally and injected per environment in
the cloud) switches voice dictation off instance-wide. When off: the speech
config save, connection test and transcription endpoints answer not-found
with the dictation-disabled error code, while removing a saved speech key
stays open so a stored key can always be deleted; the speech config read
still answers and reports the flag; saved speech configs are untouched. The
web app reads the flag from the speech config get it already makes, so
flipping it needs no rebuild: with the flag off, the flag loading, or the
read failed, the composer microphone and all its hints hide; the voice
section is removed from the assistant settings page, or reduced to a
remove-only panel when a key is saved; a recording in progress is abandoned
if the flag turns off mid-recording; and a dictation-disabled response
re-reads the config so the microphone hides. The new-app slice (NEWFRONT-608)
builds exactly this; the gate aligns the scenario. No oracle file yet.

AGT-065 — RRULE builder, target per main#561. The Recurrence field in all
three binding dialogs (project new-scheduler, edit binding, workspace
install-on-projects) is one shared Google-Calendar-style builder over the
plain stored rule string. The widget offers a repeat-every count plus unit
(day, week, month, year, or does-not-repeat for the single-shot empty
rule), Sunday-first weekday chips for weekly rules (never zero chips), a
monthly day-or-nth-weekday choice defaulting from the start, and an Ends
group (never, on-date interpreted end-of-day UTC inclusive, after-N
occurrences with a near-term prefill), with the raw textarea kept under an
advanced disclosure. Serialization respects the API validator: bare
frequency form with no rule prefix and no embedded start, the interval
omitted at one and clamped to at least one, no sub-daily cadence in the
widget. Opening a dialog reconstructs widget state from the stored rule;
anything the widget cannot express exactly opens in raw mode and submits
byte-identical unless actually edited — the widget serializes on
interaction only, never on open — and editing raw re-syncs the widget where
expressible. The plain-language confirmation line stays live under the
widget. The new-app slice (NEWFRONT-587) builds exactly this from the
inventory acceptance text plus this restatement, not from the old textarea.
No oracle file yet.

Nested main-PR targets inside oracle-green rows. AGT-013 carries two: the
Completed badge on an exhausted finite series (main#559 — the exhausted
signal does not exist in this checkout) and run-row links to run detail
(main#561). AGT-028 carries receipt side-nav anchors (main#579). AGT-010
and AGT-020 carry the builder halves of main#561 (raw-mode reconstruction,
fix-before-submit hint). Slices implement the target wordings in §§3–5 and
§7; the gate aligns the scenarios.

## 15. F-06 reuse map (survey of apps/web_new)

Surveyed: kit components, TanStack Query patterns, api-client contracts,
core helpers, shared UI, and the registered routes on web-new-dev. Per
slice, what exists versus what must be added:

- NEWFRONT-581 (catalog CRUD): exists — Dialog, Input, Textarea, Switch,
  Toast, EmptyState, Skeleton, Button; feature query/mutation plus key
  factory patterns (home, issues, notifications); permission hooks and role
  constants; the thin schedulers route mount to replace. Must add — zod
  contracts for scheduler definitions (NEWFRONT-568); a table or row-list
  presentation (no Table in the kit — propose it there since later slices
  reuse it); the shared create/edit dialog plus handle field later slices
  reuse.
- NEWFRONT-584 (installs, bulk, single, author): exists — everything above
  plus the catalog slice's dialog pieces. Must add — binding contracts
  (NEWFRONT-568); a project picker with search and select-all (no picker
  beyond the timezone one exists); the project schedulers list mount and
  the settings-embedded variant mount.
- NEWFRONT-587 (binding edit, toggle, uninstall, RRULE): exists —
  optimistic-mutation helpers in core/query (snapshot, rollback,
  invalidate) for the toggle; Dialog and Switch. Must add — the shared
  schedule-fields piece including the RRULE builder (§14), outcome-mode
  and pod field pieces (shared with NEWFRONT-590), date/time inputs (no
  Date picker exists in shared pickers).
- NEWFRONT-590 (detail, fields, tabs, gates): exists — session guards and
  permission hooks for the gates; Toast. Must add — the install detail
  route and run-history table (kit Table proposal); the recurrence
  plain-language renderer; the section tab shell (no Tabs in the kit —
  propose it there); page-and-size run paging; the self-refresh timer.
- NEWFRONT-593 (calendar, rail, occurrence panel): exists — TimezonePicker;
  local-storage platform helpers for the per-project hidden set. Must add —
  the month/week calendar views, visibility rail, occurrence panel and
  occurrences-read wiring; all calendar presentation is new.
- NEWFRONT-597 (prompt browser, edits): exists — Dialog, Textarea, Toast,
  EmptyState, Skeleton; the thin prompts route mount to replace. Must add —
  prompt-section contracts (NEWFRONT-568); the sections browser, section
  editor with compare view, and revert flow; anchor side navigation.
- NEWFRONT-599 (receipt, previews, states): exists — the editor from
  NEWFRONT-597. Must add — compiled and preview contracts (NEWFRONT-568);
  receipt views, both preview paths, and the page-level loading and error
  states.
- NEWFRONT-600 (automations): exists — the project-update contract
  (`updateProject`) and the state-list contract (`listStates`) for the
  cancelled-state target; Switch; session guards. Must add — the
  automations settings page mount (no settings routes exist yet —
  coordinate the shell with the project-settings area), the archive/close
  rows, the custom-month dialog, and the extension slot contract.
- NEWFRONT-603 (assistant landing, setup, gates): exists — the shell card
  shell, thread-list and config reads, and deep-link helpers (§13); the
  thread-list, thread-create and model-config contracts. Must add — the
  assistant landing route and layout registration (no assistant routes
  exist yet), the setup card, and the composer shell in its locked and
  unlocked states.
- NEWFRONT-605 (thread view, streaming, send, titles): exists — the
  platform event stream on web and desktop; the realtime fan-out stub in
  core/query (the only place allowed to turn server events into cache
  patches — the slice wires the assistant transport through it). Must add —
  the thread route, message and send contracts plus the parsed event union
  and cancel (NEWFRONT-577), the streaming renderer with stop/cancel, the
  exactly-once landing handoff, and title refresh.
- NEWFRONT-607 (sidebar, deeplinks, tools, paging): exists — the thread
  view from NEWFRONT-605; the thread-list contract. Must add — the
  assistant's own inline sidebar (not the shared panel per AGT-061),
  deep-link loading, tool-activity rendering with the skipped-servers
  notice, cursor transcript paging, and the poll-based completion
  fallback.
- NEWFRONT-608 (keys, voice, registry, desktop, editor): exists — the
  composer from NEWFRONT-605; the model-config contract; platform storage
  and desktop transport. Must add — speech-config, transcription,
  registry, desktop profile/token and editor-AI contracts (NEWFRONT-577);
  the voice dictation UI with the §14 kill-switch gating; the desktop-only
  call discipline (never from web chat); the editor helper integration —
  note shared/editor is still empty, so coordinate with the issues/pages
  areas that own the editor chrome.
- NEWFRONT-568/577 (contracts): exist — the client with CSRF, timeout,
  abort and normalized errors; the contract plus contract-test pattern;
  the already-covered assistant thread-list, thread-create and
  model-config contracts. Must add — every endpoint in §17 not yet
  covered, each with a contract test against the Django test server.

## 16. Own-code module to slice mapping

Verified against the tree and the Reference-not-source path list. Porting
allowed means the slice moves the logic into the new trees and adapts it
(kit components, TanStack Query plus contracts instead of MobX stores and
axios services, core/platform for desktop calls, new license header, no
runtime imports); anything the module uses from Plane code stays
reference-only and is rewritten.

- NEWFRONT-581: workspace scheduler catalog routes; the workspace
  scheduler component set (definition fields, create/edit dialog,
  delete and bulk-install dialogs, project picker, derivation and install
  helpers); the scheduler definition store; the scheduler service
  (definition lifecycle). All own code.
- NEWFRONT-584: project scheduler routes and the settings scheduler
  routes (own code) with the project picker and dialog pieces ported by
  NEWFRONT-581; the install flows' binding components are Plane code
  (reference-only, rewritten) — only the routes, store and service port.
- NEWFRONT-587: the shared schedule-fields piece and edit/uninstall
  dialogs are Plane code (reference-only, rewritten, with the AGT-065
  builder built new from §14); the scheduler service binding calls port.
- NEWFRONT-590: the install detail, run history, outcome-mode, pod,
  calendar-drawer and recurrence-text components are Plane code
  (reference-only, rewritten); the binding, runs and occurrences service
  calls port.
- NEWFRONT-593: the whole calendar tree (shell, header, rail, month and
  week views, drawer, occurrence and visibility helpers) is Plane code
  (reference-only, rewritten); the occurrences service call ports.
- NEWFRONT-597 and NEWFRONT-599: the prompts routes, the prompt-section
  store, and the prompt-section service (list, upsert, revert, compiled,
  preview) are own code and port; the store hook delegate and the
  page-level caching and presentation are Plane code (reference-only) and
  are rewritten onto TanStack Query and the kit.
- NEWFRONT-600: the auto-pm service is own code and ports; the
  automation row components, the automations settings route and the cloud
  automations slot components are Plane code (reference-only, rewritten).
- NEWFRONT-603, NEWFRONT-605 and NEWFRONT-607: the assistant routes and
  layout, the assistant component set (chat root, live and start hooks,
  setup card, landing card, widget, message router, skipped-servers
  notice), the chat component set (container, list, bubble, composer,
  history panel), the web and desktop-overlay agent runtimes, the desktop
  session helper, and the assistant, desktop-adapter and desktop
  event-source services are own code and port — with the composer,
  transport and event parsing adapted onto the platform stream and the
  realtime fan-out. The dictation hook, the settings page chrome, and the
  cloud-edition desktop runtime file are Plane code (reference-only); the
  profile area owns the settings pages.
- NEWFRONT-608: the composer microphone cell (own code, ports with the
  §14 gating built new); the desktop overlay agent runtime and desktop
  services (own code, port); the speech lifecycle endpoints contracted
  new. The dictation hook, the key and registry settings pages, the
  editor AI helper popover and page-editor AI menu, and both AI services
  are Plane code (reference-only, rewritten) — the issue body's editor-AI
  service candidate is corrected here. Anything ported that touches
  Plane UI, state or helpers is rewritten at the seam.

## 17. Endpoint reference

Contracts issues: schedulers plus prompts plus automations (NEWFRONT-568),
assistant plus voice plus registry plus editor (NEWFRONT-577). Already
covered (not redone): thread list, thread create, model-config read, auth
and session reads, workspace and project lists, issue list and detail, the
project update and the state list.

Scheduler definitions: list, retrieve (supporting read with no observed
call site), create, update (handle omitted) and delete on
`/api/workspaces/{slug}/schedulers/` and `.../schedulers/{id}/`. Definition
fields: name, handle, description, base prompt, color, enabled flag, origin,
install count, timestamps. (NEWFRONT-568; AGT-001–004.)

Scheduler bindings: list, retrieve, create, update (including enabled-only
toggle payloads) and delete on
`/api/workspaces/{slug}/projects/{id}/scheduler-bindings/` and
`.../scheduler-bindings/{bindingId}/`. Binding fields: scheduler reference,
project, start, timezone, recurrence rule, extra and skip dates, extra
context, enabled flag, finding-handling mode, pod reference, next and last
run facts, last error, installer, timestamps. (NEWFRONT-568;
AGT-006–013, AGT-018, AGT-019.)

Binding runs: paged history on
`.../scheduler-bindings/{bindingId}/runs/?page=&per_page=`, same envelope
as the runner run list. Occurrences: windowed read on
`.../scheduler-bindings/occurrences/?from=&to=` with the truncation hint
fields. Pod selector source: `GET /api/runners/pods/?project=` (active
pods only). (NEWFRONT-568; AGT-013–015, AGT-017, AGT-019.)

Prompt sections: ordered resolved-section list on `GET
/api/workspaces/{slug}/prompt-sections?kind=&scope=`; upsert on `PUT
.../prompt-sections/{key}` with scope plus body in the payload; revert on
`DELETE .../prompt-sections/{key}?scope=`; assembled template on `GET
/api/workspaces/{slug}/prompts/{kind}/compiled?scope=` including the
automatic-run variant; render preview on `POST
/api/workspaces/{slug}/prompts/{kind}/preview` with the target id, or the
draft scope, key and body for unsaved drafts. (NEWFRONT-568; AGT-023–031,
AGT-033.)

Project automations: the project update with the archive delay, close
delay and default cancelled-state fields. (Covered by the existing project
contract; NEWFRONT-568 extends the payload if needed; AGT-034, AGT-035,
AGT-037.)

Assistant threads and messages: thread get plus the covered list and
create; thread update and delete (no chat buttons — contracted for the
reserved methods); paged message reads on `GET
.../threads/{id}/messages/` with offset and limit; message create on `POST
.../threads/{id}/messages/` with the content; title helper on `POST
.../generate-title/` (create-issue dialog use). Message fields: identifier,
role, content, status, sequence number, link payload. (NEWFRONT-577;
AGT-038, AGT-042–045, AGT-053–056, AGT-058.)

Assistant streaming: event stream on `GET .../threads/{id}/events/` with
an offset parameter for replay; stop on `POST .../threads/{id}/cancel/`.
The contracted shape is the parsed event union — turn started, assistant
delta (message reference plus sequence-number dedupe), message created and
completed, tool call and tool result, skipped tool servers with reasons,
turn failed with detail, turn cancelled and turn completed — not the socket
itself. (NEWFRONT-577; AGT-042, AGT-046, AGT-056.)

Keys, voice, registry, desktop, editor: provider key lifecycle
(get/put/delete plus test post) and speech key lifecycle (same pair, with
the enabled flag on the speech get and the dictation-disabled refusals per
§14); transcription post (multipart, upload field `file`); registry
list/create/update/delete; desktop-only profile get and token post (never
called from web chat); editor task create and rephrase posts. (NEWFRONT-577;
AGT-050–052, AGT-057, AGT-059, AGT-060, AGT-063, AGT-064.)

## 18. Recorded deviations (old bugs, linked issues)

Slices implement the intended target in each case; the gate proves it.

- NEWFRONT-193 — Prompting writes rejected: workspace save (AGT-024),
  personal save (AGT-025), revert confirm (AGT-027), saved preview
  (AGT-030) and draft preview (AGT-031) fail server-side in the live old
  app while the rest of the API is exempt. The oracle pins the intended
  API flows; the new app implements them.
- NEWFRONT-194 — Receipt count template: badges and side nav render the
  raw count template instead of the section count (AGT-028). The new app
  renders the count.
- NEWFRONT-195 — Modified Enter submits: the live composer submits on
  Control+Enter as well as unmodified Enter (AGT-049). The new app submits
  on unmodified Enter only.
- NEWFRONT-197 — Live assistant events dropped: every live stream event is
  corrupt, so completion, failure lines and skipped-server notices run
  through the poll or reload replay instead of mid-turn (AGT-042, AGT-043,
  AGT-046). The new app implements the event-driven target against the
  contracted event union.
- NEWFRONT-206 — Page editor AI half missing: the AI extension is disabled
  so no entry renders, the ask-submit is unwired, and no rephrase backend
  exists (AGT-063). The page-ai-menu scenario locks this as bug state; the
  new app implements the intended helper on both editors.
- Inventory checklist slip: the token endpoint is called as POST with an
  empty body (AGT-059), not GET as the coverage checklist names it. The
  contracts issue follows the POST shape.
