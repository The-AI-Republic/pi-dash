# Intake: area spec (NEWFRONT-687)

Rows INT-001–INT-034. Written from the old intake route, the old
two-pane request list and detail, the triage dialogs, the create dialog,
the filter and sort surfaces, the old client stores and services, and the
current intake API (views, URLs, serializers). The old app is reference
only: this document records behavior, not code, strings, styles, icons or
assets. Oracle scenarios under `apps/web_new/e2e/parity/intake/` are the
observed-behavior record; this spec is what the new build implements.

Intake is the project triage screen: a request list on the left, a
request detail on the right, a create dialog in the header, and triage
actions (accept, decline, snooze, mark duplicate, delete) on a request.
An intake request wraps a real work item plus an intake state, a source
and a triage status. The project feature toggle `inbox_view` (serialized
from `intake_view`) gates the whole screen.

This spec is the shared contract for the Intake slices:

| Slice                                                                                               | Rows                          |
| --------------------------------------------------------------------------------------------------- | ----------------------------- |
| A — route shell, tabs, empty states, no-push reads (NEWFRONT-689)                                   | INT-001–005, 008, 012, 031    |
| B — list rows, paging, filters, ranges, order (NEWFRONT-690)                                        | INT-006, 007, 009–011         |
| C — create dialog, validation, create-more, dismiss guard, attachments, triage state (NEWFRONT-691) | INT-013–017, 033              |
| D — detail, properties, versions, navigation, URL (NEWFRONT-692)                                    | INT-019, 024, 026–029         |
| E — accept, decline, snooze, duplicate, delete (NEWFRONT-693)                                       | INT-020–023, 025              |
| F — guests, narrow viewport, edition seams, duplicate detection (NEWFRONT-694)                      | INT-018, 030, 032, 034        |
| Contracts (NEWFRONT-688)                                                                            | every endpoint the area calls |

Every row appears in exactly one slice. The area gate NEWFRONT-695 runs
all of them on `apps/web_new`.

## Vocabulary

Intake status is numeric on the wire and has a fixed five-value domain:

| Status    | Value | Group                                                                            |
| --------- | ----- | -------------------------------------------------------------------------------- |
| Pending   | -2    | Open                                                                             |
| Declined  | -1    | Closed                                                                           |
| Snoozed   | 0     | Open (reachable through an explicit status filter; not in the default Open read) |
| Accepted  | 1     | Closed                                                                           |
| Duplicate | 2     | Closed                                                                           |

"Open" and "Closed" are the two screen tabs; each tab has a default
status filter (INT-012) that the user may widen through the filter panel
(INT-009). A request source is one of in-app, forms or email; only
in-app is produced in this checkout (INT-034). Editable request fields
are title, description, priority, assignees, labels and due date; the
intake state is shown but not edited from the request (INT-019).

Permissions used throughout: a project admin; a project member who is
not an admin; the request creator; a project guest. Guest visibility is
also governed by the project's guest flag that lets guests view all
features (INT-032).

## Shell, tabs, empty states and entry (INT-001–005, 008, 012, 031)

### INT-001 — route and two-pane shell

The screen lives at `/{ws}/projects/{projectId}/intake`. The retired
project-inbox address `/{ws}/projects/{projectId}/inbox` redirects there,
preserving the workspace and project; the redirect adds no state and
lands on the default tab. The page shows the project breadcrumb trail
ending in an Intake crumb that links back to the intake route, a header
area carrying the create control and the syncing indication, and below
it a two-pane body: a scrollable list pane and a content pane.

With a request selected the content pane shows that request's detail;
with none selected it shows a neutral "pick a request" placeholder
instead of an empty detail. The list pane is always present while intake
is enabled (its narrow-viewport form is INT-030). The retired address
produces the same screen as the canonical one.

The route is currently a stub (`TabAreaMount`, SHELL-075) at
`apps/web_new/src/routes/$ws/_gated/projects/$projectId/intake/index.tsx`;
slice A replaces the stub. The retired inbox address is registered as a
redirect route; this is owned by the workspace-scoped routing work
(NEWFRONT-528) that the area gate depends on.

### INT-002 — feature gate

When the project's `inbox_view` is false, the screen shows a
feature-disabled empty state instead of the two-pane body: an
explanation that intake is off for the project and a single action that
leads to the project's feature settings at
`/{ws}/settings/projects/{projectId}/features`. No list, detail or
reference read for intake occurs while gated.

The settings action is enabled only for project admins; members and
guests see it present but disabled. The route still resolves — a
non-admin who deep-links to a gated project sees the gate, not a
permission error. The gate reflects the project's current flag, so
switching the feature on and re-entering shows the list.

The toggle screen itself belongs to Project settings. Intake only
consumes the project's feature flag.

### INT-003 — page title

The document title is the project name plus the Intake label. When the
project name cannot be resolved yet, the title falls back to the
product name plus the Intake label rather than showing an empty name.

### INT-004 — Open and Closed tabs

The list pane header carries two tabs, Open then Closed, with Open
active by default. The Open tab shows a live count of pending requests
taken from the list read's total; the count renders only while Open is
active, and Closed shows no badge. The active tab is reflected in the
URL as `?currentTab=open` or `?currentTab=closed`.

Switching tabs refetches the list with that tab's default status set
(INT-012) and resets the selection when the previously selected request
is not in the new tab's list. Switching back reads fresh data, so a
request created elsewhere while the Closed tab was open appears when
Open is reselected. Deep links (INT-028) select the tab before the
request.

Intended: real tab semantics (`role="tablist"` with `role="tab"`
children, `aria-selected`, `aria-controls` pointing at the list panel),
and the Open badge is part of the tab's accessible name ("Open, N
pending"). The old screen used plain clickable elements; this is an
accessibility improvement, not a behavior change.

### INT-005 — auto-selection

Entering the intake route with no `inboxIssueId`, and with a non-empty
list in the active tab, replaces the navigation with a redirect that
adds `inboxIssueId=<first row id>` while keeping `currentTab`. The first
row is the list's first item in the active order (newest-created by
default). The redirect must not loop: it fires once per entry and the
selection then lives in the URL.

If the active tab's list is empty, nothing is selected and the content
pane keeps the INT-001 placeholder. An empty Closed tab does not
auto-select either. Auto-selection does not fire when the URL already
names a request, even a stale one — INT-028 owns fallback.

### INT-008 — list and content empty states (bug NEWFRONT-262)

Three distinct situations must render differently:

- Filtered to nothing: the active tab has data conceptually but the
  applied filters match no request. The content pane shows a no-results
  state naming filters as the reason, with a way to clear them.
- Empty Open tab with no filters applied: the content pane shows
  guidance explaining that requests arriving here are waiting for
  triage, plus a create action that opens the create dialog (INT-013).
