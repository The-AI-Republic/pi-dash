# Issues — filters & display options: area spec (NEWFRONT-119)

Rows ISS-075–ISS-107. Written from reading the old issues headers, the
display-filter sections, the legacy per-dimension filter sections, the
rich-filter row/toggle/value inputs, the work-item filter HOC pair, and
the applied-filter chips — plus the inventory Acceptance column. No old
code, strings, styles or assets reused. Oracle scenarios are written
from this spec and proven against the running old app.

## Where filtering lives

Every issues surface pairs two independent systems:

- A display/arrangement control (grouping, ordering, which fields show)
  stored per user per entity on the server.
- A row filter control (which work items match), in two generations:
  the newer condition-row builder present on all main issues headers,
  and an older per-dimension dropdown family still used on a few
  secondary surfaces.

Both are personal: changing them never rewrites the URL, and returning
to the page restores the stored set. The only portable filter set is a
saved view.

## Display options (project issues header and siblings)

A header popover groups the arrangement controls into collapsible
sections. Which sections appear depends on the active layout and page:
a table-like layout offers ordering plus the sub-item switch only,
while calendar-style layouts offer almost nothing. Sections render in a
fixed order: shown fields first, then grouping, sub-grouping, ordering,
then the on/off switches.

- Shown fields: one pill per field (identifier, people, dates, labels,
  priority, state, counts of sub-items/links/attachments, estimate,
  cycle, module and more). Only pills the layout allows are offered;
  calendar-style layouts allow barely any; cycle/module pills vanish
  when the project disables those features.
- Grouping: single choice among the dimensions the layout supports.
  Picking one regroups the list/board immediately. On the board layout
  the dimension already used for sub-grouping is hidden here.
- Sub-grouping: board layout only, visible only once a grouping is
  chosen; mirrors the grouping control with the chosen grouping hidden.
- Ordering: single choice among the layout's sort orders with a stable
  newest-first default; hidden where the layout fixes its own order.
- Switches: show sub-items inline (on by default; the table layout
  always behaves as off) and show empty groups (purely local; hiding
  them drops zero-item sections from list and board).

Every change persists to the per-user per-entity record and survives
reload; the board/list regroups without a page navigation.

## Older per-dimension filters (secondary surfaces)

Where still offered, each dimension is a collapsible section with a
checkbox (multi-choice) or radio (single-choice) row set, an applied
count in its title, a shared search box, a five-row cap with an
expand/collapse control, and an explicit empty placeholder when search
matches nothing. Applied rows sort above unapplied ones; member rows
put the signed-in user first.

Dimensions: workflow state (with group icon and loading skeletons),
state group (fixed five), priority (fixed five), assignee and creator
(member pickers with avatars), mentions (member picker over mentioned
items), labels (colored dots), cycle (status icons, running cycle
pulses), module, project (cross-project views only), start/due dates
(preset relative windows plus a custom range dialog; custom entries are
detected by their dash and cleared by unticking the custom row;
presets and custom combine), and subscriber (the personal work-items
page only).

## Active-filter chips

Above the list, one chip per active value shows a marker (icon, avatar,
color swatch) plus its label; date chips spell out preset names or
formatted custom ranges. Each chip carries its own remove control where
the page is editable. Chips whose backing record vanished (deleted
state, departed member) are skipped silently rather than shown broken.

## Condition-row builder (main headers)

A header toggle button doubles as the entry point: with no conditions
and nothing unsaved it reads as an add control opening the field
picker; otherwise it shows/hides the builder row, switching to an
accented icon while conditions exist or the view has unsaved edits.

Each condition is a three-part pill: the field (click re-picks it and
re-derives operator and value), the operator (searchable; locked when
only one applies), and the value slot, plus a remove control. Unknown
fields (deleted property, lost access) render as an error pill with a
tooltip and remove-only behavior.

Adding a field happens through a searchable picker listing only
not-yet-used fields; the new condition joins with AND using that
field's default operator (negated when the default is a negation).
When every field is used the picker shows a single disabled
exhaustion row. A misconfigured field reports an error toast and adds
nothing.

Operators come in plain and negated pairs, stored as a base operator
plus a negation flag. Value slots adapt to the field kind: searchable
single pick (re-picking the current value clears it; opens by itself
when empty; dash placeholder when unset), searchable multi pick (first
two picks as chips, overflow counted; same empty behavior), single date
(bounded picker, always set, never clearable), and date range (both
ends required before it commits; visibly warned while incomplete).

The row's right side offers clear-all plus view actions: saving
prefills a create-view dialog with the current expression and display
settings; updating appears only for the owner of an unlocked,
changed view. View creation targets the workspace or project view
collections; workspace default views refuse updates.

Builder instances are either persisted (bound to an entity id) or
temporary (ephemeral ids that die with the component); their per-field
configurations register on mount and release on unmount.

Oracle deviations (observed live on the dev stack; the rebuild follows
the rows, not these bugs): the header toggle is inert in both
directions, so the row follows the stored expression instead — a
non-empty expression shows it on load and clearing the last condition
hides it (NEWFRONT-146). Single-operator properties lock their
operator control, so no negated form is offered (NEWFRONT-149). No
chip bar renders on issues surfaces; removal runs through each
condition's own control (NEWFRONT-148). The legacy dropdown chrome
(ISS-082–095 Acceptance) renders on no issues surface — the picker
plus the row is the only filter UI (inventory correction pending with
the tracker parent). Operators render lowercase; joined conditions
show no AND text; valueless conditions are UI-ephemeral and collapse
away on the next expression write.

## Analytics entry

The issues header offers an analytics entry that opens a work-items
analytics dialog scoped to the current project. It is hidden from
people who cannot create work items.
