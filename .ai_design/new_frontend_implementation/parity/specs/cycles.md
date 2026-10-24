# Cycles and active cycles — area spec (NEWFRONT-705, epic NEWFRONT-81)

Build spec for inventory rows CYC-001–CYC-048 (`parity/inventory/cycles.md`).
This is the file the eleven cycles slices build from: every section cites
the rows it covers, and every row in the inventory appears at least once.
It is prose only. API paths, query parameters, field names, storage keys
and route segments are named exactly because the contracts slice
(NEWFRONT-706) and the slices depend on them; no screen text, class name,
icon, asset or structure is taken from the old frontend.

Where the oracle pins an old defect the scenario title starts with
`bug:` and names a follow-up issue. Those rows are built to the intended
behavior stated here, not to the pinned defect; the dispositions are
collected in §26 so no slice can miss one.

Endpoint shorthand:

- `WS` is `/api/workspaces/{ws}`.
- `BASE` is `/api/workspaces/{ws}/projects/{pid}`.
- `{cid}` is a cycle id, `{wid}` a work-item id.
- All paths are relative to the app origin and carry the usual session
  credentials.

Old sources consulted (reference only, never a source): the project
cycles and archived-cycles routes and the workspace active-cycles route;
the cycles components tree (list, rows, form, delete and transfer
dialogs, peek overview, quick actions, archived list, filters and
applied-filter chips, active-cycle hero); the analytics sidebar for
cycles; the shared progress-ring, progress-stats and single-stat
renderers under the shared sidebar folder; the command palette's cycle
commands; and the cloud-edition cycle and active-cycles folders. Live
behavior pinned by the oracle suite under
`apps/web_new/e2e/parity/cycles/` is cited per section as `Oracle:`.

## 1. Routes, shell integration, and where the area lives

The area owns four addresses (CYC-001, CYC-003, CYC-004, CYC-027,
CYC-032, CYC-034, CYC-043):

| Address                                | Screen                                                          |
| -------------------------------------- | --------------------------------------------------------------- |
| `/{ws}/projects/{pid}/cycles`          | project cycles list, with the active-cycle hero                 |
| `/{ws}/projects/{pid}/cycles/{cid}`    | cycle detail: work items in switchable layouts plus the sidebar |
| `/{ws}/projects/{pid}/archives/cycles` | archived cycles list                                            |
| `/{ws}/active-cycles`                  | workspace active-cycles upsell (the paid data view is a seam)   |

The `cycles` tab is a project tab (`SHELL-075`): it sits second in the
project tab strip, requires the `cycle_view` project feature, and is
hidden from guests in the tab strip (`memberPlus` in
`src/features/projects/tabs.ts`). The tab's page is reached from the
strip and from the project breadcrumb; deep links must work without a
tab click first.

Cycle detail is addressed by cycle id alone. A cycle id that does not
belong to the project in the address, or was deleted, is a gone-away
state (§5). The list address carries no required query keys. The peek
key `peekCycle` is described in §2; unknown query keys are ignored.
The archived list keeps its own address and is also reached from the
archives surface (§9).

**Current state in `apps/web_new`.** The shell tab is mounted at
`src/routes/$ws/_gated/projects/$projectId/cycles/index.tsx`, which today
renders only the placeholder `TabAreaMount area="Cycles"` (SHELL-075).
`src/routes/$ws/_gated/projects/$projectId/cycles/$cycleId.tsx` already
hosts the shared list layout for the `{ scope: "cycle", projectId,
cycleId }` entity; it renders the list only and owns no cycle chrome.
There is no `archives/cycles` route yet: the archives shell today is
`src/routes/$ws/_gated/projects/archives/index.tsx` (archived projects,
NEWFRONT-698). There is no workspace active-cycles route; slice H adds it for the
upsell of §16 (not a data view).
There is no cycles feature directory yet. The slices replace both cycle
route mounts with the real area, add the archived-cycles route under the
archives shell, and add the workspace route; nothing else in the shell
moves.

`@pidash/api-client` today carries only a thin cycle contract: the
picker row (`listCycles` → `CycleListItem`: id, project, name,
favorite, status), `updateCycle` and `transferCycleIssues`, used by the
palette and the issues surfaces. It does not yet model the full cycle
row, the date check, archive/restore, favorites, progress or workspace
actives; NEWFRONT-706 adds those.

Oracle: `list-search-filters.spec.ts` (rows open a shareable detail
address), `detail-sidebar.spec.ts` (detail host).

## 2. Project cycles list: hero, groups, rows, peek

The list is ordered, not paginated (CYC-001, CYC-002, CYC-003, CYC-004):

1. The active-cycle hero (§15) whenever a cycle covers today.
2. An **Upcoming** group holding cycles whose status is upcoming or
   draft, expanded by default.
3. A **Completed** group holding finished cycles, collapsed by default.

Current cycles appear through the hero rather than repeated in a group;
if no cycle covers today the hero is a no-active-cycle view (§15) and
the upcoming and completed groups still render. Each group header shows
its name, its count, and a disclosure control; collapsing hides its
rows and keeps the count visible. An empty group still renders its
header with a zero count. Group membership is decided by the cycle's
status, so a draft row is an upcoming row everywhere (CYC-001).

The group headings are, in order, **"Active cycle"**, **"Upcoming
cycle"** and **"Completed cycle"** (singular, as the oracle matches each
heading by its prefix). Each heading is a disclosure button that carries
`aria-expanded` matching the group's state. Upcoming and Completed end
with their count number inside the button (the CYC-001 driver reads the
trailing integer from the heading text); Active carries no count. Do not
pluralise the headings, and do not move the counts outside the buttons.

**Row anatomy** (CYC-002). Every row shows, left to right: name; a
circular completion ring; the lifecycle marker; the date span; and the
contextual extras for its state:

- The ring shows completed work items over all work items excluding
  cancelled ones: `completed / (total - cancelled)`, rounded. When that
  denominator is zero (no work items, or every item cancelled) the ring
  reads `0%` — an empty ring with the `0%` label — matching the old
  behavior; it is never drawn as full or as a check. Completed cycles
  read `completed / total` from the frozen `progress_snapshot` so the
  ring cannot drift after transfer, and the ring shows `100%`/a check
  only when the percentage is exactly 100.
- The date span is rendered with the project timezone rules of §17.
  An undated cycle (a draft with neither date) renders the ring, marker
  and counts but **no date span and no date placeholder**: the row has
  no date element at all, which is what the CYC-002 oracle reads
  (`dateText === null` for the undated draft). A one-sided range (one
  date set) still renders the single date. Do not render a dash or a
  "No dates" string in the row; the empty state for dates belongs to the
  detail sidebar and hero cards (§15), not the row.
- Draft and upcoming rows show a work-item count; current and completed
  rows do not. The current row, shown in the hero, shows the creator's
  avatar and no separate assignee cluster. Every other row (upcoming,
  draft, completed) shows the creator's avatar **and** the assignees'
  avatars; with no assignees it shows the members icon instead of an
  empty cluster. Avatars have a stable fallback (initial) for members
  without images.
- A favorite star renders on non-archived rows for members and admins
  only (§11). Guests and the read-only role get no star at all, not a
  disabled one (CYC-047); the server refuses favorite writes from them.
- The row menu trigger is described in §10.

**Opening a row** (CYC-003). Clicking the name or the row body navigates
to `/{ws}/projects/{pid}/cycles/{cid}`. The address is canonical,
loadable and shareable: pasting it in a fresh browser renders the same
cycle, its work items and its sidebar. The row itself is not a link
element wrapping its menu; the row menu's entries never navigate.