- Empty Closed tab with no filters applied: an informational state
  saying nothing has been resolved yet. It offers no create action.

"The default tab status is not an applied filter" is the rule. The old
screen treated its default status filter as applied, so every empty tab
showed no-results and the guidance and informational states were
unreachable (NEWFRONT-262). The oracle scenario pins the old no-results
outcome; the new build implements the three-way intended behavior, and
the gate must assert the three states, not the pinned old one.

### INT-012 — tab defaults

Without any explicit status filter, Open reads exactly Pending
(`status=-2`) and Closed reads Accepted, Declined and Duplicate
(`status=1,-1,2`). Snoozed rows are Open in vocabulary but are not part
of the default Open read; they appear when the user adds Snoozed in the
status filter (INT-009). Applying a status filter replaces the tab
default for that tab session; clearing the filter restores it. Switching
tabs restores the tab's own default rather than carrying the other
tab's filter.

### INT-031 — no push, fetch-triggered updates

Intake has no websocket, poll or focus-refetch. The list refreshes on:
initial mount, tab switch, filter change, sort change, next-page load,
manual navigation between requests after a triage action, and the
mutations that invalidate it. The detail is a one-shot read per selected
request; it does not refetch when the window regains focus, and it does
not refetch when another viewer changes the record.

The new build must override the app-wide query default that refetches on
window focus for intake list, detail and reference reads, so re-focusing
the tab never refetches. A change made outside this screen appears only
when one of the triggers above runs.

## List pane

### INT-006 — request row

Each row, in order, shows:

- the project-scoped identifier from the sequence id (the project
  identifier prefix plus the work item's sequence number);
- a status chip for any status other than Pending; a pending row shows
  no chip;
- the request title, truncated to the row's width with the full title
  available on hover/focus;
- the created date, with a tooltip carrying the full timestamp;
- a priority marker that renders for every priority including none, and
  whose tooltip carries the row's priority key;
- labels: up to three label chips; four or more collapse to a single
  count chip for the remainder;
- the author avatar.

The author is the work item's creator. A request created by the system
intake identity (forms or email ingestion) shows an intake identity
avatar instead of a member avatar. A forms-sourced row that a member
created through the in-app form keeps the member's avatar: the rule is
about the creating identity, not the source value.

The priority tooltip is part of the intended behavior. In the old app
the marker's trigger was a fragment, so its tooltip never opened
(NEWFRONT-273); the oracle scenario therefore asserts the marker's
presented priority key rather than a visible tooltip. The new build
renders a real trigger and the gate must be able to assert the tooltip.

Rows are selection controls: the selected row is marked (INT-027), and
activating a row writes `inboxIssueId` into the URL without a full page
load.

### INT-007 — cursor paging

The list reads ten requests per page (`per_page=10`), ordered by the
active sort (INT-011, default newest created). When the list pane nears
its end the next page is requested with the previous page's
`next_cursor` as `cursor`; the server reports whether another page
exists. Appending keeps the already-rendered rows and never duplicates
ones already shown; the combined order is the server order.

Placeholder rows render below the loaded rows while a page is in
flight, and are removed when it settles. The header shows a syncing
indication while a page is loading. In the old app the syncing
indication never appeared because nothing set the paged-loading state
(NEWFRONT-269); the oracle pins its absence. The new build implements
the indication and the gate asserts it appears during a delayed page
then clears.

Reloading the list (tab, filter or sort change) resets to the first page
and the first page's cursor chain; the old rows are not reused.

## Filtering and ordering

### INT-009 — filter panel

The filter panel opens from the list header and has sections in this
fixed order: status, priority, assignees, created by, labels, created
date, updated date. A search box filters the sections and their options
by name. Each section that has an active value shows a count.

Section behavior:

- **Status**: multi-select over Pending and Snoozed, in both tabs. It
  starts with the tab's default values (INT-012) — Pending on Open, and
  the three resolved values on Closed — which render as chips. The last
  remaining value is not removable, so the section is never empty, and
  clearing the section restores the tab default.
  Consequence of that fixed option set: on the Closed tab the status
  section cannot narrow between accepted, declined and duplicate. This
  matches the observed behavior and the inventory; the new build does
  not invent a resolved-status filter. If that narrowing is wanted, it
  is a new capability: add an inventory row and a bug/feature issue
  first, not a silent extension in slice B.
- **Priority**: single-select over Urgent, High, Medium, Low, None.
- **Assignees**: multi-select over project members.
- **Created by**: single-select over project members.
- **Labels**: multi-select over project labels.
- **Created date** and **updated date**: single-select over quick
  presets plus an explicit range (INT-010).

Selecting an option refetches the list with the corresponding query
parameter: `status`, `priority`, `assignees`, `created_by`, `labels`,
`created_at`, `updated_at`. Multi-value parameters are comma-separated.
Every applied value is visible as a chip outside the panel, grouped by
section, and each chip and each section group can be cleared; clearing
restores the tab default where a default exists and refetches. The panel
shows a running applied-filter count that excludes the tab's default.

Changing any filter resets paging to the first page and drops the
currently selected request if it is no longer in the result (INT-028
fallback).

### INT-010 — date ranges (bugs NEWFRONT-270, 271, 274)

Both date sections offer the same five options in this order: Today,
Yesterday, Last 7 days, Last 30 days, Custom.

A preset becomes an inclusive day range and is sent in the parameter as
a pair of day bounds, `FROM;after,TO;before`, where `FROM` and `TO` are
ISO days. Today is `T;after,T;before`; Yesterday is `Y;after,Y;before`;
Last 7 and Last 30 end today and start 6 or 29 days earlier
respectively.

Custom opens a range dialog with a from day and a to day, defaulting to
a sensible recent span, not allowing an end before the start, and
requiring both ends before it applies. Applying sends the same
`FROM;after,TO;before` encoding. While a custom range is open the
dialog's title names the section it was opened from, so the created-date
dialog and the updated-date dialog are distinguishable.

Old defects and intended behavior:

- NEWFRONT-274: the old custom range sent an empty `,` pair and never
  narrowed the list. The new build sends `FROM;after,TO;before` and the
  list narrows server-side. The oracle pins the empty pair; the gate
  asserts the narrowing.
- NEWFRONT-271: the old custom dialog title was the created-date title
  in both sections. The new dialog title follows the section. The gate
  asserts both titles.

One pair of date predicates is not the client's to fix, so INT-010 is
honest about it:

- NEWFRONT-270 / PDASHOSS01-312: the shared backend helper that applies
  the updated-date filter predicates the created column. The intake
  list read still sends `updated_at=FROM;after,TO;before`; it must not
  omit the parameter, because omitting it would silently drop the user's
  filter and would leave the client divergent from the work-item list.
  web_new therefore inherits the backend defect for rows whose updated
  day differs from their created day.
- Consequence for the area gate: the INT-010 updated-date scenario can
  become new green only after PDASHOSS01-312 is Done. NEWFRONT-695 is
  related as blocked by PDASHOSS01-312. Until then the gate may record
  the created-date, preset and custom-range assertions as green and must
  keep the updated-date assertion red with this link, rather than
  weakening the scenario.
- The oracle's updated-date preset assertion passes on same-day fixture
  data because created and updated share a day; it does not prove the
  correct column.

### INT-011 — ordering

The header's order menu lists: created date, last updated date, project
id. Each can be ascending or descending; the selected entry is marked
and the direction is toggled from the same menu. The default is created
date, descending (newest first). Choosing an order refetches from the
first page with `order_by`: `issue__created_at`, `issue__updated_at` or
`issue__sequence_id`, prefixed with `-` for descending. Ordering is
server-side; the client never re-sorts locally. Changing the order keeps
the current filters and tab and drops a selection that moves out of the
first page.

## Create (INT-013–017, 033)

### INT-013 — create dialog

An authorized user (admin; the feature must be on) sees a create control
in the intake header. It opens a dialog with a required title, a
rich-text description, and inline properties: intake state, priority,
assignees, labels and due date.

Defaults: the intake state is the project's triage state (INT-033); all
other properties start empty. Title is a single line; description is
rich text with the shared editor's toolbar (headings, formatting, lists,
links, code, attachments). Properties use the shared pickers: intake
state (INT-033), priority (INT-009 picker), members (INT-009 picker),
labels, and the shared single-date picker for the due date.

