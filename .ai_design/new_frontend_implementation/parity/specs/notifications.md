# Notifications area spec — inbox, detail, snooze, email preferences (NTF-001–032)

Parent epic NEWFRONT-76. Inventory
`.ai_design/new_frontend_implementation/parity/inventory/notifications.md` (rows
NTF-001–032, all oracle green). Oracle scenarios
`apps/web_new/e2e/parity/notifications/` (25 spec files, green on `apps/web`).

Method: the old frontend was read as a behavior reference only — the
notifications route, the workspace-notifications component tree (core plus the
cloud-edition override), the notification stores and hooks, the
workspace-notification service, the preference methods of the user service,
and the notification constants and types. Nothing is imported, copied or
translated from those sources; this spec restates what was learned in prose.
The only code-like tokens below are API paths, query parameter names and
payload field names, which the contracts issue (NEWFRONT-556) turns into zod
contracts. New code is written from this spec, the wiki pages (Reference not
source; Parity; Architecture; Data layer; UI kit and editor) and the kit —
never with old files open.

Status convention: this spec owns no inventory Status changes. Slices append
`built` to the rows they implement.

Where the running old app contradicts the inventory row text, the oracle
pinned the observed behavior and filed a bug issue. Those deviations are
recorded in §13. Slices implement the target stated in each section below;
the gate proves the target.

## 1. Inbox entry and two-pane shell (NTF-001, slice NEWFRONT-559)

The inbox lives at the workspace notifications address
(`/{workspaceSlug}/notifications/`). Entering it loads two things in
parallel: the notification list for the default stream and the unread totals
for the workspace. The document title names the workspace alongside the
inbox. Oracle: `inbox-shell.spec.ts`.

The screen is a two-pane view: a notification list on one side and a detail
area on the other. On a desktop-width viewport both panes are visible at
once; the list pane keeps a modest fixed share of the total width (between
roughly one-sixth and one-third) and the detail pane takes the visibly larger
remainder. Selecting a card does not move the split — pane widths stay
effectively unchanged after selection. On a narrow handset-width viewport the
list collapses away once a card is selected and the detail takes the screen.
Entering the inbox fresh always lands on the unselected view: any previous
selection is cleared on entry, restoring the full list width. No selection,
tab or filter state is carried in the address (see NTF-027 in §11).

Reads on entry: `GET /api/workspaces/{workspaceSlug}/users/notifications`
for the list and `GET
/api/workspaces/{workspaceSlug}/users/notifications/unread/` for the totals.

## 2. Stream tabs and unread totals (NTF-002, NTF-004, slice NEWFRONT-559)

The list pane carries exactly two tabs: the full stream, active by default,
and a mentions-only stream. Switching tabs discards the current list and
loads the matching stream from its first page; the two streams never mix. The
mentions stream is the same list endpoint with a mentioned-only flag, and
every row it returns is mention-marked while no row of the default stream is.
Oracle: `tabs-badges.spec.ts`.

Each tab shows its own unread badge. The full-stream badge mirrors the
overall unread total and the mentions badge mirrors the mention unread total;
a tab badge hides when its count reaches zero. The totals come from the
unread-count endpoint, whose payload carries an overall unread count field
and a mention unread count field (`total_unread_notifications_count`,
`mention_unread_notifications_count`). Tab badges are the exact-count
surface of the inbox: they always agree with the server totals. Reads: the
entry list fetch plus the unread-count read above. The oracle pins tab
identity by position and stream key, not by visible label wording, so labels
may be redesigned.

## 3. Navigation badge seam (NTF-003, slice NEWFRONT-559)

Capability: from anywhere in the workspace, the member sees at a glance
whether anything is unread, with unread mentions distinguished, and sees
nothing once everything is read. The oracle proved the old app only partly
does this: with an empty inbox there is no marker anywhere, and with unread
items a presence dot appears — but the intended count badge (mentions count
with a distinct marker when mentions are unread, otherwise the total unread
count, hidden at zero) never renders. That gap is filed as NEWFRONT-204 and
recorded in §13.

Ownership split with the shell epic (NEWFRONT-70, Done), matching shell spec
§10 from the notifications side:

- The notifications area owns the unread-count source: the workspace-scoped
  query keys, the unread-count query options, the API contract, fetching
  semantics (stale time, enabled only with a workspace, a failing count never
  fails the shell), the fixtures that produce unread and mention states, and
  the mentions-priority selection rule as a unit-tested data function that
  prefers the mention total when positive and falls back to the overall
  total. The foundations already exist in the new tree
  (`features/notifications/api/keys.ts`, `api/queries.ts`) and slices extend
  them; the shell never duplicates them.
