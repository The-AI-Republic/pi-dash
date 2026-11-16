> **Superseded.** This note was folded into [`space.md`](space.md) by NEWFRONT-751; that file is the authoritative build spec for the area. Kept for history (the oracle research notes it was written from).

# Area spec: space board filters + issue peek shell (SPC-034–046)

Learned by reading the old space frontend (filter dropdown, applied-filters
strip, navbar controls, layout switcher, peek overlay views, filter/detail
stores, and the public API call sites) plus live runs against the seeded
parity stack. Prose only: no old code, copy, styles, or assets are reused.
The driver extension and the oracle scenarios are written from this note.

## Address parameters (shared)

- The board address carries lowercase filter keys: one for the view, one
  for the open issue, and one per filter facet (priority, state, labels).
  Multi-value facets join their values with commas under a single key.
- On load the board validates the view against the publisher-enabled set,
  falls back to the first enabled view, and rewrites the address to match.
  Filter keys seed the filter store; unknown keys are ignored.
- Clearing filters strips the filter keys while keeping the view key.
- Known defect (oracle pins current, fix tracked as NEWFRONT-463): the
  seeded pills do not narrow the initial load — the first list fetch goes
  out without the address parameters and nothing refetches after the seed,
  so a pasted filtered address shows pills over an unfiltered list. The
  rebuild must send the address parameters with the initial fetch (or
  refetch once the seed lands), not reproduce the dropped fetch.

## Filter panel (SPC-034, SPC-038, SPC-039)

- A Filters control in the board navbar opens a dropdown panel. The panel
  leads with a search box, then one collapsible section per facet the
  active layout supports.
- The panel offers exactly two facets: priority (five fixed options from
  highest to lowest plus none) and state (one option per workflow state,
  fetched from the public states endpoint, initially capped at five rows
  with an expand control past that). Option rows show a checkbox marker,
  a facet-appropriate glyph, and the option name; applied options read as
  checked and each section header counts its applied options.
- No label section is offered: the label options component exists but its
  section is disabled, so labels can never be picked in the panel.
- Every layout maps to the same supported facet set, and the board offers
  only the list and board layouts, so switching layouts keeps the offered
  facets identical. The rebuild must still drive the offered facets from
  the per-layout map rather than hard-coding two sections.
- The panel search narrows option rows across all sections at once (matched
  against the priority keys and the state names); a section with no matches
  shows an empty line instead of rows. Clearing the search restores every
  option. The search never filters issues and never refetches: there is no
  free-text issue search anywhere on the board, and typing in the panel
  leaves the list and the network untouched.

## Applied pills (SPC-035)

- Active filters render as a strip above the layouts: one shell per facet
  holding that facet's key label, one pill per applied value (priority
  pills show the priority marker plus its key; state pills show the state
  glyph plus the state name), a per-facet clear control, plus one global
  clear-all control. The strip unmounts when no filter is active.
- Three removal levels: one pill, one facet, everything. Every removal
  updates the store, rewrites the address, and refetches the list from
  page one, exactly like picking an option in the panel does.
- Known defect (oracle pins current, fix tracked as NEWFRONT-464):
  removing one state value while others remain rewrites the survivors
  under a plural key the loader never reads, so the live board stays
  filtered but a reload or share drops the state filter. Priority and
  label removals use the correct singular keys.

## Address-only label filter (SPC-037)

- Intended: a labels key in the address narrows the list fetch
  server-side like any other facet, even though the panel offers no label
  options. Current: same dropped-initial-fetch defect as above
  (NEWFRONT-463) — the server honors the key when sent, but the app never
  sends it on load. The rebuild implements the intent.
- The strip renders the labels facet as a bare shell: the facet label plus
  its facet-level clear, with zero value pills. The shell clears through
  the same facet-level control as any other facet.
- Resolves inventory open confirmation (2): the shell is exactly the
  facet label plus the clear control, no pills, no names.

## Peek overlay shell (SPC-040–042)

- An address naming an issue opens the board with that issue's overlay
  already mounted; the board loads behind it. The overlay fetches the
  issue detail (plus its engagement data) on mount.
- Three presentations share one overlay: a half-width side panel, a
  centred modal over a dim backdrop, and a near-full-screen view with the
  properties in a side column. A header switcher moves between them while
  keeping the same issue open and the same address; the default is side.
- Closing drops only the issue key from the address: the view and every
  filter key survive, and the board underneath is untouched. Each
  presentation closes through its own mechanism (header control in side
  view, backdrop click in modal, keyboard dismiss everywhere); all three
  funnel into the same close path.
- Opening an issue from a card keeps the view but drops the filter keys
  from the address (adjacent observation for the layouts area, which owns
  card behavior; the store keeps filtering until reload).

## Overlay content (SPC-043–044)

- The detail block shows the public key (project short code plus sequence
  number), the title, and the formatted description rendered read-only.
  Issues without a description render no description block at all (neither
  an empty editor nor a placeholder); the empty-content marker the API
  returns for undescribed issues counts as empty.
- The property panel shows state (group glyph plus spaced name), priority
  (marker plus translated label inside a tinted chip), and due date. A
  missing due date renders an explicit empty marker. A due date on or
  before today on an incomplete issue renders with overdue emphasis;
  future dates and completed/cancelled states never emphasise.

## Copy link + notices (SPC-045)

- The overlay header offers a copy action only while the browser grants
  clipboard-write permission, and only in the side and modal
  presentations: the near-full-screen header never shows it. The
  properties column holds a second copy control that nothing renders (its
  enabling prop is never passed), so it is dead UI.
- Copying writes the current address to the clipboard and raises a short
  confirmation notice naming the copy. Denied permission hides the action
  instead of showing a disabled control.

## Loading + activity gating (SPC-046, confirmation 3)

- While the detail fetch is pending, the overlay shows skeleton
  placeholders (text rows in the detail column, property rows in the
  full-screen side column) instead of half-rendered content; the real
  content replaces them on resolve.
- The side view mounts the activity (comments) section only while the
  publisher's comment flag is on; the full-screen view mounts it
  unconditionally, even with commenting disabled (showing the thread plus
  the signed-in-out prompt). That contradicts the engagement row's "disabled
  hides the whole section", so the oracle pins it as a defect (NEWFRONT-461,
  filed by the engagement area which owns the target behavior). Resolves
  inventory open confirmation (3).
- Loading detail: the overlay mounts instantly with skeletons while the
  issue data loads, and the list fetch can seed the overlay store ahead
  of the detail fetch (list rows carry no description, so the description
  block only appears once the detail lands).