Saving posts to `POST /api/workspaces/{ws}/projects/{pid}/intake-issues/`
with:

- `source`: `"IN_APP"`;
- `issue`: an object carrying `name`, `description_html`, `priority`,
  `state_id`, `label_ids`, `assignee_ids` and `target_date` (the due
  date). The server overwrites `state_id` with the project's triage
  state and ignores a client-supplied intake state.

The server guarantees the request lives in the project's triage state,
creating that state if the project has none (INT-033), and records a
creation activity on the work item. The response is the created
request's detail payload; the screen consumes its work-item id. On
success the dialog closes, the URL becomes
`?currentTab=open&inboxIssueId=<new id>` (switching to Open if needed),
and the list is invalidated so the new request appears. A success notice
is shown.

The button remains disabled until the title has non-blank content; the
dialog stays open with the server's error if creation fails.

### INT-014 — create validation

- Title longer than 255 characters: submission is blocked client-side,
  the dialog stays open, the field shows a length hint, and no request
  is sent.
- Empty title: the client normally prevents this; if it reaches the
  server, the server answers 400 with an error naming the missing name,
  the dialog stays open, and the message is surfaced.
- Priority outside the five-value domain: the server answers 400 with an
  invalid-priority error; the dialog stays open and surfaces it.

In all three cases nothing is created and the form keeps its input.

### INT-015 — create more

The dialog has a "create more" toggle, off by default. With it on,
saving keeps the dialog open, clears the form back to its defaults
(including the intake state default), and leaves the created request
visible behind it; the screen does not navigate to the new request.
With it off, saving closes the dialog and navigates to the new request
as in INT-013. The toggle's state persists only for the open dialog.

### INT-016 — dismiss guard (bug NEWFRONT-304)

The dialog's close control is blocked while the rich-text editor is
still processing — specifically while an attachment upload started from
the description is in flight. Blocking does not close the dialog; it
shows a short wait notice and the dialog stays with its content. Escape
is blocked the same way. Once the editor is idle, close and Escape both
dismiss; dismissing discards the draft without confirmation.

The old app closed on Escape even while busy (NEWFRONT-304); the oracle
pins that. The new build treats Escape exactly like the close control
and the gate asserts the dialog stays open while busy and closes once
settled.

### INT-017 — attachments while composing

Images and files added through the description editor are uploaded
before save and attached to the created work item on save. The
description HTML references the uploaded asset, and a post-create bulk
attachment confirms the assets against the new work item's id as
description assets.

This is not intake's uploader. It reuses the Issues create path owned by
NEWFRONT-631:

- `apps/web_new/src/features/issues/create/ModalDescription.tsx` —
  uploads through the shared editor, reports each parked asset through
  its `onAssetUploaded(assetId)` callback and editor readiness through
  `onReadinessChange(ready)`; its `assetScope` prop names the asset
  entity/scope for the draft. Intake supplies the draft scope while
  composing.
- `apps/web_new/src/features/issues/create/createApi.ts` —
  `uploadDescriptionAsset` / `attachDescriptionAssets`, with the
  contract in `packages/api-client/src/contracts/issue-description-assets.ts`.
  Intake calls the attach hook with the created work-item id (it is not
  the intake-entry id) and the parked asset ids, and does so after the
  create response returns.
- The readiness callback is what INT-016's guard reads.

Slice C carries a blocked_by edge on NEWFRONT-631 (already merged on
`web-new-dev`), because a second uploader would fork the Issues create
path. If the create path later changes its props, intake extends it and
opens an edge; it never copies the upload logic. Intake needs no
attachment surface of its own: every composed attachment belongs to the
work item's description.

### INT-033 — triage state

The create dialog's intake-state picker, and the detail's read-only
state, read the project's single triage state from
`GET /api/workspaces/{ws}/projects/{pid}/intake-state/`. The picker
reads lazily when opened and falls back to the first intake state when
the value is unset. If the project has no intake state yet the read
answers 404, which the picker treats as "none"; the first create then
recreates it: the server creates a state whose name is the triage
default and whose group is the triage group, not the project's default
state. After creation the picker and detail resolve to that state.

Reference data for intake states lives in the shared picker module
(`shared/pickers`), not in intake; intake consumes it. Slice C carries
the edge rather than duplicating the read.

## Detail pane

### INT-019 — request detail

The content pane shows the selected request:

- an inline-editable title (single line, 255-character cap) and a
  rich-text description with autosave and a save-status indicator;
- the reactions strip;
- the attachments list for the underlying work item;
- a properties panel: intake state (read-only, shown as its state chip
  with group); assignees, priority, due date and labels (editable, each
  with a clear control, subject to INT-026). Titles and descriptions
  save on blur/debounce; a failed save surfaces an error and keeps the
  text.

