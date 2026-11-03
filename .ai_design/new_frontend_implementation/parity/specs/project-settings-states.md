# Project settings — workflow states: area spec (NEWFRONT-566)

Rows PSET-028–035. Written from reading the old state group, list,
row, form, options and loader components — plus the inventory
Acceptance column and the states oracle scenarios. No old code,
strings, styles or assets reused. Oracle scenarios are written from
this spec and proven against the running old app.

Observed behavior the oracle pins as bugs is described as observed
and named with its issue (duplicate-rename path NEWFRONT-402, silent
move/promote rollback NEWFRONT-403, never-rendering skeleton
NEWFRONT-405). The new surface keeps the observed behavior except
where a bug issue says otherwise. Two inventory wordings are
corrected by observation here: the review group starts collapsed
rather than expanded, and the loading skeleton never renders during
fetch — slices must follow the observed behavior the oracle pins.

## Group cards and read-only mode (PSET-028)

Exactly seven groups exist — backlog, unstarted, started, review,
test, completed and cancelled — with no group create, rename or
delete anywhere; missing groups backfill as empty lists so all seven
cards always render. Each group is a bordered card with a collapse
control, a per-group glyph and the group name. Cards start expanded
except review, which starts collapsed. Each state row carries a
share-of-group indicator computed as its position in the group over
the group size, feeding the group glyph. No triage concept exists
anywhere client-side; the ban on triage-group creation is enforced
server-side only.

Edit rights derive at the tab root: admins, or members while the
project's members-may-edit-states flag is on (defaulting to on until
project details land). Read-only rows hide the hover grip and the
edit, delete and mark-default controls; the per-group add control
disables; dragging is refused. Outsiders who are neither admin nor
member never get the list — the page swaps in a not-authorized view.
The list reads
`GET /api/workspaces/{ws}/projects/{pid}/states/`, one call per
project cached by project id with no refetch on focus.

## Inline creation (PSET-029)

The add control sits in each group card header and disables while
that group's form is open. Opening it expands the group and docks a
form under the list: swatch picker, name field, description field,
create and cancel controls. Name is required (an inline field error,
form stays open, nothing sent) and capped at 100 characters by the
input; the fresh form pre-tints the swatch with the group-convention
color. Submit posts
`POST /api/workspaces/{ws}/projects/{pid}/states/` with name,
description, color and group; success confirms and closes. A
duplicate-name refusal keeps the form open with an already-exists
notice; other failures get a generic creation notice. Nothing may be
created into the triage group — the server refuses it. The submit
controls never show a pending state while the request runs.

## Edit in place (PSET-030)

The row edit control swaps the row for the same shared form,
prefilled with the state's name, description and color. Saving sends
`PATCH .../projects/{pid}/states/{sid}/` with the form fields;
cancel discards edits and restores the row with no call. Success
confirms and closes back to the row.

Pinned divergence NEWFRONT-402: the duplicate-rename path is
unreachable as designed — the store's patch helper rejects with the
bare response body, which carries no numeric status for the
wrapper's status branch, so a duplicate rename falls into the
generic-failure branch; the optimistic rename additionally remounts
the row (rows key by state name), closing the open form. The data
itself rolls back correctly. Slices must preserve the observed
generic notice plus form-close behavior as pinned.

## Drag reorder and regroup (PSET-031)

Rows drag by a hover grip onto above/below placements with a visible
insertion marker at the hovered row's edge; the dragged row dims
while dragging. A group holding exactly one state is not draggable
at all, and all dragging is off in read-only mode. On drop the client
sends `PATCH .../projects/{pid}/states/{sid}/` with the id, the
destination group key (cross-group moves just set the new group) and
an ordering value derived from the destination row's neighbors
(midpoint between adjacent values mid-list, below the current value
past the end, above the first value at the top, a large default step
when no edge is known). Server order within a group is ascending by
that value; across groups by the fixed group order.

Pinned divergence NEWFRONT-403: the move helper snapshots the whole
state map by live reference, so its failure restore is a no-op, and
the drop path logs without any user notice — a refused move keeps
the dropped position silently while the server row is untouched.
Slices must preserve this silent behavior as pinned.

## Delete confirmation and refusals (PSET-032)

Deletion confirms through a dialog naming the state and warning the
removal is permanent: cancel leaves everything untouched, confirm
sends `DELETE .../projects/{pid}/states/{sid}/` and removes the row
on success. The delete control disables with an explanatory note
when the state is the project default or the only state left in its
group (groups may never be emptied). There is no reassignment step
in the dialog. A state still holding work items passes the client
guards but the server refuses, and the client answers with a notice
telling the operator to move the items to another state first; any
other failure gets a generic deletion notice. The row survives in
both cases.

## Mark default (PSET-033)

Exactly one default exists per project, set with an empty-body
`POST .../projects/{pid}/states/{sid}/mark-default/`. The current
default renders a static marker instead of an action; every other
row offers a promote action disabled while a request runs. The
client flips the flags optimistically (old default cleared, new one
set) and attempts to restore on failure. The default state cannot be
deleted (PSET-032 reads the same flag, so a wrongly marked default
also blocks deletion in the view until refresh).

Pinned divergence NEWFRONT-403: the promotion path shares the
live-reference snapshot flaw, so its rollback is a no-op and the
failure branch shows no notice — a refused promotion keeps the stale
marker silently while the server still names the previous default.
Slices must preserve this silent behavior as pinned.

## Member-editing toggle (PSET-034)

An admin-only boxed control above the list flips the
members-may-edit-states flag through the project update with the
negated flag, wrapped in pending/success/failure notices; the toggle
reads back immediately on success, while a failure notices and the
switch stays where it was. The tab root derives edit rights as admin
or (member and flag); members never see the toggle. The server
mirrors the gate: member writes are refused while the flag is off
and allowed while on. Unauthorized writes are refused server-side in
both modes.

## Loading, empty groups and error handling (PSET-035)

The four-row skeleton loader is gated on the grouped-states
selector being empty — but that selector returns a value as soon as
a project id exists, before any fetch completes — so the skeleton
never renders during fetch (pinned as NEWFRONT-405). Cold loads
render the cards immediately with empty lists and per-group empty
notes, then populate when the fetch lands. Each emptied group shows
its own empty note naming the group plus a create prompt for
editors. All other failures are notice-only with no error panels:
duplicate creates keep the form open, single-record updates restore
correctly, deletes never remove the row until success — except the
silent NEWFRONT-403 paths (move, promote) and the generic-message
NEWFRONT-402 path (duplicate rename).

## Reuse map for the states slice (NEWFRONT-598)

Exists in apps/web_new and reusable as-is: the state list read in
the shared API client. New for this slice: the grouped cards with
expand/collapse and share indicators, the docked creation form, the
row edit swap, drag reorder with neighbor-derived ordering, the
delete confirmation with its guards, the mark-default action, the
member-editing toggle, and the write contracts, which belong to the
contracts issues (members, states and labels writes).