- The shell owns all chrome rendering: the top-bar inbox entry linking to
  the notifications address with its presence dot, and the rule that the app
  sidebar stays unmounted on the notifications route itself. The dot appears
  whenever either total is positive and disappears at zero.
- Exact counts with the mentions distinction live in the inbox tab badges
  (§2), which the notifications slices own end to end.

Gate proof for this row: the top-bar dot appears on unread arrival and
disappears at zero; per-tab badges match the server totals exactly;
unit tests pin the mentions-priority selection rule. If NEWFRONT-204 is
later fixed with a count badge in chrome, it consumes the same registered
source with no new seam.

## 4. Notification list cards (NTF-005, NTF-006, slice NEWFRONT-559)

Cards list newest first. Each card summarizes one notification with the same
four elements: who acted (the triggering member), a human-readable summary
of what changed, the affected work item's reference (project key plus
sequence number) with its title, and a relative age. The summary sentence
covers the full change vocabulary the extension map supports: assignment and
assignee changes, start and due date changes, labels, parent changes,
relations, comments (with an excerpt), archiving and restore, attachments,
description excerpts, estimates, state changes and direct assignment notices.
Change kinds outside that vocabulary fall back to a generic verb-based
sentence plus the raw new value. Cards that cannot be resolved — missing
workspace, project, item, actor or change description — render nothing rather
than a broken row. Oracle: `list-cards.spec.ts` (which exercises comment-type
cards for ordering, fields and server/UI title agreement; per-type summary
wording is inventory-described, not oracle-pinned per type).

Unread cards are distinguishable at a glance: each carries a marker dot and a
tinted background, and all unread cards share the same background. Reading a
card clears both. The selected card is highlighted. Read state comes from the
list payload's read timestamp field (`read_at`): null means unread.

Reads: `GET /api/workspaces/{workspaceSlug}/users/notifications` with the
stream flags, a page-size parameter and a cursor. Card fields consumed from
each row: the trigger details, the activity payload (change field, verb, old
and new values), the item payload (identifier, sequence number, title,
project and workspace references, inbox/triage flag), the creation timestamp,
and the state flags (read, archived and snooze timestamps).

## 5. Detail pane (NTF-007, NTF-008, NTF-009, slice NEWFRONT-561)

Selecting a card opens its detail in the detail pane and, only on the first
open of an unread card, marks it read in the background: one write to the
per-item read endpoint, after which the card clears its unread marker, the
server read timestamp is set, and the totals and tab badges drop. Reopening
an already-read card issues no write and changes no counts. Oracle:
`detail-select.spec.ts`.

Triage-queue items embed the full triage detail view; ordinary work items
render the peek overview. Because resolving which view applies requires
project access information, the pane shows a loading indicator while that
lookup runs. With no selection the pane shows a neutral placeholder —
including on first entry before any card is picked. Closing the preview
clears the selection and returns to the placeholder. For ordinary items the
selection additionally opens the peek overlay unless one is already open;
unmounting the inbox clears any open peek state. The detail decision and its
seam are resolved in §12. Oracle: `detail-views.spec.ts`,
`detail-placeholder.spec.ts`.

Reads and writes: the project membership lookup for the triage branch plus
the existing issue/triage reads owned by their areas; `POST
/api/workspaces/{workspaceSlug}/users/notifications/{id}/read/` on first
open only.

## 6. Pagination, loading and empty states (NTF-010, NTF-011, NTF-012, slice NEWFRONT-561)

Long histories page through an explicit next-page control at the bottom of
the list. The control appears only when the server says another page exists
(`next_page_results` in the list envelope); activating it appends the older
chunk to the existing list, showing a loading label while the fetch runs.
The list envelope carries the rows plus pagination info
(`next_page_results`, `next_cursor`, total count); page size is fixed and
large (300 rows per page) and the cursor is opaque — the first load uses the
initial cursor and each continuation uses the stored next cursor, with newer
pages never overlapping older ones in time. A short stream shows no control
at all. Oracle: `pagination.spec.ts`.