Title, description and property writes all go through the intake update
`PATCH …/intake-issues/{issueId}/`, which carries work-item fields inside
an `issue` object (`name`, `description_html`, `priority`, `assignee_ids`,
`label_ids`, `target_date`, `state_id`) and intake fields at the top
level (`status`, `snoozed_till`, `duplicate_to`). The input echo comes
back as the detail payload, whose `issue` carries the updated work item
and whose top-level fields carry the intake state. The detail read is a
one-shot fetch (INT-031).

The attachments list belongs to the Issues attachments slice
(NEWFRONT-650, rows ISS-187–191), which is not merged yet. Intake
consumes that list component with the selected work item's id and its
workspace/project scope; it does not render or fetch attachments itself,
and slice D carries a blocked_by edge on NEWFRONT-650 for the list. If
an intake-only attachment surface is ever needed, it must be a new
component with its own inventory row — nothing in INT-019 requires one.

Reactions reuse `features/issues/reactions/IssueReactions` with the
work-item id and a disabled state for read-only viewers.

### INT-026 — edit permissions

| Actor                                                | Editable                                                  |
| ---------------------------------------------------- | --------------------------------------------------------- |
| Project admin                                        | title, description, assignees, priority, due date, labels |
| Creator (member)                                     | the same as an admin                                      |
| Member, not creator                                  | read-only                                                 |
| Guest, creator                                       | title and description only; no property editing           |
| Guest, not creator                                   | read-only                                                 |
| Any resolved request (accepted, declined, duplicate) | read-only for everyone                                    |

The old UI showed a guest-creator the full property UI while the server
kept only name and description; a priority write returned 200 but was
ignored. The new build matches the intended matrix in the UI: a
guest-creator sees the title and description editable, the property
panel read-only. The server remains authoritative: a member's write on
another's request is refused (403), and the client surfaces a
permission notice rather than silently reverting.

Description versions hide whenever the view is read-only (INT-029). The
add control for labels hides when the viewer may not edit labels.

### INT-024 — work-item link

An admin or member can copy the selected request's work-item link from
the detail's overflow menu while the request is open, and from a direct
control once it is resolved. Guests do not get copy on open requests.
The link targets `/{ws}/browse/{identifier}-{sequenceId}/`, and copying
shows a confirmation toast. On a resolved request, an "open work item"
action navigates to that address. Both reuse
`shared/shell/CopyWorkItemLinkButton` with the workspace slug and work
item key; intake adds no clipboard or link-building code.

### INT-029 — description versions

The description carries an "edited by" line that opens the stored
versions: a list of versions (author, age, preview), each viewable in
the editor's read-only viewer, with newer/older navigation and a restore
that writes that version's HTML back as the current description through
the description update.

Behavior rules from the API: every work item has at least one version
(a creation entry); consecutive edits by the same author within a short
window (ten minutes) fold into one version; separate authors or later
edits add versions. The versions list is a one-shot read; restoring
invalidates the detail and the versions list.

The contract already exists as
`packages/api-client/src/contracts/issue-versions.ts` on
`/api/workspaces/{ws}/projects/{pid}/work-items/{issueId}/description-versions/`.
The old intake alias under `intake-work-items/` also exists; the new
build uses the `work-items` path — it is the same resource and
NEWFRONT-688 must not add a second contract. Intake reuses
`DescriptionHistory` (props: workspace, project, issue id, creation
timestamp and creator for the unsaved line, synced and restore-disabled
flags, and an `onRestore` callback), and `IssueDescriptionField` for the
restore write.

## Navigation, URL and keyboard (INT-027, INT-028)

### INT-028 — URL as source of truth

The intake URL carries `?currentTab=open|closed` and
`?inboxIssueId=<work item id>`, validated by the route's search schema.
`currentTab` defaults to open; an unknown value falls back to open.
`inboxIssueId` defaults to none.

Resolution order on entry:

1. Settle the tab from `currentTab`, reading that tab's list.
2. If `inboxIssueId` is present and the request is in the settled
   tab's list (and readable), open it. A link to a closed request
   therefore settles the Closed tab before loading the detail.
3. If the id is stale, belongs to another tab's list, or is forbidden
   to this viewer (INT-032), drop it and fall back to the tab list.
   With a non-empty list this triggers auto-selection (INT-005); an
   empty list keeps the placeholder.
4. With no id, auto-select (INT-005).

Selecting a row, triage actions that advance, and create all keep the
URL in sync: `inboxIssueId` updates on every selection change, and
`currentTab` updates on tab change. Browser back/forward moves between
those recorded states. A full navigation to a deep link renders the
same state without an intermediate flicker of the wrong tab.

### INT-027 — previous/next navigation

The detail header carries previous and next chevrons; the ArrowUp and
ArrowDown keys do the same. Both walk the active list in its displayed
server order, wrapping from the last to the first and the first to the
last. Navigation preserves `currentTab` and updates `inboxIssueId`.

The arrows are suppressed while the user is typing in the title or
description, or while any dialog is open, so they never steal caret
movement. The chevrons stay enabled (they are explicit controls).

While a boundary read is in flight (next page not yet loaded) the
chevrons behave on the loaded rows; after a triage action the neighbour
is the next row in the current list, or the previous row when the
removed row was last.

## Triage actions (INT-020–023, 025)

Triage controls appear on an open (pending or snoozed) request in the
detail header and overflow menu, and in the narrow-viewport action
header (INT-030). They are absent for resolved requests. Each action's
permission is server-enforced; a member attempt is refused with a
permission notice that names the admin requirement, the dialog does not
open, and the local state is unchanged.

### INT-020 — accept

Accept opens the shared work-item add-to-project form (the Issues
create/edit form, NEWFRONT-631) with the request's work item, titled so
the user can see which request they are adding (its identifier and
sequence). The form's confirm action is the add-to-project action.
Cancelling closes the form and leaves the request pending, with no
change.

Confirming writes `PATCH /api/workspaces/{ws}/projects/{pid}/intake-issues/{issueId}/`
with `status=1`. The server moves the work item out of triage into the
project's default state, keeps the work item, and the request becomes
Accepted. The screen advances to the neighbouring request (INT-027) and
invalidates list, detail and the pending count. Accepted requests show
no triage controls (INT-026 read-only).

A member sees the accept control but the server refuses the change; the
UI shows a permission notice containing the admin requirement and never
opens the form.

### INT-021 — decline

Decline opens a confirmation dialog warning that declining cannot be
undone. Confirming writes `PATCH …/intake-issues/{issueId}/` with
`status=-1`; the request becomes Declined, the work item remains, and
the screen advances. Cancelling changes nothing. Members are refused
as in INT-020.

### INT-022 — snooze (bug NEWFRONT-272)

Snooze lives in the overflow menu for pending and snoozed requests. It
opens a date picker for a future day: days before today are not
selectable, and confirming writes
`PATCH …/intake-issues/{issueId}/` with `status=0` and `snoozed_till`
(the chosen day, sent as an ISO timestamp). The screen advances.

