# Area spec — project settings, members tab (PSET-018–027)

Source: NEWFRONT-391 oracle research. Behavior paraphrased from the old
frontend's members settings page and its member list, row, invite-dialog,
remove-confirm and defaults components, plus the member service endpoints.
No code, selectors, copy or structure are carried over; the new frontend
is written from this prose.

## Page composition and gating

The members settings page stacks three blocks: a defaults section (lead,
default assignee, guest access), a teamspace strip, and the member list
(search, role filters, admin add control, table). Viewing requires project
member standing or better, with workspace admins always admitted; project
guests and outsiders see the not-authorized view instead of the table.
The general tab stays visible to every role, but this members tab does not.

## Member table (PSET-018)

One row per project member with five columns: full name (given plus family
name beside an avatar that links to that member's profile, falling back to
an initial when no portrait exists), display name, email address, account
type (the project role rendered as a word), and joining date in the app's
standard date shape. The header row carries a per-column sort menu.

## Search (PSET-019)

The search box filters the already-loaded roster in the browser, matching
case-insensitively against display name and full name. Typing issues no
requests; clearing restores the full list. An empty result shows a centered
no-match notice rather than an empty table.

## Role filter (PSET-020)

A Filters dropdown offers one checkbox per project role (admin, member,
guest). Choices persist per project across visits, and the button shows an
active indicator while any filter applies.

## Sorting (PSET-021)

Each column header menu offers ascending, descending and clear choices.
The choice persists as a display filter and the header shows a direction
indicator while its column drives the order.

## Adding members (PSET-022)

An admin-only add control opens an invite dialog with one or more rows;
each row pairs a co-worker picker (workspace members outside the project;
current members are excluded) with a role picker. Picking a co-worker
defaults that row's role to the co-worker's workspace role. Offered roles
follow the co-worker's workspace role (workspace guests may only enter at
guest level, workspace admins only at admin level, workspace members get
every role); no cap against the inviter's own level exists in the dialog
or on the create endpoint (NEWFRONT-399). Rows can be added and removed;
every row needs both a member and a role before submit. One submit
bulk-adds all rows, confirms with a success notice, and closes; closing or
submitting resets the rows. A failed submit is silent apart from a console
entry (no failure notice).

## Inline role change (PSET-023)

The account-type cell is an inline role editor only for the permitted
cases: a workspace admin's effective project role is admin, so the editor
shows on every row except another workspace admin's (NEWFRONT-401), while
a project admin without workspace standing sees it on every row except
their own and except a workspace admin's. All other viewers see read-only
text. The editor offers every role except for workspace-guest targets,
who are capped to the guest option. The server governs writes by the real
project role, so a workspace admin's writes against others' rows fail and
only the own-role write lands; the server additionally refuses promotions
to a role at or above the inviter's own level. There is no last-admin
guard (the sole admin may demote themselves). Failures surface the
server's message in an error notice.
Server quirk (NEWFRONT-400, do not reproduce): the role endpoint computes
its workspace-admin bypass from the target's workspace role instead of the
requester's, so anyone's write against a workspace-admin target currently
succeeds; the UI correctly shows those cells read-only.

## Removing a member (PSET-024)

Hovering a row reveals a contextual menu (visible to admins, and to anyone
on their own row). Choosing the remove entry opens a confirmation naming
the member and warning that project access is lost permanently with no
undo. Confirming deactivates the membership softly (server-side record
kept); failures toast.

## Leaving (PSET-025)

One's own row offers leave-worded entries instead of remove-worded ones,
explaining that rejoining is possible by invitation or for public
projects. Confirming calls the leave endpoint (not the remove endpoint)
and then routes to the projects list. Failures toast and the membership
stays.

## Project defaults (PSET-026)

Two pickers choose the project lead and the default assignee from current
non-guest project members, each with a None entry that clears the slot.
Every change saves immediately and confirms with a success notice. A guest
access toggle lets guests view all project work items. Non-admins see all
three controls disabled (pickers inert, toggle off-limits); while details
load, loaders stand in.

## Loading and failure states (PSET-027)

While the member ids are unset, a skeleton stands in for the table. Search
or role filters that match nobody show the centered no-match notice. A
failed role change raises a durable error toast carrying the server
message without losing the table. A failed removal is worse: after the
server refuses, the page fires a burst of further failing requests and
then dies (the page closes with no navigation or crash event; NEWFRONT-406)
— its error toast is only ever a flash on the way down (NEWFRONT-398).
The invite dialog resets its rows after submit or close.

## Observed edge notes for scenario authors

- The seeded stack carries an owner-admin (workspace and project admin),
  a second plain member and a project guest: enough distinct roles for
  filter, search, edit-permission and not-authorized scenarios without
  re-seeding. (A bot project membership exists but is filtered from the
  member list, so the table shows three rows.) The seeded owner doubles as
  the workspace-admin viewer; staging a project-admin-only or
  workspace-admin-only viewer needs temporary role changes through the API
  (member<->admin workspace moves leave project roles untouched), always
  restored afterwards.
- Adding and removing members mutates shared seed state, so those
  scenarios must restore the seeded membership through the API afterwards.
- The leave scenario ends its own membership and must re-add itself (or
  run as a throwaway user) so later scenarios see a clean stack.