While the inbox list loads for the first time, skeleton placeholder rows
stand in for the cards; they are replaced by cards or by an empty state once
the entry fetch lands. Each tab has its own empty illustration and message —
one variant for the full stream, a different one for mentions — and the empty
state clears as soon as items arrive. A failed first list load surfaces no
inline error or retry control (see NTF-031 in §11); the pane reads as the
per-tab empty state until a later refresh recovers. Oracle:
`loading-skeletons.spec.ts`, `empty-states.spec.ts`.

Reads: the list endpoint with the page-size and cursor parameters.

## 7. Refresh and mark-all-read (NTF-013, NTF-014, slice NEWFRONT-562)

A header refresh control re-reads the current stream from the server and the
list reflects the latest server state afterwards. Refresh is single-flight:
repeat presses while a fetch runs are ignored and the control shows progress
until the fetch lands. Arrivals that land while the inbox sits open stay
invisible until the member refreshes, switches tabs or filters, or re-enters
(see NTF-028 in §11). Oracle: `refresh.spec.ts`.

A header mark-all-read control marks every notification in the current filter
scope as read at once. The request posts the current scope to `POST
/api/workspaces/{workspaceSlug}/users/notifications/mark-all-read/`: the
origin set joined into `type` in the list's vocabulary (absent when no
origin is active; `subscribed` is its own value), plus the snoozed and
archived flags (the old app also carried a `read` flag when its unread-only
mode was on). That flag is inert — the bulk endpoint only ever reads unread
rows — and the new inbox does not send it. While
it runs the control shows progress and repeat presses are ignored. Afterwards
every cached notification in scope carries a read timestamp, the current
tab's unread counter zeroes (other tabs' counters are untouched), and the tab
badges and server totals agree at zero for the scope. Oracle:
`mark-all-read.spec.ts`. (The row's "navigation counts reset" phrasing is
unprovable against chrome that renders a presence dot rather than counts —
see §3; the tab badges plus server totals are the pinned proof.)

Scope fidelity is its own row, NTF-032 (§7a). The server's bulk endpoint does
not yet share the list's origin vocabulary, so the old app widens a
subscribed-only or multi-origin bulk read to the whole workspace; that
deviation is recorded in §13 under NEWFRONT-665.

Reads and writes: the list endpoint with the current-view cursor for
refresh; the mark-all-read endpoint with the scope body for bulk read.

## 7a. Bulk-read scope fidelity (NTF-032)

The bulk read must cover exactly the stream on screen. With one origin
active, unread items outside that origin stay unread; with several active,
the union of those origins is read and anything else stays unread. The
control is never allowed to reach beyond the visible scope.

The old app only partly does this, because its two notification endpoints
use different origin vocabularies. The list treats `type` as a comma-joined
set of `assigned`, `created` and `subscribed`, where `subscribed` means
following the item without having created it or been assigned to it. The
bulk endpoint compares `type` against one value and recognizes only
`assigned`, `created`, and its own name for subscriptions (`watching`); its
subscription branch lacks the list's created/assigned exclusion, and the
read flag it receives is never used. A subscribed-only scope therefore
matches no branch, and a two-origin scope such as `assigned,created` matches
none either, so both widen the read to every unread item in the workspace.
Only a lone `assigned` or `created` narrows. The inbox flips only the cached
pages it has in scope, so the widening becomes visible on the next read.

Target: the bulk read honours the joined origin scope exactly like the list,
including the list's subscription rule. Until the backend contract settles
(§13, NEWFRONT-665), the inbox keeps posting the list value unchanged — it
must not invent a translation that disagrees with the list. The intended
fix is on the server side: accept the list's comma-joined vocabulary and
treat `subscribed` with the list's rule. Oracle: a `bug:` scenario in
`mark-all-read.spec.ts` pins the observed widening for a subscribed-only and
a two-origin scope.

Reads and writes: the list endpoint for the visible stream; the bulk
endpoint with the same origin scope.

## 8. Origin filters and stream modes (NTF-015, NTF-016, NTF-017, slice NEWFRONT-562)

A filter menu offers three origin options — items assigned to the member,
items created by the member, items the member subscribed to — as multi-select
rows with checkmarks. Each active origin appears as a removable chip above
the list alongside a clear-all chip that resets all three. Selecting origins
joins them into the list endpoint's origin parameter in a fixed order; any
change — toggling an option, removing a chip, clearing all — discards the
cached list and reloads from the first page. The entry query carries no
origin and no read flag. Oracle: `origin-filters.spec.ts`. One deviation is
recorded in §13: an item matching two origins currently appears under a
single-origin filter for the other origin (NEWFRONT-212); the target is
strict per-origin isolation.