A snoozed request's status chip shows a countdown in whole days ("N
days to go"). Un-snoozing writes `status=-2` and `snoozed_till=null`;
the request returns to Pending and the screen advances.

Once the snooze day has passed the request reads as overdue/due: the
lapsed state uses the passed/danger styling and the chip stays visible
with the due wording rather than disappearing. The old app hid the chip
for a lapsed snooze (NEWFRONT-272); the oracle pins the hidden chip, so
the gate asserts the visible passed chip. Lapsed rows remain in the
Open tab's snoozed set.

### INT-023 — mark as duplicate

Mark as duplicate opens a searchable work-item picker. Typing searches
the project with
`GET /api/workspaces/{ws}/projects/{pid}/search-issues/?search=<text>&workspace_search=false`;
the current request's own work item is excluded from results, and an
empty search lists recent project work items. Picking one and confirming
writes `PATCH …/intake-issues/{issueId}/` with `status=2` and
`duplicate_to=<work item id>`.

The request becomes Duplicate and the detail shows a "duplicate of"
link carrying the target's identifier and sequence; following it opens
the target work item. Unlike the other actions, marking duplicate does
not advance — the detail stays on the now-resolved request so the user
can follow the link. The picker reuses the Issues project search
contract (`packages/api-client/src/contracts/issue-search.ts`); intake
does not add a search endpoint.

### INT-025 — delete

Delete opens a confirmation dialog warning that deletion cannot be
undone. Confirming writes
`DELETE /api/workspaces/{ws}/projects/{pid}/intake-issues/{issueId}/`.
For a request in any status other than Accepted, the server removes the
intake entry and cascades to the work item. For an Accepted request it
removes only the intake entry and keeps the work item.

After deletion the screen advances to the neighbour, or, if the list
becomes empty, returns to the list placeholder and clears
`inboxIssueId`. The list, detail and pending count are invalidated.

Delete is offered in the UI only for Pending and Snoozed requests. The
creator may delete their own request; an outsider's attempt is refused
(403) and the client shows the permission notice. Admins may delete
any request.

## Cross-cutting behavior

### INT-018 — duplicate detection (cloud seam)

Where the edition enables duplicate detection, composing a request
surfaces likely existing work items for the title/description after a
short debounce, through a control that opens a comparison of the draft
with the candidate; viewing a request offers the same comparison.
Accepting a candidate follows whatever the comparison offers (open the
candidate, or proceed with the draft) without changing intake status.

In the OSS and desktop builds this is entirely absent: no control, no
comparison, and no dedupe search traffic while composing or viewing.
Duplicate-detection UI reuses
`apps/web_new/src/features/issues/create/dedupe.ts` and the
`ISSUE_DEDUPE_FLAG` edition flag; intake does not implement its own
similarity search. Slice F owns the seam and carries the edge on
NEWFRONT-630.

### INT-030 — narrow viewport

Below the shared narrow-viewport breakpoint the screen becomes
single-pane: the detail fills the width with a mobile action header, and
the list is opened from a toggle. The list, when opened, covers the
content; picking a row selects it and closes the list. The mobile action
header carries a back/list toggle and the triage actions for an open
request — Snooze, Mark as duplicate, Accept, Decline and Delete — so
every action remains reachable without the desktop overflow menu.
Resolved requests show no triage actions but keep the list toggle and
any read-only navigation. The header's controls keep their labels
available to assistive technology (INT-030 accessibility notes below).

The breakpoint and the narrow-viewport hook come from
`shared/shell/narrowViewport.ts`; intake does not define its own media
query.

### INT-032 — guest visibility

A project guest sees only the requests they created unless the project
flag that lets guests view all features is on. Without it:

- the list read returns only the guest's own requests (the server scopes
  it by creator);
- retrieving another user's request is refused (403), so the detail
  shows the permission notice rather than content;
- a deep link to a forbidden request falls back to the tab list
  (INT-028);
- creating a request is allowed for a guest and the new request is
  theirs to see.

With the flag on, a guest sees every request exactly like a member.
Members and admins always see every request. The client does not filter
rows itself; it renders what the scoped list returns and handles the
refused detail.

### INT-034 — editions and desktop

OSS and desktop are identical for intake: no source indicator on rows
or detail, no duplicate-detection UI (INT-018), and no duplicate-search
traffic. Desktop adds no intake-specific behavior; it runs the same
routes and the same OSS edition. Cloud adds: a source indicator on rows
and the detail for forms and email requests (in-app requests are the
bare default), duplicate detection while composing and viewing, and
forms/email request ingestion on the server.

Edition seams the new build declares:

- **Scoped file:** intake lives under `apps/web_new/src/features/intake/`
  (not present yet — slice A creates it). Nothing under it may import
  from an old app.
- **Flags:** the existing `ISSUE_DEDUPE_FLAG` (`issue-dedupe`) governs
  duplicate detection for create and view (INT-018). For the source
  indicator, intake reads a named edition flag
  (`intake-source-pill`) through the edition's `flags` record. Both
  default to absent, so OSS renders nothing. No `SlotComponents` entry
  is added: a flag is enough for a small, self-contained indicator, and
  adding a slot is a design decision recorded on the Decisions page, not
  something this spec should force. If the cloud overlay later needs to
  swap in a heavier component, that is a new decision and the flag
  remains the fallback.
- **Server-side ingestion:** forms and email creation happen on the
  server; intake only reads the resulting rows and their source. No
  client seam is needed for ingestion.

The area gate can prove in this checkout: OSS/desktop show no source
indicator and no duplicate UI, no duplicate-search request is made, and
the OSS intake behavior matches every non-cloud row. The gate cannot
prove the private cloud overlay (source pills rendered, dedupe traffic,
ingestion) because the overlay is absent. Those stay unproven here and
must be recorded as such rather than asserted green.

## Intended vs observed

Where the old app deviates from the intended behavior, the new build
implements the intended column and the gate asserts it. The oracle
scenario keeps pinning the observation as evidence.

