# Project settings — members: area spec (NEWFRONT-566)

Rows PSET-018–027. Written from reading the old member list, table
row, column, sort, picker and dialog components — plus the inventory
Acceptance column and the members oracle scenarios. No old code,
strings, styles or assets reused. Oracle scenarios are written from
this spec and proven against the running old app.

Observed behavior the oracle pins as bugs is described as observed
and named with its issue (role-change display versus write outcome,
NEWFRONT-401; failed removal closing over the page, NEWFRONT-406).
The new surface keeps the observed behavior except where a bug issue
says otherwise.

## Member table (PSET-018)

The tab renders one table row per member with five columns: full
name (avatar plus first and last names), display name, email, account
type as the role word, and joining date formatted from the
membership creation timestamp. Rows read from
`GET /api/workspaces/{ws}/projects/{pid}/members/`. Non-human
memberships are filtered out of the table (a bot membership never
renders as a row). The page stacks three blocks: the defaults
section (PSET-026), a teamspace strip, and the member list with its
search, filters, add control and table.

Cross-epic seam: both avatar states (photo and initial fallback)
link out to the member profile route under the workspace. This area
builds the table cell and preserves the link target; the profile
area owns the destination — its address stability and what it
renders. At gate time this area proves the link target is present on
every avatar and treats the profile page itself as an outbound
integration owned elsewhere.

The page renders a not-authorized view unless the viewer is a
project admin, project member or workspace admin — project guests
and workspace outsiders both get the denial even though the sidebar
shows guests the Members entry (PSET-004) and guest list reads
succeed server-side. The page gate is therefore UI-only on reads.

## Search, filter and sort (PSET-019, PSET-020, PSET-021)

Search (PSET-019) is purely client-side over the already-fetched
memberships, matching display name and full first-plus-last name
case-insensitively with zero member-list requests per keystroke;
clearing restores the full roster. The role filter (PSET-020) is a
dropdown with one checkbox per role; checked roles narrow the table
and mark the trigger button with an active indicator, and unchecking
restores everything and clears the indicator. Sort (PSET-021) lives
in every column header menu with ascending, descending and (once
sorted) clear choices, and a direction marker on the active column
only; sorting keys are per-field (name parts and email lowercased,
join date with missing values treated as oldest, role word).

Persistence for filter and sort is an in-memory per-project map:
choices survive in-app tab visits within the session and a full
reload resets to the default order. Spec and test text must never
claim reload survival.

## Add members (PSET-022)

An admin-only control opens a dialog starting with one row; rows can
be added or removed past the first, each pairing a co-worker picker
with a role dropdown. The picker lists workspace members excluding
anyone already holding a project membership, searchable by name
parts and display name. Picking a co-worker defaults that row's role
to the person's workspace role; offered roles follow the co-worker's
workspace role (workspace guests enter at guest level only,
workspace admins at admin level only, workspace members get every
role). A nominal per-row cap against the inviter's own project role
exists in the dialog logic but never binds — only project admins
reach the dialog and admin outranks every option — so the effective
contract is no inviter-level cap, and the create endpoint applies
none either. Submitting a row with an empty picker fails per-row
validation and keeps the dialog open; closing resets the dialog back
to a single blank row shortly after.

Submit bulk-adds all rows at once with
`POST /api/workspaces/{ws}/projects/{pid}/members/` carrying the
`members` array of member-plus-role pairs, then confirms and closes.
A failed submit is silent apart from a console entry — no failure
notice renders. A single-member read is
`GET .../projects/{pid}/members/{mid}/`.

## Inline role changes (PSET-023)

A row's role is editable when the editor is a workspace admin
editing themselves, or a project admin editing someone who is
neither themselves nor a workspace admin; everyone else sees plain
role text, and guest rows offer only the guest option even to
admins. The change sends
`PATCH .../projects/{pid}/members/{mid}/` with the `role` field
(note: membership id, not user id), applied optimistically and
rolled back on failure, with a failed change toasting the server
message durably while the table keeps the row. The server
additionally refuses promotions to a role at or above the editor's
own level. There is no last-admin guard: the sole admin can demote
themselves and restore standing through the same cell.

Pinned mismatch NEWFRONT-401: a workspace admin is shown the editor
on every row except another workspace admin's, but only their own
write lands — other rows' writes are refused with a permission-error
notice. Slices must preserve the observed display-versus-write split
as the oracle pins it. Server quirk NEWFRONT-400: the role endpoint
computes its workspace-admin bypass from the target's workspace role
instead of the requester's, so a write against a workspace-admin
target currently succeeds regardless of who sends it — while the UI
correctly shows those cells read-only. Slices must match the
observed server behavior as pinned, not the stricter rule.

## Remove and leave-self (PSET-024, PSET-025)

A hover menu on the name cell offers Remove for others (admins) and
Leave on one's own row. Remove confirms through a dialog warning
that access is lost permanently and naming the member; cancel keeps
the membership, confirm sends
`DELETE .../projects/{pid}/members/{mid}/` (membership id). Removal
is a server-side soft deactivation: the row disappears and the
membership no longer resolves. The dialog closes unconditionally
after submit settles, even when the removal call failed — only the
notice signals the failure.

Leave-self reuses the same dialog with leave-specific wording;
confirm calls
`POST .../projects/{pid}/members/leave/` and on success routes to
the workspace project list. Failures notice and membership stays.

Failed removal, observed (bugs NEWFRONT-406, NEWFRONT-398): in the
old app, submitting a removal for a membership already deleted out
of band makes the DELETE answer 404, which is followed by a burst of
further 404s and then the page goes away with no navigation and no
crash signal; even an ordinary refused removal only flashed its
notice while the confirm dialog came down. Intended, and what the
new surface does: a refused removal is one request whose refusal is
shown, nothing is re-read or re-routed, the confirmation withdraws
on settle (success or failure), the table keeps its rows, and the
tab stays usable. The notice names the action and carries the
server's message, and stays up after the dialog is gone. The oracle
still pins removal failure through the role-change path and does
not submit a doomed removal, because the old app cannot survive
it; if that door is ever opened by an old-app fix, a
removal-failure step can join PSET-027.

## Project defaults (PSET-026)

The defaults section offers a lead picker and a default-assignee
picker over current project members excluding guests, plus a toggle
letting guests view all project work items. Either picker offers a
none choice clearing back to null; every pick or clear saves
immediately through the project update with the lead, assignee or
guest-visibility fields and confirms. The guest-visibility flip takes
effect as viewing rights for guest roles. All three controls stay
visible but disabled for non-admins; while project details load,
loaders stand in for the controls.

## Loading and failure states (PSET-027)

A skeleton stands in while the roster is unfetched; the General tab
carries its own analogous skeleton (PSET-005). Search/filter
combinations matching nobody show a centered no-match notice with no
extra action; clearing restores the rows. A refused role change
toasts the server message and preserves the table with the
optimistic change reverted. The invite dialog resets to one blank
row after close (a very fast reopen can briefly show stale rows);
the remove/leave dialog resets its loading state on close.

## Reuse map for the members slices (NEWFRONT-574 read, NEWFRONT-579 writes)

Exists in apps/web_new and reusable as-is: the member, state and
label list reads plus the workspace-member list in the shared API
client, and the avatar-plus-name presentation components. New for
these slices: the settings member table with search, role filter
and sort; the multi-row invite dialog; the gated inline role
editor; the remove/leave confirmations with post-leave routing; the
defaults section; and the write contracts, which belong to the
contracts issues (members, states and labels writes).