An overflow menu offers three mutually independent-looking modes — show
unread only, show archived, show snoozed — where the archived and snoozed
modes exclude each other: enabling one disables the other. Unread-only sends
the read flag set false; archived mode sends the archived flag true with the
snoozed flag explicitly false and shows only archived items; snoozed mode is
the mirror. Every mode change, like every filter change, restarts the stream
from the first page. The default stream carries explicit archived-false and
snoozed-false flags and contains neither archived nor snoozed items; items
due to return in the future may still arrive in the payload and are hidden on
the client. The unread-only mode is server-driven (no client-side read
filter). Oracle: `stream-modes.spec.ts`.

Reads: the list endpoint with the origin, read, archived, snoozed, mentioned
and cursor parameters.

## 9. Per-card actions and snooze (NTF-018, NTF-019, NTF-020, NTF-021, NTF-022, NTF-023; slice NEWFRONT-563)

Card actions stay hidden until the card is hovered; hovering one card reveals
only that card's actions, in read-then-archive order, and activating an
action never selects the card. There are no keyboard, drag or export paths to
these actions (see NTF-029 in §11). Oracle: `card-read-archive.spec.ts`.

The read toggle flips one card between read and unread (`POST` the per-item
read path to mark read, `DELETE` it to mark unread), adjusts the unread
totals, and reports a confirmation toast naming the outcome. The archive
toggle works the same way over the per-item archive path: archiving removes
the card from the default stream (it reappears in archived mode) and
unarchiving returns it. Both updates are optimistic with rollback — a failed
write leaves the card, the counts and the server state unchanged and shows
no success toast, and a retry succeeds. Oracle: `card-read-archive.spec.ts`.

The snooze picker offers exact preset delays — one day, three days, five
days, one week, two weeks — measured from the moment of snoozing, plus a
custom option and, for already-snoozed cards, a remove-snooze entry.
Snoozing writes the resume timestamp through the per-item update endpoint
(`PATCH .../notifications/{id}/` with the snooze-till field); unsnoozing
clears it. A snoozed card leaves the default stream with a confirmation
toast and is visible in snoozed mode; unsnoozing returns it. A failed snooze
write rolls the optimistic removal back. Oracle: `snooze-presets.spec.ts`,
`snooze-resume-label.spec.ts`.

The custom option opens a dialog with a date section and a time section. Both
a date and a time are required — submitting with either missing keeps the
dialog open. Dates may not precede today. Times come in half-hour steps with
a meridiem switch; when the chosen day is today, already-elapsed slots are
not offered (and when none remain the dialog says so). One deviation is
recorded in §13: noon and midnight compositions are currently swapped
(NEWFRONT-213); the target composes twelve-hour picks to the correct
instant. Submitting snoozes to that moment with a confirmation toast and
closes the dialog. Oracle: `snooze-custom.spec.ts`.

A snoozed card shows its scheduled resume moment in place of its age, in a
fixed month-day-year plus twelve-hour time shape. Oracle:
`snooze-resume-label.spec.ts`.

Writes: per-item read/archive endpoints for the toggles; the per-item update
endpoint with the snooze-till field for snooze and unsnooze.

## 10. Email preferences (NTF-024, NTF-025, NTF-031, slice NEWFRONT-575)

A notifications page under the profile settings hosts email delivery
controls. While the current settings load, a loader stands in; then one
toggle per topic appears: property changes, state changes, comments and
mentions, with a completed-only refinement nested under the state-change
toggle. Each toggle saves instantly and independently — flipping one writes
just that key and reports a success toast, or an error toast with the prior
value kept. The form re-syncs when fresh settings arrive. Oracle:
`email-preferences.spec.ts`. One deviation is recorded in §13: on a failed
save the toggle currently stays on the unsaved value instead of springing
back (NEWFRONT-211); the target reverts the toggle to the persisted value.

Reads and writes: `GET /api/users/me/notification-preferences/` to load;
`PATCH` the same path with the single changed key per toggle. Topic keys:
property change, state change, comment, mention, and the completed-only
refinement.

Delivery effect and its seam are resolved in §12: the frontend's job ends at
persisting the toggle. Disabling a topic stops its emails while inbox items
keep arriving; enabling resumes them. The oracle proved the property topic
end to end and found the comment topic leaking through while disabled
(NEWFRONT-215, recorded in §13); the target is independent per-topic gating.