**Quick look** (CYC-004). A quick-look control on the row (an eye
action revealed from the row's hover area, inline on touch) opens a side
panel over the list without leaving it. The open panel is
reflected in the address as `?peekCycle={cid}`; closing removes the key
and leaves the plain list address. Reloading with the key open reopens
the panel for that cycle; sharing the address opens the same panel. The
panel shows the cycle's summary — name, state marker, dates, progress
ring and work-item counts — and links to the full detail. The panel
opens on top of the current list state; search, filters and the peek
key coexist in the address.

The list reads cycles from `GET BASE/cycles/` with **no `cycle_view`
query parameter** (the old list call sends none, and the backend treats
an absent value as `all`), so the response holds every live cycle in
the project; the payload is the cycle row shape of §23. Do not send the
project feature flag or the remembered tab as `cycle_view`: the
parameter is a separate server-side filter whose only values with
meaning here are `current` (the hero's current-cycle read, §15) and the
other status filters the backend also accepts. The project feature flag
(`cycle_view`, a boolean on the project payload) decides whether the tab
and area render at all (§5). The hero additionally reads progress and
analytics for the current cycle (§15). Rows use the row's own counters
and `progress_snapshot`; the list does not make one request per row.

Oracle: `list-search-filters.spec.ts` (hero first with counted groups,
collapsing hides rows; ring, date span, work-item counts, creator
avatar, favorite marker; row opens shareable detail; peek sets and
clears the key, reload and shared link reopen it).

## 3. Search

The list header carries a search box (CYC-005). It starts collapsed as
an icon-only control and expands to a text field on activation; the
field does **not** take focus on the live list (the reference mounts the
input only when the box opens, so its focus call lands before the input
exists; the oracle asserts expansion, and typing focuses through the
fill itself). Build the collapsed/expanded swap as is; do not add a
deferred focus call to "fix" it, because the archived box relies on the
opposite behavior in §9. The query filters rows as the user types,
without a submit step. Matching is on the cycle name only,
case-insensitive substring, evaluated client-side over the already
loaded list; the search never issues a request. Matching rows keep
their group; groups whose rows all stop matching render empty and the
list falls to the search no-match view of §5.

Escape in the field first clears the query and only a second Escape (or
an outside click) collapses the box. An outside click collapses the box
only when the query is empty; with text in it the box stays open so the
filtered state is not lost by accident. The query survives navigation
away and back within the project cycles area and is restored into an
open, unfocused box. The control that collapses the box is a clear
control that empties the query as well; the oracle treats the clear
control as collapsing the box. The archived list (§9) starts the same way — the
magnifier renders and the box is collapsed on a fresh visit — but it
opens **focused**: its text input stays mounted inside the collapsed
box, so the activation can move focus into it, whereas the live list
swaps the input in only when it opens and the focus call lands before
the element exists. That focus behavior, not an always-open box, is the
CYC-005/CYC-027 difference; see §9.

The expanded field's placeholder reads exactly `Search`. The
`apps/web_new` parity driver (whose cycle methods the slices implement,
§28) needs a stable handle for the magnifier and for the expanded box;
the old driver's class/test-id selectors are its own, not a product
requirement, so choose handles deliberately and keep them unchanged
once the oracle passes.

Oracle: `list-search-filters.spec.ts` (search expands, filters live,
Escape clears then collapses, query survives navigation and returns
unfocused).

## 4. Filters and applied chips

The list can be narrowed by two dimensions (CYC-006, CYC-007):

- **Status of the cycle**: any combination of the four statuses,
  selected from a multi-select section headed **"Status of the cycle"**
  (with an applied-count suffix once values are selected). Its options
  are, in order, **"In progress"**, **"Yet to start"**, **"Completed"**
  and **"Draft"** — the `current`, `upcoming`, `completed` and `draft`
  values. The oracle picks by these exact labels, so do not rename them.
  Values selected together combine as OR within the dimension. The menu
  is three sections in this order — status, start date, then due date
  (the archived list omits the status section) — under a single search
  field whose placeholder is `Search` and which narrows options by
  label and shows `No matches found` when nothing matches. Each section
  header carries an applied-count suffix and a preview toggle. The menu
  stays open while options are toggled. "In progress" is the current status, which the backend
  sets for a cycle covering today.
- **Start date** and **Due date** windows: the filter menu renders two
  sections, headed "Start date" and "Due date"; "Due date" means the
  cycle's end date, and the oracle picks its options by that label, so
  the section must keep the "Due date" heading and must not be renamed
  "End date". Both windows offer the same options — `1 week from now`,
  `2 weeks from now`, `1 month from now`, `2 months from now` — plus
  **"Custom"**, which opens an exact-date picker. The "Due date" section
  does not offer past-tense options such as "1 week ago"; that is the
  shared issues filter list, not this one. Each window is multi-select,
  and every selected value in a window is ANDed with the others and
  with the other window. A cycle is kept when its start date falls _inside_ the selected start
  window and its end date falls _inside_ the selected "Due date" window
  (containment of each cycle date in its own window, not range overlap).
  The two windows combine with AND. A missing or undated cycle never
  matches a window, so drafts drop out of every dated filter.

Filters combine (a cycle must match both dimensions) and combine with
the search term. All filtering runs on the loaded list; no filter
change issues a request. In the archived list the lifecycle dimension is
absent (archived rows only exist for finished cycles); the date windows
remain (CYC-027).

Active filters render as a row of removable chips above the list, one
chip per applied lifecycle value and one per date window (CYC-008).
Clicking a chip removes exactly that value; removing the last value in a
dimension removes the dimension. The row also carries a clear-all action
whose accessible name contains `clear all` (case-insensitive, as the
oracle matches it). Clear-all clears every filter
and chip, and leaves the search text alone — search has its own
clear control in the box — so a list filtered to nothing by both
dimensions needs both actions to return to the full list. Chips and
clear-all are available to guests (CYC-047).

Oracle: `list-search-filters.spec.ts` (status multi-select keeps only
matching lifecycles; start/end windows keep only cycles whose own date
falls inside the window;
chips remove per dimension, clear-all resets filters while search
persists).

## 5. Loading, empty, no-match, feature-off, and gone-away states

**Loading** (CYC-012). Until the first cycle list response arrives the
page shows a skeleton of the header, the hero slot and a few row
shapes. It is not a spinner-only page and not an empty state: once data
arrives it yields to rows, to the zero state, or to the hero plus
groups. A refresh after mutations uses the freshness rules of §18 and
does not flash the skeleton.

**First run** (CYC-011). With `cycle_view` on and zero cycles, the list
renders an explained empty view: what a cycle is (a dated window that
groups work items), and a creation shortcut. The view is visible to
every role; the shortcut is enabled only for members and admins and is
hidden or disabled for guests and viewers, without error. Activating
the shortcut opens the create dialog of §6 over the empty view. The
shortcut's label is **"Set your first cycle"** (the oracle selects the
button by that name and reads its disabled state on a guest), so the
new empty view keeps that string as the action label even if its
illustration and explanatory copy are redesigned.

**Feature off** (CYC-010). When the project has `cycle_view` disabled,
the page explains that cycles are turned off for this project instead of
showing rows, plus a management shortcut to the project's feature
settings at `/{ws}/settings/projects/{pid}/features`. The explanation
is visible to guests; the shortcut is enabled only for admins and
renders disabled for everyone else. The feature-off view takes
precedence over the zero state. The shortcut's label is **"Manage
features"**; it stays rendered (disabled, not removed) for non-admins
because the oracle asserts both its presence and its disabled state.

**No match** (CYC-009). When the project has cycles but the current
search and filters hide all of them, the no-match view differs by
cause:

- Search only (or search and filters): tells the user that no cycle
  matches their search and offers to clear the search.
- Filters only: tells the user that no cycle matches the filters and
  offers clear-all.

The action must actually restore the list: clearing search must not
silently clear filters, and clear-all must not clear the search text
(§4). No-match is never conflated with the zero state, and a no-match in
the archived list uses the same two-way split (CYC-027). The no-match
title is **"No matching cycles"** in both lists (an `h5`, as the driver
reads it). The hint under it is **"Remove the filters to see all
cycles"** for the filters-only case and **"Remove the search criteria to
see all cycles"** for search (alone or combined); the oracle tells the
two apart by the phrase "Remove the search criteria". Because the driver
keys on those strings, keep them as the rendered hint text; the
illustration and layout are free.

**Gone away** (CYC-013). A detail address for a missing, deleted, or
foreign cycle renders a dedicated not-found view: a short explanation
that the cycle does not exist, and a working route back to the project's
cycles list. It must not be a generic 404, must not render the sidebar
or layout chrome, and must not auto-redirect. The same view covers an
archived cycle accessed through a live detail address.
The back action's label is **"View other cycles"** (the oracle finds and
clicks it by name and requires the URL to end at `/cycles`); the
explanation reads like "Cycle does not exist". After the click the
address is the bare list path, with no cycle segment and no query.

**Archived zero state.** The archived list with no archived cycles
shows a heading **"No archived cycles yet"** and a short line inviting
the user to archive completed cycles; it never offers creation. The
heading is matched by name, so keep that string.

Oracle: `empty-states-create.spec.ts` (skeleton mid-fetch; zero state
with member shortcut and guest-disabled shortcut; feature-off view with
the admin shortcut routed to feature settings and the guest shortcut
disabled; missing/deleted id gone-away with a working back-to-list;
filters-hide-all and search-hide-all hints differ).

## 6. Creating a cycle

The create dialog is opened from the list header's **"Add cycle"**
button (the oracle clicks the header action by that name), from the zero-state
shortcut, and from the command palette's "New cycle" command (CYC-030).
The palette entry is shown and enabled only while a project is in
context, that project has the cycles feature on, and the user is a
member or admin; it is absent — not merely disabled — otherwise. The
work-item create modal is not a create-cycle entry point: it only
pre-fills a work item's cycle, and the cycles area adds nothing to it.
The dialog asks for (CYC-014, CYC-015):

- a title, required, trimmed, at most 255 characters, validated inline
  as the user leaves the field and on submit; an empty or overlong title
  blocks submission and shows the message next to the field rather than
  as a toast;
- a description, optional, plain text and long enough for the project's
  normal description needs;
- a start date and an end date, both optional. On **create** the range
  is both-or-neither: the create endpoint refuses a single date (400),
  so the dialog refuses a one-sided range inline, before any request,
  with the input intact. A single date alone is accepted only on
  **edit** (§7), where PATCH allows it and stored one-sided cycles must
  stay editable. The overlap check never runs unless both dates are
  present. The past is not selectable for the start date (today is
  allowed); the end date cannot precede the start date. The dialog
  heading is **"Create cycle"** (edit uses
  **"Update cycle"**), and the footer's confirm control is the button
  **"Create cycle"** on create / **"Update cycle"** on edit, next to a
  **"Cancel"** button; the oracle waits for those headings and button
  names.
- a project picker on **every create** (CYC-014), whether the dialog
  was opened from a project page (list header, zero-state shortcut) or
  from a workspace-level creation surface. It defaults to the current
  project (the one in the address) and lists only projects where the
  user may create cycles; the selected project receives the cycle and
  the dialog re-evaluates that project's feature flag and overlap
  universe. The picker is the dialog's first dropdown, ahead of the
  date controls, and is not hidden on a project page: the CYC-014
  oracle opens creation from project A's cycles list, picks project B
  and requires the cycle to land in B only. The picker is absent on
  **edit** (§7) — a cycle cannot move project. In the create dialog the
  picker is the form's first focusable control in the DOM, ahead of the
  title (see the keyboard rule below); that is a markup order, not a
  visible-order claim.

Submission is `POST BASE/cycles/` for the target project with
`{ name, description, start_date, end_date }` (dates as ISO instants or
null). While the request is in flight the submit control is busy and
cannot be double-submitted. Success closes the dialog, shows a
confirmation toast (`Cycle created successfully.` under a success
heading), and refreshes the list without a reload: a cycle covering
today appears in the hero immediately, others
in their group (CYC-019). The remembered list tab is reset to the
all-cycles tab so the new row is visible (CYC-018): the stored value at
`localStorage["cycle_tab"]` becomes the JSON-encoded string `"all"`. A server validation
error is shown in the dialog, not as a silent close.

**Copy and fields.** The name field's placeholder is **"Title"**, the
description's is **"Description"**, and the range control's two
placeholders are **"Start date"** and **"End date"** (the oracle locates
the range by a button whose text contains "Start date", fills the name
via `input[name="name"]` and the description via
`textarea[name="description"]`, and opens the range by clicking its
first span). Keep those field names and placeholders.

**Overlap** (CYC-016). Creation always asks the backend whether the
range collides with another cycle in the same project, via
`POST BASE/cycles/date-check/` with `{ start_date, end_date }` (no
`cycle_id`). The answer carries a `status`. A collision refuses the
save with an explanation that names the draft workaround: a conflicting
draft can be left undated, or its dates removed, to free the window. The
refusal is a toast titled **"Error!"** whose message contains "you
already have a cycle" (the oracle asserts both the title and that
phrase) and the same draft guidance, and the dialog stays open. A free
range saves. An undated (draft) cycle is always allowed and skips the
check. The check is asked once per submission, not per keystroke.

**Keyboard** (CYC-020). The dialog traps focus while open. Focus starts
in the title field. Forward Tab from there visits the other controls in
**DOM order**: description, the date range, cancel, submit, and then the
project picker (which sits before the title in the markup), wrapping
back to the title. Read as a trail starting at the autofocused title,
the stops are title, description, date range, cancel, submit, picker,
title. Shift+Tab reverses that cycle. The picker is reachable and focus never leaves the
dialog (§26, NEWFRONT-263): the picker is not second, and a design that
inserts it there breaks the oracle trail that begins with the
autofocused title.

**Permissions** (CYC-021). Header creation, the zero-state shortcut, and
the palette's create entry are available to members and admins only;
guests and viewers do not see a working creation affordance and any
direct call is refused by the server. The dialog is never opened for an
unauthorized role.

Oracle: `empty-states-create.spec.ts` (header create saves
title/description/dates; project picker retargets; empty and overlong
titles refused inline; past start disabled; overlap refused naming the
draft workaround; free and undated save). `edit-delete.spec.ts` (tab
order, creation permissions).

## 7. Editing a cycle

Editing reuses the create form with the cycle's current values, opened
from the row menu (§10) and from the sidebar dates (§14) where the
sidebar only edits dates (CYC-017). The project picker is never shown
for an edit; a cycle cannot move project. The dialog heading and the
footer confirm control both read **"Update cycle"**. The title keeps the
same required/maximum rules as creation.