| Row                    | Oracle observation (pinned)                                                                                                                     | Intended behavior in web_new                                                                                                                                                                                             |
| ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| INT-006 (NEWFRONT-273) | Row priority marker present, but its tooltip never opens (fragment trigger); the scenario asserts the presented priority key.                   | Marker is a real tooltip trigger; hovering/focusing shows the priority key. Gate asserts the tooltip opens.                                                                                                              |
| INT-007 (NEWFRONT-269) | Placeholder rows appear while a page loads, but the header syncing hint never appears.                                                          | Syncing hint appears while a page is in flight and clears when it settles. Gate asserts it appears and clears.                                                                                                           |
| INT-008 (NEWFRONT-262) | Every empty tab shows the no-results state (the default status counts as an applied filter); guidance and informational states are unreachable. | Three distinct states: filtered-to-nothing → no-results; empty Open (no filters) → guidance plus create; empty Closed (no filters) → informational.                                                                      |
| INT-010 (NEWFRONT-274) | Explicit range sends `,` and never narrows; the list keeps every row.                                                                           | Sends `FROM;after,TO;before`; the list narrows to the range.                                                                                                                                                             |
| INT-010 (NEWFRONT-271) | Custom-range dialog title is the created-date title for both sections.                                                                          | Title follows the opened section (created vs updated).                                                                                                                                                                   |
| INT-010 (NEWFRONT-270) | Same-day fixture data masks the defect; the updated-date preset assertion passes coincidentally.                                                | Client sends `updated_at`; the updated-date predicate must use the updated column. Blocked on PDASHOSS01-312 (server); until Done the gate keeps INT-010's updated-date assertion red. The client still sends the range. |
| INT-016 (NEWFRONT-304) | Escape closes the create dialog while the editor is busy.                                                                                       | Escape is guarded exactly like close: wait notice while busy, close once settled.                                                                                                                                        |
| INT-022 (NEWFRONT-272) | A lapsed snooze hides the status chip.                                                                                                          | Lapsed snooze keeps the chip with due/passed styling.                                                                                                                                                                    |
| INT-026                | Guest-creator sees the full property UI; the server keeps only name and description (a priority write 200s but is ignored).                     | Guest-creator sees title/description editable and properties read-only; the server remains authoritative. Gate asserts the intended UI.                                                                                  |

## Reuse map (new code, not old)

All of the following exist on `web-new-dev` unless marked otherwise.
"Reuse as-is" means intake consumes the component with its current
props. "Extend" means a prop or export is added in the owning slice —
intake comments on that slice and opens a blocked_by edge; it never
forks.

**Shared, consumed by several slices**

| Module                                             | Status      | Intake use                                                                                                                                                              |
| -------------------------------------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `shared/pickers/IntakeStatePicker`                 | reuse as-is | INT-013/019/033 — create default and read-only detail state; lazy reference read via `shared/pickers/reference.ts` (five-minute reference staleness, no focus refetch). |
| `shared/pickers/PriorityPicker`                    | reuse as-is | INT-009/013/019 — priority filter and property.                                                                                                                         |
| `shared/pickers/MemberPicker`                      | reuse as-is | INT-009/013/019 — assignees, created-by, member filters.                                                                                                                |
| `shared/pickers/LabelPicker`                       | reuse as-is | INT-009/013/019 — labels.                                                                                                                                               |
| `features/issues/dropdowns/DatePicker`             | reuse as-is | INT-013 due date (single date); INT-010 custom range uses its range calendar export. If a range prop is missing, extend that module (NEWFRONT-616) and open an edge.    |
| `shared/editor` (`IssueDescriptionEditor`, handle) | reuse as-is | INT-013/019/029 — description composition, autosave editor and version viewer.                                                                                          |
| `features/issues/filters/shared/FilterSection`     | reuse as-is | INT-009 — section chrome (options, search, count, cap).                                                                                                                 |
| `shared/shell/CopyWorkItemLinkButton`              | reuse as-is | INT-024 — copies and toasts.                                                                                                                                            |
| `shared/shell/narrowViewport`                      | reuse as-is | INT-030 — breakpoint detection.                                                                                                                                         |
| `features/issues/reactions/IssueReactions`         | reuse as-is | INT-019 — reactions strip with disabled state.                                                                                                                          |

**Slice A (INT-001–005, 008, 012, 031)** — all new:
`features/intake/route` (search schema, redirect, gate), `IntakeShell`,
`IntakeTabs`, `IntakeEmptyStates`, and the intake query module. Reuses
the project feature flag read and the app shell breadcrumb; the route
stub is replaced. Depends on NEWFRONT-528 for the workspace-scoped
prefix and the retired-inbox redirect.

**Slice B (INT-006, 007, 009–011)** — new: `IntakeRow`,
`IntakeListPager`, `IntakeFilterPanel` (composing `FilterSection`),
`IntakeDateRangeFilter`, `IntakeOrderMenu`, `IntakeChips`. Reuses
`FilterSection` and the `DatePicker` range export. Edges: NEWFRONT-626,
627, 629 (filter inputs and chrome) and NEWFRONT-616 (date/range). The
row and chip visuals are new intake components.

**Slice C (INT-013–017, 033)** — new: `IntakeCreateDialog`,
`IntakeCreateForm` and the create mutation. Reuses
`WorkItemCreateDialog`/`useCreateWorkItem` (`features/issues/api/create.ts`),
`ModalDescription` (`onAssetUploaded`, `onReadinessChange`,
`assetScope`), `createApi` upload/attach hooks, the dedupe seam
(`dedupe.ts` + `ISSUE_DEDUPE_FLAG`) with NEWFRONT-630, and the shared
pickers. Edges: NEWFRONT-631 (assets/description), NEWFRONT-630
(dedupe). Intake does not write a second uploader or a create form.

**Slice D (INT-019, 024, 026, 027, 028, 029)** — new:
`IntakeDetail`, `IntakeDetailHeader`, `IntakeProperties`,
`IntakeAttachmentList` (thin wrapper only), `IntakeNavigation`, URL
wiring and the intake detail queries. Reuses `IssueTitleField`,
`IssueDescriptionField`, `DescriptionHistory`, `IssueDetailRoot`
(editable/sidebar/topSlot/activity props), `IssueReactions`,
`CopyWorkItemLinkButton`, `issue-versions.ts`. Edges: NEWFRONT-639
(detail fields), NEWFRONT-640/642 (properties surfaces), NEWFRONT-650
(attachments list, ISS-187–191 — **not merged**), NEWFRONT-654
(reactions). If `IssueDetailRoot`'s sidebar slot needs a new prop,
extend it in its owning slice and open an edge; do not fork.

**Slice E (INT-020–023, 025)** — new: `IntakeTriageMenu`,
`IntakeAcceptDialog`, `IntakeDeclineDialog`, `IntakeSnoozeDialog`,
`IntakeDuplicatePicker`, `IntakeDeleteDialog`, and the triage
mutations. Reuses the Issues add-to-project/edit form and
`issue-search.ts`. Edge on NEWFRONT-631 for the accept form.

**Slice F (INT-018, 030, 032, 034)** — new: `IntakeSourcePill` (flag
gated), dedupe seam wiring, mobile layout/action header, guest
scoping/fallback handling and edition flag registration. Reuses
`narrowViewport`, `dedupe.ts`, `ISSUE_DEDUPE_FLAG`. Edges:
NEWFRONT-630 (dedupe), NEWFRONT-689/692/693 (the surfaces it adapts).

