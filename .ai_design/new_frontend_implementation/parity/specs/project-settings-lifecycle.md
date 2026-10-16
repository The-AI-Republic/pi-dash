# Project settings — lifecycle: area spec (NEWFRONT-566)

Rows PSET-013–017. Written from reading the old creation dialog, the
join and leave dialogs, the publish dialog, and the list search — plus
the inventory Acceptance column and the lifecycle and danger-zone
oracle scenarios. No old code, strings, styles or assets reused.
Oracle scenarios are written from this spec and proven against the
running old app.

Target row: PSET-013 carries no cover input and the description field
shows its placeholder (targets per main#523 and main#503). The old
creation dialog still shows cover controls; the new surface omits
them, so no cover failure can occur there.

## Create a project (PSET-013)

Creation opens from the settings guidance view, the project-list
creation button and several other hosts through a shared open flag,
rendering a large top-anchored dialog: a header block (template
control, close control, emblem picker) above the common-attributes
block (name, short identifier, description, repository URL, base
branch) and a lower row (visibility-network selector, lead selector),
ending with cancel/confirm. Escape dismisses; close resets the form
after a short delay and re-arms identifier auto-sync. There is no
cover input (target per main#523) and the description input shows its
guidance placeholder (target per main#503).

Name is required and capped at 255 characters. While the auto-sync
latch is on, each name edit derives the identifier by sanitizing and
truncating to ten characters; clearing the name clears the identifier.
Any hand edit inside the identifier input permanently turns the latch
off for that dialog session, after which name edits no longer touch
the identifier. The identifier is required, one to ten characters,
validated client-side to an uppercase-alphanumeric pattern, and
uppercased before sending; the availability precheck from PSET-006
applies. Description is optional multi-line text. The repository URL
is optional and capped at 512 characters. The base branch is optional,
capped at 128 characters of letters, digits and a small separator
set. The emblem picker stores an emoji-or-icon object defaulting to a
random choice. The network selector defaults to the public option and
the lead selector to no lead. The visible template selector renders
no content; creation accepts template data but no template flow is
observed.

Valid input posts `POST /api/workspaces/{ws}/projects/` with the
whole form payload. Creation seeds the default workflow states
server-side, records the creator as admin from the creation response,
and ends with a success notice and the dialog closed — no
feature-selection step appears in the modal flow (the combined
feature list exists as a component but the create host wires its
next step to close, as the oracle pins). Duplicate names or
identifiers surface distinct already-exists notices from the
server-error codes. An artwork failure on this path warns and
continues rather than blocking, but with no cover input the case
cannot arise from the new dialog.

## Join a project (PSET-014)

Direct navigation to a project the caller cannot access renders the
access-restriction view (PSET-003). Its join variant offers a single
join action whose label switches to a progress form while pending;
no form or typed input exists and the only gate is the
disabled-while-joining flag. Confirming posts
`POST /api/users/me/workspaces/{ws}/projects/invitations/` with
`project_ids` holding the single project id, records membership
locally, refreshes workspace-level data, lifts the restriction in
place and reveals the project's issue board at the same address —
no navigation away occurs on this path. The restriction-screen join
carries no user-facing failure notice: the button clears its progress
flag when the call settles and failures are only logged, so spec and
test text must not promise an error notice here. (A parallel card-grid
join path confirms through a small dialog and routes to the issues
board; it is not the PSET-014 subject.)

## Leave a project (PSET-015)

The leave dialog mirrors the delete dialog structurally — warning
header, a paragraph noting associated items become inaccessible, then
two inputs for the project name and a fixed leave phrase — except its
submit control always renders enabled and validation happens inside
submit with error notices. Both entries must match exactly: the name
against the project's name, the confirmation against the fixed phrase
with capital initials. Wrong name, wrong phrase or empty submission
each produce a distinct error notice and no request, with membership
intact.

On correct entries the handler navigates to the workspace project
list first, then fires
`POST /api/workspaces/{ws}/projects/{pid}/members/leave/` with no
body, closing the dialog only after the write resolves and surfacing
an error notice on failure. The list-first ordering is observable:
with the write held, the list route lands immediately while
membership remains until the held write completes. A failed leave
after navigation therefore leaves the user on the list with
membership still present and only the notice as signal.

## Publish to a public board (PSET-016)

The publish dialog is a form with a header (heading plus an unpublish
control only once published), a content area and a footer noting link
access plus cancel and publish/update controls. Opening triggers a
settings read; while loading, a skeleton shows instead of content.
Defaults before load are both layouts on and all three toggles off.

The layouts control offers exactly two options — list and kanban
views — multi-select, with a guard refusing to deselect the last
remaining layout, and submitting with zero layouts selected is
refused with an error notice. Three independent toggles map to the
comments, reactions and votes flags. Settings persist through
`GET`/`POST /api/workspaces/{ws}/projects/{pid}/project-deploy-boards/`
and `PATCH`/`DELETE .../project-deploy-boards/{id}/` carrying the
toggle flags plus the layout selections.

Published status derives from the presence of the board anchor on the
stored settings. Once published the dialog shows a link row with
open-in-new-tab and copy actions plus a live indicator; copy writes
the link to the clipboard with a confirmation notice. For an
already-published project the form prefills from stored settings and
the update control renders only when the form is dirty; unpublishing
is a separate action with its own progress state, after which the
board reads back empty.

Cross-epic seam: this area builds the dialog, the settings reads and
writes, and the link-out actions — it does not build or preview the
public board itself. The live link targets the public-board surface
owned by the spaces epic; the spaces area owns the board rendering,
its address stability and what the link opens into. At gate time this
area proves the dialog end (loader, layouts minimum, toggles,
dirty-gated update, unpublish, link-plus-copy actions) and treats the
link destination as an outbound integration owned elsewhere.

## Project-list search (PSET-017)

The list header shows only a search icon button when collapsed.
Activating it expands an inline input and focuses it; a clear control
inside the field empties the query and collapses. Escape is
two-stage: with a non-empty query the first press clears the text and
keeps the box open, and the next press collapses; clicking outside
collapses only when the field is empty, and with text present the box
stays open. Typing filters locally with no requests: the visible list
is computed by matching the query case-insensitively against each
project's name and identifier, combined with workspace scoping and
the existing display filters. Spec and test text must never describe
a debounced server search here.

## Reuse map for the danger-zone plus lifecycle slice (NEWFRONT-596)

Exists in apps/web_new and reusable as-is: the create, archive,
delete, join, leave and publish dialogs, the projects list screen
with search and empty state, and the create, archive, restore,
delete, leave, join and favorites mutations in the shared API
client. New for this slice: the danger-zone rows wiring the dialogs,
the settings-side creation entries, the restriction-screen join
action, and any rework the target rows demand (cover-input removal
in the creation dialog per PSET-013).