The date range is checked for overlap only when **both** new dates are
present and the range actually changed. An edit that leaves the dates
untouched, or that clears either date (converting the cycle back to a
draft), saves without a date check: `PATCH BASE/cycles/{cid}/` carries
the null dates and the server accepts it. An edit that sets both dates
and changes one of them first calls `POST BASE/cycles/date-check/` with
`{ start_date, end_date, cycle_id }` so the cycle does not collide with
itself; the check is never sent with a null date. A collision refuses the save with the same draft guidance as
creation. A successful save shows a confirmation toast and refreshes the
cycle row, the group counts and, when the range now covers today, the
hero; an edit that moves the range away from today removes the cycle
from the hero without a reload. The success toast reads `Cycle updated
successfully.`; the overlap refusal uses the same `Error!` toast and
draft guidance as creation. An edit that changes nothing still closes
the dialog and issues the patch, but sends no date check (the oracle
counts date-check requests and expects zero for untouched dates).

The edit form obeys the keyboard rules of §6 and is closed by Escape
without saving. A finished cycle cannot be edited at all (§8); an
archived cycle is restored before editing (§9). Guests cannot edit.

Oracle: `edit-delete.spec.ts`.

## 8. Deleting a cycle, and finished cycles as read-only

**Delete** (CYC-022). Delete has two layers, and the spec keeps both
visible:

1. **Which menu shows it.** The row menu offers delete to every member
   and admin, and never on a finished, archived or read-only cycle. A
   guest or viewer gets no delete entry.
2. **What the server accepts.** `DELETE BASE/cycles/{cid}/` is allowed
   for the cycle's creator/owner **or** a project admin. A plain member
   who is not the owner sees the entry, confirms, and receives a 403
   even though the entry was shown. That refusal is the dedicated
   permission message (below), not a hidden capability.

Confirming opens a dialog that names the cycle and states that deletion
is permanent; cancelling does nothing. The dialog heading is **"Delete
cycle"** and its body names the cycle (the oracle reads both). On
success the flagged cycle is
gone, and the UI shows a confirmation toast (`Cycle deleted
successfully.`) and removes the row and its
group count. When the viewer was on the deleted cycle's detail page or
had it open in the peek panel, they are returned to the project cycles
list. A cycle that disappears because someone else deleted it lands on
the gone-away view (§5) rather than retrying forever.

A permission failure — including a non-owner member's delete — shows the
dedicated permission message (a toast whose text contains "permission",
titled `You don't have permission to perform this action.`) and never
the generic `Failed to delete cycle` message, and leaves the row in
place; the list does not optimistically drop it. The CYC-022 forbidden-
delete scenario relies on this second layer: the entry is present for a
member, and the refusal is graceful and explains permissions.

**Finished cycles are read-only** (CYC-023). For a finished cycle the
detail page, sidebar, row menu and peek all suppress edit, delete, and
work-item creation. The detail carries a read-only notice that explains
the cycle has finished; where transfer is offered (§12) the same notice
introduces it. Analytics, the work-item badge and layout switching stay
available. The server remains the authority: a direct mutation call from
a stale page is rejected and surfaces the permission or state error, not
a silent success.

Oracle: `edit-delete.spec.ts`.

## 9. Archiving, restoring, and the archived list

**Archive** (CYC-024, CYC-025). Archiving is offered for finished
cycles only. The row menu's archive entry is present for draft, current
and upcoming cycles but disabled, with the hint `Only completed cycles
can be archived`; a finished row's entry is enabled. Archiving
asks for confirmation in a dialog headed **"Archive cycle {name}"**.
Confirming calls
`POST BASE/cycles/{cid}/archive/` with **no request body** and shows a
confirmation toast titled **"Archive success"** with the line `Your
archives can be found in project archives.`, and returns the view to the
project cycles list (the reference navigates there even when archiving
from the detail). A failure shows an error toast and leaves the row.
The server
stamps `archived_at` itself (the serializer lists it as read-only) and
answers `{ archived_at }`; the client must not send an `archived_at`
field. The cycle leaves the live list and groups; its completion ring
and counts stay frozen. Archiving never deletes work items.

**Restore** (CYC-026). An archived row's menu offers restore as a
**direct menu action**: picking it calls `DELETE BASE/cycles/{cid}/archive/`
immediately, with no confirmation dialog. Only archive and delete open a
dialog; restore must not, because the oracle picks "Restore" and
immediately waits for the success toast and the archives path, and a
dialog step would stall it. On success the cycle is live again; the
toast is titled **"Cycle restored"** with the line `The cycle has been
restored.`, and the view moves to the archived cycles list so the user
sees the row leave it. The restored cycle is a finished
cycle and reappears in the live list's completed group with its
snapshot intact.

Restore failure must be honest (see §26, NEWFRONT-244): if the restore
request fails, the UI shows an error that names the failure and keeps
the cycle in the archived list and archived state; it must not show the
success toast, must not navigate away as if it worked, and must not
optimistically drop the row. The row stays selectable so the user can
retry. The failure toast is titled **"Cycle restore failed"** with a
retry invitation; the success strings above must never appear on the
failure path.

**Archived list** (CYC-027). `/{ws}/projects/{pid}/archives/cycles`
lists archived cycles with the same row anatomy as the live list (name,
ring, marker, date span), without the hero and grouping. Its search
control is the same collapsed magnifier on arrival (the CYC-027 oracle
clicks it before typing), and activating it opens the field **focused**
because the input stays mounted under the collapsed box (§3). Search
filters by name only; its date filters and chips behave as in §4
without the lifecycle dimension; its zero state
says **"No archived cycles yet"** and does not offer creation; its
no-match state splits search from filters as in §5. Rows open a peek
panel on the same `peekCycle` key and offer only the archived actions
(open, copy, restore). Activating an archived row (name or body, not a
menu entry) opens that peek, exactly as the CYC-004 quick look does;
"Open in new tab" is the separate menu entry and goes to the canonical
detail address, which renders the gone-away view for an archived cycle
on a live detail route (§5). The row menu never navigates by itself.
Archived rows carry no favorite toggle (archiving removes the favorite).

