# Estimates area spec — measurement systems, settings, creation, editing, consumers (NEWFRONT-730)

Build spec for inventory rows EST-001–EST-029. It is written for the
estimates contracts issues (NEWFRONT-734, NEWFRONT-737), the seven build
slices (NEWFRONT-740–744, NEWFRONT-747, NEWFRONT-774) and the area gate
(NEWFRONT-748). Every section names the rows it covers; §12 maps each row
to its section and its oracle scenario. A slice author should be able to
build every row from this file without opening the old app.

Prose only. The old settings route, component folder, edition stubs,
stores, services and API views were read as reference and are never a
source: nothing here is a translation of their files, and no component,
hook, store, class name, style, icon or asset is reused. Two kinds of
literal text appear deliberately:

- **Contract text** — API paths, query parameters, body and response
  fields, enum values and cache keys. These must match the server exactly
  and are named verbatim.
- **Parity labels** — a short piece of user-visible text that the oracle
  asserts literally (a heading, a radio option, an error sentence). Each
  is quoted and marked _pinned_. Everything else is described by meaning,
  so slices are free to write their own copy.

Old sources consulted (reference only): the project-settings estimates
page and its header; the shared estimates root, list, list item, enable
switch, loader and empty screens, radio select, creation modal and its
first step, delete modal, point create/update/preview roots and the
number/text inputs; the edition variants for row buttons, helper, update
modal, point delete, time input and their barrels; the project-estimate
and estimate-point stores, the edition estimate entity, the hooks that
bind them, and the estimate service; the API estimate view set,
serializers, URL routes, storage models and activity worker; and the
consumers named in §6 (work-item property dropdown, detail sidebar,
spreadsheet columns, create modal, drafts block, inbox creation, cycle
progress, analytics axis selectors, command palette, read-only and peek
rendering, public board properties, issue activity feed).
`estimate-search.tsx` was not read on purpose: it renders a placeholder
and is mounted nowhere; EST-028 pins the absence of any list search.

