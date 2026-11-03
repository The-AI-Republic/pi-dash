# Project settings — General tab: area spec (NEWFRONT-566)

Rows PSET-005–012. Written from reading the old General form, its
loader, and the danger-zone controls — plus the inventory Acceptance
column and the general, artwork, execution and repository oracle
scenarios. No old code, strings, styles or assets reused. Oracle
scenarios are written from this spec and proven against the running
old app.

Target rows (inventory describes the post-main-PR target, NEWFRONT-156
rule): PSET-005 renders no cover banner and the description field
carries its placeholder (targets per main#523 and main#503); PSET-007
is emblem upload only, with no cover control and no gallery search
(target per main#523). The current old app still shows the cover
artwork; the new surface must not.

## General tab view (PSET-005)

The tab renders the project emblem control, the name field, the
description field, the repository URL row with its bind control, the
base-branch field, the execution section (target picker, ticking
switch, budget and cadence), the short-identifier field, the
visibility-network picker, the timezone picker and the creation
timestamp, followed by the save control and, for admins, the danger
zone. No cover banner renders anywhere on the tab (target per
main#523). The description input shows its guidance placeholder (target
per main#503). While details are absent a skeleton stands in with
block placeholders shaped like the emblem, name, description,
identifier/network and save rows.

The page has no denial view. Everyone sees the same tab; non-admins
get the whole form dimmed with every input disabled and the danger
zone hidden (admin-only). Outsiders who cannot access the project get
the shell restriction screen (PSET-003), not a tab-level refusal. All
fields render from the same detail fetch as PSET-001
(`GET /api/workspaces/{ws}/projects/{pid}/`).

## Edit, save and validation (PSET-006)

Submit sends `PATCH /api/workspaces/{ws}/projects/{pid}/` with the
name, network, identifier, description, emblem object, timezone,
base branch, execution target, ticking flag, run budget and cadence
fields (review/test cadence columns ride along only when the cadence
picker moved). Success confirms and stays on the tab; navigation and
headers follow the rename through the shared project record. Failure
keeps the form open with an error notice in every case.

Name is required and capped at 255 characters; clearing the name
blocks the save with a field error, as does clearing the identifier.
The identifier is required, one to ten characters, stripped to
alphanumerics and uppercased as typed while still accepting
non-Latin letters. The base branch caps at 128 characters of
letters, digits, dot, underscore, slash and dash. The repository URL
caps at 512 characters but never rides the normal save (it binds
through PSET-008 only).

The identifier availability precheck fires only when the identifier
changed: `GET /api/workspaces/{ws}/project-identifiers` with the
`name` query parameter, consulting the `exists` flag in the response.
A hit sets a field error and blocks the save. The endpoint answers
from the reservation table rather than live membership, so its
all-clear does not guarantee the write succeeds: a reserved sibling
identifier stops early with a field error, while a reservation-less
seeded identifier passes the precheck and collides at write time,
surfacing an already-taken notice instead. Spec and test text must
never present the precheck as authoritative.

Write-time name and identifier collisions are parsed out of the error
payload by distinct taken-codes, each producing its own field-level
refusal; anything else gets a generic failure notice.

## Emblem artwork (PSET-007)

The emblem picker swaps the project mark into the form — dirtying it
without saving on its own — and the save persists it inside the
emblem object of the project update. There is no cover control and no
gallery or provider search anywhere in the flow (target per main#523,
verified against the cover-removal branch): artwork is a user-asset
upload for the emblem only, confirmed through the asset pipeline
before the save completes. For context, the current old picker
offers a bundled gallery (a pick applies straight into the form), an
upload path, and a provider search only when the instance configures
one — all removed by the target. The creation dialog's artwork path
is covered under PSET-013 (creation-dialog oracle scope NEWFRONT-390).

Failure semantics carry over from the observed flow: a failed emblem
upload aborts the whole settings save with an error notice — the form
stays open and neither the emblem nor any accompanying field persists.
On the creation path the same failure only warns and continues
(PSET-013).

## Repository binding (PSET-008)

The repository row is admin-only and lives outside the normal save:
typing never dirties the submit payload and only the bind control
persists. Empty input is refused client-side with an error notice and
no request.

The form fetches binding status with
`GET /api/workspaces/{ws}/projects/{pid}/repository/` and shows a
disabled bound state exactly when the status says bound and the
trimmed input equals the stored canonical repository URL (blank never
counts); otherwise an enabled bind control shows. Binding posts
`POST .../projects/{pid}/repository/bind/` with `repo_url`, then
writes the returned canonical URL back into the project via the
standard project update and the form field, revalidates status, and
points the operator at enabling sync in repository settings. The
service layer also carries legacy provider-specific equivalents and a
sync-toggle/unbind pair used by the repository tab (PSET-051); the
General tab uses only the generic binding family.

Pinned bug NEWFRONT-395: on a stack with no git provider connected
the bind refusal returns unauthorized and trips the global session
handling, which reloads the page instead of rendering the intended
bind-failed notice — the failure surfaces as silence with no state
change. Slices must preserve this divergent case as the oracle pins
it.

## Execution target and automated-run policy (PSET-009)

The target picker offers the local runner, the cloud agent family and
the desktop agent family, persisted as the default executor. The local
runner is always selectable as the unconditional fallback; the other
two disable from the per-family availability flags arriving with the
project details. Switching persists through the normal save and
affects new runs only — work created before the switch keeps its
original executor.

The master ticking switch defaults on; switching it off leaves the
budget and cadence controls visible but inert, and nothing validates
while it is off. Human-started runs are unaffected by the switch. The
run budget stores whole ticks with a no-cap sentinel: the number
input carries a native floor of one that blocks submit with no notice
and no request, while a cleared field reaches form validation and
stops the save with a field error; the no-cap option writes the
sentinel while remembering the last typed capped value to restore on
untick, and a blank budget falls back to the default on save. The
cadence picker offers presets at half an hour through a day; any
out-of-band value renders as a custom marker. Picking a cadence
writes it to every ticking stage, while any other save leaves
per-stage rhythms set elsewhere untouched — with one wrinkle: dirty
flags survive a save within one form session, so a cadence touched
earlier in the same session still propagates on the next save, and
only a fresh load isolates an unrelated save. New budgets and
cadences apply from the next tick; already-paused or already-scheduled
items are untouched.

## Danger zone: archive and restore (PSET-010, PSET-011)

The admin-only danger area stacks an archive control above the delete
control. Opening the archive control shows a shared confirm dialog
used for both directions, with a flag selecting archive versus
restore mode. The dialog names the project in its heading, carries a
short explanatory paragraph that differs by mode (archiving takes the
project plus its nested planning objects, hides its items from search
and reserves restoration to admins; restoring reactivates the project
and makes it visible to members again), and offers dismiss plus a
confirm control whose label tracks progress while the request runs.

Archive posts `POST /api/workspaces/{ws}/projects/{pid}/archive/` and
restore deletes the same path. On success the view confirms, closes
the dialog and lands on the workspace project list; the record stamps
or clears the archived timestamp and favorites bookkeeping drops or
restores the entry. The archived project leaves navigation and the
default card grid but stays reachable by direct settings address;
search-side and edit-side rejection are server-enforced (its items
leave search results and its updates are refused). The dialog itself
is always opened in archive mode; restore is reached from archived
cards through admin-gated entries reusing the same dialog in restore
mode. No client-side guard blocks archiving the workspace default
project; default protection exists only on delete (PSET-012).

## Danger zone: delete (PSET-012)

The delete control opens a dedicated modal bound to the current
project: a permanence warning, then two labeled inputs — first the
project name, then a fixed confirmation phrase. Submit stays disabled
until both match exactly: the first against the project name
character-for-character, the second against the fixed lowercase
phrase (the modal states the literal as the required input and as its
own placeholder hint), with the handler re-checking the gate and the
workspace slug before calling.

On success the record is removed, matching favorites and permission
entries are cleared, and the view lands on the workspace project list
when inside that project (otherwise it stays and closes the modal)
with a success notice. A later fetch of the id fails; favorites carry
no entry for it. Deleting the workspace default project is refused
server-side with a bad-request error naming it as non-deletable, and
the project stays fetchable afterwards — there is no client-side
guard or message, the button stays enabled and the server decides.

## Reuse map for the General slice (NEWFRONT-572)

Exists in apps/web_new and reusable as-is: project detail reads and
the project update, identifier check, archive, restore and delete
mutations in the shared API client, plus the archive and delete
dialogs. New for this slice: the General form with all fields and
validation, the emblem upload control, the repository bind row, the
execution section, and the danger-zone rows wiring the existing
dialogs and mutations.