Failure feedback stays lightweight by design: preference saves report
through toasts, and a failed list fetch shows no inline error or retry
control — the list reads as empty until a refresh recovers. Oracle:
`failure-feedback.spec.ts`.

## 11. Cross-cutting rows (slices NEWFRONT-559/561/562/563/575)

Equal access for every workspace role (NTF-026, slice NEWFRONT-559). The
notification sources contain no role gates: administrators, members and
guests alike open the same inbox, act on cards and edit their own email
preferences through the same endpoints with no role-specific variants. Guests
see only what they can resolve — cards for items a guest cannot read are
skipped exactly like any other unresolvable card, so guest fixtures need a
project the guest may actually open. Oracle: `roles-access.spec.ts`.

No shareable selection link (NTF-027, slice NEWFRONT-561). Selection,
tab and filter state live only in client state: select, tab and filter
changes leave the address clean, reloading with a card open returns to the
unselected view, and revisiting the inbox address shows the placeholder with
the list length unchanged. The slices build no route parameters for
selection, filter or tab. Oracle: `selection-links.spec.ts`.

No live push (NTF-028, slice NEWFRONT-562). The list and badges update only
on explicit refresh, tab or filter change, or remount: server-confirmed
arrivals stay invisible until the member acts. The notification code holds no
socket, polling or event source; the slices build none. The general
real-time question stays open on Decisions and open questions, but for this
area the oracle proved pull-only and the target is pull-only. Oracle:
`no-live-push.spec.ts`.

No keyboard shortcuts, drag-and-drop or export/import (NTF-029, slice
NEWFRONT-563). Probing keys, dragging cards and searching the list pane and
overflow menu for export, import or download controls all change and show
nothing; every inbox action goes through the visible pointer controls. The
slices build none of these paths. Oracle:
`no-shortcuts-drag-export.spec.ts`.

No edition or desktop-only behavior (NTF-030, slice NEWFRONT-559). The same
screens, cards and preferences serve every build over the same endpoints:
inbox entry fires exactly the list and unread-count requests and nothing
else, the preference keys are the same five on every edition, and the
notification code never asks for native notification permission. The
cloud-edition content extension point ships empty — an empty map — so there
is a single implementation with no edition fork: card rendering always falls
back to the shared field map plus the generic verb-based sentence. See §12.
Oracle: `edition-desktop-parity.spec.ts`.

## 12. Cross-epic seams (resolved)

NTF-003 navigation badge — the shell owns the chrome. The notifications
slices register the unread-count source (workspace-scoped keys, query
options, contract, fetching semantics, fixtures, and the unit-tested
mentions-priority selection rule) and own the inbox tab badges end to end;
the shell renders the top-bar inbox entry with its presence dot off the
registered source and keeps the sidebar unmounted on the notifications
route. Neither side duplicates the other's part. This matches shell spec §10
from the notifications side. Proven at gate time by the dot behavior, the
exact tab badges and the selection-rule unit tests (see §3).

NTF-008 detail pane — consume, do not build. The new tree already has the
shared preview pieces: the shell owns the overlay preview panel in the
shared kit (built for SHELL-016 and reused by home recents), and the issues
area owns triage content and the full peek overview with their data fetching.
The notifications slices therefore build no shared preview host and no
triage or peek internals. Each slice renders at most a thin detail host
inside the inbox detail pane — selection to loading indicator to content
switch to placeholder — and hosts the issues-owned triage and peek content
components through their public feature interface, exactly as the home
recents reuse the shared panel today. Until the issues area ships those
pieces, the detail branch stays gate-provable with stub content behind the
same host contract; triage versus peek routing, access-loading and
close-clears-selection are proven through the host. Owned by other areas and
never duplicated here: triage view internals and membership semantics,
peek overview internals and issue-data fetching, the overlay panel shell.

NTF-025 delivery effect — persist only. The frontend reads and writes the
preference flags over the preferences endpoints and does nothing else: no
sending, templating, scheduling or mailer logic lives in the frontend. The
mailer consumes the persisted flags outside the frontend. Proven at gate
time by the toggle persistence plus the server flag state; the email-log
observation in the oracle is reference behavior, not frontend surface. See
§10 and the NEWFRONT-215 deviation in §13.

NTF-030 cloud-edition extension point — ships empty. The old
cloud-edition content map is an empty map with rendering falling back to the
shared field map and the generic sentence, which the oracle confirmed
produces no visible difference. The new code is therefore a single
implementation with no edition fork and no desktop-shell integration
(badges, native toasts): one inbox, one card renderer, one preferences page
for every build. Proven at gate time by identical entry requests, identical
preference keys and zero native-permission calls on every edition (see §11).