## Data layer

**Query keys.** All intake keys are nested under `workspaceScope`
(`["workspace", slug]`) so workspace switching clears them:

- `intakeKeys.list(projectId, { tab, filters, order })` — the infinite
  list (page param = cursor), including the tab's default status in the
  key. The key excludes `inboxIssueId`, so selecting a row does not
  refetch the list.
- `intakeKeys.pendingCount(projectId)` — pending total for the Open
  badge.
- `intakeKeys.detail(projectId, issueId)` — one-shot detail read.
- `intakeKeys.versions(projectId, issueId)` — description versions,
  shared through `issueKeys.descriptionVersions`.
- `intakeKeys.search(projectId, term)` — duplicate-picker search.
- reference data (`intakeState`, members, labels, states) uses
  `pickerKeys` from `shared/pickers/reference.ts`.

**Query options.** Intake list, detail and pending-count reads set
`refetchOnWindowFocus: false` (overriding the app default), and stale
time per the app's list default (30s); reference data keeps the five
minute staleness with no focus refetch. The list is an infinite query
with `getNextPageParam` from `next_cursor` and `initialPageParam`
`undefined`; changing tab, filters or order changes the key, which
resets paging.

**Mutations and optimistic rules.**

| Action                           | Write                                                         | Optimistic                                                  | On success                                                                                       | On error                                     |
| -------------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | -------------------------------------------- |
| Create (INT-013)                 | `POST intake-issues/` + bulk attach                           | No (needs the server id)                                    | Invalidate list + pending count; navigate to new id; attach assets with the created work-item id | Keep dialog and input; show the server error |
| Accept (INT-020)                 | `PATCH status=1`                                              | No                                                          | Invalidate list, pending count, detail; advance to neighbour                                     | Show permission/error notice; keep state     |
| Decline (INT-021)                | `PATCH status=-1`                                             | No                                                          | Same as accept                                                                                   | Same                                         |
| Snooze (INT-022)                 | `PATCH status=0, snoozed_till`                                | No                                                          | Same; the row gains the countdown chip                                                           | Same                                         |
| Un-snooze                        | `PATCH status=-2, snoozed_till=null`                          | No                                                          | Same                                                                                             | Same                                         |
| Mark duplicate (INT-023)         | `PATCH status=2, duplicate_to`                                | No                                                          | Invalidate list, pending count, detail; stay on the request                                      | Same                                         |
| Delete (INT-025)                 | `DELETE intake-issues/{id}/`                                  | No                                                          | Invalidate list, pending count, detail; advance or clear selection                               | Same                                         |
| Title/description edit (INT-019) | Work-item update                                              | Yes, local draft with save indicator (shared save pipeline) | Mark saved                                                                                       | Keep text, show failure                      |
| Property edit (INT-019/026)      | Intake update `PATCH …/intake-issues/{id}/` with `issue{...}` | Yes, the picker shows the new value                         | Keep value; invalidate detail                                                                    | Revert to server value, show notice          |
| Restore version (INT-029)        | Description update                                            | No                                                          | Invalidate detail + versions; replace editor content                                             | Show error, keep current                     |

Optimistic property edits are limited to fields the viewer may edit
(INT-026); the UI never optimistically applies a write the server will
ignore, because the guest-creator property panel is read-only.

**Pending count.** The Open badge reads `pendingCount(projectId)`, which
is the pending total from the list read's `total_results` with the tab's
default status (`-2`) and no user filters. Every triage mutation and
create invalidates it, and tab switches re-read the Open list, so the
badge reflects accepted/declined/duplicate/created requests without a
socket. It renders only while Open is active (INT-004).

**URL as truth.** `currentTab` and `inboxIssueId` are route search
params validated by the route schema (INT-028). Components derive
selection from the URL, not from local selection state; selecting writes
the URL. Mutations that advance compute the neighbour from the cached
list and write the URL. Local state exists only for transient things
(open panels, dialogs, drafts).

**No push.** No subscriptions, intervals or focus refetch (INT-031).
The only refreshes are the invalidations above and the user's
tab/filter/order/paging actions.

## Cross-area seams

| Seam                                                                   | Owner                                                                                          | Intake consumes                                                                                                                                                           |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Project feature toggle screen `/{ws}/settings/projects/{pid}/features` | Project settings (feature toggle slice, placeholder owner NEWFRONT-582)                        | INT-002 reads the project's `inbox_view` and links to the toggle; it does not build the toggle.                                                                           |
| Intake-state reference read (`GET …/intake-state/`)                    | `shared/pickers` (`IntakeStatePicker`, `reference.ts`)                                         | INT-033 and INT-013/019 use the picker; intake never reads or creates states itself.                                                                                      |
| Work-item add-to-project form (accept)                                 | Issues create/edit (NEWFRONT-631)                                                              | INT-020 opens it with the request's work item and confirm action; intake only issues the status write after confirm.                                                      |
| Duplicate target picker search                                         | Issues search (`issue-search.ts`, project issue search)                                        | INT-023 calls it with `search` and `workspace_search=false`; intake adds no search surface.                                                                               |
| Description create assets (INT-017)                                    | Issues create (NEWFRONT-631): `ModalDescription` + `createApi` + `issue-description-assets.ts` | Intake passes the draft asset scope, collects parked ids via `onAssetUploaded`, reads readiness via `onReadinessChange`, and attaches after create with the work-item id. |
| Detail attachments list (INT-019)                                      | Issues attachments (NEWFRONT-650, ISS-187–191)                                                 | Intake renders that component with the work-item id and project scope; it does not fetch or upload attachments in the detail.                                             |
| Reactions (INT-019)                                                    | Issues reactions (NEWFRONT-654)                                                                | Intake renders the strip with a disabled flag for read-only viewers.                                                                                                      |
| Description versions (INT-029)                                         | Issues detail (NEWFRONT-639) + the versions contract                                           | Intake renders `DescriptionHistory`; it does not build a versions API.                                                                                                    |
| Copy work-item link / browse address (INT-024)                         | `shared/shell`                                                                                 | Intake uses the button and the browse address.                                                                                                                            |
| Narrow viewport                                                        | `shared/shell/narrowViewport`                                                                  | Intake adapts layout; it does not add a media query.                                                                                                                      |

Attachments have no single owner today; the modules named above are the
owners intake consumes. Slice C's edge is NEWFRONT-631 (merged); slice
D's is NEWFRONT-650 (not merged). Neither slice writes a second uploader
or a second list.

## Accessibility and keyboard