**Who can read archives.** Archived reads are member+ only: the
archived-cycles endpoint (`CycleArchiveUnarchiveEndpoint.get`) and the
archive/restore writes allow admin and member, so a guest cannot list
them and sees the archives tab's own permission/zero state rather than
rows. That server gate is the first reason guests have no archived
list; the incomplete archived-row payload tracked by NEWFRONT-240 is
the second (see §25). The list reads archived cycles from
`GET BASE/archived-cycles/` (and `GET BASE/archived-cycles/{cid}/` for
one row's detail). It never refetches on window focus (§18).

Oracle: `archive-menu-transfer.spec.ts` (archive gate on draft and
current rows with a hint, completed row enabled; restore toasts success
and lands on the archives list; the pinned failure bug; archived rows
with working search and filters and an archives zero-state).
`edit-delete.spec.ts` (archive confirmation).

## 10. The row menu

Every cycle row has a menu, reachable by a trigger that appears on row
hover on desktop and is rendered inline on touch and narrow layouts
(§21). The menu is visible to every role, but its entries are gated by
role and cycle state (CYC-028):

| Cycle state                | Entries, in order                                                                                                                   |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| Draft or current           | **Edit** (member+), **Open in new tab** (all), **Copy link** (all), **Archive** (member+, disabled with hint), **Delete** (member+) |
| Finished (completed)       | **Open in new tab** (all), **Copy link** (all), **Archive** (member+)                                                               |
| Archived                   | **Open in new tab** (all), **Copy link** (all), **Restore** (member+)                                                               |
| Any state, guest or viewer | **Open in new tab** (all), **Copy link** (all)                                                                                      |

The titles are exactly these strings (the oracle filters menu items by
`h5` text), and the order is fixed by the DOM order above: disabled
entries are filtered out of the menu but keep their place, so removing
Edit from a completed row yields Open in new tab → Copy link → Archive.
Archive is not rendered at all on a completed row for a guest, and
Delete/Restore are absent whenever the viewer cannot mutate.

Disabled archive stays in place for draft and current rows so the
restriction is discoverable; it is not hidden. A guest's menu contains
no mutation entry at all rather than disabled mutation entries. Destructive
entries are visually separated from navigation entries and are never
the default focused item. Actions that would open a dialog close the
menu first; actions that navigate replace the list with the detail or
the new tab.

Oracle: `archive-menu-transfer.spec.ts` (state-valid entries in order,
reachable on hover; guests see only link actions).

## 11. Favorites and palette commands

**Favorites** (CYC-029). Favorites are for project members and admins
only — the same set that may edit — not for guests or the read-only
role. The star is rendered only where editing is allowed and the cycle
is not archived (the server's favorite endpoints allow admin and member
and refuse others), so a guest sees no star rather than a disabled one.
Activating the star flips its filled/unfilled mark at once and shows a
progress indication while the write is in flight; the oracle toggles
until the row and the server agree, so a silent no-op or a revert after
a successful write fails it. Copy the row's own `is_favorite` into the
star after every read, and re-read the cycle row after a toggle settles
(the reference sets the mark optimistically and reverts it on error).
The favorite is per user and is stored as a **workspace favorite**, not
through the project-scoped cycle endpoints. Adding toggles the star
optimistically and issues `POST WS/user-favorites/` with
`{ entity_type: "cycle", entity_identifier: <cid>, project_id,
entity_data: { name } }`; removing first resolves the row's favorite id
and issues `DELETE WS/user-favorites/{favoriteId}/`. Both endpoints allow
admin and member only. The sidebar's Favorites section reads the same
`GET WS/user-favorites/` collection, which is why the toggle and the
section stay in step; the project-scoped `BASE/user-favorite-cycles/`
endpoints exist but are not the toggle's path, and the new UI must not
mix the two. While the request
settles the UI shows progress feedback, then an outcome message that
names whether the cycle was added to or removed from favorites; a
failure reverts the star and reports the failure. The first favorite a
user ever creates reveals the favorites section in the app sidebar
(without a reload); later toggles do not. Archived rows do not offer the
toggle. The favorite state also comes back on the cycle row
(`is_favorite`) so lists and the sidebar agree.

**Palette** (CYC-030). On a live cycle page the command palette offers
two cycle-scoped commands whose titles are exactly **"Add to
favorites"** / **"Remove from favorites"** (one or the other, depending
on the row's `is_favorite`) and **"Copy URL"**. The favorite command is
member+ only (the same gate as the star); **Copy URL** is visible to
every role. Both act on the cycle in the current
address. Archived cycles
expose no cycle commands in the palette (neither title appears on the
archived tab). The palette results are
filtered to this cycle, not to the whole workspace. The favorite action
updates the same row state as the star and toasts the same outcome.
The create command lives on project-level palettes (§6), not on a
cycle page.

The palette's copy uses the browser's current address
(`window.location.href`, which may carry `?peekCycle=…`) rather than the
canonical path; that is the reference's behavior and is **not** the
canonical-copy rule of §19 for the row menu. Keep that distinction:
palette copy = current address with its query, row copy = canonical
detail path.

Oracle: `archive-menu-transfer.spec.ts` (row toggle marks, toasts
progress and outcome, persists both ways; first-ever favorite reveals
the favorites menu; live palette offers the toggle plus copy-address
while the archived tab offers no cycle commands).

## 12. Transferring remaining work

A finished cycle offers transfer from **two entry points** (CYC-031).
They share one eligibility helper and one dialog; only the snapshot
clause differs between them (below).

1. **Detail banner.** The finished cycle's detail banner carries the
   read-only notice of §8 and a transfer action.
2. **List row.** A completed row on the project cycles list carries an
   inline **"Transfer {n} work items"** action alongside its other extras
   (revealed with the row's hover area on desktop, inline on
   touch/narrow).

The two gates are **not identical**, and both are the reference's:

- **List row:** status is completed **and** `total_issues -
(cancelled_issues + completed_issues) > 0`. It does not look at
  `progress_snapshot`.
- **Detail banner:** status is completed, the same transferable count is
  greater than zero, **and** `progress_snapshot` is empty.

So a completed cycle that still has an empty snapshot shows the row
action and the banner action; one whose snapshot was already written
loses the banner action even with work left, while the row action
follows the count alone. The two counts
should share one helper so the definitions cannot drift; only the
snapshot clause differs. The detail banner carries the read-only notice
of §8 whenever the cycle is completed, independently of whether
transfer is offered. Both entry points open the **same** transfer
dialog — one dialog component, not two implementations.
Transfer opens a dialog headed **"Transfer work items"** with a
searchable picker of destination cycles, whose search input's
placeholder is exactly **"Search for a cycle..."** (the oracle waits on
that placeholder). An option row renders the cycle name plus a
capitalised status pill (`Current`, `Upcoming`, `Draft`) and is a
button whose accessible name starts with the cycle name.
The picker offers only cycles in the same project that are still open —
draft, current, upcoming — never the source cycle, never a finished or
archived cycle; the search narrows by name. With no eligible
destination the dialog explains that another open cycle must be created
first — the text reads `You don't have any current cycle. Please create
one to transfer the work items.` — and offers no submit. Selecting a
destination immediately performs the transfer (no second confirm).

Confirming calls `POST BASE/cycles/{cid}/transfer-issues/` with
`{ new_cycle_id }`; the response carries a message. On success the UI
shows a toast whose message contains `transferred successfully` (the
reference title is `Success!`), closes the dialog, and refreshes **both**
cycles: the destination gains the items and its counts and ring update,
and the source's banner, counts and ring update as well, without a
reload (the source-side refresh is the intended behavior of NEWFRONT-267,
§26, and applies whether transfer started from the detail banner or the
list row — the source row and its ring refresh too). The server **writes
the source's `progress_snapshot`** from its pre-transfer counters
(`utils/cycle_transfer_issues.py`), so after a successful transfer the
source's frozen numbers describe the work it had before the items left,
the detail-banner transfer entry disappears (snapshot now present) and
the row entry disappears because nothing transferable remains. Do not
spec a source with an empty snapshot afterwards. A failure shows an
error toast (`Unable to transfer work items. Please try again.`),
leaves the dialog's state intact, and moves nothing. Transfer is
member+ only, like every other mutation.

Oracle: `archive-menu-transfer.spec.ts` (only open cycles offered,
search narrows, confirming moves items with both-sides refresh; no-open-
cycle zero state).

## 13. Cycle detail: host, header, breadcrumbs

The detail page renders, top to bottom: the breadcrumb trail; the cycle
header; the work-item area; and the collapsible sidebar. It is the host
for the shared issues layouts (CYC-032), not a reimplementation of them.

**Layouts** (CYC-032). The work-item area offers list, board, calendar,
spreadsheet and timeline through the shared `IssuesLayoutRoot` bound to
the `{ scope: "cycle", projectId, cycleId }` entity. Every role that can
read the cycle gets every layout; the choice is stored per user per cycle
in the cycle's user-properties `display_filters.layout` and survives a
reload. Switching cycles switches to that cycle's own stored layout.
On narrow screens the area shows the compact header and offers list,
board and calendar only through its layout menu (§21). Archived cycles
never render here: archived cycles are opened through the archives list.
The work items are read from `GET BASE/cycles/{cid}/cycle-issues/`
through the shared list data layer; the layout's own filters, grouping
and display options live in the same cycle user-properties record
(`filters`, `display_filters`, `display_properties`, `rich_filters`).

**Header** (CYC-033). The header shows the cycle name, the lifecycle
marker, a cycle switcher, a work-item count badge, and actions:

- The switcher **is the last breadcrumb crumb**: the cycle name in the
  trail is itself the switcher control, not a separate header widget and
  not a plain label. Opening it lists the project's non-archived cycles
  in their stored order in a searchable menu (the reference shows the
  name with a cycle icon per option and does not regroup them), and
  picking one navigates to that cycle's detail. It is available to
  every role. The crumb therefore must not be an inert `aria-current`
  text node — it is the interactive last item, while the project and
  "Cycles" crumbs remain links.
- The badge counts the cycle's work items and is hidden when the count
  is zero. It is shown to every role that can read items; the intended
  guest behavior is in §26.
- Analytics is a member+ action on desktop; on the narrow bar every role
  gets it. The cycle header mounts the shared `AnalyticsEntry` with a
  cycle scope (slice F, NEWFRONT-712), so the dialog reads `Analytics
for {project} in {cycle}`; the mount keeps the desktop trigger
  member+ and adds a narrow-only trigger for every role. (Maintainer
  decision on NEWFRONT-712: slice F mounts the entry with cycle scope
  instead of blocking on NEWFRONT-738 for the dialog.) NEWFRONT-738
  keeps the dialog body: it refactors the interim body into the scoped
  dialog leaf, adds the `cycle_id` parameter to its project-scoped
  requests, and owns the `analyticsDialog*` driver hooks. Slice F never
  forks a dialog.
- Add work item is a member+ action and is hidden for a finished cycle
  (read-only, §8). Its label is **"Add work item"** and it opens the
  shared create modal pre-filled with this cycle (NEWFRONT-630).
  **Analytics** is the label of the desktop analytics button.

**Breadcrumbs** (CYC-034). The trail is project → cycles → cycle name.
The project crumb goes to the project's work-items page; the middle
crumb goes to the project cycles list; the last crumb is the current
cycle and doubles as the switcher above. On narrow screens the trail collapses to a back control
that returns to the cycles list. Archived rows opened from the archives
list carry an archives crumb as well, described in §9.

**Gone away** (CYC-013) replaces the whole page for a missing cycle. A
finished cycle keeps the page, the layouts, the badge and analytics,
suppresses creation and editing, and shows the read-only notice.

Oracle: `detail-sidebar.spec.ts` (all five desktop layouts and per-cycle
prefs and the small-screen menu; timeline survives reload; switcher,
badge, analytics and creation incl. finished cycles; crumbs navigate to
list and project; small-screen back; guest switching).

## 14. The sidebar

The sidebar is collapsible and persisted per browser (CYC-035). A toggle
in the header collapses and expands it; the panel also carries a close
control. The collapsed flag is the string `"true"` at
`localStorage["cycle_sidebar_collapsed"]` (absent or anything else means
expanded) and is restored on the next visit
to any cycle in the same browser; the toggle's label and state
(`aria-expanded`) reflect it. Collapsing does not change the URL and does
not touch work-item state.

Sections, top to bottom:

- **Header** (CYC-036): the cycle name and lifecycle marker. Members and
  admins edit the start and end dates inline here; guests and finished
  or archived cycles see them read-only. An inline date save goes
  through the same overlap protection as the edit dialog: setting both
  dates to a changed range asks `POST BASE/cycles/date-check/` with the
  cycle id (clearing either date saves without a check, §7), a
  collision refuses the save with the same draft guidance, and a
  successful save confirms with a toast and refreshes the row, the
  groups and the hero (CYC-019). Two saves in quick succession must not
  swallow the second result: if two toasts would collide the newer one
  replaces or queues visibly, and an overlap rejection is always shown
  (§26, NEWFRONT-264).
- **Dates and timezone** (CYC-044, see §17): the date span with the
  project offset chip whenever the project offset is non-zero.
- **Description** (CYC-037): the cycle's description, read-only, with
  paragraph breaks preserved. An empty description renders nothing — no
  placeholder text — matching the old sidebar. Editing the description
  goes through the edit dialog.
- **Lead** (CYC-037): the responsible user's avatar and name; avatar
  image with a fallback initial. The row always renders; with no lead it
  shows an empty avatar and no name text (no "No lead" string), which is
  what the old sidebar does. A member list follows when assignees exist,
  or **"No assignee"**. The row labels are exactly **"Lead"**,
  **"Members"**, **"Work items"** and (when it renders) **"Points"**;
  the oracle reads the assignee row as "Members", not "Assignees".
- **Work items** (CYC-038): a `completed/total` pair
  (`completed_issues/total_issues`) using the server's counters as they
  are; `total_issues` includes cancelled items, and only the ring (§2)
  excludes them. The oracle reads `0/2` and `1/3` here, so always render
  the pair, including a zero numerator; the reference's `0 Work item`
  string appears only when the underlying total is itself zero, and the
  pair is the contract the gate proves. Finished cycles read the frozen
  snapshot's counters. The **"Points"** row renders only when the
  project's estimate system is points, the cycle is not completed, and
  (for a finished cycle's snapshot) the snapshot carries an
  `estimate_distribution` (the reference hides it for item-based
  estimates); it reads the `completed_estimate_points /
total_estimate_points` pair, e.g. `0/2`.
- **Progress by state** (CYC-038): the cycle's work items grouped by
  workflow state, with the count per state group and the closed count,
  into a total that includes cancelled items. Each group card has its
  own empty placeholder when it holds no items; the cancelled group
  renders when it has items.
- **Burn-up/burn-down chart** (CYC-038): a chart measured in work items
  by default, with a measure picker offering estimates only when
  estimates are enabled. The sidebar section heading is **"Progress"**
  and the measure control's two options are exactly **"Work items"**
  and **"Estimates"** (the oracle locates the trigger by those two
  names). The chart is hidden when the cycle has no valid
  past-start range; an undated cycle shows a "no data" placeholder
  rather than a broken axis. A future-start cycle hides the chart.
- **Breakdown entries** (CYC-039): states, assignees and labels entries.
  Their tabs are labelled exactly **"States"**, **"Assignees"** and
  **"Labels"** in that order; **Assignees** is the default selected tab
  (the reference stores the choice per cycle in
  `localStorage["cycle-analytics-tab-{cid}"]` and falls back to
  Assignees), and the chosen tab is `role="tab"` with `aria-selected`.
  Clicking an entry applies the matching rich filter to the cycle's work
  items and shows it as an active filter; clicking the same entry again
  lifts it; entries combine. The chart itself has no click action. The
  applied filter is persisted in the cycle user-properties
  `rich_filters` so it survives a reload. The first click on a fresh
  cycle must work even before a user-properties row exists for this user
  and cycle (§26, NEWFRONT-266): the write path creates the record
  rather than dropping the change.
- **Loading** (CYC-040): the whole sidebar renders a skeleton until the
  cycle and its progress data arrive; the peek panel does the same. The
  skeleton keeps the layout height stable so the page does not jump.

Progress and analytics reads: `GET BASE/cycles/{cid}/progress/` for the
snapshot and chart series, `GET BASE/cycles/{cid}/cycle-progress/` for
the cloud read, and `GET BASE/cycles/{cid}/analytics?type=issues|points`
for the breakdown distributions. The sidebar reads user-properties with
`GET`/`PATCH BASE/cycles/{cid}/user-properties/`.

Oracle: `detail-sidebar.spec.ts` (toggle persists across reloads;
inline dates save with confirmation and overlaps are rejected with
guidance; description and lead with avatar; counts, chart and breakdowns
without estimates, the Estimates measure where enabled, undated and
future-start hide the chart; state-group entries narrow, combine and
lift).

## 15. Active-cycle hero

The list leads with the cycle that covers today (CYC-041). The hero has
a group heading **"Active cycle"** (singular; the oracle matches the
heading by that prefix and the driver reads the hero's name from the row
inside it), the active row itself with its ring and dates, and three
cards:

1. **"Progress"** — work items by state group with counts, plus a
   summary ending in the word **"closed"** (`{completed +
cancelled}/{total - cancelled} Work items closed`, matching the
   reference) and a cancelled-count line, both only when the cycle has
   items; clicking a state group navigates to the
   cycle detail with that state group as an applied filter (the same
   persisted rich filter as the sidebar entries), and the detail shows
   the filtered work items.
2. **"Work item burndown"** — the `Pending work items - {n}` line, the
   chart, and the
   same measure picker as the sidebar (items, estimates when enabled);
   that picker follows the sidebar's categories rule: where the
   project's active estimate system is the categories (text) family,
   the hero hides the picker entirely and keeps counting items, even
   if the cycle carries point data;
   an undated or future-start cycle shows the chart's empty
   placeholder.
3. **Breakdown** — tabs labelled exactly **"Priority work items"**,
   **"Assignees"** and **"Labels"** in that order (the oracle compares
   the full list). The priority
   tab lists urgent and high work items; clicking an item opens its look
   panel and applies a priority filter. Assignees and labels tabs list
   their entries with counts, and clicking an entry applies the matching
   filter to the cycle's work-item view, navigating to the detail when
   the click originates from the list. The first click on a fresh cycle
   must work before a user-properties row exists (§26, NEWFRONT-266).

Each card has its own empty placeholder for "no work yet", "no dates"
and "nothing to show" rather than an empty box. The hero is built from
the same cycle row, `progress_snapshot`, `distribution` and progress
reads as the sidebar; it does not duplicate their requests when the
sidebar is open on the same cycle. The progress card's empty copy is
`Add work items to the cycle to view it's progress`; the burndown card
shows its own empty state and no chart when there is nothing to plot.
Do not replace either with a bare box or a spinner once data has
arrived.

With no cycle covering today the hero is replaced by an explainer
(CYC-042) titled **"No active cycle"** whose description says a cycle
is active when its range includes today's date; it offers a creation shortcut to
members and admins and no shortcut to guests. The upcoming and completed
groups follow the explainer as usual. The oracle checks the title and
description for the words "active cycle" and "today", so both must be
present.

Oracle: `hero-crosscutting.spec.ts` (hero shows progress, burn-down and
breakdown cards; breakdown interactions filter the view; per-card empty
views; the pinned fresh-click bug; the no-active-cycle explainer).

## 16. Workspace active cycles

`/{ws}/active-cycles` is the workspace-level active-cycles page
(CYC-043). In this checkout it renders the **paid-feature upsell in
every edition**: the CE root mounts the upgrade component
(`apps/web/ce/components/active-cycles/root.tsx` →
`workspace-active-cycles-upgrade.tsx`), and the page mounts that root
unchanged. Nothing in this tree renders a list of active cycles; the
service method for the endpoint exists but no page calls it.

- **Free (OSS) workspace**: the page shows a heading for active cycles,
  a badge marking the paid feature, a hero explaining on-demand
  snapshots of all cycles, a short set of benefit cards, and an upgrade
  action that opens `https://airepublic.com/pricing` in a new tab. The
  page never requests the active-cycles endpoint. The pinned shape the
  oracle reads is: a level-2 heading whose text contains "snapshots"
  (the reference reads "On-demand snapshots of all your cycles"),
  exactly **six** benefit cards, each headed by a level-3 heading, an
  anchor labelled **"Upgrade"** whose `href` contains `pricing` and whose
  `target` is `_blank`, and a header badge whose text is exactly
  **"Pro"**. Keep those counts and strings; the illustrations, colours
  and card copy are free.
- **Paid cloud workspace**: nothing in this checkout changes the
  rendered page — it is the same upsell component. A real paid data view
  may live in the private EE overlay, but the overlay at
  `private-pi-dash/ee-overlay/apps/web/` contains no active-cycles
  source at this head, so there is no rendered screen to cite. Treat the paid data
  view as a **seam**, not a spec'd screen: no slice builds a cloud list
  and the gate proves only the upsell. If a cloud data view is wanted,
  a new inventory row and oracle scenario come first, with the source
  path recorded. The workspace endpoint `GET WS/active-cycles/` is
  cursor-paged and documented in §23, but is unused by the page.
- **Desktop**: follows the edition the build is running as; the route is
  reachable from the workspace navigation in both, and renders the same
  upsell in this checkout.

The edition switch is a single capability check supplied by the build,
not a fork of the page. The workspace route appears in the workspace
navigation only where the edition supports the view or the upsell. The
page does not read data, so it needs no empty or error state of its own;
the seam's data view, when it exists, owns those.

Oracle: `hero-crosscutting.spec.ts` (workspace actives page is a paid
upsell: "snapshots" heading, six benefits, a pricing link opened in a
new tab, and the Pro badge).

## 17. Dates and timezones

Cycle dates are stored as instants and displayed in the project's
timezone (CYC-044). A two-sided span renders as one compact range with
the year **always** present (both ends in the same year):

- same month: `Sep 01 - 18, 2026` (month and year once, days on both
  ends);
- different months: `Sep 01 - Oct 18, 2026`;
- different years: `Dec 28, 2025 - Jan 04, 2026` (year on both ends).

The year is not conditional on "current year"; do not drop it for a
current-year range. A one-sided span is `MMM dd, yyyy` (`Oct 15, 2026`);
a cycle with neither date renders no date text. The oracle reads a
month-and-digit string from the row, so any of the shapes above must
contain the month name and a digit. Whenever the project timezone's offset from UTC
is non-zero, the span carries a small offset chip (`UTC -04:00`, `UTC
-05:00`) and a tooltip explaining that the dates are shown in the
project's timezone; the chip also exposes the viewer's equivalent day
when it differs. The chip is **not** gated on comparing the project
zone with the browser's: a viewer in the same zone as a non-UTC project
still sees it, because it marks that the dates are project-zoned. The
suppression rule is the **project offset** alone: when the offset from
UTC is zero (whether the project is configured as `UTC` or as any other
name with a zero offset) no chip renders. The chip reads the project's
UTC offset; it must not key on the browser timezone or the string
`"UTC"`. The chip's text starts with `UTC` followed by a signed offset
(`UTC +05:30`, `UTC -04:00`), exactly as the oracle matches
`/^UTC [+-]/`; render the offset as its own element inside the span.
The "In your timezone" tooltip is shown only when the project zone
differs from the viewer's; that condition affects only the tooltip,
never the chip.

A missing start or end
renders a one-sided range (the single date); a cycle with neither date
renders no date text (§2, CYC-002), while the sidebar and hero show
their own "no dates" placeholder.

Day boundaries are computed with the shared timezone/date helper so the
list, hero, dates in the create dialog, the date-check request and the
sidebar all agree on what "today" and "covers today" mean. The helper
takes the project timezone as an argument; it is part of slice A
(NEWFRONT-707) and is the only place the area converts calendar days to
instants.

Oracle: `hero-crosscutting.spec.ts` (cycle dates carry the project
offset and user-timezone equivalents; no offset chip when the project
timezone is UTC — the oracle's UTC case exercises the zero-offset rule
above).

## 18. Freshness and refresh behavior

The area keeps its data fresh without a full page reload (CYC-045):

- Creating a cycle refreshes the list, the group counts and the hero;
  a cycle covering today appears in the hero on the spot (CYC-019).
- Editing or deleting refreshes the affected row, its group count and
  the hero or empty state that follows.
- Archiving removes the cycle from the live list and adds it to the
  archived list; restoring does the reverse.
- Transfer refreshes the destination and the source (§12).
- Favoriting reflects in the row and the sidebar.

These refreshes invalidate only the affected keys: the cycle list, the
specific cycle, and the progress/analytics reads of the cycles that
changed. The list must not refetch everything on a timer. The archived
list never refetches on window focus, so returning to the tab does not
reorder or flash the archived rows; the live list may refetch on focus,
but it must render its previous data while doing so, without a skeleton
or an empty flash. Preferences (layout, filters, collapsed groups) are
read once on mount and not refetched on focus.

Oracle: `hero-crosscutting.spec.ts` (creating a cycle refreshes the list
without a reload; the archived list never refetches on window focus; the
pinned transfer-source staleness).

## 19. Copy link and open in a new tab

The **row menu** can copy a cycle's address to the clipboard (CYC-046).
Row copy uses the canonical detail address
(`/{ws}/projects/{pid}/cycles/{cid}`) — the same string on the list, the
detail and the archived list — and confirms visibly after
the clipboard write succeeds (the reference toasts `Link copied!` /
`Link copied to clipboard`); a clipboard refusal reports an error and
does not claim success. The **palette's `Copy URL`** is the one
exception: it copies `window.location.href` (current address, query
included), as noted in §11. Open in a new tab opens the canonical
address in a new browsing context and leaves the current view
untouched. Archived
rows offer the same copy and open entries as live rows; the copied
string is the same canonical detail address. The command palette never
offers cycle commands for archived cycles (§11).

Oracle: `archive-menu-transfer.spec.ts` (row menu copies the address
with confirmation and opens it in a new tab; the palette copies the same
address on a detail page, where current address and canonical path
coincide after any `peekCycle` key is dropped).

## 20. Guests and viewers

Guests and viewers are read-only across the area (CYC-047):

- They can open the cycles list and detail, browse rows, use search and
  filters, clear chips, open the peek panel, and copy links or open them
  in new tabs.
- They cannot create, edit, delete, archive, restore, transfer, or
  favorite; none of those affordances renders as a working control, and
  the server refuses a direct attempt.
- The create shortcut in the zero state and the feature-off management
  shortcut render disabled or absent.
- On the detail page they get every layout's content, the badge and the
  breakdown filtering as read-only (§26, NEWFRONT-261), and the narrow
  bar's analytics entry.
- They do not get the project's cycles tab in the tab strip (SHELL-075);
  direct addresses still render the list.

Search and filters must remain available to guests; losing them behind
the create gate is the pinned defect of NEWFRONT-268 (§26).

Oracle: `hero-crosscutting.spec.ts` (guests browse, peek and copy links;
no working mutation affordance; the pinned search/filter-loss bug).
`detail-sidebar.spec.ts` (guest layout switching, guest header
behavior). `archive-menu-transfer.spec.ts` (guest row menu).
`empty-states-create.spec.ts` (guest shortcuts disabled).

## 21. Touch and narrow layouts

On touch devices and narrow viewports the area keeps every action
reachable without hover (CYC-048):

- Row menu triggers render inline in every row instead of appearing on
  hover; the menu opens as a sheet or popover anchored to the row and
  dismisses on outside tap.
- The **list**'s narrow header carries a layout control whose menu
  offers exactly two entries, **"List layout"** and **"Gallery
  layout"** (the oracle asserts that exact array). "Gallery layout"
  switches the list's row presentation only; every cycle row stays
  listed and the hero and groups remain, so a name visible before the
  switch is still visible after it. The reference's third entry
  (timeline) is filtered out on narrow screens; do not add it.
- The detail's narrow bar offers a **Layout** menu with **"List"**,
  **"Board"** and **"Calendar"** (option names are those three; picking
  Board writes `kanban` to the cycle's user-properties
  `display_filters.layout`), plus Display and analytics; the sidebar opens as a full-height
  panel over the content and closes back to the content.
- The breadcrumb trail collapses to a back control to the cycles list.
- The hero's cards stack; the sidebar toggle remains visible and the
  collapsed flag still persists.
- All narrow controls meet touch target sizes and stay keyboard
  operable, and no action exists only behind a hover state.

Oracle: `hero-crosscutting.spec.ts` (row menus render inline on narrow
screens; layout switching stays reachable on narrow screens).
`detail-sidebar.spec.ts` (small-screen menu, back trail, guest bar).

## 22. Reuse map

Cycles is a host area: it must use the shared pieces built by the Issues,
Analytics and Archives epics instead of forking them. Where a shared
piece does not exist yet, the slice that needs it is blocked by the
slice that owns it. Nothing here is an import from the old frontend.

**Shell and routes**

- Cycles tab mount: `apps/web_new/src/routes/$ws/_gated/projects/$projectId/cycles/index.tsx`
  (today a `TabAreaMount` placeholder). Slice A (NEWFRONT-707) replaces
  the mount; later slices fill the screen.
- Detail route: `.../cycles/$cycleId.tsx`, today hosting
  `EntityListScreen` for the cycle scope. Slice F (NEWFRONT-712) replaces
  it with the full detail host and mounts `IssuesLayoutRoot`.
- Archived list: new route under
  `routes/$ws/_gated/projects/archives/`, alongside the projects
  archived view (`archives/index.tsx`, NEWFRONT-698). The Archives shell
  supplies the archived-view frame and crumb; the cycles slice supplies
  the row list. Archives shell owner: NEWFRONT-698. Consumer: slice E
  (NEWFRONT-711).
- Workspace actives: new route `routes/$ws/_gated/active-cycles/`; slice
  H (NEWFRONT-714) builds the upsell page of §16, including its pricing
  link and upgrade marker, and no data view. No shared shell piece
  exists; it uses the workspace app shell.

**Issues layouts and their data**

- Layout root: `src/features/issues/layouts/IssuesLayoutRoot.tsx`.
  Detail host passes `entity = { scope: "cycle", projectId, cycleId }`,
  the cycle title, total and result counts, and the offered layouts.
- Layout kinds and order: `src/features/issues/layouts/kinds.ts`
  (`LAYOUT_ORDER`, `MOBILE_LAYOUT_ORDER`, `LAYOUT_LABELS`); all five
  desktop layouts, three on narrow.
- Entity prefs: `src/features/issues/prefs/entities.ts` (`IssuesEntity`
  cycle scope and the cycle user-properties write path),
  `prefsQuery.ts` (read/prefetch), `displayPrefs.ts`. Per-cycle layout,
  filters and display options are stored in the cycle user-properties
  record, never in a cycles-local store.
- List layout: NEWFRONT-619. Spreadsheet: NEWFRONT-620. Calendar:
  NEWFRONT-621. Board: NEWFRONT-622 and NEWFRONT-623. Timeline:
  NEWFRONT-624. Display options: NEWFRONT-618.
- Filters and rich filters: NEWFRONT-626–629 (the sidebar's state,
  assignee and label entries are rich filters, and the hero's
  breakdown clicks are the same filters).
- Dropdown primitives (menus, pickers, chips): NEWFRONT-615 and
  NEWFRONT-616. The row menu, the filter menus and the applied-filter
  chips all use these; no cycle-local menu primitive.
- Create work item modal: NEWFRONT-630 (the detail's add action and the
  list's group-header quick add).
- Detail and sidebar patterns: NEWFRONT-639, NEWFRONT-640,
  NEWFRONT-642. The cycle sidebar follows the property-row and
  progress-stat patterns from those slices; its data is cycle-specific.
- Peek: NEWFRONT-646 (`shared/shell/PeekPanel.tsx`) hosts the peek panel
  frame; the cycle peek body is cycles-owned and reads the cycle
  summary. Bulk selection is NEWFRONT-633 and is not used by the cycle
  rows.
- Analytics: the shared `AnalyticsEntry.tsx` (ISS-107) mounted with a
  cycle scope by slice F (NEWFRONT-712): an optional cycle (`id`,
  `name`) switches the heading to `Analytics for {project} in {cycle}`,
  the desktop trigger stays member+, and a narrow-only trigger opens it
  for every role including guests. (Maintainer decision on NEWFRONT-712,
  superseding the earlier plan that slice F block on NEWFRONT-738 for
  the dialog.) NEWFRONT-738 keeps the dialog body: it refactors the
  interim body inside `AnalyticsEntry.tsx` into the scoped
  `AnalyticsDialog` leaf, adds the `cycle_id` query parameter to its
  project-scoped requests, and owns the `analyticsDialog*` driver
  hooks. The cycle area consumes the shared entry and never forks a
  dialog.

**Shared plumbing cycles reuses**

- Pickers: `src/shared/pickers/` — `CyclePicker`, `ProjectPicker`,
  `MemberPicker`, `EstimatePicker`, `TimezonePicker`. The create/edit
  dialog's project picker, the switcher cycle menu, the transfer
  destination picker and the sidebar lead use these; cycles builds no
  picker of its own.
- Shell: `src/shared/shell/PeekPanel.tsx` (panel frame), `toaster.ts`
  (`notifySuccess`/`notifyError`), `clipboard.ts` (copy with
  confirmation), `narrowViewport.ts` (compact detection),
  `CommandPalette.tsx` (palette entries), `preferenceStorage.ts`
  (localStorage helpers for the sidebar flag and the remembered tab).
- Project tab model: `src/features/projects/tabs.ts` (`cycle_view`
  flag, `memberPlus`).
- Contracts: `@pidash/api-client` — `contracts/cycles.ts` today
  (`CycleListItem`/`listCycles` for the picker, `CycleUpdate`/
  `updateCycle`, `transferCycleIssues`); NEWFRONT-706 adds the full
  row, date-check, archive/restore, favorites, progress/analytics,
  workspace actives and cycle user-properties schemas and their
  contract tests.

**Ownership summary**

| Screen or piece                                                                                                       | Built by                 |
| --------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| Route shell, feature-off, zero, skeleton, gone-away, date helper, freshness                                           | NEWFRONT-707 (slice A)   |
| List groups, rows, detail link, peek, search, no-match, copy                                                          | NEWFRONT-708 (slice B)   |
| Lifecycle/date filters, chips                                                                                         | NEWFRONT-709 (slice C)   |
| Create/edit dialog, validation, overlap, tab order, permissions                                                       | NEWFRONT-710 (slice D)   |
| Row menu, delete, read-only finished, archive/restore, archived list                                                  | NEWFRONT-711 (slice E)   |
| Detail host, shared layouts, header, breadcrumbs                                                                      | NEWFRONT-712 (slice F)   |
| Sidebar toggle, dates, description, lead, progress, breakdowns                                                        | NEWFRONT-713 (slice G)   |
| Favorites, palette, transfer (both entry points, one dialog, both-sided refresh), hero, workspace actives, guest read | NEWFRONT-714 (slice H)   |
| Touch/narrow plus the intended bug fixes                                                                              | NEWFRONT-715 (slice I)   |
| Contracts                                                                                                             | NEWFRONT-706             |
| Parity proof                                                                                                          | NEWFRONT-716 (area gate) |

## 23. Data shapes and API calls

**Cycle row** (project list, detail, archived list). The UI relies on: `id`,
`name`, `description`, `project_id`, `workspace_id`, `status`
(current, upcoming, completed, draft; compare case-insensitively),
`start_date` and `end_date` (instant or null), `sort_order`,
`owned_by_id` (lead), `assignee_ids`, `created_by`, `created_at`,
`archived_at` (null when live), `is_favorite`, `progress_snapshot`,
counters for total, completed, backlog, started, unstarted, cancelled
work items and the same for estimate points, `distribution` (assignees,
labels and the completion chart), `estimate_distribution`, `progress`
(series), and `version`. `GET BASE/cycles/` answers a **bare array** of
these cycle rows. `GET WS/cycles/` also answers a bare array, but with a
narrower projection: `id`, `workspace_id`, `project_id`, `name`,
`description`, dates, `owned_by_id`, `view_props`, `sort_order`, the
external ids, `progress_snapshot`, `logo_props` and the six work-item
counters — no `status`, `is_favorite`, `assignee_ids`, `version` or
`created_by`. Read both as arrays, not as `{results}` envelopes. Only
`GET WS/active-cycles/` is a
cursor-paged envelope (`per_page`, `next_cursor`); nothing else in this
area is paged, so the UI never assumes a fixed page size.

**Workspace active rows** (only for the seam of §16, not built today)
would add the project reference needed to link to the cycle detail
(project id and, where supplied, the project name).

**Progress and analytics.** Progress reads answer the snapshot and the
series used by the ring and chart; analytics answers, per `type`,
either `distribution` (items) or `estimate_distribution` (points). The
breakdown entries derive their counts from those distributions.

**Errors.** Validation errors from cycle create/patch are field-addressable
where the service supplies field keys; the dialog shows them next to the
field. A permission failure on any mutation is shown as a permission
message; a missing cycle on any read or write is the gone-away state; a
date-check collision is a refusal with guidance, not a generic error.
Missing/deleted detail reads are a real 404 from the API; the UI maps
that to the gone-away view rather than treating it as retryable.

**Peek and detail reads.** The peek panel reads one cycle
(`GET BASE/cycles/{cid}/`) and its progress, exactly like the detail
page, so opening the peek does not depend on the list row still being
loaded after a filter change. The archived peek reads
`GET BASE/archived-cycles/{cid}/` and must tolerate the incomplete row
payload described in §25 (NEWFRONT-240).

| Method and path                                                            | Purpose                                                             | Rows                              |
| -------------------------------------------------------------------------- | ------------------------------------------------------------------- | --------------------------------- |
| `GET BASE/cycles/` (no `cycle_view`; `current` only for the hero read)     | project cycles list (all live cycles) and the hero's current cycle  | 001, 002, 011, 012, 032, 033, 045 |
| `POST BASE/cycles/`                                                        | create `{name, description, start_date, end_date}`                  | 014, 016, 018, 019                |
| `GET BASE/cycles/{cid}/`                                                   | cycle detail and peek body                                          | 003, 004, 013, 035, 037, 040      |
| `PATCH BASE/cycles/{cid}/`                                                 | edit title/description/dates                                        | 017, 036                          |
| `DELETE BASE/cycles/{cid}/`                                                | delete                                                              | 022                               |
| `GET BASE/cycles/{cid}/cycle-issues/`                                      | the cycle's work items for the hosted layouts                       | 032, 033, 039                     |
| `POST BASE/cycles/date-check/`                                             | overlap check `{start_date, end_date, cycle_id?}` → `{status}`      | 016, 017, 036                     |
| `POST BASE/cycles/{cid}/transfer-issues/`                                  | `{new_cycle_id}` → `{message}`                                      | 031                               |
| `POST WS/user-favorites/` / `DELETE WS/user-favorites/{favoriteId}/`       | favorite toggle (star and palette); the sidebar section reads `GET` | 029, 030                          |
| `POST BASE/cycles/{cid}/archive/`                                          | archive (no body) → `{archived_at}`                                 | 024, 025                          |
| `DELETE BASE/cycles/{cid}/archive/`                                        | restore                                                             | 026                               |
| `GET BASE/archived-cycles/` and `GET BASE/archived-cycles/{cid}/`          | archived list and row                                               | 004, 027                          |
| `GET WS/cycles/`                                                           | workspace-wide cycles for switchers and pickers                     | 030, 033                          |
| `GET WS/active-cycles/?per_page&cursor`                                    | unused by the page today (upsell only, §16); cursor-paged envelope  | 043                               |
| `GET BASE/cycles/{cid}/progress/`, `GET BASE/cycles/{cid}/cycle-progress/` | snapshot and series (cloud variant)                                 | 038, 041, 045                     |
| `GET BASE/cycles/{cid}/analytics?type=issues\|points`                      | breakdown distributions                                             | 038, 041                          |
| `GET`/`PATCH BASE/cycles/{cid}/user-properties/`                           | per-cycle layout, filters, display, rich filters                    | 032, 035, 039                     |
| `GET BASE/` (project features)                                             | `cycle_view` flag, feature-off view                                 | 010                               |

The date-check answer is `{ status: boolean }`: `true` means the range is
free. Its request takes `start_date` and `end_date` in `yyyy-MM-dd` form
and, on edit, `cycle_id` so the cycle is excluded from its own check.
The two dates must be sent as date-only values computed in the project's
timezone (§17), not as UTC instants, or a late-evening local day shifts
by one and a legitimate range is refused.

## 24. Edition seams

- **OSS**: everything in this spec; the workspace active-cycles page is
  the pricing upsell (§16). No cloud-only cycle
  dialog or extension entry renders: the old cloud slots for end-of-cycle
  and additional row actions are empty in OSS and have no observable
  capability, so the new area has no equivalent mount. If cloud sources
  later add a real capability there, a row is added to the inventory
  first.
- **Cloud**: the workspace active-cycles page renders the same upsell in
  this checkout; a paid data view is a seam with no source path here
  (§16), and is built only after a row and scenario exist. The cloud
  progress read (`cycle-progress/`) backs the same sidebar and hero
  charts; no additional cycle screen exists.
- **Desktop**: follows the same edition capability as its build; the
  desktop window uses the same routes and the same wide/narrow behavior
  (§21) driven by window width, not the platform. Opening links in a new
  tab and copying addresses use the shell's external-link and clipboard
  helpers, which already handle the desktop runtime; the area never
  calls a browser-only API directly.

The single capability switch for the workspace actives page is supplied
by the build and read once; it must not be inferred from the workspace
payload.

## 25. Cross-area seams and backend gaps

- **Issues**: the detail host is the issues layouts (NEWFRONT-618–624,
  626–629). The work-item create modal (NEWFRONT-630) is not a
  create-cycle entry point; it pre-fills a work item's cycle at most.
  The cycle create dialog is reached from the cycles list header, its
  zero state and the palette's "New cycle" command; the cycles area owns
  that dialog, its fields, project target and date-check, and does not
  fork the Issues modal.
- **Issues pickers**: `CyclePicker` (shared/pickers) lists project
  cycles and filters on status; cycles must not fork it for the
  switcher or transfer. The transfer destination adds its own
  eligibility rule (open cycles except the source) on top of the shared
  picker.
- **Analytics**: the shared `AnalyticsEntry` mounted with the cycle
  scope is the only analytics surface (NEWFRONT-712; maintainer
  decision on that issue); the cycle detail's wide header (member+) and
  narrow bar (every role) open it, and its heading names the project
  and the cycle. NEWFRONT-738 keeps the dialog body.
- **Archives**: NEWFRONT-698's shell is the frame for the archived
  cycles list; the cycles slice supplies the rows, search, filters and
  restore.
- **Shell**: the project tab strip (`memberPlus`, `cycle_view`), the
  workspace navigation entry for active cycles, the command palette and
  the sidebar favorites section are shell-owned; cycles plugs into them
  and does not reimplement them.
- **Backend guest read (PDASHOSS01-310)**: the intended CYC-032/CYC-033
  guest behavior — guests see cycle work items, the badge and breakdown
  results in every layout — depends on the backend granting guests a
  read of `cycle-issues` (and the analytics/progress reads it needs).
  Until that lands, the guest scenarios stay pinned to the old behavior
  (bug NEWFRONT-261) and slices build the intended UI; the area gate
  (NEWFRONT-716) cannot mark those rows new-green before the backend
  change is merged. Slices must not weaken or skip the scenario and must
  not fake guest data client-side.
- **Archived cycles backend (NEWFRONT-240)**: the archived list depends
  on the backend returning archived cycles with their rows and their
  frozen snapshot. Where that read is incomplete, the list must show
  the honest error/empty state, not invent rows. This is a seam; the
  contracts slice (NEWFRONT-706) types what exists and the gap is
  tracked by NEWFRONT-240.
- **Notifications and favorites**: revealing the favorites section on
  first favorite is the shell sidebar's behavior; cycles only writes the
  favorite and reports the outcome.

## 26. Bug-row dispositions

These seven rows pin an old defect in the oracle. Slices build the
intended behavior below; the scenarios are aligned at gate time. Where
a scenario currently asserts the old outcome, the slice records that in
its workpad rather than weakening the assertion.

| Row              | Follow-up    | Old behavior pinned                                                                                                                                             | Intended behavior                                                                                                                                                                                                                                                         |
| ---------------- | ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| CYC-026          | NEWFRONT-244 | A failed restore reports success and the cycle stays archived.                                                                                                  | A failed restore shows the `Cycle restore failed` toast and leaves the cycle in the archived list and archived state; no success toast, no navigation to the archived list (the success destination), no optimistic removal. The row stays retryable.                     |
| CYC-032, CYC-033 | NEWFRONT-261 | Guests cannot read cycle work items: the layouts are empty, the badge hides and breakdown filtering yields nothing, though creation/mutation gating is correct. | Once PDASHOSS01-310 lands, guests browse cycle items read-only: every layout shows the items, the badge counts them, and sidebar/hero breakdown entries filter them. Creation and all mutations stay gated (§20). No client-side fake data while the backend gap is open. |
| CYC-020          | NEWFRONT-263 | Tab order skips the project picker (and can escape the dialog).                                                                                                 | The picker sits before the title in the DOM, so the trap wraps submit → picker → title; after the autofocused title the stops are description → dates → cancel → submit → picker. Focus never leaves the dialog; Shift+Tab reverses; Escape closes.                       |
| CYC-036          | NEWFRONT-264 | A second inline date save while a toast is showing can swallow the overlap rejection, so the user sees no error.                                                | Toasts queue or visibly replace one another; an overlap rejection is always rendered in the panel next to the dates and never swallowed, even when a success toast is on screen.                                                                                          |
| CYC-041          | NEWFRONT-266 | The first hero breakdown click does nothing because the user-properties row does not exist yet.                                                                 | The first breakdown click on a fresh cycle works: the write path creates the record, applies the filter, and shows it, with no second click needed. Same for the sidebar entries.                                                                                         |
| CYC-045          | NEWFRONT-267 | Transfer refreshes the destination but the source banner and ring stay stale.                                                                                   | Transfer refreshes both sides: the source's banner, counts and ring update without a reload, from either entry point, even though the server has just written the source's snapshot (§12).                                                                                |
| CYC-047          | NEWFRONT-268 | Guests lose the list search and filter controls behind the create gate.                                                                                         | Guests keep search, filters, chips, clear-all, peek and copy; only creation and mutation affordances are gated (§20).                                                                                                                                                     |

### Rules settled by the spec review

These are not old bugs; they are points the first draft stated loosely
or wrongly. Slices must build exactly these:

| Row                                | Rule to build                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| CYC-014                            | The project picker is shown on **every create**, including from a project page, and retargets the create to the picked project; it is hidden only on edit. §6.                                                                                                                                                                                                                                                                                       |
| CYC-020                            | The dialog's forward tab cycle after the autofocused title is description → dates → cancel → submit → project picker → title, wrapping inside the dialog; the picker is reachable and trapped, and is not second. §6.                                                                                                                                                                                                                                |
| CYC-017                            | The edit date check runs only when **both** dates are set and the range changed; clearing either date saves without a date check. §7, §14.                                                                                                                                                                                                                                                                                                           |
| CYC-002                            | An undated row renders **no date element** (not a dash); a one-sided range renders the single date; the ring for a zero denominator reads `0%`. §2, §17.                                                                                                                                                                                                                                                                                             |
| CYC-027                            | Archived reads are **member+**; activating an archived row opens its peek, while "Open in new tab" goes to the canonical detail address (gone-away for an archived cycle). The archived search box is collapsed on arrival and opens focused; the live box opens unfocused. §3, §9.                                                                                                                                                                  |
| CYC-029                            | Favorites are **member+ only**; guests get no star, not a disabled one. §11.                                                                                                                                                                                                                                                                                                                                                                         |
| CYC-031                            | Transfer has **two entry points** — the finished detail banner and an inline action on a completed list row — opening one shared dialog. Row gate: completed **and** `total - (cancelled + completed) > 0`. Banner gate: the same count **and** an empty `progress_snapshot`. The server writes the source snapshot on success. §12.                                                                                                                 |
| CYC-033                            | The analytics trigger opens the shared `AnalyticsEntry` mounted with the cycle scope (`Analytics for {project} in {cycle}`); NEWFRONT-738 keeps the dialog body. §13, §22.                                                                                                                                                                                                                                                                           |
| CYC-044                            | The offset chip renders for any **non-zero project offset** (no browser-zone comparison) and is suppressed by a zero offset, not by the `"UTC"` label. §17.                                                                                                                                                                                                                                                                                          |
| CYC-037                            | An empty description renders nothing and a missing lead renders an empty avatar with no name; no "No description"/"No lead" strings. §14.                                                                                                                                                                                                                                                                                                            |
| CYC-038                            | The sidebar Work items row and state-group total use the raw `completed_issues/total_issues` counters (cancelled included); only the ring excludes cancelled. §2, §14.                                                                                                                                                                                                                                                                               |
| CYC-041                            | A hero state-group click navigates to the cycle detail with that state group filtered (and persisted in rich filters). §15.                                                                                                                                                                                                                                                                                                                          |
| CYC-001                            | The list read sends **no `cycle_view`** (the backend treats it as `all`); `current` is the separate hero read, and the project feature flag is a separate boolean on the project payload. §2, §23.                                                                                                                                                                                                                                                   |
| CYC-026                            | Restore is a **direct menu action** with no confirmation dialog; only archive and delete open dialogs. A failure leaves the row archived and retryable (§9). §9.                                                                                                                                                                                                                                                                                     |
| CYC-022                            | Delete has two layers: the menu entry is shown to member+, but the server accepts the cycle owner/creator or a project admin only, so a non-owner member gets the permission message. §8.                                                                                                                                                                                                                                                            |
| CYC-043                            | The workspace page is **the upsell in every edition** in this checkout; no cloud list renders and there is no EE source to cite. A paid data view is a seam needing a row and scenario first. The inventory no longer claims a full view. §16, §24.                                                                                                                                                                                                  |
| CYC-014                            | The palette's "New cycle" command is an entry point (project in context, `cycle_view` on, member+); the work-item create modal is **not**. §6, §25.                                                                                                                                                                                                                                                                                                  |
| CYC-029                            | The star and palette write `POST WS/user-favorites/` `{entity_type:"cycle", entity_identifier, project_id, entity_data:{name}}` and `DELETE WS/user-favorites/{favoriteId}/`; the sidebar reads the same collection. §11.                                                                                                                                                                                                                            |
| CYC-007                            | The end-date filter section is labelled **"Due date"** (the oracle picks by that label); it filters the cycle's end date. §4.                                                                                                                                                                                                                                                                                                                        |
| CYC-001                            | `GET BASE/cycles/` and `GET WS/cycles/` answer **bare arrays**; the workspace route is a narrower projection without lifecycle, favorite, assignee, version or creator fields. Only `GET WS/active-cycles/` is cursor-paged. §23.                                                                                                                                                                                                                    |
| CYC-002                            | Rows: current row shows exactly one avatar (the creator) and no count; every other row shows creator **and** assignee avatars (members icon when none). Draft/upcoming show the count. Group headings are `Active cycle` / `Upcoming cycle` / `Completed cycle`; the last two carry their count inside the disclosure button, Active does not. §2.                                                                                                   |
| CYC-004, CYC-005                   | The live search box opens **unfocused** (no deferred focus "fix"); the archived box opens focused. The expanded input's placeholder is exactly `Search`. §3, §9.                                                                                                                                                                                                                                                                                     |
| CYC-006, CYC-007, CYC-008          | Filter labels are fixed: `Status of the cycle` with `In progress` / `Yet to start` / `Completed` / `Draft`; both date sections use `1 week from now` … `2 months from now` plus `Custom`; chips row has a `clear all` action. §4.                                                                                                                                                                                                                    |
| CYC-009, CYC-010, CYC-011, CYC-013 | Empty-state labels are fixed: `No matching cycles` + `Remove the filters to see all cycles` / `Remove the search criteria to see all cycles`; `Manage features`; `Set your first cycle`; `View other cycles`; archived zero is `No archived cycles yet`. §5, §9.                                                                                                                                                                                     |
| CYC-014, CYC-015, CYC-016          | Create dialog: header button `Add cycle`; heading and submit `Create cycle`; placeholders `Title`/`Description`/`Start date`/`End date`; create refuses a **one-sided** range inline (the endpoint demands both-or-neither) while edit accepts one; overlap toast is `Error!` + "you already have a cycle" + draft guidance. §6, §7.                                                                                                                 |
| CYC-017                            | Edit dialog heading and submit are `Update cycle`; success toast `Cycle updated successfully.`; untouched dates send no date check. §7.                                                                                                                                                                                                                                                                                                              |
| CYC-022                            | Delete dialog heading is `Delete cycle`; success `Cycle deleted successfully.`; a 403 shows the permission toast (`You don't have permission to perform this action.`), never `Failed to delete cycle`. §8.                                                                                                                                                                                                                                          |
| CYC-024, CYC-025, CYC-026          | Archive: hint `Only completed cycles can be archived`, dialog `Archive cycle {name}`, toast title `Archive success` + `Your archives can be found in project archives.`, and a return to the live list. Restore: direct action, `Cycle restored` + `The cycle has been restored.`, failure `Cycle restore failed`. §9.                                                                                                                               |
| CYC-028                            | Row-menu titles are fixed: `Edit`, `Open in new tab`, `Copy link`, `Archive`, `Delete` (draft/current) and `…`, `Archive` (completed) and `…`, `Restore` (archived); filtered entries keep that relative order. §10.                                                                                                                                                                                                                                 |
| CYC-029, CYC-030                   | Palette titles are `Add to favorites` / `Remove from favorites` and `Copy URL`; palette copy uses the current address, row copy the canonical path. §11, §19.                                                                                                                                                                                                                                                                                        |
| CYC-031                            | Transfer: dialog `Transfer work items`, placeholder `Search for a cycle...`, name + capitalised status pill per option, empty text `You don't have any current cycle. Please create one to transfer the work items.`, success `transferred successfully`, failure `Unable to transfer work items. Please try again.` **The server writes the source snapshot** after transfer; the row gate omits the snapshot clause, the banner gate adds it. §12. |
| CYC-033, CYC-034                   | The last breadcrumb crumb is the cycle switcher; the labels `Analytics` and `Add work item` are fixed. §13.                                                                                                                                                                                                                                                                                                                                          |
| CYC-037, CYC-038, CYC-039          | Sidebar labels are `Lead`, `Members`, `Work items`, `Points` (points estimate + not completed), measures `Work items` / `Estimates`, breakdown tabs `States` / `Assignees` / `Labels` with Assignees default. §14.                                                                                                                                                                                                                                   |
| CYC-041, CYC-042                   | Hero: group heading `Active cycle`, cards `Progress` and `Work item burndown` (with `Pending work items - {n}`), tabs `Priority work items` / `Assignees` / `Labels`; empty copy `Add work items to the cycle to view it's progress`; no-active explainer `No active cycle` with "today" in its description. §15.                                                                                                                                    |
| CYC-043                            | Workspace upsell: heading containing "snapshots", exactly **six** benefit cards, `Upgrade` anchor to the pricing URL with `target="_blank"`, header badge exactly `Pro`. §16.                                                                                                                                                                                                                                                                        |
| CYC-044                            | Two-sided spans always carry the year (`Sep 01 - Oct 18, 2026`); one-sided is `MMM dd, yyyy`; the chip text starts `UTC ` + signed offset; only a zero project offset suppresses it, and the viewer-zone comparison affects only the tooltip. §17.                                                                                                                                                                                                   |
| CYC-048                            | Narrow **list** layout menu offers exactly `List layout` and `Gallery layout` and keeps rows; narrow **detail** menu offers `List` / `Board` / `Calendar` and Board writes `kanban` to the cycle prefs. §21.                                                                                                                                                                                                                                         |

## 27. Row coverage index

Every inventory row is cited in the section below; each section names
its screens, states, rules and API calls.

| Row     | Section          |
| ------- | ---------------- |
| CYC-001 | §1, §2           |
| CYC-002 | §2               |
| CYC-003 | §1, §2           |
| CYC-004 | §1, §2, §9       |
| CYC-005 | §3               |
| CYC-006 | §4               |
| CYC-007 | §4               |
| CYC-008 | §4               |
| CYC-009 | §5               |
| CYC-010 | §1, §5           |
| CYC-011 | §2, §5           |
| CYC-012 | §5               |
| CYC-013 | §1, §5, §13      |
| CYC-014 | §6               |
| CYC-015 | §6               |
| CYC-016 | §6               |
| CYC-017 | §7               |
| CYC-018 | §6               |
| CYC-019 | §6, §7, §14, §18 |
| CYC-020 | §6, §7           |
| CYC-021 | §6, §20          |
| CYC-022 | §8               |
| CYC-023 | §8, §13          |
| CYC-024 | §9               |
| CYC-025 | §9, §10          |
| CYC-026 | §9, §26          |
| CYC-027 | §9               |
| CYC-028 | §10              |
| CYC-029 | §11              |
| CYC-030 | §11              |
| CYC-031 | §12              |
| CYC-032 | §1, §13          |
| CYC-033 | §13              |
| CYC-034 | §1, §13          |
| CYC-035 | §14              |
| CYC-036 | §14              |
| CYC-037 | §14              |
| CYC-038 | §14              |
| CYC-039 | §14              |
| CYC-040 | §14              |
| CYC-041 | §15              |
| CYC-042 | §15              |
| CYC-043 | §1, §16          |
| CYC-044 | §14, §17         |
| CYC-045 | §18, §12, §9     |
| CYC-046 | §19              |
| CYC-047 | §20, §2, §4      |
| CYC-048 | §10, §21         |

## 28. Parity driver contract

The oracle specs call a fixed surface on the parity driver. The cycle
methods in `apps/web_new/e2e/parity/drivers/web-new.ts` are `todo()`
stubs; each slice implements the ones its rows need, in that file, by
re-reading the old driver's _intent_ (which is reference-only) and
finding the equivalent in the new UI. Do not change a method's
signature or its return shape to suit the new markup: the specs are the
contract. Where a stub's current selector cannot work in `web_new`,
implement it from the semantics in this spec (accessible names, roles,
text) rather than loosening the spec's assertion.

What the specs depend on, and therefore what the slices must render:

- Group headings are disclosure **buttons** whose text starts with
  `Active cycle` / `Upcoming cycle` / `Completed cycle`; Upcoming and
  Completed end with their count, and `aria-expanded` reflects the
  group.
- Rows are anchors to the detail address with the cycle name in a
  truncation span; the hover/inline eye control opens the peek, whose
  panel carries the name in a heading.
- The search control is a toggle that expands a text box whose
  placeholder is `Search`; the archived one focuses on open and the live
  one does not.
- The filter menu exposes section headings and options by the labels
  pinned in §4 and §26; chips are removable and a `clear all` control
  resets them.
- Dialog headings, submit labels, placeholders, toast titles and toast
  messages are the strings pinned in §26; drivers wait on those exact
  strings or case-insensitive phrase matches.
- The create form's focus trail is `Title`, `Description`, `Start date
End date` (one range control), `Cancel`, `Create cycle`, with the
  project picker before the title in DOM order and focus trapped inside
  the dialog.
- The sidebar is a panel with a stable width class-independent hook,
  a heading, and `role="tab"` controls for `States` / `Assignees` /
  `Labels`; its measure trigger is a button named `Work items` or
  `Estimates`.
- The hero renders a group button starting `Active cycle`, a card with
  the `Progress` heading, a card with a `burndown` heading and a
  `Pending…` line, and a tablist with `Priority work items` /
  `Assignees` / `Labels`.
- The workspace actives page renders an `h2` containing "snapshots", six
  `h3` benefit headings, an `Upgrade` anchor, and a `Pro` badge.
- The narrow list menu yields exactly `List layout`, `Gallery layout`;
  the narrow detail menu yields `List`, `Board`, `Calendar`.
- The transfer dialog uses a `Search for a cycle...` placeholder and
  destination buttons named after the cycle; the empty-state sentence is
  the one pinned in §12.

Anything this section pins that is missing from an earlier section is a
requirement, not a description of the reference: build to §26 and this
section together.