Current-state source of truth for what is already built:
`apps/web_new/src/features/estimates/` (`EstimatesScreen`,
`EstimateCreateDialog`, `model.ts`, `api.ts`) from NEWFRONT-602 (PR
#1848), the picker and write path in `apps/web_new/src/shared/pickers/`
and `apps/web_new/src/features/issues/api/estimates.ts`, and the
consumers already wired (spreadsheet/display matrix, command palette
estimate page, activity model). §8 reconciles each row.

## 1. Mental model (EST-001–029)

An **estimate system** is a named, project-scoped list of **values**
(sometimes called points) plus a **family** (the measurement type). The
family decides what a value means and how it is validated:

- **points** — positive numbers, effort in abstract units.
- **categories** — short non-numeric labels (sizes, difficulty, t-shirt
  labels).
- **time** — minutes on the wire; rendered as hours and minutes.

A project may own many systems but has at most one **active** system.
A system is active in two related but distinct senses, and both matter:

1. **In-use flag.** A project's current system carries the in-use flag
   (`last_used`). The settings screen treats the flagged system as the
   active row and every unflagged system as **archived** (replaced,
   read-only). The server does not enforce a single flagged row per
   project (creating a system does not clear the flag on others); the
   client picks the first flagged system in list order as active, and
   the normal flows — create with the flag on, then replace — leave one.
   Do not depend on the database for uniqueness; if several are flagged,
   any stable pick is acceptable as long as the screen and gating agree
   on it. (Nothing in the product links an archived system.)
2. **Project link.** The project record holds a reference to an
   estimate system id (`project.estimate`). This reference, not the
   list, is what the **enable switch** reflects and what gates estimate
   controls across the product. It can be `null` while a system still
   exists (estimates turned off), and it always points at the flagged
   system when estimates are on.

Creating a system sends the in-use flag with the new system, so the
new system becomes the active row. It does **not** link the project:
the old client's create path deliberately does not write the project
reference, and a freshly created system shows as active while the enable
switch stays off until the admin turns it on (or it was already on and
now points at the previous system). The oracle seeds the link with a
separate project update and pins exactly this split. The new app keeps
it: creation and linking are two writes, and §3.8 states the resulting
state explicitly.

Values are ordered by an integer **key**. Keys are not user-visible;
they are the ordering contract for saved-system updates (the update
endpoint persists keys). Read ordering is a caveat: the old value model
orders by `value` text, not by `key`, so a plain list read can come back
in text order (which also masks the old client's duplicate-key quirk).
Any list that must follow the arranged order sorts by `key` explicitly;
the screen and pickers do that, and the new app must not rely on the
server's default ordering for a value list it renders. The new app
keeps keys unique and contiguous 1..n in display order at all times
(decision D21, §9.1), which is stricter than the old client and has no
visible parity effect.

Values carry an optional description field (present in the API, never
edited by any estimate UI in scope); the UI never sets or displays it.

The **project name** is part of the screen title. The screen's address
is project-scoped only: no address selects one system (§2.6).

Permissions are role-based on the project: **admin** (a project
membership with the admin role), **member**, **guest**, **outsider**
(a workspace member with no membership on this project), and
**anonymous**. Workspace admins do not automatically gain the estimates
view set (it checks project membership), but the project update used by
the switch and the link accepts a workspace admin as well. §5 is the
matrix.

Editions: the OSS build offers points and categories and full
create/delete; the cloud build adds the time family, saved-system
editing and single-value deletion with reassignment; desktop behaves
exactly like its edition's web build (§10). Everything else — the
screen, the switch, the creation steps, whole-system deletion, the
picker, gating, cycle/analytics consumers, history — is identical in
every edition.

## 2. Screen inventory (EST-001, EST-002, EST-003, EST-005, EST-006, EST-007, EST-024, EST-026, EST-027, EST-028)

### 2.1 Shell and gate (EST-001, EST-024)

The estimates settings screen is reached from the project settings
navigation, tab key `estimates`, label "Estimates" (_pinned_). The
settings shell owns the tab, the sidebar/first-party breadcrumb and the
page frame; the estimates area owns everything below the tab title. The
screen must not build a second settings frame or a second breadcrumb.

Gate: the route is visible only to project admins. A non-admin member,
a guest, an outsider and an anonymous visitor are each refused and get
the shared not-authorized view instead of the screen: no estimate
heading, switch, rows, add action, dialog trigger or list content. The
refusal must be the shell's shared not-authorized component (one view
across settings areas), not an estimates-specific message, and the
address stays where it was (no redirect). Outsiders and anonymous users
normally never reach the settings shell at all; the shared gate handles
them, and the estimates screen must not render "empty" content behind
it. The screen itself still takes an explicit `canManage` (or
equivalent) capability input and the row/switch actions must be
conditional on it, so a future route that shows the screen read-only
cannot expose controls (EST-007, EST-024).

Access control is enforced again on the server (§5); the UI gate is
presentation only and is never the security boundary.

### 2.2 States (EST-001, EST-002, EST-003)

The screen has exactly these states, and they are mutually exclusive
(after the loader resolves, exactly one of failed/empty/populated):

1. **Loading.** While the project's systems read is in flight, render a
   dedicated loader in the content area: several skeleton bars of row
   height (the old screen shows four). Nothing else from the content is
   shown — no empty state, no switch, no add button — so a slow read
   never flashes "no estimates yet". The loader is not a spinner; it is
   the row-shaped skeleton set. It is also shown on a refetch that starts
   from no data (first mount, reload), not on background refresh.
2. **Failed read.** If the systems read fails, show a visible inline
   error in the content area, announced to assistive technology, with a
   retry action (re-run the read). Do not show the empty state (that
   would read as "no systems"), and do not show the switch or rows. The
   old screen degrades to the empty state with no message; the new app
   must not. This branch is a deliberate, tracked improvement (EST-002's
   error half belongs to slice 740).
3. **Empty (no systems at all).** An illustrated empty state with a
   short title, "No estimates yet" (_pinned_), a supporting sentence,
   and a primary action whose accessible name begins with "Add"
   followed by estimate/system wording (the oracle matches
   `/add estimate/i`; the old label reads "Add estimate system"). Only a
   project admin sees a working action; any other viewer who somehow
   reaches this state sees the illustration and text with no action.
   Pressing the action opens the creation dialog on step one (§3).
4. **Populated.** An active (flagged) system exists. The content
   contains, in order: the enable box (§2.3), the active-system section
   (§2.4), and the archived section when there are unflagged systems
   (§2.5). A populated screen never shows the empty state. There is no
   create trigger on a populated screen: the old screen offers creation
   only inside the empty state's action, and the archived section adds
   no action of its own. To replace the active system an admin deletes
   it first (which returns the screen to the empty state) and then
   creates a new one. The new app must not add a second create entry
   point: a populated screen's only buttons are the switch, the active
   row's edit (cloud) and delete, and the read-only archived rows. The
   empty state's action is the only place the create label appears
   (§2.8).

Archive-only edge: if every system is archived (no flagged system), the
screen keeps the empty state for the active slot (the old screen does
this — the active row is chosen by the flag) while still listing the
archived section below it, and the empty state's add action is present
(since there is no active system, this is the empty state, not the
populated one, so creation is reachable). The new app must reproduce
this: the empty state is about "no active system", not "no systems".
The archived section is not hidden by the empty state.

### 2.3 Enable box (EST-004)

When an active system exists, the top of the content is a boxed control
with:

- a title naming project estimates (the old title reads "Enable
  estimates for my project"; the oracle finds the control by role
  (`switch`), not by that text, so slice 741 may write the title in the
  product's voice as long as it is a title, not a button);
- one supporting sentence saying estimates help communicate complexity
  and workload (paraphrase; not pinned);
- a toggle switch at the trailing edge.

Switch position comes from **the project's estimate reference**, never
from list position: on when the reference is non-null, off when null.
This distinction is observable: a project whose only system is flagged
in-use but unlinked shows the switch off while the system still appears
as the active row.

Behaviour:

- On (reference set) → off: write the project update with the estimate
  reference `null`. On success show a success notice whose text says
  estimates were **disabled** (_pinned_ direction word "disabled"), set
  the switch off, and keep every system and every work-item value
  untouched (disabling is not deletion).
- Off → on: write the project update with the estimate reference equal
  to the active system's id (again, not "the first row"). On success
  show a success notice saying estimates were **enabled** (_pinned_
  direction word "enabled") and set the switch on.
- Pending: the **old app has no pending state for this switch**. The
  switch is disabled by role only (`disabled={!isAdmin}`); while a
  write is in flight it stays operable and a second click fires a
  second project update, whose direction is computed from the same
  stale reference (so two rapid clicks can send the same value twice,
  and their results race). The new app disables the switch while the
  write is in flight so a second click cannot fire a second write;
  that is a deliberate improvement, not old behaviour, and slice 741
  records it as a divergence (the oracle does not exercise a double
  click).
- Failure: show an error notice and leave the switch exactly where it
  was. The notice's **title** contains the word "Error" (_pinned_; the
  old app's title is "Error!"). This is not optional: the oracle reads
  the notice's combined text and requires "Error" in it, so a title such
  as "Couldn't change estimates" fails the scenario even though it reads
  naturally. The old app's failure body always names the disable
  direction even when enabling failed (NEWFRONT-341, §9.2); the new app
  names the attempted direction. The oracle asserts only the title
  word, so the corrected body is safe.
- A caller without manage rights gets no operable switch. (In the old
  build such a viewer never reaches the switch because the route is
  gated; the control also renders disabled when the capability is false.
  The new screen keeps the disabled/absent contract.)

### 2.4 Active row (EST-005, EST-007, EST-024)

The active-system section carries a small section title ("Estimates
list", _pinned_) and exactly one row for the flagged system:

- System **name** as the row title. (Systems whose name was omitted at
  creation carry a generated ten-letter lowercase name; show it as-is.)
- Every value, in key order, joined into one comma-separated line.
  Values must all be present; truncation is visual only and must not
  hide values from the accessible text.
- **Time-family** values are converted from minutes to an hours/minutes
  form before display: 90 → "1h 30m", 150 → "2h 30m", 60 → "1h",
  45 → "45m". Raw minute numbers must never appear in the row text.
  Zero and negative values are impossible through validation, but the
  formatter must not crash on them: fall back to the raw value.
- Points and categories show their stored text exactly.

Row controls: in **OSS** the row has exactly one control, a destructive
delete (trash) affordance; it has an accessible name naming the row.
There is **no** edit control on the OSS row. (The old OSS/community
build's row-actions component renders delete only; the shared parent
passes an edit callback that nothing can call, and the edition update
dialog is an empty stub. The oracle asserts a one-button row with no
edit affordance and no "update estimate" text anywhere.) A caller
without manage rights gets a row with zero controls. Archived rows
always have zero controls regardless of role (§2.5).

The edit affordance belongs to the **cloud** edition (§4.1, EST-029):
where the edition provides it, the active row gains an edit control in
addition to delete, and the row stays otherwise identical.

### 2.5 Archived section (EST-006, EST-007)

Rendered only when at least one system is not flagged in-use. Contains:

- Section title, "Archived estimates" (_pinned_), and an explainer of
  one or two sentences that says these are systems replaced by a newer
  one, kept so older values stay meaningful, and that the reader can
  learn more — the explainer sentence contains the words "older
  versions" (_pinned_ substring).
- An outbound link on a public `https://` address that opens in a new
  tab (`target` new window, no opener). The link is plain documentation,
  not a settings route; opening it must leave the app (Oracle: the
  opened page's host matches the href's host and the path is not the
  estimates settings path).
- One row per archived system: name plus every value in the same format
  as the active row (time humanized), newest-created first. Rows are
  read-only: zero buttons, no trigger that opens a picker, no editable
  text. `last_used` is the only classifier; creation time orders the
  section (descending).

Archived systems with a system still flagged: the archived section sits
below the active section. Archived systems are never reachable from the
enable switch (the switch only ever links the active system id).

### 2.6 Addresses and titles (EST-026)

- One address: `/{workspaceSlug}/settings/projects/{projectId}/estimates`.
  It is the settings tab's suffix, `estimates`.
- Browser/tab title tracks the project: the old title is the project
  name followed by " - Estimates"; the new app must include the project
  name (the oracle asserts the project name is contained), and may
  follow the shell's title convention (area + project) as long as the
  project name is present and the area reads as estimates.
- No per-system address exists. `.../estimates/{some-id}` must render
  the app's not-found view, even when that id names a real system, and
  must not render the estimates screen behind it. Do not add a
  system-detail route for parity.
- The screen is project-scoped: the workspace/project come from the
  route, and switching projects re-reads systems for the new project
  (cache keys are per workspace and project, §2.7).

### 2.7 Freshness (EST-027)

- The screen reads systems through a plain fetch, cached per workspace
  and project. The old client's cache key shape is
  `PROJECT_ESTIMATES_<workspaceSlug>_<projectId>`; the new client keeps
  an equivalent per-workspace/per-project query key and must not share
  one key across projects.
- There is **no live push**: no websocket/SSE subscription for
  estimates, no polling. A change made elsewhere (another tab, the API)
  while the screen is open stays invisible until a fetch actually runs.
  The oracle mutates via the API, waits several seconds and requires the
  screen to be unchanged.
- The old app's global data layer does revalidate on focus: its shared
  configuration turns on focus revalidation, stale revalidation and
  mount revalidation for every read that does not override it, and the
  estimates read does not override it. So returning to the window after
  the data has gone stale triggers a fetch, exactly as a reload does.
  What it does **not** have is a push channel: an open, focused screen
  never updates by itself.
- The new app matches that shape. Its shared client already enables
  focus refetching by default; the estimates read currently opts out of
  it, which is the divergence to remove. The old read holds no staleness
  window (mount and stale revalidation are on), so a revisit fetches
  rather than replaying a cache; slice 740 makes the estimates read
  follow the same rule — refetch on focus and on mount — instead of
  serving a cached list for a fixed freshness period.
- A refresh writes whatever the read returns, including a newly created
  system (the oracle creates one behind an open screen, waits, and
  requires the screen unchanged, then reloads and requires it shown).
- Failed reads do not overwrite the cache with an empty list: a retry
  state, not "empty".
- Regardless of freshness strategy, the screen remains stale while the
  document stays focused and no fetch runs; tests that prove staleness
  must not blur or refocus the page before asserting it.

### 2.8 Pinned absences (EST-028)

The estimates settings screen has **none** of the following, in any
edition, and must not add them for convenience:

- keyboard shortcuts (pressing the usual help, create and search keys —
  including `?`, `c` and `/` — changes nothing: no dialog, no navigation,
  no focus jump);
- export or import of systems or values, any download;
- bulk or multi-select operations (no row checkboxes, no "select all",
  no bulk delete/archive);
- list filters, sorting controls, or search over systems or values.

The rendered screen text must not mention these capabilities, so the
oracle's word scan of the visible text (shortcut, export, import, bulk,
filter, search) stays clean. The archived ordering and active selection
described above are not "sorting controls". The command palette's
work-item estimate page (§6.5) is a different surface and is not bound
by this row; this row is about the settings screen only.

## 3. Creation dialog (EST-008–EST-013)

The creation dialog is opened from the empty state's primary action, by
a project admin. That is its only entry point: a populated screen has no
create trigger, so an admin who wants a different system deletes the
active one first (§2.2 item 4). It is a modal over the settings screen;
closing it never navigates.

It has two steps with a visible counter, "Step 1 of 2"/"Step 2 of 2"
(_pinned_ form). Title and description are the dialog's own (old title:
"New estimate system"; either way the title names creating an estimate
system).

### 3.1 Steps, back, cancel, reset (EST-008)

- Step one is the **starting-point** step: family and starter choice
  (§3.2, §3.3). On open the counter reads step one and there is **no**
  back control.
- Choosing any starter (including the scratch card) moves to step two,
  seeds the value list with that starter's values, and shows a back
  control.
- Back returns to step one and **discards** the step-two values and any
  inline edits. The old dialog also resets the family radio to points on
  Back — arriving from categories still leaves points selected — and
  clears the chosen starter. The new app may either copy that reset or
  keep the family the user picked; if it keeps it, record that as a
  deliberate divergence in its slice, never as old behaviour. What is
  not negotiable is the discard: Back never keeps step-two values.
- Cancel closes the dialog at either step with no API call and no local
  persistence. Reopening starts again at step one with defaults: points
  family selected, no starter chosen, values and errors cleared.
- Dismissal: the **old dialog has no Escape or overlay-close
  behaviour**. It mounts without a close handler on the modal root, so
  Escape and a backdrop click do nothing at any step, write in flight
  or not; only the Cancel button and the post-success close leave it.
  The new app treats Escape/overlay as Cancel when no write is in
  flight and ignores both during a submit. That is a deliberate
  improvement; slice 742 records it as a divergence (the oracle drives
  Cancel and submit only and does not assert either dismissal).
- Step two has Cancel plus the primary submit ("Create estimate");
  step one has only Cancel (there is nothing to submit yet).
- The dialog's footer is fixed at the bottom; long value lists scroll
  inside the dialog body.

### 3.2 Family radio and edition availability (EST-009)

Step one begins with a labelled radio group over the families (the old
group label reads "Choose an estimate system"; the oracle reads the
radios and their option labels, not the group legend). Each family is a
radio whose option label is its display name — "Points", "Categories"
(_pinned_) — and the first available one is selected by default.

The set of families is built from a family catalogue filtered by an
**edition capability**, not by ad-hoc environment checks:

| Family                    | OSS        | Cloud   | Desktop    |
| ------------------------- | ---------- | ------- | ---------- |
| `points` (Points)         | offered    | offered | as edition |
| `categories` (Categories) | offered    | offered | as edition |
| `time` (Time)             | **absent** | offered | as edition |

OSS omits time entirely — it is not rendered disabled, not rendered
with an unavailable marker, and its value must never be submitted from
an OSS build. The old OSS helper enables exactly points and categories;
the catalogue marks time as cloud-only, and the old cloud build would
render it enabled with a time input. The oracle asserts the radio values
are exactly `["points","categories"]`, both enabled, points checked, and
that `time` never appears.

The old app has **no disabled family row**: its family helper
returns false for every family except points and categories, and the
first step maps the unavailable ones to nothing and filters them out
before rendering, so an OSS build simply shows two enabled radios. The
disabled row described here is a **new-app fallback for future editions
and catalogue states**, not a rendering of the old app: if a family is
in the catalogue but unavailable in the current edition, render it
disabled with an explanatory marker (a tooltip-style "not available in
this edition" note), never selectable and never submitting. OSS must
still omit time outright (the oracle counts exactly two radios), so the
fallback must not be the mechanism OSS uses to hide time.

Choosing a family replaces the starter card set (§3.3). Choosing a
family never preserves a starter from the previous family.

Internally the family value is the API enum string (`points`,
`categories`, `time`). Comparisons are exact and lowercase; the server
stores the enum, and the client must not rely on display casing.

### 3.3 Starters: scratch and templates (EST-010)

Below the radio, step one offers a **scratch** card ("Start from
scratch" heading, card "Custom") and, under "Choose a template", a grid
of template cards. Each card shows its title and a plain preview of the
values it will load, comma-separated, in order. Previews are display
only; they do not fetch or create anything.

Starter sets by family (exact values, order is the initial key order):

- **points**
  - Custom (scratch; hidden from the template list): `1, 2`
  - Fibonacci: `1, 2, 3, 5, 8, 13`
  - Linear: `1, 2, 3, 4, 5, 6`
  - Squares: `1, 4, 9, 16, 25, 36`
- **categories**
  - Custom (scratch; hidden): `Easy, Hard`
  - T-Shirt Sizes: `XS, S, M, L, XL, XXL`
  - Easy to hard: `Easy, Medium, Hard, Very Hard`
- **time** (cloud only)
  - Custom (scratch; hidden from the template list): `1, 2`
  - Hours: `1, 2, 3, 4, 5, 6`
  - Stored unit: **whole minutes as a decimal string** (`"1"` …
    `"6"`); the stored text is never the humanized form. Every preview
    and rendered value is humanized by the shared formatter, so the
    Hours card reads `1m, 2m, 3m, 4m, 5m, 6m` and the scratch card
    reads `1m, 2m`. Both cards load exactly those stored values into
    step two.
  - Decision (this spec, not the old app): the old structure renders
    the scratch-and-template block only for non-cloud families, so
    choosing time in that build exposes a radio with no card and no
    path to step two — a dead end, not a starter. The cloud slice must
    not copy that dead end. It offers the family's one template
    (Hours, the `hours` entry of the family catalogue) plus the
    two-value scratch set above, pins both in a unit test, and sends
    only values the server accepts. No invented templates, no empty
    value set, and no other time template until the catalogue gains
    one.

The scratch card is always present (first) and always loads the
family's minimal two-value set (points and categories as above; time
where the edition offers it, §3.3's decision). Templates are
never pre-selected. Picking a card moves to step two with those values
and no call to the server. Browsing starters and cancelling stores
nothing.

Previews must match the values actually loaded into step two (the
oracle asserts each template's preview string and the loaded list).

### 3.4 Step two layout (EST-011)

Step two shows, at the top, the family name as a card label (the old
label is the lowercased family key; capitalize for display if the shell
prefers, but it must identify the family). Below it, one row per value,
in key order. Each row shows:

- a drag grip (the reorder affordance; it must be a real drag source,
  see §3.5);
- the value, or a muted "enter a value" placeholder for an empty row
  (time-family values are shown humanized, e.g. 90 → "1h 30m");
- an edit control (pencil) that opens the inline editor;
- a remove control (trash), **hidden** when only two values remain;
- an inline problem marker when the row is invalid (§3.6).

At the bottom, an **add** control whose accessible name begins with
"Add" and names the family (the old label is the family name after it,
e.g. "Add points"; the oracle matches `/^add /i`, not the wording). It
is **hidden** when six values are present.
The count bounds are 2 (minimum) and 6 (maximum): removal cannot take
the list below two and addition cannot take it above six.

Double-clicking the value text opens the same inline editor as the
pencil. The inline editor is a single-line input seeded with the raw
value (time: the raw minute number, not the humanized text), focused on
open, with a confirm and a cancel. Enter or confirm commits the draft;
cancel discards it. The **old editor has no Escape handler**: its
inputs bind change/submit only, so Escape does nothing and the draft
stays in the field. The new app makes Escape discard the draft like
cancel; slice 742 records that as a divergence.

Committing an **empty** draft does **not** create or blank a row. Both
old editors (add and edit) branch on the draft being truthy: an empty
commit reports the empty-value problem (§3.6), keeps the editor open,
and never calls the row-update callback, so an existing row keeps its
previous value and a new row is never added. An empty row therefore
exists only as the untouched **add** placeholder (the empty input the
add control opens); it is never produced by an edit. The new app must
reproduce that shape: empty = a problem, not a mutation.

While the editor is open the row's other controls may be subdued, but
removing or reordering the row must close the editor.

### 3.5 Reordering (EST-011, D21)

Rows are reorderable by dragging the grip (the value itself may also be
the drag handle if the drag library requires it, but the grip must be
present and usable). Dropping a row at a new position reorders the list
and renumbers keys 1..n in the new display order immediately.
Reordering is local to the dialog until submit; it never calls the
server. Cancelling after a drag stores nothing.

Keyboard alternative: the drag affordance must at least announce its
position and offer move-up/move-down actions to assistive technology,
because a pointer-only reorder is not usable parity for keyboard users.
This is a target improvement; it does not change the stored result.

Add allocates a **new key greater than every existing key** (max + 1)
and appends the row. Remove deletes the row and immediately renumbers
the remainder 1..n. Combined, keys stay unique and contiguous; the old
client's add-allocates-length+1 quirk is not reproduced (D21, §9.1).
Keys are invisible, so this cannot change what the user sees.

### 3.6 Per-value validation (EST-012)

Validation is per row. The **old editors test the raw draft**, not a
trimmed copy: their commit handler branches on the draft string being
truthy and then runs the family check on that string as typed.

- **Empty.** Only a genuinely empty draft is refused. A
  whitespace-only draft is truthy, so it is not treated as empty: for
  numeric families it reaches the "greater than 0" branch (the numeric
  conversion of a space is zero), and for categories it reports the
  character-value problem. The message must contain the word "empty"
  (_pinned_ fragment; the old sentence is "Estimate value cannot be
  empty." — write the new app's own sentence around that fragment).
- **points** (and time, where offered): the value must parse as a
  number. The message for a non-number must contain "numeric"
  (_pinned_ fragment); the message for zero or a negative number must
  contain "greater than 0" (_pinned_ fragment). The old client uses
  **two** sentences here and a whitespace-only draft falls into the
  second. Accept decimals. The server stores a string and the old
  create path sends the draft untrimmed, so nothing is normalised on
  the way out. The new app commits a **trimmed** value once, on commit,
  and validates and sends that (a §9.3 divergence); it must not
  re-format a number (`1.0` stays `1.0`).
- **categories**: the value must be non-numeric text; a numeric string
  (including "0", "1.5", "-2") reports a message containing
  "character" (_pinned_ fragment).
- **Duplicates** within the set are refused, message containing
  "already exists" (_pinned_ fragment). Comparison is by the family's
  semantics: for numeric families compare the numeric value (`1` and
  `1.0` are the same, ` 1` and `1` are the same), for categories
  compare the trimmed text exactly (case-sensitive: "Easy" and "easy"
  are different categories — the old client compares exact strings).
  The 602 model lowercases categories; slice 742 changes it to the
  exact comparison the oracle pins.
- **Length cap.** The cap is **20 characters** (the old input constant
  and the server serializer's check are both 20). The old client
  enforces it by **ignoring the whole edit**: its change handler
  assigns the new string only when its length is at most 20, so typing
  the 21st character changes nothing and pasting 25 characters leaves
  the field at its previous value (no truncation, no error message, no
  visual cut). The new app truncates an over-long change to its first
  20 characters instead, which keeps the typed prefix the user meant
  and cannot submit more than the server accepts; slice 742 records
  truncation as a deliberate divergence. Either rule ends at ≤20
  characters and neither silently submits more. The oracle types
  character by character, which cannot distinguish the two, so the
  gate does not pin one. The 602 model uses 40 and an explicit "too
  long" message; that divergence is recorded as remaining work in §8
  and must be brought to 20 by slice 742.

The pinned fragments above are exactly what the oracle matches
(`/numeric/i`, `/greater than 0/i`, `/character/i`, `/already exists/i`,
`/empty/i`). They are a floor, not a script: slices write their own
sentences containing those words and must not lift the old strings.

Presentation: an invalid row shows an inline marker (an information
icon with a tooltip, or an equivalent inline error), and the message is
available as accessible text on the row. A row that has never been
touched shows no marker; once the row is committed/submitted, markers
appear. Problems are not shown for a row while its inline editor is open
and dirty; they recompute on commit.

Submit gating: pressing the primary action with any unresolved problem
must **not** call the API. It reveals the markers, keeps the dialog
open on step two and focuses/labels the first problem. The old dialog
has no row list to walk: its gate iterates the **problem map** (entries
recorded by the open add/edit form and by committed edits), so an empty
row reaches submit through the **open add form** — the one place an
empty value lives. Submitting while the add form is open and still
empty must surface the empty-value problem on that form and refuse. The
old gate also blocks any entry whose draft differs from its committed
value (an unsaved edit), independent of its message; the new app's
equivalent is to line up its pending edits with the problem set before
deciding. There is no dialog-level "some values are invalid" banner
required, but problems must be visible.

Server-side length validation also exists (§4.4); the client cap is the
first line, not a substitute.

### 3.7 Submit and failure (EST-013)

Submit is enabled on step two (not disabled merely because a starter was
never chosen — every starter path seeds values; the scratch path is a
starter). On submit:

1. Run the full validation pass over the committed row values (trimmed
   on commit, §3.6); if anything is invalid, stop as in §3.6.
2. Send one create request (§7.1) carrying the estimate object and the
   full point list:
   - name = the family's display name (the old client sends "Points",
     "Categories", "Time"; the new app keeps that convention so a
     default-named system is recognisable and duplicate-name refusals
     are predictable);
   - type = family key (`points` | `categories` | `time`);
   - in-use flag = true;
   - points = one entry per row, key = position (1..n), value = the
     row's committed (trimmed) text, time as raw minutes in a string.
3. While pending: the dialog stays open, the primary button shows a
   busy/disabled state, dismissal is blocked, and the rows are not
   editable.
4. Success: show a success notice saying the estimate was created
   (_pinned_ substring "Estimate created"), close the dialog, refetch the
   systems list and the project row, and render the new system as the
   active row with its values. The empty state must give way to the
   populated state in the same view without a manual reload.
5. Failure: show an error notice saying creation failed (_pinned_
   substring "creation failed"), keep the dialog open on step two with
   every value and the family intact so the admin can retry or cancel,
   and store nothing new. The duplicate-name case is the canonical
   failure: a project that already holds an unflagged system named
   "Points" refuses a second "Points" (§4.4); the dialog reports the
   failure and stays open.

### 3.8 Creation and the project link (EST-013, EST-018)

The create response includes the system with its points. After success:

- the dialog closes, the list refetches, and the new system is the
  **active row** (it was created with the in-use flag);
- the **project link is unchanged**. If the project was unlinked, the
  enable switch reads off and work-item estimate controls stay hidden
  even though the screen now shows an active row. If the project was
  linked to an older system, the switch stays on and still points at
  that older system while the new one is the active row — an observable
  inconsistency in the old app. The new app must not silently relink;
  it shows the truth (switch state from the project reference, active
  row from the flag), and the admin turns estimates on/off as the next
  explicit action. Creating a replacement is a separate, later step:
  the active system must be deleted first, which returns the screen to
  its empty state where creation is offered (§2.2 item 4).

This means a create slice must **not** add a project update after the
create, and the oracle's EST-013 scenario must not expect the project
reference to change. The gating consequence (EST-018) is that a newly
created system offers no work-item controls until the switch is used;
that is the old behaviour and it is within parity. Slice 742 proves the
create stores the flag and the values, and the switch slice (741) proves
the link write.

`last_used` and `project.estimate` are separate fields; do not treat
either as a proxy for the other anywhere in the client.

## 4. Editing, deletion, server rules and editions (EST-014, EST-015, EST-016, EST-024, EST-025, EST-029)

### 4.1 Saved-system update — cloud only (EST-014, EST-029)

Availability: the update dialog is a **cloud** capability and is absent
in OSS. OSS renders exactly the delete-only active row (§2.4); there is
no "update estimate" text, trigger or stub dialog. The old OSS edition
ships an update modal that renders nothing, and the old row-actions
component renders no edit button, so this is the observed behaviour,
not a stripped feature.

What the dialog does (when the edition offers it), opened from the
active row's edit control:

- Shows the system name (editable text) and its full value set, with the
  same per-row affordances and validation as creation step two
  (§3.4–§3.6): reorder, inline edit, remove (hidden at two), add (hidden
  at six), family-correct validation, 20-character cap. Family changing
  is **not** offered: the family of a saved system is fixed in this
  dialog (the API accepts a type in the estimate object, but changing a
  system's family reinterprets every stored value, and the old UI never
  offers it). Name, order and values are editable.
- Saving is **one save action** for the whole arrangement, but it maps
  onto three API shapes because the update endpoint only mutates
  existing values:
  1. **PATCH the system** with the name and the complete list of values
     that already exist (each entry carries its point `id`, its new
     `key` and its new `value`). Only ids the server recognises are
     updated; unknown ids are ignored, and the endpoint never creates or
     deletes a value.
  2. **Adds**: a value that had no id goes through the single-point
     create endpoint (POST under the system) with its key and value;
     the server returns the created point with an id. New rows are sent
     after the PATCH, in display order.
  3. **Removes**: a value present before and absent now opens the §4.2
     confirmation for that value — the admin chooses to move the
     affected work items to another value of the system (send
     `new_estimate_id`) or to clear them (send nothing) — and only then
     is the single-value delete endpoint called. A removal saved with no
     confirmation silently clearing every affected item would lose data
     the user never agreed to lose; §4.2 is the removal path, not a
     separate feature. The server renumbers the remaining keys.

  Order of operations: PATCH existing ids first (it writes the final
  keys), then the point deletes (the server renumbers after each), then
  the adds, then re-read the system and renumber once so the dialog and
  the row show the server's ids and the user's final order. If a
  removal is cancelled in its confirmation, that row stays and the rest
  of the save may proceed; if a step fails, report the failure, keep the
  dialog open with the arrangement, and do not claim success; never
  silently drop a row that failed to add or remove.

- The full PATCH shape requires a non-empty point list: an update with
  an empty value set is refused with 400 `{"error": "Estimate points
are required"}`. The dialog cannot produce an empty set (minimum two),
  so this is a server rule surfaced as a generic failure if it ever
  happens.
- Success: notice, dialog closes, the row re-renders with the new name
  and ordered values, and the change survives a reload/refetch. Failure
  (400 or transport): error notice, dialog stays open, nothing is
  presented as saved.
- Cancelling discards local edits and never calls the API.
- Editing an active system keeps it active; editing name/values never
  changes which system is flagged or linked.

The old OSS update stub is _not_ a template for the cloud dialog; the
cloud dialog is written from this section. Slice 743 owns it behind the
edition capability.

### 4.2 Single-value delete with reassignment — cloud only (EST-015, EST-029)

Availability: absent in OSS (no per-value delete confirmation; the old
edition's point-delete component renders nothing). Present in cloud.

From the update dialog, removing one existing value opens a small
confirmation (a modal or inline confirm inside the dialog) that must
explain the consequence and offer the choice:

- **Reassign affected work items** to another value of the same system,
  chosen from the remaining values (a single-select excluding the one
  being deleted; time values humanized). The confirmation's primary
  action applies it.
- **Clear affected work items** (no replacement). This is the default
  option when no other value is chosen, but it must be an explicit
  choice — the confirm names both outcomes.
- Cancel aborts; nothing is sent, the row stays.

The request is the single-value delete endpoint with an optional
`new_estimate_id` (§7.1): present for reassignment, absent for clearing.
The server moves or clears every work item carrying that value, records
one history entry per affected item on the reassignment path (§6.7), and
renumbers remaining keys without gaps. On success re-read the system so
the dialog reflects the new keys; the deleted value disappears from
pickers and from any item that had it (moved or cleared).

History on the clear path: the old server records **no** entry because
the activity worker crashes on the null replacement (NEWFRONT-345,
§9.2). The new app does not work around this in the UI and does not
fabricate entries; slice 743 surfaces the DOM behaviour (items clear)
and the oracle pins the absent entry. When the server is fixed, the
feed should show a removal entry; that is out of this area's scope.

Removing a value below the two-value minimum is not offered (§3.4); the
whole-system delete is the way to remove one of the last two.

### 4.3 Whole-system delete — all editions (EST-016)

Reachable from the active row's destructive control (all editions,
admin only) and, in editions that list them, from an archived row when
the row exposes the control. OSS exposes delete on the active row only;
archived rows are read-only in every edition as observed, so the
archived-delete path is not offered by the new UI. It still exists in
the API (delete by id) and must not be invented into the read-only
section.

Confirmation dialog:

- Title names deleting an estimate system ("Delete Estimate System" in
  the old copy; "Delete estimate system" is equivalent — the oracle only
  asserts the title contains "Delete").
- Body names the system in question and warns that it is removed from
  all work items permanently, that the action cannot be undone, and
  that adding estimates again later means updating work items. The body
  contains the system name (_pinned_) and the word "permanently"
  (_pinned_).
- Actions: Cancel (secondary) and a destructive confirm ("Delete
  Estimate" old copy; any destructive label naming deletion). While the
  write is pending the confirm shows a busy state ("Deleting" old copy)
  and both actions are disabled. Dismissal: the old confirmation also
  mounts without a close handler, so Escape and a backdrop click do
  nothing at any time; the new app allows them as Cancel while idle and
  ignores them while the write is pending (a deliberate improvement,
  recorded with the create dialog's dismissal divergence in §9.3).
- Cancel closes and changes nothing: the system, the project link and
  every work item keep their values.

Confirm:

1. `DELETE /estimates/{eid}/` (soft delete server-side), which also
   strips the value from every work item (the issue's estimate point
   becomes null through the point relation).
2. The second write is the project unlink, and it follows the **narrow
   rule**: after the delete succeeds, `PATCH /projects/{pid}/` with
   `{estimate: null}` **only when the deleted system's id equals the
   project's stored estimate reference**. Deleting any other system —
   an unlinked active system while the project still points at a
   different (for example older) system, or any archived system —
   leaves the reference untouched, and estimates stay on for the
   project. The comparison is against the stored reference, not
   against the flagged/active row and not against "any link exists":
   a freshly created system is flagged active while the reference may
   still point at its predecessor, and those two must not be confused.
   The old client instead unlinked whenever the project had _any_
   linked system, so deleting an unlinked system cleared an unrelated
   link too. That broad outcome is the old behaviour, pinned by the
   oracle as **bug: NEWFRONT-809** (see §9.2); the narrow rule above
   is the specified behaviour for the new app, chosen deliberately in
   NEWFRONT-809. Slices must not silently treat "active" or "enabled"
   as equivalent to "is the linked system".
3. Success: notice saying the estimate was deleted (_pinned_ "deleted"),
   close the dialog, refetch systems and the project; when the last
   system went, the screen shows the empty state and the active row is
   gone.
4. Failure: error notice (old body: unable to delete, try again —
   contains "unable to delete", which the oracle asserts), dialog stays
   open on the confirmation, the system and link are unchanged. Do not
   close on failure. The old app titles this notice "Estimate creation
   failed" (NEWFRONT-344, §9.2); the new app titles it as a deletion
   failure and does not reproduce the wrong title. The oracle does not
   pin the title.

If the delete succeeds and the unlink fails, report the failure but do
not pretend the system still exists: refetch, and let the next state
show the truth (the project may briefly reference a deleted system; the
gate must fall back to "no estimate controls" when the reference
resolves to nothing). The delete itself is the irreversible half; the
client must not attempt to re-create the system.

### 4.4 Server rules matrix (EST-014, EST-015, EST-016, EST-024, EST-025)

Read from the API view set, serializers and models; these are facts the
client must handle, not behaviours it may assume away.

| Rule                                                      | Endpoint                                                   | Result                                                                                                                                                                                                                                                               | New app's handling                                                                                                                              |
| --------------------------------------------------------- | ---------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| System list needs project membership (admin/member/guest) | `GET estimates/`                                           | 200 for members; 403 outsider; 401 anonymous                                                                                                                                                                                                                         | screen gate + shared refusal                                                                                                                    |
| Single system needs project membership                    | `GET estimates/{eid}/`                                     | 200; 404 `{"error": "The required object does not exist."}` if the id is not in this project; 403/401                                                                                                                                                                | not user-facing in OSS (the screen has no per-system read)                                                                                      |
| Project-points read needs admin or member; guest refused  | `GET project-estimates/`                                   | 200 with the active system's values, or `[]` when unlinked; 403 guest/outsider                                                                                                                                                                                       | pickers use the list, not this endpoint                                                                                                         |
| Create needs admin or member                              | `POST estimates/`                                          | 200 with the created system + points; 403 guest/outsider; 400 invalid points                                                                                                                                                                                         | creation dialog; guest never sees the screen                                                                                                    |
| No point list at all (field absent or not a list)         | `POST estimates/`                                          | 400 (serializer list error)                                                                                                                                                                                                                                          | client always sends a list of ≥2                                                                                                                |
| Empty point object `{}` in the list                       | `POST estimates/`                                          | 400 as a **nested field error**, not the flat form: `value` is a required field on the point serializer, so DRF rejects the object before `validate()` runs and the response is `[{"value": ["This field is required."]}]` (`key` carries a model default, so it is not flagged); the `non_field_errors: "Estimate points are required"` sentence is reachable only on the partial point-update path. The system row is written before validation and can remain empty (bug 346) | client never sends empty entries; report the failure and refresh                                                                                |
| Explicitly empty point list `[]`                          | `POST estimates/`                                          | **accepted** (200) and stores a system with zero values — a server gap the oracle does not pin; the client always sends ≥2                                                                                                                                           | client always sends ≥2                                                                                                                          |
| Value longer than 20 characters, bulk create              | `POST estimates/`                                          | 400 `[{"non_field_errors": ["Value can't be more than 20 characters"]}]` (one entry per offending point). The cap lives in `EstimatePointSerializer.validate`, which raises a plain string, so DRF files it under `non_field_errors`, **not** under `value`; the point rows are not stored (an empty system row can remain, bug 346) | client caps at 20 (§3.6)                                                                                                                        |
| Value longer than 20 characters, point update             | `PATCH .../estimate-points/{ptid}/`                        | 400 `{"non_field_errors": ["Value can't be more than 20 characters"]}` (the partial serializer skips the required-field check, then the same `validate` raises); nothing stored                                                                                                                                                     | client caps at 20                                                                                                                               |
| Value longer than 20 characters, system update            | `PATCH estimates/{eid}/`                                   | **accepted**: this path writes keys and values without serializer validation (bounded only by the column)                                                                                                                                                            | client caps at 20; do not rely on the server here (contracts B should note this inconsistency)                                                  |
| Bulk create validates after storing the system            | `POST estimates/`                                          | a refused point list can leave an empty system row behind (NEWFRONT-346, §9.2)                                                                                                                                                                                       | do not pre-empt; on failure the client reports failure and refreshes; do not auto-delete the orphan                                             |
| Single-point create requires key and value                | `POST .../estimate-points/`                                | 400 `{"error": "Key and value are required"}` when either is falsy                                                                                                                                                                                                   | client always sends both                                                                                                                        |
| Single-point create skips the 20-character cap            | `POST .../estimate-points/`                                | 200 even for 21+ characters (NEWFRONT-347, §9.2)                                                                                                                                                                                                                     | client cap already prevents it; do not rely on the server                                                                                       |
| Point update validates                                    | `PATCH .../estimate-points/{ptid}/`                        | 200; 400 over 20 characters; nothing stored on refusal                                                                                                                                                                                                               | inline edit path (§4.1)                                                                                                                         |
| Point delete reassign/clear                               | `DELETE .../estimate-points/{ptid}/`                       | 200 with only the values whose keys were decremented (not the full set); 403 guest/outsider; the clear path's history worker fails (bug 345)                                                                                                                         | cloud update dialog re-reads the system instead of using that body as the new list                                                              |
| System update requires a non-empty value list             | `PATCH estimates/{eid}/`                                   | 400 `{"error": "Estimate points are required"}` when the list is empty or absent                                                                                                                                                                                     | dialog always sends its set                                                                                                                     |
| System update only touches known ids                      | `PATCH estimates/{eid}/`                                   | unknown ids ignored; no create/delete                                                                                                                                                                                                                                | adds/removes go through point endpoints                                                                                                         |
| Issue write creator exception                             | `PATCH projects/{pid}/issues/{iid}/`                       | admin/member pass; a guest passes **only for an item they created**, else 403; outsider 403                                                                                                                                                                          | picker follows the item's own permission, never a blanket guest rule                                                                            |
| System delete                                             | `DELETE estimates/{eid}/`                                  | 204 for admin and member; 403 guest/outsider                                                                                                                                                                                                                         | whole-system delete                                                                                                                             |
| Omitted system name                                       | `POST estimates/`                                          | a random 10-letter lowercase name is generated                                                                                                                                                                                                                       | not used by the dialog (it always names the family), but a generated name is a normal system: it lists, refetches with points and can be linked |
| Omitted type                                              | `POST estimates/`                                          | defaults to `categories` in the view; the model's own field choices restrict storage to `points`/`categories` (the API accepts `time` in practice, as the oracle's time systems show, despite the model enum)                                                        | dialog always sends a type; contracts should record the enum/migration mismatch rather than trusting either list                                |
| In-use flag omitted                                       | `POST estimates/`                                          | defaults to false (dormant/archived)                                                                                                                                                                                                                                 | dialog always sends `last_used: true`                                                                                                           |
| Duplicate name in one project                             | `POST estimates/`                                          | 400 (`{"error": "The payload is not valid"}` from the base view set's integrity-error handler, not a field-level name error); exactly one system with that name remains                                                                                              | dialog shows the failure and stays open (§3.7); do not key copy off a field name                                                                |
| Point key omitted                                         | `POST estimates/` bulk                                     | default 0 for that row (bulk creation does **not** renumber)                                                                                                                                                                                                         | the client always sends 1..n; the oracle re-positions API-created systems before proving gapless delete                                         |
| Project update needs project admin or workspace admin     | `PATCH projects/{pid}/`                                    | 200; 403 otherwise                                                                                                                                                                                                                                                   | switch and delete-time unlink                                                                                                                   |
| Archived project                                          | `PATCH projects/{pid}/`                                    | 400 archived projects cannot be updated                                                                                                                                                                                                                              | the settings screen is not offered for archived projects; a failure is surfaced                                                                 |
| Issue estimate write                                      | `PATCH projects/{pid}/issues/{iid}/` with `estimate_point` | **204 with an empty body** on success (the issue view returns no representation); 400 when the point is not in this project                                                                                                                                            | picker/write path (§6.1)                                                                                                                        |

Guest note: guests can read the system list and a single system, but
cannot read project-points, create, update or delete. Since the
settings route is admin-only, the guest's read access is not a UI
surface in this build; the matrix matters for API parity and future
read-only views.

Creator note: the issue write is the one estimate-bearing write with a
creator exception. A guest who created that specific item may set its
estimate even though guests are otherwise refused on issue writes; the
check is "creator of this item", not "guest on this project". Admin and
member are unaffected, and an outsider is always refused. The picker
must not hard-code "guest ⇒ denied": it asks the item surface whether
the write is allowed and lets the server answer the creator edge (§6.1).

Delete note: system delete answers 204 for both admin and member; the
settings screen still hides the control from members (§2.1, EST-007).
"204" here describes the endpoint, not who sees the button.

### 4.5 Editions, in one place (EST-029)

| Capability                                 | OSS                | Cloud                    | Desktop    |
| ------------------------------------------ | ------------------ | ------------------------ | ---------- |
| Settings screen, gate, loader, empty state | yes                | yes                      | as edition |
| Enable switch                              | yes                | yes                      | as edition |
| Active row + archived section              | yes                | yes                      | as edition |
| Creation dialog (2 steps)                  | yes                | yes                      | as edition |
| Families offered                           | points, categories | points, categories, time | as edition |
| Active-row delete                          | yes                | yes                      | as edition |
| Saved-system edit dialog                   | **no**             | yes                      | as edition |
| Single-value delete + reassignment         | **no**             | yes                      | as edition |
| Whole-system delete                        | yes                | yes                      | as edition |
| Consumer rows EST-017–023                  | yes                | yes                      | as edition |

Desktop adds nothing estimate-specific. There is no `if (isDesktop)`
branch outside `core/platform`; the desktop build is the web build with
the desktop platform adapter. The OSS-vs-cloud split is implemented as
an **edition capability** (a flag in the edition module or a slot the
edition fills), not as a build-time environment read inside the
estimates feature, so both behaviours are unit-testable without a
private build.

## 5. Permissions matrix (EST-001, EST-004, EST-007, EST-024)

Roles are project roles. "Workspace admin" is treated as project admin
for the project update (the server checks both); for the estimates view
set the server checks the project membership row.

| Surface / endpoint                                    | Admin | Member              | Guest                                       | Outsider       | Anonymous                               |
| ----------------------------------------------------- | ----- | ------------------- | ------------------------------------------- | -------------- | --------------------------------------- |
| Settings route and full screen                        | shown | not-authorized view | not-authorized view                         | shell refusal  | sign-in                                 |
| Enable switch (visible/operable)                      | yes   | no                  | no                                          | no             | no                                      |
| Active row delete                                     | yes   | no                  | no                                          | no             | no                                      |
| Active row edit (cloud)                               | yes   | no                  | no                                          | no             | no                                      |
| Add action / creation dialog                          | yes   | no                  | no                                          | no             | no                                      |
| `GET estimates/` (list)                               | 200   | 200                 | 200                                         | 403            | 401                                     |
| `GET estimates/{eid}/`                                | 200   | 200                 | 200                                         | 403            | 401                                     |
| `GET project-estimates/`                              | 200   | 200                 | 403                                         | 403            | 401                                     |
| `POST estimates/`                                     | 200   | 200                 | 403                                         | 403            | 401                                     |
| `PATCH estimates/{eid}/`                              | 200   | 200                 | 403                                         | 403            | 401                                     |
| `DELETE estimates/{eid}/`                             | 204   | 204                 | 403                                         | 403            | 401                                     |
| `POST/PATCH/DELETE .../estimate-points/`              | yes   | yes                 | 403                                         | 403            | 401                                     |
| `PATCH projects/{pid}/` (link/unlink)                 | 200   | 403                 | 403                                         | 403            | 401                                     |
| Issue estimate write (`estimate_point`)               | 204   | 204                 | 204 if the guest created the item, else 403 | 403            | 401                                     |
| Read the estimate on a work item (detail, peek, feed) | yes   | yes                 | yes                                         | no item access | public/peek rules of the owning surface |

UI consequences to implement:

- The screen is built so that a false manage capability removes every
  action and leaves content (this is the contract a future read-only
  route uses). Today the route gating means non-admins never see content.
- A refused write never optimistically sticks: the switch snaps back,
  the delete dialog stays open with an error, the create dialog stays
  open, and the picker rolls back any optimistic issue value (§6.1).
- Do not attempt a member-side workaround (for example calling the API
  directly). The UI is the admin surface; the picker is the member
  surface.

## 6. Consumers (EST-017–EST-023)

The estimates epic owns the estimate **data and rules** inside each
consumer; the owning epic owns the surrounding surface. Every consumer
below reads the same system list (or an equivalent query) and the same
value formatter, and never calls an estimate endpoint to _configure_
systems. Contract ownership: NEWFRONT-734 (list/create/delete, project
link, project points read) and NEWFRONT-737 (system update, point CRUD,
issue estimate write, cycle/analytics reads).

Shared pieces every consumer uses (already in `apps/web_new`):

- active-system selection = the single `last_used` system of the project;
- value formatting = time → hours/minutes, other families unchanged;
- picker options = a leading clear row ("No estimate", _pinned_) then
  the active system's values in key order, each with a stable id.

### 6.1 Work-item estimate picker (EST-017) — owning epic: Issues

Issues owns the generic property row and the dropdown family
(`parity/specs/issues.md` §17, ISS-216). Estimates owns:

- **Option source.** The active system of the item's project; time
  humanized; the clear row is first and maps to a `null` write. Values
  are rendered in **key order**, sorted client-side: the server returns
  nested values in the model's `value` text ordering, and the old
  picker sorts by `key` before rendering.  With no active system, the picker is a single
  non-interactive row ("No estimate") — the picker still opens, shows
  its search box, and offers nothing actionable. The old behaviour is
  the same: the options list is the clear row only and the picker does
  not write.
- **Search.** The old picker matches the search term as a
  case-insensitive substring of the **raw stored value**, not of the
  displayed label. For points and categories the two are the same
  string; for **time** they are not: a stored `90` displays "1h 30m"
  while search compares against "90", so typing `1h` finds nothing and
  typing `90` finds the row whose label never shows "90". The `${value}`
  filter is built from the stored text, and the clear row is indexed by
  its "No estimate" label. This is a pinned quirk, **bug:
  NEWFRONT-820**, not intended behaviour.

  **Resolution (NEWFRONT-744, closed out by NEWFRONT-820).** The new
  picker matches the **displayed label**, not the stored value. On a
  time system a stored `90` reads "1h 30m", so typing `1h` finds it and
  typing `90` finds nothing; the clear row still matches "No estimate".
  Points and categories behave exactly as before because their stored
  and displayed text are the same string. This is a deliberate
  divergence from the old stored-value indexing (recorded in §9.3) and
  makes the picker agree with the palette page (§6.5), which filters
  rendered row text. The rule is pinning-free against the oracle —
  EST-017/EST-021 seed points and categories, where both rules agree —
  so no oracle scenario can distinguish them; the unit tests are the
  proof. Do not describe the old app as filtering labels.
- **Write.** One issue update carrying `estimate_point` = the chosen
  point id, or `null` for the clear row. It answers **204 with no
  body** (§7.2) — there is no updated issue to read back, so the client
  updates its cache from the request, or refetches. Optimistic apply across the
  cached issue rows, rollback plus an error notice on failure
  ("Couldn't save the estimate" in the 602 write path), then refetch.
  Permission follows the item, not a blanket rule: admin and member can
  write; a guest can write on an item they created and is refused on
  others; an outsider has no item access (§5). The control therefore
  uses the item surface's own write capability and never hard-codes
  "guest cannot estimate".
- **History.** The write is what generates the activity entry (§6.7);
  the picker itself does not write history.
- **Placement.** Detail sidebar, list/inbox row property menus,
  spreadsheet cells (when the column is shown), create/edit modal
  default properties, drafts blocks, inbox creation — all gated by
  §6.2. Issues decides the exact placement; estimates supplies the
  control and gating input.
- **Read-only contexts** never render the editable picker (§6.6).

Already built: `shared/pickers/EstimatePicker.tsx`,
`shared/pickers/estimates.ts` (options and clear row), and the write
hook in `features/issues/api/estimates.ts` (optimistic + rollback).
Remaining for the issues slices: mount the picker in every placement
under the gating input and prove history end to end (the write itself is
built).

### 6.2 Project-wide gating (EST-018) — owning epic: Issues

Estimates owns the rule; Issues owns each surface's rendering:

- **Estimates off** (`project.estimate === null`): no estimate control
  appears anywhere — not on the detail sidebar, not as a spreadsheet
  column, not in the create/edit modal, not on drafts, not on inbox
  creation, not in the peek. The property is not shown as a disabled
  row; it is absent.
- **Estimates on** (reference non-null): the control appears wherever
  that surface's layout includes the estimate property and the user's
  display preferences show it.
- Flipping the switch on the settings screen changes these surfaces
  after their project data refreshes; the oracle flips via the API and
  reloads/observes. The new app derives gating from the project row, not
  from "a system exists".

Already built: the route passes `hideEstimate={project.estimate === null}`
to the issues layout, the spreadsheet model filters the `estimate`
column, and the create/edit form has the field. Remaining: the issues
slices wire the same flag into the detail sidebar/list row/palette
gating and prove the on/off matrix.

### 6.3 Cycle progress grouping (EST-019) — owning epic: Cycles

Cycles owns the progress panel and the measure selector. Estimates owns:

- **Option list.** Exactly two options, "Work items" (_pinned_) and
  "Estimates" (_pinned_), in that order, with the first selected by
  default. The stored measure is a per-cycle preference keyed
  `"issues"` | `"points"` (the estimate option's value is `points`;
  the label is "Estimates"). When estimates are off on the project the
  stored measure reads back as `"issues"` regardless of what was picked
  before, so turning estimates off falls back to counting items.
- **Availability.** The selector is shown when the cycle has point data
  (v2 progress rows with a non-zero estimate total, or a non-empty v1
  estimate completion chart) **or** the project has estimates enabled —
  and the active system is **not** the categories family. For a
  categories (text-family) system the selector is hidden entirely (not
  rendered disabled). When shown, "Estimates" recounts the same progress
  by summing value numbers per state instead of counting items: with
  one item at value 2, work-item mode reads 0/1 and estimate mode reads
  0/2; stats rows report item counts in work-item mode and summed values
  in estimate mode.
- Cycles reads the estimate totals from its own progress payload; the
  estimates side supplies the active system's family and the gating
  flag. No new estimate endpoint is needed for this row beyond the
  project/list reads.

### 6.4 Analytics estimate axis (EST-020) — owning epic: Analytics

Analytics owns the chart builder, axis selectors, and the table. Estimates
owns the estimate axis semantics:

- **X axis.** "Estimate point" (_pinned_) is always present in the x-axis
  list, with or without an estimate system enabled (the catalogue
  includes it unconditionally; the oracle asserts it in both states).
  Choosing it groups rows by the work item's estimate value and labels
  each group with the **raw stored value**: the server groups on the
  point's stored value text without formatting it, and the old chart
  only capitalises priority/state-group names and reformats date axes,
  so a time system charts "90", not "1h 30m". The table prints the
  group name verbatim. The oracle's EST-020 scenario uses a points
  system ("1"/"3") and cannot tell the rules apart. If the new app
  humanises time here, that is a §9.3 divergence owned by Analytics,
  not a rendering of the old app. The table's first row is the axis
  name and the metric ("Count"). Unestimated items do **not** form a
  group: the analytics query drops rows whose estimate value is null on
  every axis (the same "exclude null dimension" step it applies to all
  non-date axes), so the estimate chart shows only values that exist and
  an item with no estimate is absent from it. Do not invent a "None"
  bucket for this axis — or for any other axis of this chart builder:
  the null-dimension exclusion runs before the count/estimate split, so
  no axis produces a synthetic null group. The oracle's two items both
  carry a value, so it cannot tell the two behaviours apart; parity is
  the absence.
- **Y axis.** There is **no** estimate metric offered, in any project
  state. The old catalogue defines an estimate-count metric but the
  selector hides it in every state, and the oracle asserts the y-axis
  options are exactly `["Work item"]`. Analytics builds the axis list
  accordingly; estimates adds nothing to the y axis.
- The estimate value written per item and the aggregation are server
  reads; no estimate configuration is touched.

### 6.5 Command palette estimate page (EST-021) — owning epic: Shell/Issues

The palette is the shell's; the contextual work-item commands are
Issues'. Estimates owns:

- **Command.** "Change estimate" (_pinned_), shown only when the item's
  project has estimates on (the gating input of §6.2). It opens a page
  rather than acting immediately; the command lists its shortcut (the
  old palette shows shift+e; keep an equivalent hint or none — the
  oracle asserts the command and its page, not the rendering of the
  shortcut).
- **Page options.** "No estimate" first, then the active system's values
  in **key order** (sorted client-side, since the nested values arrive
  in value-text order; time humanized), with the current value marked
  selected. Choosing a value runs the same write as the inline picker
  (§6.1), produces the same history, and closes the palette. Choosing
  the current value still closes without a write.
- **Search inside the page.** The page has no filter of its own; what
  narrows it is the palette's shared search box, applied by the
  palette's command list to the **rendered row text** while the query
  is non-empty. Opening a page clears the query, so the page starts
  showing every value and the user opts into filtering by typing;
  clearing the box restores the full list. The rows are indexed by the
  string they show — "1h 30m" for a stored `90`. The two surfaces
  search different things in the old app; the new app agrees
  deliberately (§6.1 resolution): both match displayed labels, so a
  stored `90` is found by `1h`, never by `90`. The old page
  shows a plain "no estimate found" line only when the active system
  has no values at all.
- The page must not appear when estimates are off: the command is
  absent, not disabled, and no estimate page is registered for that
  item.

Already built: `shared/shell/contextual/workItems.ts` registers the
command and page behind `estimatesEnabled`, using the shared formatter
and the same update action. Remaining: prove it through the issues
palette slice and with the on/off matrix.

### 6.6 Read-only, peek and public boards (EST-022) — owning epics: Issues (peek), Space (public)

Issues owns the peek shell and its read-only rules; Space owns the
public board overlay. Estimates owns the estimate label and the "no edit
affordance" rule:

- **Peek / read-only contexts.** The estimate row is visible with its
  value (time humanized). The value is read-only: the property trigger
  is present but disabled, pressing it opens nothing, and there is no
  path to write from the peek. When the item has no value, the row
  shows the empty placeholder ("None"). The estimate data must be
  loaded in the peek context regardless of which list opened it — the
  peek resolves the label through its own read, not through a store
  that only a picker open would have hydrated.
  Bug pinned by the oracle (NEWFRONT-349, §9.2): the old archived-peek
  context never loads the estimates, so it shows "None" though a value
  is set. The new app implements the intended reading (the set value,
  read-only) — slice 774 does this deliberately and the oracle scenario
  is aligned at gate time (drop the bug marker and assert the value).
- **Public boards (Space).** The public board shows **no estimate at
  all**. In the old public overlay the estimate block in the board
  properties is commented out, so a public visitor sees no estimate row,
  no value and no control even when the project has estimates on. The
  new public surface must keep that absence. This is an absence, not a
  gap: do not add an estimate to the public board, and do not write a
  Space row for it. The parity stack runs no public-board service, so
  this clause is proven by code absence plus the Space area's own rows,
  not by an estimates scenario.
- Shared/authenticated read-only contexts (for example a shared view)
  follow the peek rule: value visible, control disabled.

### 6.7 History entries (EST-023) — owning epic: Issues

Issues owns the activity feed, its rendering, filtering and links.
Estimates owns the entry shape and sentence:

- **Field.** Every new entry writes `field = "estimate_" + <type of the
replacement value's system>`, so `estimate_points` (points),
  `estimate_categories` (categories) and `estimate_time` (time). The
  value-delete reassignment path (§4.2) routes through the same worker
  with the replacement value's id and therefore lands under the **family
  field** too: a points reassignment is `estimate_points`, not a legacy
  key. The oracle asserts exactly that field for the reassignment case.
  Legacy `estimate_point` entries exist only in older history rows (the
  field was renamed to a family key after they were written); the clear
  path writes no entry at all (below), so it never produces a field. The
  feed must recognise the whole `estimate_*` family, not only three
  hard-coded keys; unknown future families render with a generic estimate
  treatment rather than being dropped.
- **Verb and values.** Assigning a value writes `verb: updated`,
  `old_value: null`, `new_value: <value text>`. Replacing writes
  `verb: updated`, the old and new value text. The feed sentence names
  the value and reads as "set the estimate point to <value>" (_pinned_
  substring; the 602 model currently says "set the estimate to <value>"
  — slice 774 changes it to the pinned wording). When the entry is shown
  alongside other items (a cross-item feed) it also links the item; in
  the item's own feed the entry's links must navigate (the oracle
  follows the first link in the entry and expects an in-app address).
  The old block renders the value as text and, when showing the issue,
  appends a link to it.
- **Clearing.** Clearing through the picker (`estimate_point: null`)
  currently writes **no** entry (NEWFRONT-348 / NEWFRONT-345, §9.2):
  the worker's estimate tracker compares values, finds the new value
  `None`, and then dereferences the missing replacement while building
  the field name. The feed therefore shows only the earlier set entry.
  The new app does not fabricate a removal entry; when the server is
  fixed the entry will be a removal with the old value, and the feed
  must be ready to render `verb: removed` (or a null new value) as
  "removed the estimate point <old value>".
- Value delete with reassignment (§4.2) writes one entry per affected
  item on the replacement path; the value-delete clear path writes none
  (same bug).
- The feed is fetched on mount (no live push); after an estimate write
  the item detail must be re-opened or refetched to see the entry.

Already built: `features/issues/activity/model.ts` renders
`estimate_point`, `estimate_points` and `estimate_categories` with a
generic sentence. Remaining: the whole-family recognition, the pinned
sentence, the removal shape, and the link assertion (slice 774, with
the issues feed slice).

## 7. API reference (EST-001–EST-029)

Base: `/api/workspaces/{ws}/projects/{pid}`. All bodies are JSON;
all reads need an authenticated project member unless stated. Error
bodies are DRF shapes: a field map for serializer errors, or
`{"error": "<sentence>"}` for explicit checks.

### 7.1 Estimate endpoints

| Method   | Path                                       | Body / params                                                                                                                                                                  | Success                                                                                                                                     | Errors                                                                                                                                                                                                                                                                                                                                                                                                                        | Rows                    |
| -------- | ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| `GET`    | `/estimates/`                              | —                                                                                                                                                                              | 200, array of systems, each with nested `points` (nested values arrive in the model's default `value` ordering — sort by `key` client-side) | 401 anon, 403 non-member                                                                                                                                                                                                                                                                                                                                                                                                      | 001, 003, 005, 006, 024 |
| `GET`    | `/estimates/{eid}/`                        | —                                                                                                                                                                              | 200, one system with `points`                                                                                                               | 404 `{"error": "The required object does not exist."}` for an id outside this project; 403/401                                                                                                                                                                                                                                                                                                                                | 024, 026                |
| `POST`   | `/estimates/`                              | `{"estimate": {"name"?, "type"?, "last_used"?}, "estimate_points": [{"key", "value", "description"?}]}` (the `estimate` object itself is required; the client always sends it) | 200, created system with `points`                                                                                                           | 400 absent/non-list points (DRF list error under `non_field_errors`); 400 for an empty point object as a nested field error `[{"value": ["This field is required."]}]` (not the flat `{"error": …}` form and not the `non_field_errors` sentence, which belongs to the partial point update); 400 over-long value in bulk create: `[{"non_field_errors": ["Value can't be more than 20 characters"]}]` (raised as a plain string in `validate`, so DRF files it under `non_field_errors`, not under `value`); 400 duplicate name (`{"error": "The payload is not valid"}`); a request without the `estimate` object is malformed (server error, never sent by the client); 403 | 013, 024, 025           |
| `PATCH`  | `/estimates/{eid}/`                        | `{"estimate"?: {"name"?, "type"?}, "estimate_points": [{"id", "key", "value"}]}`                                                                                               | 200, system with `points`                                                                                                                   | 400 empty/absent list (`{"error": "Estimate points are required"}`), 403; **no length or content validation** (see the matrix below)                                                                                                                                                                                                                                                                                          | 014, 025                |
| `DELETE` | `/estimates/{eid}/`                        | —                                                                                                                                                                              | 204                                                                                                                                         | 403, 404                                                                                                                                                                                                                                                                                                                                                                                                                      | 016, 024                |
| `GET`    | `/project-estimates/`                      | —                                                                                                                                                                              | 200, array of values of the project's linked system; `[]` when unlinked                                                                     | 403 guest/outsider, 401                                                                                                                                                                                                                                                                                                                                                                                                       | 017, 024                |
| `POST`   | `/estimates/{eid}/estimate-points/`        | `{"key", "value"}`                                                                                                                                                             | 200, created value                                                                                                                          | 400 `{"error": "Key and value are required"}`; **no length check** (bug 347)                                                                                                                                                                                                                                                                                                                                                  | 014, 025                |
| `PATCH`  | `/estimates/{eid}/estimate-points/{ptid}/` | `{"key"?, "value"?}`                                                                                                                                                           | 200, updated value                                                                                                                          | 400 over 20 characters; 403                                                                                                                                                                                                                                                                                                                                                                                                   | 014, 024                |
| `DELETE` | `/estimates/{eid}/estimate-points/{ptid}/` | optional `{"new_estimate_id": "<point id>"}` (the field names a replacement value, not a system)                                                                               | 200 with the values whose keys were decremented (not the whole remaining set)                                                               | 403                                                                                                                                                                                                                                                                                                                                                                                                                           | 015, 024                |

System object (read): `id`, `workspace`, `project`, `name`,
`description`, `type` (`points` | `categories` | `time`), `last_used`,
`created_at`, `updated_at`, `created_by`, `updated_by`, `points`.
Value object (read): `id`, `key`, `value`, `description`, `estimate`,
`workspace`, `project`, created/updated fields. Clients rely on `id`,
`key`, `value`, `type`, `name`, `last_used`, and `created_at` (for
archived ordering); everything else is passthrough.

### 7.2 Project and issue writes

| Method  | Path                            | Body                                                             | Success     | Errors                                                                                                                           | Rows          |
| ------- | ------------------------------- | ---------------------------------------------------------------- | ----------- | -------------------------------------------------------------------------------------------------------------------------------- | ------------- |
| `PATCH` | `/projects/{pid}/`              | `{"estimate": "<system id>"}` or `{"estimate": null}`            | 200 project | 403 non-admin (project or workspace), 400 archived project                                                                       | 004, 013, 016 |
| `PATCH` | `/projects/{pid}/issues/{iid}/` | `{"estimate_point": "<point id>"}` or `{"estimate_point": null}` | 204, empty body | 400 point from another project: `{"non_field_errors": ["Estimate point is not valid please pass a valid estimate_point_id"]}` (the check raises a plain-string `ValidationError` in `validate()`, so DRF files it under `non_field_errors`); 400 unknown/non-existent id: `{"estimate_point": ["Invalid pk \"…\" - object does not exist."]}` (the FK field rejects it before `validate()`); 403 guest unless the guest created the item | 017, 021, 023 |

The project read (`GET /projects/{pid}/`) carries `estimate` (id or
null) and is the source of the switch and gating. The issue read
carries `estimate_point` (id or null). History is read from the issue
history endpoint with `?activity_type=issue-property`; estimate entries
are the ones whose `field` starts with `estimate`.

### 7.3 Workspace-level read (no row; recorded for contracts)

`GET /api/workspaces/{ws}/estimates/` returns the linked systems of
every project in the workspace (with values), under workspace
membership, cached for two hours server-side. The old app has consumers: the
shared workspace issue-properties hook calls it under its own workspace
cache key (stale and focus revalidation off) and is mounted by the
all-issues root, the workspace spreadsheet root, the drafts root and the
notifications root, to preload estimate systems for workspace-wide item
piles. The project settings screen never uses it. The new app needs it
only where those surfaces need every project's systems; contracts may
include it, but slices must not use it as a substitute for the project
list on the settings screen.

### 7.4 Cache keys and invalidation (EST-027)

- Systems list: per workspace + project. Old key shape
  `PROJECT_ESTIMATES_<ws>_<pid>`; new query keys are structured per
  workspace/project (the 602 `estimatesKeys.project(ws, pid)`).
- Project row (switch/gating): the shared project query, invalidated
  after link create/link toggle/delete-unlink.
- Issue rows: optimistic update + refetch on the estimate write.
- Refetch triggers for the systems list follow the old client's global
  config: mount, stale data and window focus (no overrides). The new
  read's focus-refetch opt-out is the divergence §2.7 and slice 740
  remove.
- No realtime channel for any of these.

## 8. Reconciliation with NEWFRONT-602 (PR #1848) — satisfied vs remaining

Read from `apps/web_new` on `pi-dash/newfront-602` (not merged at the
time of writing). "Built" means the described behaviour exists and
looks testable in the 602 code; "remaining" names the slice that closes
the difference. The spec above is binding where it differs.

| Rows    | Built in 602                                                                                                                                                                                                                      | Remaining                                                                                                                                                                                                                                               |
| ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| EST-001 | Admin gate comes from the settings shell; screen renders under it; heading and content                                                                                                                                            | Slice 740 proves the member/guest denial carries no estimate controls (the shell gate is shared) and keeps the screen's own `canManage` branch                                                                                                          |
| EST-002 | Four-skeleton loader while the list is pending                                                                                                                                                                                    | Slice 740: a failed read must show an error + retry **instead of** the empty/content below it (602 renders an error paragraph and then falls through to the empty state, which reads as "no systems"; the error branch must return early)               |
| EST-003 | Empty state with title and add action for admins; no action otherwise. Creation is reachable only from the empty state: 602 removed the extra populated-screen button (review pass on `fb0b02f16`) and asserts its absence in a test                                                                                                                                                             | mainly proven by existing tests; slice 740 confirms                                                                                                                                                                                                     |
| EST-004 | Switch reads `project.estimate`; toggles by writing the active id/null; success notices name the direction; disabled while pending (the deliberate divergence in §2.3/§9.3 — the old switch is role-disabled only) and without manage rights | Slice 741: the **error title must contain "Error"** (§2.3) — 602's "Couldn’t change estimates" has neither that word nor a direction, so the EST-004 oracle scenario fails on it as-is; keep the snap-back and add the direction-correct body (bug 341) |
| EST-005 | Active row with name and values joined; time humanized by the shared formatter                                                                                                                                                    | nothing structural; slice 741 covers the time-format branch                                                                                                                                                                                             |
| EST-006 | Archived section with heading, explainer containing "older versions", newest-first ordering, zero controls, **and** an outbound `https://` link in a new tab (added by 602's review fix `35c290cb7`, asserted in its screen test) | nothing structural (603/602 review fix)                                                                                                                                                                                                                 |
| EST-007 | Delete-only active row in OSS; no edit control; zero controls on archived rows                                                                                                                                                    | nothing structural (edit lands with EST-014 in cloud)                                                                                                                                                                                                   |
| EST-008 | Two steps, step counter, back only on step 2, cancel, reset on open                                                                                                                                                               | Slice 742: confirm back discards values, cancel at both steps, no API                                                                                                                                                                                   |
| EST-009 | Points/categories radios only                                                                                                                                                                                                     | Slice 742: build the family catalogue with an edition capability so time can be added by the cloud edition; keep time absent in OSS                                                                                                                     |
| EST-010 | Custom + Fibonacci/Linear/Squares; T-Shirt Sizes; previews                                                                                                                                                                        | Slice 742: add the missing categories "Easy to hard" starter and time starters for cloud; previews must humanize time                                                                                                                                   |
| EST-011 | Rows with edit (pencil + double-click), remove hidden at 2, add hidden at 6, renumber on remove                                                                                                                                   | Slice 742: **real drag reorder** (602 lists rows without a drag source); keyboard move actions; add allocates max+1                                                                                                                                     |
| EST-012 | Per-row problems; submit blocked; submitted gate                                                                                                                                                                                  | Slice 742: cap **20** (602 uses 40 and a "40 characters" message); exact category duplicate comparison (602 lowercases); oracle's pinned message fragments; over-long edits resolve to 20 characters (old app ignores the edit; new app truncates — a recorded divergence, §3.6). 602 already trims on validate and submit, matching the §9.3 trim divergence |
| EST-013 | Create with family name/type/last_used and keys 1..n; success closes + refetch; failure keeps dialog open; no project write                                                                                                       | Slice 742: prove the flag is set and the project link is untouched (§3.8); time never submitted in OSS                                                                                                                                                  |
| EST-014 | — (not built)                                                                                                                                                                                                                     | Slice 743 (cloud): update dialog, PATCH existing ids + point add/delete routing, empty-set failure, persistence across refetch                                                                                                                          |
| EST-015 | — (not built)                                                                                                                                                                                                                     | Slice 743 (cloud): single-value delete confirmation, reassignment/clear, renumber, history caveat                                                                                                                                                       |
| EST-016 | Row delete + confirmation copy; success/failure notices; unlinks only when the deleted id equals the stored project reference (narrow rule, NEWFRONT-809) | Slice 743 + NEWFRONT-809: confirm dialog title/body contain the name and "permanently"; failure body says "unable to delete" (602's copy does not); do not reproduce the old wrong title; **match §4.3** — the old client clears the reference whenever the project is linked (bug NEWFRONT-809), while the new app unlinks only the linked system's own delete and leaves any other link untouched |
| EST-017 | Picker options, clear row, humanized time, optimistic write + rollback                                                                                                                                                            | Issues slices mount it everywhere; slice 744 sorts options by key before rendering (602 maps `points` as received and the server orders by value text), proves the full searchable picker, persistence, clear and history                                                                                                                                          |
| EST-018 | `hideEstimate` flag into issues layout/spreadsheet; create/edit field                                                                                                                                                             | Slice 744/Issues: sidebar, list, drafts, inbox; prove on/off via project flip                                                                                                                                                                           |
| EST-019 | —                                                                                                                                                                                                                                 | Slice 747 (Cycles surface)                                                                                                                                                                                                                              |
| EST-020 | — | Slice 747 (Analytics surface): labels are the raw stored value (a time system charts "90", not "1h 30m"); unestimated items are absent (no None bucket); humanising is a §9.3 divergence 747 may take deliberately |
| EST-021 | Palette command + page behind `estimatesEnabled`                                                                                                                                                                                  | Slice 744: sort options by key (602's palette maps `active.points` as received); prove options, selection, write and absence when off                                                                                                                                                                                         |
| EST-022 | Picker disabled path exists (`disabled` trigger)                                                                                                                                                                                  | Slice 774: peek read-only label with estimates loaded (intended behaviour, bug 349), public-board absence stays                                                                                                                                         |
| EST-023 | Activity model handles `estimate_point` + `estimate_points` + `estimate_categories`, sentence "set the estimate to X"                                                                                                             | Slice 774: whole `estimate_*` family, pinned "set the estimate point to X", removal shape, link                                                                                                                                                         |
| EST-024 | Screen gate + server rules                                                                                                                                                                                                        | Slice 741/743: role-matrix reads and UI absences per role                                                                                                                                                                                               |
| EST-025 | Client validation covers most rules                                                                                                                                                                                               | Slice 742/743: align cap and messages; surface duplicate-name and empty-set failures without changing server behaviour                                                                                                                                  |
| EST-026 | Single route; shell title                                                                                                                                                                                                         | Slice 740: title contains the project name; guessed system id → not-found                                                                                                                                                                               |
| EST-027 | Per-project query key; loader on first read                                                                                                                                                                                       | Slice 740: **drop the read's focus-refetch opt-out** so focus refetching follows the shared client default, as the old global config does; keep the per-project key; no realtime; gate on reload and on focus return                                    |
| EST-028 | No extra controls                                                                                                                                                                                                                 | Slice 740: prove the absences and the screen-text word scan                                                                                                                                                                                             |
| EST-029 | OSS only today                                                                                                                                                                                                                    | Slice 743: edition capability for time/edit/value-delete; desktop same as edition                                                                                                                                                                       |

Re-read at 602's head `fb0b02f16` (the branch was rebased onto
`web-new-dev`; the estimates files are unchanged from `be6a51ee7`).
Divergences in the 602 code to correct (do not keep):
`ESTIMATE_VALUE_MAX_LENGTH = 40` (must be 20);
duplicate comparison lowercased for categories (must be exact);
the missing "Easy to hard" starter and the missing drag source;
the activity sentence ("set the estimate to X" must become "set the
estimate point to X");
the error-state-vs-empty-state ordering;
the enable-failure history's generic title (must contain "Error", §2.3);
the estimates read's focus-refetch opt-out (§2.7).

Already corrected on 602 and **not** remaining: the archived section's
"older versions" explainer with its outbound `https://` reference link
in a new tab (`35c290cb7`, covered by its screen test), and the honest
delete-failure notice that no longer promises the system still exists.

The 40-character cap, lowercased duplicate check, missing starter and
missing drag source are still present at `fb0b02f16`, so those bullets
stand.

## 9. Decisions and known bugs

### 9.1 D21 — position keys on add (accepted divergence)

The old client allocates `length + 1` when adding a value, so removing
one of three and adding another can reuse a key and produce duplicate
keys; the server tolerates duplicates (no unique constraint since
migration 0030), orders values by value text, and the update endpoint
persists whatever keys it is sent. The server has a TODO to reject
duplicate keys.

Decision (NEWFRONT-371): the new app keeps keys unique, contiguous
1..n in display order after remove, and allocates `max + 1` on add. Keys
are not user-visible and the visible order/values are identical, so
parity is unaffected. The area gate must not flag this divergence.
Saved-system edits always send the full 1..n arrangement.

### 9.2 Pinned old bugs — leave as-is, do not spec the fix as current

These behaviours exist in the old app and are pinned by oracle
scenarios. The new app must not reproduce the _defect_ where it is
user-visible in a way parity does not require, but must not pretend the
bug does not exist. Where the oracle pins the absent/broken outcome,
the gate aligns the scenario when the new app implements the intended
behaviour (noted per row).

- **NEWFRONT-341 — enable failure notice names the wrong direction.**
  Turning estimates on and failing still reports "could not be
  disabled". Intended: name the attempted direction. The oracle asserts
  only the error title. New app: direction-correct.
- **NEWFRONT-344 — whole-system delete failure notice is titled
  "Estimate creation failed".** Intended: a deletion failure title. The
  oracle asserts only the correct body ("unable to delete") and that the
  dialog stays open; it does not pin the title. New app: deletion title.
- **NEWFRONT-345 — deleting a value without replacement records no
  history entry.** The worker crashes building `field` from the missing
  replacement (`AttributeError`), so affected items are cleared but no
  entry appears. Intended: one removal entry per affected item.
  Oracle: the absence is pinned within a bounded wait. New app: do not
  fabricate; surface cleared items. Gate: align when the server is
  fixed.
- **NEWFRONT-348 — clearing an item's estimate records no history
  entry.** Same tracking code path on the issue write (`estimate_point`
  null): the value clears, the feed gains nothing. Intended: a removal
  entry. Oracle: the absence is pinned. New app: no fabricated entry;
  render a removal shape when it exists.
- **NEWFRONT-346 — a refused bulk create leaves an empty system row.**
  The system is stored before its points are validated; a 400 leaves an
  orphan with no points. Intended: atomic create. Oracle asserts the 400
  and no stored points, and deliberately does not pin the orphan.
  New app: report the failure and refetch; do not auto-delete (the
  server contract is the fix).
- **NEWFRONT-347 — single-point create skips the 20-character cap.**
  The point-create path stores overlong values (200) while bulk create
  and point update enforce 20. Intended: enforce the cap everywhere.
  Oracle pins the enforcing paths only. New app: client caps at 20; do
  not depend on this endpoint for the cap.
- **NEWFRONT-349 — archived peek shows "None" for a set estimate.**
  The archived peek context never loads the estimates, so a set value
  renders as the empty placeholder; the control is correctly disabled.
  Intended: show the set value read-only. Oracle pins "None" with a bug
  marker. New app: intended behaviour (value visible) — slice 774; the
  gate aligns the scenario (drop the bug marker, assert the value).
- **NEWFRONT-809 — whole-system delete clears an unrelated project
  link.** The old client unlinks whenever the project has any linked
  system, so deleting an unlinked system while the project points at a
  different system clears that other link too. Decided in NEWFRONT-809:
  the new app implements the narrow rule — unlink only when the
  deleted id equals the stored reference (§4.3). Oracle: the old
  broad outcome is pinned by a `bug:` scenario (deleting the unlinked
  active system while linked elsewhere still clears the link); the
  gate aligns that scenario on the new app to assert the preserved
  link instead.
- **NEWFRONT-820 — the estimate picker searches the stored value, not
  the label — RESOLVED.** Time options display "1h 30m" while the old
  search index was the stored `90`, so a label search failed and a
  stored-value search matched text the user could not see. Not pinned
  by an oracle scenario (EST-017/EST-021 use points and categories,
  where stored and displayed text are identical). New app: slice 744
  chose to search displayed labels, and the picker and palette now
  agree (§6.1 resolution, §9.3). The old app is unchanged; no gate
  alignment is needed because no scenario distinguishes the rules.
- **No estimate search.** `estimate-search.tsx` renders a placeholder
  and is mounted nowhere; EST-028 pins the absence of list search. Do
  not build a search field, and do not cite that file as a behaviour
  source.

### 9.3 Deliberate divergences from the old app

These are target improvements, not old behaviour and not bugs. Each
slice that implements one records it in its PR description so review
and the area gate can tell intent from drift. None is pinned by an
oracle scenario, so none can fail the gate on its own.

| Divergence | Old behaviour | New behaviour | Owner |
| --- | --- | --- | --- |
| Switch pending state | Enabled during a write; a second click fires a second update from the same stale reference | Disabled while the write is in flight (§2.3) | 741 |
| Dialog dismissal | Create and delete dialogs mount without a close handler: Escape/backdrop do nothing | Escape/backdrop cancel when idle; ignored while a write is pending (§3.1, §4.3) | 742, 743 |
| Inline editor Escape | No Escape handler; the draft stays | Escape discards the draft like cancel (§3.4) | 742 |
| Over-long edits | The whole change is ignored when it exceeds 20 characters | The change is truncated to its first 20 characters (§3.6) | 742 |
| Value trimming | Raw drafts are validated and sent untrimmed; whitespace-only reads as zero (numeric) or as text (categories), not as empty | A value is trimmed once on commit and that trimmed value is what validation and submit see, so whitespace-only reads as empty (§3.6, §3.7) | 742 |
| Time starter (cloud) | No starter block for cloud families: choosing time is a dead end | Hours template plus a two-value scratch set, stored as minutes (§3.3) | 743 |
| Key allocation on add | `length + 1`, which can reuse a key | `max + 1`, keys contiguous 1..n (D21, §9.1) | 742, 743 |
| Picker search index | The work-item picker searches the raw stored value, so on a time system `1h` misses "1h 30m" and `90` matches a row that never shows it (NEWFRONT-820) | The picker searches displayed labels, agreeing with the palette page; `90` matches nothing unless a row shows it (§6.1) | 744 |
| Analytics estimate labels | Groups carry the raw stored value; a time system charts "90" and the table prints it verbatim | May render the humanised value through the shared formatter if slice 747 chooses so (§6.4) | 747 |

## 10. Desktop and editions (EST-029)

- **No desktop-only estimate code.** The desktop overlay, the instance
  admin app and the cloud overlay's web/admin apps contain no estimate
  screens or controls; the desktop build's estimate behaviour is
  exactly its edition's web behaviour.
- **OSS vs cloud** is the only split, and it is a capability, not a
  copy: points/categories + create + whole-system delete in OSS; add
  time, saved-system editing and single-value delete/reassign in cloud.
  The cloud UI is written from §4.1–§4.2; the OSS screen must render
  neither an edit trigger nor a value-delete confirmation, and must not
  expose a disabled stub that looks like a broken feature.
- **Desktop platform contract.** No `if (isDesktop)` outside
  `core/platform`; the estimate feature talks only to the API client
  and the edition capability. File/network differences (desktop HTTP)
  stay in the platform adapter. External links (the archived
  documentation link) go through the platform's external-open capability
  where one exists, falling back to a normal new-tab link on web.
- **Parity gate wiring.** The area gate (NEWFRONT-748) must run the
  cloud leg of the oracle by bringing the stack up with
  `PARITY_ORACLE_EDITION=cloud` (separate namespace) so the time family,
  edit and value-delete rows are proven against the cloud oracle, and
  the desktop leg by building/running the desktop target against the
  same parity stack (no separate desktop backend). Rows tagged `all`
  must pass in both legs; EST-029 is the row that names this
  obligation. A failure that only exists in one edition is a fix issue
  under the epic, not a skipped scenario.
- The OSS leg continues to prove that time/edit/value-delete are
  **absent** (not broken), and that points/categories create and
  whole-system delete work.

## 11. Definition of done for the slices reading this spec

Each slice: implement its rows as described; cover every changed line
and branch with unit tests; run its package's typecheck/lint/tests plus
`check:boundaries`, `check:headers`, `check:deps`,
`check:coexistence`, the similarity check and the paths gate; append
"built" to its rows' Status cell; no parity stack (the area gate runs
it). Stubs, skipped tests and "coming soon" surfaces for a row in scope
are defects. Where this spec marks a behaviour _pinned_, the label text
is part of the contract; elsewhere write copy in the product's voice.

## 12. Row coverage map

| Row     | Sections      | Oracle scenario                                                                                        |
| ------- | ------------- | ------------------------------------------------------------------------------------------------------ |
| EST-001 | 2.1, 2.2, 5   | `screen-shell.spec.ts` (admin breadcrumb/heading; member/guest denial)                                 |
| EST-002 | 2.2           | `screen-shell.spec.ts` (skeletons during held fetch)                                                   |
| EST-003 | 2.2           | `screen-shell.spec.ts` (empty state + add; unauthorized no action)                                     |
| EST-004 | 2.3, 5        | `enable-display.spec.ts` (flip on/off, member denial, refusal)                                         |
| EST-005 | 2.4           | `enable-display.spec.ts` (name + values; time humanized)                                               |
| EST-006 | 2.5           | `enable-display.spec.ts` (archived read-only, explainer, link)                                         |
| EST-007 | 2.4, 2.5, 5   | `enable-display.spec.ts` (delete-only row; non-admin no controls)                                      |
| EST-008 | 3.1           | `create-dialog.spec.ts` (two steps, back, cancel, reset)                                               |
| EST-009 | 3.2, 10       | `create-dialog.spec.ts` (OSS radio exactly points/categories)                                          |
| EST-010 | 3.3           | `create-dialog.spec.ts` (previews, loaded values, scratch)                                             |
| EST-011 | 3.4, 3.5, 9.1 | `create-dialog.spec.ts` (drag reorder, inline edit, add/remove guards)                                 |
| EST-012 | 3.6           | `create-dialog.spec.ts` (per-value rules, cap, blocked submit)                                         |
| EST-013 | 3.7, 3.8      | `create-dialog.spec.ts` (stored active; link untouched; failure keeps dialog)                          |
| EST-014 | 4.1, 4.4, 5   | `edit-delete.spec.ts` (OSS no edit; PATCH persists; empty set 400)                                     |
| EST-015 | 4.2, 6.7      | `edit-delete.spec.ts` (OSS no value confirm; reassign; clear; bug 345 history)                         |
| EST-016 | 4.3, 5        | `edit-delete.spec.ts` (strip + unlink; cancel; refused delete; bug: unlinked delete clears the other link) |
| EST-017 | 6.1, 7.2      | `consumers.spec.ts` (picker options, assign/clear, history)                                            |
| EST-018 | 6.2           | `consumers.spec.ts` (sidebar + spreadsheet follow the flag)                                            |
| EST-019 | 6.3           | `consumers.spec.ts` (measure options; estimates totals; categories hidden)                             |
| EST-020 | 6.4           | `consumers.spec.ts` (x-axis estimate point; y-axis only Work item; one row per stored value)                                  |
| EST-021 | 6.5           | `consumers.spec.ts` (palette command, page, assignment, absence when off)                              |
| EST-022 | 6.6, 9.2      | `consumers.spec.ts` (bug 349 archived peek; no edit)                                                   |
| EST-023 | 6.7           | `consumers.spec.ts` (set/replace entries and links)                                                    |
| EST-024 | 5, 4.4        | `permissions-edition.spec.ts` (endpoint role matrix; UI per role)                                      |
| EST-025 | 4.4           | `edit-delete.spec.ts` (key/value required; cap; empty update; generated name; duplicate name)          |
| EST-026 | 2.6           | `screen-shell.spec.ts` (single URL, project title, no system link)                                     |
| EST-027 | 2.7, 7.4      | `screen-shell.spec.ts` (stale until fresh fetch)                                                       |
| EST-028 | 2.8           | `screen-shell.spec.ts` (absences and word scan)                                                        |
| EST-029 | 4.5, 10       | `permissions-edition.spec.ts` (OSS families/creates/deletes; no edit/value delete; no desktop runtime); `cloud.spec.ts` (cloud time/edit/value-delete/matrix, green on the cloud build); desktop leg (full 58-scenario suite on the desktop target, every scenario green) |