## 13. Recorded deviations (old bugs, linked issues)

Each item below is an observed old behavior the target intentionally differs
from. None is encoded silently; each names its tracker.

- NEWFRONT-204 (NTF-003): chrome shows only a presence dot, never the
  intended mentions-priority count badge. Target: the at-a-glance signal via
  the shell dot plus exact per-tab counts, with the mentions-priority rule
  implemented and unit-tested as the registered selection function (§3).
- NEWFRONT-211 (NTF-024, NTF-031): a failed preference save keeps the server
  value and toasts failure, but the toggle stays on the unsaved value.
  Target: the toggle reverts to the persisted value (§10).
- NEWFRONT-212 (NTF-015): an item matching two origins appears under a
  single-origin filter for the other origin. Target: strict per-origin
  isolation (§8).
- NEWFRONT-213 (NTF-021): noon and midnight compositions are swapped in the
  custom snooze dialog. Target: twelve-hour picks compose to the correct
  instant (§9).
- NEWFRONT-215 (NTF-025): disabling the comment topic does not stop its
  emails while the property topic stays on. Target: independent per-topic
  gating consumed by the mailer (§10, §12).
- NEWFRONT-665 (NTF-032): bulk mark-all-read uses a different origin
  vocabulary from the list, so a subscribed-only or multi-origin scope
  widens to every unread item in the workspace; its subscription rule also
  does not exclude created/assigned items, and it ignores the read facet.
  Target: the bulk read honours the list's joined origin scope and
  subscription rule (§7a). The backend fix lives outside the frontend
  allowlist; the inbox keeps posting the list value unchanged until it lands,
  and the `bug:` scenario pins the old widening.

Smaller oracle-vs-inventory notes, all honored above: tab identity is pinned
by position and stream key rather than label wording (§2); only comment-type
cards are oracle-exercised while the summary vocabulary comes from the
extension map (§4); the default stream sends its exclusion flags explicitly
and hides future-snoozed arrivals on the client (§8); elapsed-slot hiding is
proven for the early-morning slot with time-dependent skip (§9).

## 14. Endpoint reference (contracts issue NEWFRONT-556)

Every endpoint the area uses, with the parameters and fields the slices and
contracts build to. All workspace-scoped paths interpolate the workspace
slug; all per-item paths interpolate the notification id.

- List: `GET /api/workspaces/{workspaceSlug}/users/notifications` (no
  trailing slash). Query: origin (comma-joined enabled origins, absent when
  none), snoozed flag, archived flag, read flag (false only in unread-only
  mode), mentioned flag (true only on the mentions tab), page size
  (`per_page`, fixed 300) and cursor (initial cursor for fresh loads, stored
  next cursor for continuations). Envelope: result rows plus pagination info
  with the next-page flag and next cursor.
- Unread totals: `GET
  /api/workspaces/{workspaceSlug}/users/notifications/unread/`, no
  parameters. Response fields: overall unread count and mention unread count
  (`total_unread_notifications_count`,
  `mention_unread_notifications_count`).
- Mark one read: `POST .../users/notifications/{id}/read/`; mark one unread:
  `DELETE` the same path. No body.
- Archive one: `POST .../users/notifications/{id}/archive/`; unarchive one:
  `DELETE` the same path. No body.
- Snooze and unsnooze: `PATCH .../users/notifications/{id}/` with a partial
  body carrying the resume timestamp field (`snoozed_till`) set or cleared.
- Bulk mark read: `POST
  /api/workspaces/{workspaceSlug}/users/notifications/mark-all-read/` with a
  body mirroring the current list scope (origin, snoozed and archived
  flags). The origin value is the list's comma-joined vocabulary, including
  `subscribed` on its own. The endpoint currently recognizes a different
  vocabulary and ignores read (NEWFRONT-665, §7a); the client still posts
  the list value verbatim.
- Email preferences: `GET /api/users/me/notification-preferences/` to load;
  `PATCH` the same path with the single changed topic key per toggle
  (property change, state change, comment, mention, completed-only
  refinement).

No other endpoint is called by the inbox or the preferences page: entry
fires exactly the list and unread-count requests, and the notification code
holds no socket, polling or native-permission calls. The contracts issue
covers any of the above not already under contract; the unread-count read is
already contracted and consumed by the shell.