- **Tabs (INT-004):** `role="tablist"` with `role="tab"` children,
  roving focus, `aria-selected`, and `aria-controls` naming the list
  panel; Left/Right move between tabs, Enter/Space activate. The Open
  tab's accessible name includes the pending count.
- **Row selection (INT-005/027):** the list is a set of selection
  controls; the selected row carries `aria-current` (or
  `aria-selected` in a listbox) and is visibly marked. Rows are
  reachable with Tab and activated with Enter/Space; the row's
  accessible name includes its identifier, title and status.
- **Arrow navigation (INT-027):** ArrowUp/ArrowDown move to
  previous/next request with wrap. They are ignored while the focus is
  in a text field, a contenteditable (the description editor), a
  combobox or any open dialog, and while a save is in flight. The
  header chevrons are ordinary buttons with previous/next labels and are
  always available.
- **Focus after actions (INT-020–025):** accept, decline, snooze and
  delete move focus to the neighbour's detail heading (or the list's
  first row when the list empties); marking duplicate leaves focus on
  the resolved detail. Create moves focus to the new request's heading.
  A toast/live region announces the outcome, so a screen-reader user
  hears the change without hunting.
- **Dialogs (INT-013–016, 020–025):** open dialogs trap focus, mark
  the dialog with `role="dialog"` and an accessible name, and return
  focus to the control that opened them on close. The create dialog's
  closed Escape is guarded while the editor is busy (INT-016) with a
  wait notice, and the guard does not hide the dialog's close control
  from assistive technology — it stays focusable and announces why it
  did not close.
- **Filter panel (INT-009):** sections are labelled groups; each
  option's state is exposed (checked/selected); applied chips are
  buttons with a clear label naming the section and value; the applied
  count is announced when it changes.
- **Mobile action header (INT-030):** the list toggle exposes
  `aria-expanded` and opens the list pane as a drawer that traps focus
  while open and returns focus on close; the triage actions are all in
  the header's accessible menu with visible labels, and the header's
  back/list control has a name, not only an icon.
- **Tooltips (INT-006):** the priority and created-date tooltips open on
  hover and on keyboard focus, and their content is also available in
  the row's accessible description.

## API reference used by this area

Paths and fields only; no response bodies are copied.

| Purpose          | Call                                                                                                                                                                                                                    |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| List             | `GET /api/workspaces/{ws}/projects/{pid}/intake-issues/` — `status`, `priority`, `assignees`, `created_by`, `labels`, `created_at`, `updated_at`, `order_by`, `cursor`, `per_page`                                      |
| Detail           | `GET /api/workspaces/{ws}/projects/{pid}/intake-issues/{issueId}/` (pk is the work-item id)                                                                                                                             |
| Create           | `POST …/intake-issues/` — `source`, `issue{name, description_html, priority, state_id, label_ids, assignee_ids, target_date}`                                                                                           |
| Triage/edit      | `PATCH …/intake-issues/{issueId}/` — intake fields `status`, `snoozed_till`, `duplicate_to`; work-item fields nested under `issue` (`name`, `description_html`, `priority`, `assignee_ids`, `label_ids`, `target_date`) |
| Delete           | `DELETE …/intake-issues/{issueId}/`                                                                                                                                                                                     |
| Triage state     | `GET …/intake-state/`                                                                                                                                                                                                   |
| Duplicate search | `GET …/search-issues/` — `search`, `workspace_search`                                                                                                                                                                   |
| Versions         | `GET …/work-items/{issueId}/description-versions/` (+ `/{versionId}/`)                                                                                                                                                  |
| Version restore  | work-item description update (`description_html`)                                                                                                                                                                       |
| Create assets    | `POST /api/assets/v2/workspaces/{ws}/projects/{pid}/{issueId}/bulk/` with `asset_ids`                                                                                                                                   |

Contract coverage: NEWFRONT-688 adds zod contracts for the intake-issues
list/detail/create/update/delete, the triage-state read and the bulk
asset confirm if any are still uncovered. The list envelope is
`results` plus `total_results`, `next_cursor` and `next_page_results`;
create validation errors are `{error}` with 400 (missing name, invalid
priority); permission failures are 400/403 according to the server's
rules. The `work-items` description-versions contract already exists
and must be reused, not duplicated.

## Row coverage

| Row     | Section                   | Slice |
| ------- | ------------------------- | ----- |
| INT-001 | Shell, tabs, empty states | A     |
| INT-002 | Shell, tabs, empty states | A     |
| INT-003 | Shell, tabs, empty states | A     |
| INT-004 | Shell, tabs, empty states | A     |
| INT-005 | Shell, tabs, empty states | A     |
| INT-006 | List pane                 | B     |
| INT-007 | List pane                 | B     |
| INT-008 | Shell, tabs, empty states | A     |
| INT-009 | Filtering and ordering    | B     |
| INT-010 | Filtering and ordering    | B     |
| INT-011 | Filtering and ordering    | B     |
| INT-012 | Shell, tabs, empty states | A     |
| INT-013 | Create                    | C     |
| INT-014 | Create validation         | C     |
| INT-015 | Create more               | C     |
| INT-016 | Dismiss guard             | C     |
| INT-017 | Create attachments        | C     |
| INT-018 | Cross-cutting             | F     |
| INT-019 | Detail pane               | D     |
| INT-020 | Triage actions            | E     |
| INT-021 | Triage actions            | E     |
| INT-022 | Triage actions            | E     |
| INT-023 | Triage actions            | E     |
| INT-024 | Detail pane               | D     |
| INT-025 | Triage actions            | E     |
| INT-026 | Detail permissions        | D     |
| INT-027 | Navigation and keyboard   | D     |
| INT-028 | URL                       | D     |
| INT-029 | Description versions      | D     |
| INT-030 | Cross-cutting             | F     |
| INT-031 | Shell / no push           | A     |
| INT-032 | Cross-cutting             | F     |
| INT-033 | Create triage state       | C     |
| INT-034 | Cross-cutting / editions  | F     |

## Known gaps and what the gate can prove

- The private cloud overlay is absent: cloud source pills, cloud dedupe
  and forms/email ingestion cannot be exercised. The gate proves the
  OSS/desktop absence (INT-018, INT-034) and leaves cloud behavior
  unproven.
- The updated-date filter depends on PDASHOSS01-312; INT-010's
  updated-date assertion stays red until that issue is Done and
  NEWFRONT-695 is unblocked for it.
- The detail attachments list depends on NEWFRONT-650, which has no
  branch yet; INT-019's attachments assertion cannot be driven until it
  merges. The rest of INT-019 does not depend on it.
- The retired inbox redirect and the `/{ws}/...` paths depend on
  NEWFRONT-528 (workspace-scoped migrated routes).
