# Area spec — Project settings, General tab (PSET-005–012)

Source of truth for behavior: the running old app plus the old code paths
listed under "Reference entry points". This file is prose: what the screen
does, in which order, for whom. New code is written from this file, never
from the old files.

Target deltas (behavior the inventory names but unmerged main PRs still own):
main#523 removes the cover banner, gallery search and cover upload entirely;
main#503 changes the description placeholder. Until those merge, the oracle
pins the current behavior and cites the PR at each affected assertion.

## Reference entry points (read-only, never imported or copied)

General form and save flow; the settings page shell with the admin gate and
skeleton; the danger-zone rows with the archive and delete modems; the
project card that hosts restore mode; the image picker popover behind the
cover control; the cover-image helper that decides save-vs-warn.

## PSET-005 — view General tab

Any project role opens the general address and sees one long form. From top
to bottom: a cover banner with the project emblem overlapping its lower
edge, the project name and short identifier beside the visibility marker;
then labeled rows for project name, description, repository URL with its own
bind button, base branch, the execution-target picker, the automated-run
policy group (master switch, per-item budget with a no-cap toggle, cadence
picker), the short identifier, the visibility network picker, the timezone
picker, a submit button and a creation-timestamp line. While the details are
unresolved the tab shows a skeleton with the same skeleton blocks (banner,
emblem, name, description, identifier, network, submit). A viewer with no
access gets the restriction screen, not the form. TARGET (main#523): the
cover banner is gone everywhere on the tab. TARGET (main#503): the
description box carries the AI-context placeholder.

## PSET-006 — edit and save as admin; read-only for others

Admins edit every field and press the submit button; a valid save confirms
with a success notice, persists through the project update endpoint, and the
new name and identifier show up in navigation and headers. Name and
identifier are required: clearing the name blocks the save with a field
error; clearing the identifier blocks it too. The identifier field strips
everything but alphanumerics and uppercases as you type; it accepts
non-latin letters, must be 1–10 characters, and when it changed the save
first asks the availability endpoint — a taken identifier stops the save
with an identifier error, while a name or identifier that collides at write
time surfaces its own already-taken notice; any other failure keeps the form
open with a generic error notice. Base branch caps at 128 characters and
only letters, numbers, dot, underscore, slash and dash; the repository URL
caps at 512. Non-admins see the same tab dimmed with every input disabled
and no danger zone below the form.

## PSET-007 — emblem and cover artwork

The emblem opens a picker offering emoji and material icons; picking one
marks the form dirty and the new emblem renders after save. The cover
control opens a gallery popover: bundled-image search plus an upload path
backed by user assets. Uploads are processed before the save completes: a
cover that fails to process aborts the settings save with an error notice,
while on the creation path the same failure only warns and the project is
still created. TARGET (main#523): cover control, gallery and search are
gone; the emblem remains upload-backed with the same abort-vs-warn split.

## PSET-008 — repository bind control

The repository row is admin-only and lives outside the normal save: typing
in it never dirties the submit, and only the bind button persists. An empty
input is refused with an error notice. When the typed URL matches the
stored canonical URL the button flips to a disabled bound state. A
successful bind writes the canonical URL (as the provider reports it) back
into the field and the store, refreshes the binding status, confirms with a
notice that points at enabling sync, and backend failures surface verbatim.
Seeded-stack limit (probed 2026-10-12, no re-seed): the scratch stack has
no git provider connected, so every bind answers 401 {"error":"GitHub
token is missing"}. That 401 trips the global session interceptor, which
reloads the page to / and back onto the tab via next_path — no "Bind
failed" toast ever renders (bug NEWFRONT-395; intended: the refusal
surfaces verbatim). The oracle pins the refusal path as observed (empty
rejection client-side; provider-missing reload with no toast and no state
change); the bound pill and canonical writeback need a provider-backed
stack and stay unproven there.

## PSET-009 — execution target and automated-run policy

The target picker offers the local runner, the cloud agent and the desktop
agent; families the project cannot use render disabled and cannot be
picked, and switching target affects only runs created afterwards. The
master ticking switch defaults on; switching it off leaves the budget and
cadence controls visible but inert, and nothing validates while it is off.
The budget takes whole numbers of at least one, enforced twice: the input
carries a native floor of 1 that blocks the submit with no notice and no
request, while a cleared input reaches form validation and stops the save
with a field error. The no-cap toggle stands in for the pool sentinel,
remembers the typed value while ticked, and restores it when unticked; a
blank budget falls back to the default on save. The
cadence picker offers fixed presets from half an hour up to a day and adds
a custom marker row for values set outside the picker; picking a cadence
writes it to every ticking stage, while any other save leaves per-stage
rhythms set elsewhere untouched — with one wrinkle: dirty flags survive a
save within one form session, so a cadence touched earlier in the same
session still propagates on the next save; only a fresh load isolates an
unrelated save.

## PSET-010 — archive from the danger zone

Below the form, admins see the danger zone with archive and delete rows.
The archive row explains the project leaves navigation but stays reachable
and its items leave search. Its confirm modal names the project, warns the
contents archive with it, and offers cancel plus a confirm action; confirming
archives through the archive endpoint, confirms with a success notice, and
lands on the project list. Afterwards the project is gone from navigation,
still opens directly, no longer scopes search, and refuses updates.

## PSET-011 — restore an archived project

The same confirm modal in restore mode explains restoring re-activates the
project for its members and offers cancel plus restore; confirming restores
through the un-archive endpoint, confirms with a success notice, and lands
on the project list with the project usable again. (Wiring note: restore
mode is currently opened from the project card, while the danger zone always
opens archive mode — the scenario drives the control that actually restores
and the inventory entry point should be corrected to match.)

## PSET-012 — delete with double typed confirmation

The delete row warns everything inside the project is permanently removed.
Its modal asks for the exact project name and the exact confirmation phrase;
the destructive submit stays disabled until both match. Confirming deletes
through the project endpoint, confirms with a success notice, lands on the
project list, removes all contained data with no recovery, and clears the
caller's favorites bookkeeping for that project. Attempting the default
project is refused server-side.
