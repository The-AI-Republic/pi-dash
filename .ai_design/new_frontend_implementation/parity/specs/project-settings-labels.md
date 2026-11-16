# Project settings — labels: area spec (NEWFRONT-566)

Rows PSET-036–042. Written from reading the old label list, group,
row, inline-form, delete-dialog and drag components — plus the
inventory Acceptance column and the labels oracle scenarios. No old
code, strings, styles or assets reused. Oracle scenarios are written
from this spec and proven against the running old app.

Observed behavior the oracle pins as a bug is described as observed
and named with its issue (silent failed drops, NEWFRONT-408). One
inventory wording is corrected by observation here: no automatic
scrolling exists in the drag implementation, so slices must not
build auto-scroll and must follow the observed behavior the oracle
pins.

## Two-level hierarchy (PSET-036)

Labels form a tree at most two levels deep, derived client-side from
the flat list through parent links. Parents render as expandable
group cards starting open, with a grouping glyph tinted in the
label's color and their children indented beneath; plain labels
render as rows with a round color dot. Cards collapse down to zero
visible children and expand back. Hierarchy is created only by
dragging one label onto another (PSET-039): drop targets that are
themselves children refuse nesting, and group sources can never
become children, so depth never exceeds parent plus children.

Unlike states there is no member-edit flag: editing requires admin
standing. Non-admins get no add control, no drag grips, no row menus
or quick-delete — the drag layer does not even bind for them — while
reading the same list. The list reads
`GET /api/workspaces/{ws}/projects/{pid}/issue-labels/`, sorted by
the ordering value, with the tree derived locally.

## Inline creation (PSET-037)

The header add control (admin-only) opens one inline form at the top
of the list: swatch button with a palette popover, title field,
cancel and confirm. A fresh form seeds a random starter swatch from
the fixed palette. Title is required and capped at 255 characters,
both enforced as inline form errors that keep the form open. Submit
posts `POST .../projects/{pid}/issue-labels/` with name and color
(top-level; no parent is sent). Success closes and resets the form
silently — this tab toasts no success on create. A duplicate title
surfaces an already-exists notice with the form kept open and values
preserved; other failures surface the server detail or a generic
failure notice, also keeping the form open with nothing persisted.

## Rename and recolor (PSET-038)

The row menu's edit entry swaps the row for the same inline form
prefilled with the label's current name and swatch. Saving sends
`PATCH .../projects/{pid}/issue-labels/{lid}/` with the form fields;
cancel restores the row with no call. Duplicate renames are refused
with a same-name notice and the form stays open with values
preserved; other failures get a generic update notice. Contrast with
states (PSET-030): the label form survives a duplicate rename
because rows key by id and the failure branch restores the attempted
values.

## Drag arrange (PSET-039)

Dragging is handle-based with a preview chip; drop targets are whole
group cards and top-level rows, with child rows resolving through
their parent's target. Reordering shows an insertion line; nesting
shows a highlight ring around the prospective parent. Observed rules:
plain labels may reorder among top level, nest under a parent, or
move between parents; whole groups reorder at top level carrying
their children and never nest (a group source forced into a nest
instruction converts to reorder-above); drops onto self are
rejected; a parent dropped onto its own child is rejected; drops
onto a child row land as siblings under that child's parent.
Dragging is fully disabled for non-admins.

The drop payload is parent plus neighbor-derived order:
`PATCH .../projects/{pid}/issue-labels/{draggedId}/` with the target
parent id (or null for top level) and an ordering value computed from
surrounding siblings — midpoint when both neighbors exist, half of
next when first, previous plus a fixed step when last; when the
destination list is empty no order is sent and the server assigns
one. A drop with no movement inside the same parent is skipped
client-side with no call. No automatic scrolling exists anywhere in
the drag implementation: long lists do not auto-scroll during a drag.

Pinned divergence NEWFRONT-408: the drop handler fires the position
save without awaiting or catching it, so a refused save rolls the
optimistic change back silently with no notice (the restore itself
works; only the notice is missing). Slices must preserve this silent
rollback as pinned.

## Detach to top level (PSET-040)

A child row's menu offers a detach entry visible only when the label
has a parent. Choosing it sends
`PATCH .../projects/{pid}/issue-labels/{lid}/` clearing the parent.
The label keeps its name and color, appears at top level, and stays
there on reload. No order value is sent, so placement falls to the
server default.

## Delete confirmation (PSET-041)

Deletion confirms through a dialog naming the label and warning it
is stripped from every carrying work item and from any filtering
saved views, with no undo and no merge-or-replace step. Cancel leaves
everything untouched; confirm sends
`DELETE .../projects/{pid}/issue-labels/{lid}/` and removes the row
on success — confirmed deletion strips the label from its work items
(the carrying issue reads back with an empty label list). A refused
delete notices, keeps the dialog open and keeps the label. Plain rows
expose both a quick-delete control and the menu path; group cards
delete through their menu.

## Loading and empty states (PSET-042)

While the label list is unfetched the tab shows a multi-row
skeleton; opening the creation form suppresses the skeleton even
mid-load (the loader branch is skipped whenever the form is open).
A project with no labels shows an illustrated empty panel with a
create action opening the form; once any label exists the panel
disappears, and opening the form hides the panel. All failures are
notice-only: refused creates notice and keep the form open with
values preserved, nothing persists server-side, and no error panel
state exists on this tab.

## Reuse map for the labels slice (NEWFRONT-601)

Exists in apps/web_new and reusable as-is: the label list read in
the shared API client. New for this slice: the two-level hierarchy
rendering with expand/collapse, the inline create/edit form with
swatch palette, handle-based drag with insertion-line and ring
feedback plus the parent/order payloads, the detach action, the
delete confirmation, and the write contracts, which belong to the
contracts issues (members, states and labels writes).
