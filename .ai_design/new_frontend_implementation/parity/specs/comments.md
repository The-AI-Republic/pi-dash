# Comments, activity, mentions, reactions — area spec (CMT-001–025)

Parent epic NEWFRONT-72. Inventory:
`.ai_design/new_frontend_implementation/parity/inventory/comments.md` (rows
CMT-001–025). Oracle scenarios:
`apps/web_new/e2e/parity/comments/` (25 spec files, green against the old
app). Slices: NEWFRONT-812 (composer, paste, guest rule, attachments,
keyboard, mention autocomplete), 813 (cards, git-sync delete, identity,
fold, edit), 814 (activity feed), 815 (mention render, save, notify,
unmention), 816 (intake + editions), 817 (visibility, deep link, notices,
reactions); contracts NEWFRONT-811; CMT-025's missing scenario NEWFRONT-819;
area gate NEWFRONT-818.

Method: the old app was read as behaviour reference only — the comment
components and their edition overrides, the issue-detail activity and
reaction folders, the mention embed renderers and mention hooks, the lite-text
composer host, the shared activity sentence renderer, the comment and activity
services, and the API views, serializers and URLs for comments, reactions and
history. Nothing is imported, copied or translated. This document restates in
prose what was learned; the only code-like tokens are API paths, query
parameters, body fields, enum values, status codes and browser-storage keys,
which must match the server exactly. Quoted user-visible text is used only
where an oracle scenario pins it literally, and is marked _pinned_.

The issue-detail feed, comment thread, cards, composer, fold, deep links and
both reaction rows are **already built** in the issues area
(`apps/web_new/src/features/issues/{comments,activity,reactions}/` and
`apps/web_new/src/shared/editor/`), delivered by NEWFRONT-651 (ISS-197–202)
and NEWFRONT-654 (ISS-194–196, ISS-203–204). This spec does not re-specify
them as new work. For each row §12 states what already exists, the gap
verified against the oracle, and the one slice that closes it.

Status convention: this spec owns no inventory Status edits. A slice appends
`built` to its rows; the area gate turns passing rows into `new green`.

## 1. Mental model (CMT-001–025)

A **comment** is a rich-text message on one work item: an author (a human
account, an agent, or a bot/integration account), a stored HTML body, a
visibility access specifier, free-form labels (one of which is the fold
marker), an edit stamp, and an embedded list of reactions. Comments are
immutable in place: editing replaces the body and stamps the edit time;
"deleting" removes the row.

The **activity feed** is the single timeline on the work item detail. It
interleaves two server sources: **property activities** (a field changed, the
item was created, deleted, archived, triaged…), fetched from the history
endpoint with `activity_type=issue-property`, and **comments**, fetched from
the same history endpoint with `activity_type=issue-comment`. The client
merges them by creation time; the server is never asked for a merged list
(§11 explains why, and NEWFRONT-128 is the old bug that makes this a hard
rule).

**Mentions** are stored inside the comment HTML as a `mention-component`
element carrying `entity_name="user_mention"`, `entity_identifier` (the
member id) and a display name. The server reads that markup on every comment
write and on description writes: it records per-item mention links and fans
out a "mentioned" notification. The client's only job at save time is to emit
that markup correctly; it must not try to compute mentions itself.

**Reactions** are rows keyed by the reacting user and an emoji encoded as its
decimal code points joined with `-` (for example the thumbs-up character is
`128077`; a skin-toned variant is `128077-127995`). One row per person per
emoji; a second identical add is refused by the server.

**Permissions** are project-membership roles plus authorship. Four actors
matter: **admin** (project admin), **member**, **guest** (role 5), and
**outsider / anonymous** (no membership; cannot see the item, so no comment
surface). Authorship matters on top of role: edit and delete are
author-only even for admins (the issues spec §16 already pins this), while
commenting, folding, reacting and copying links follow the role rules in §9.

**Surfaces**. The comment surface belongs to the work-item detail (§21 of
`parity/specs/issues.md`). The same thread component is also hosted by the
intake item detail (CMT-018) and, in a different product surface, by the
published public board (CMT-006). Every surface reuses one comment model; the
surface only decides which affordances are offered (access switch, deep
links, composer).

## 2. Composer — draft, post, paste, attachments, notices, keyboard (CMT-001, 002, 009, 010, 011)

Slice split: CMT-001, 002, 009, 011 and the mention autocomplete half of
CMT-019 are NEWFRONT-812; the operation notices row CMT-010 is NEWFRONT-817.
The failed-upload notice is already built; 817 proves the one-notice rule and
reconciles the `bug:` scenario that still asserts silence, so it needs only
812's non-stuck draft node to be in place before the gate runs.

### 2.1 The composer (CMT-001)

The composer sits inside the activity section of the work item detail. It is
a labelled group whose accessible name is _pinned_ **"Add comment"**; the
oracle finds the editing surface as a contenteditable inside that group and
the submit control by role. It holds a rich-text editing area, an attachment
control (image files), the submit control, and — only on externally shared
projects — a visibility switch (§5.1).

Drafting supports formatted text and **paste**: pasting an HTML fragment from
the clipboard inserts its formatting (bold fragments stay bold) rather than
flattening to plain text. The submit control is disabled while the draft has
no content. "No content" means neither words nor a stored content node: an
image-only or mention-only draft is a real post and must arm submit (the
shared content test already ignores `img`, `image-component` and
`mention-component`).

Submitting posts `POST /api/workspaces/{ws}/projects/{pid}/issues/{iid}/comments/`
with `comment_html` and `access` (`INTERNAL` by default, `EXTERNAL` when the
switch is on the public side). On success the comment appears in the feed
with author, relative time and rendered body, the author sees one success
notice, and the editor clears for the next comment. Only one post runs at a
time: a second Enter or click while the request is out is a no-op.

**Verified gap.** Present in `features/issues/comments/CommentComposer.tsx`
and the shared editor: draft state, submit gating, Enter handling, the access
switch, and image uploading with park-then-attach. **Missing:** paste
handling is not asserted for the comment composer (the shared editor's paste
rules were built for the description editor and are not proven here), and the
comment editor cannot yet store a mention at all (CMT-019, §6.1 — the comment
extension set has no mention node). The failed-create draft is a real
divergence, recorded once in §11.3 as a parity rule: a refused post **clears
the draft** as the oracle pins, and `CommentComposer` currently keeps it.

**Slice: NEWFRONT-812.**

### 2.2 Guest commenting rule (CMT-002)

A guest (project role 5) may comment only when one of these holds:

- the project flag `guest_view_all_features` is on, or
- the guest is the item's creator (`issue.created_by` equals the guest).

Otherwise `POST …/comments/` is refused with **400** and a body whose only
field is `error`; nothing is stored. The same rule hides the item from that
guest entirely — the detail screen shows the missing-item state and no
composer. Turning the project flag on lets the guest post and see the
composer. The rule is **server-side**; the client must not duplicate it as its
own gate, and must not show a composer on an item it cannot load. A refused
guest normally cannot reach the composer at all, so the refusal notice path is
reachable only when the item was already open and the rule begins to apply
(for example the flag is turned off mid-session): then one error notice
carries the server text and the draft follows the refused-post rule in §11.3.

**Verified gap.** `apps/web_new` has no client-side rule (correct) and no
tested refusal path: the `commentFailureDetail` helper already surfaces an
API error message, but the flow is unproven. The missing-item state for the
refused guest belongs to the detail screen (issues area, §21 of
`parity/specs/issues.md`), not to this area: 812 **verifies** it by opening
another member's item as the guest and asserts the item is hidden with no
composer, and files the gap to the issues epic if the detail does not already
render that state — it does not build a second empty state here.
**Slice: NEWFRONT-812** (proves the refusal, the missing state and the
flag-on success together).

### 2.3 Attachments (CMT-009)

Attaching a file inside the composer uploads it immediately and binds it to
the draft; the editor embeds an image node referencing the parked asset. It
is attached to the saved comment only after the comment exists. The upload is
three-legged (contract `comment-assets.ts`): it creates an asset under
`/api/assets/v2/workspaces/{ws}/projects/{pid}/`, uploads the bytes, then
marks the asset uploaded, and the comment is linked to the asset ids in a
second call. A posted comment renders its images as embedded image nodes in
the body (`image-component` with `status="uploaded"`); the card shows no
file-name list.

Editing a comment can add images and can keep existing ones; keeping an
existing image must **duplicate** it into the new revision so the new body
stays self-contained. Removing the image on edit drops it.

Two oracle-locked deviations exist and are recorded as `bug:` scenarios:
NEWFRONT-145 (a failed upload is silent: no notice, a stuck 0%-progress node;
submit usually re-arms but intermittently stays disabled) and NEWFRONT-147
(an edit draft that keeps the image never arms save; only removing the image
re-arms it). The **intended** behaviour is: a failed upload raises one error
notice and keeps the draft; every image-bearing edit can be saved and keeps
its attachments. The `bug:` scenarios lock the old behaviour; they cannot
stay as they are once the intended behaviour lands, so the slice that fixes
each one branches its scenario on `driver.target` — the established pattern
(see the AUTH-013 scenario) — keeping the old-app leg on the old behaviour
and asserting the fixed outcome on the `web_new` leg (§11.1, §11.2).

**Verified gap.** Upload-and-attach on create and attach-on-edit-upload
exist (`useCreateComment` attaches parked assets, `CommentEditForm` uploads
with `commentId`), and a refused upload already raises one error notice
(`useUploadCommentImage`'s `onError`, pinned by a unit test). The edit form
does not gate save on upload state at all — save is disabled only while
saving or when the body is empty — so NEWFRONT-147's stuck-save branch cannot
reproduce on it. **Missing:** a non-stuck draft node after a failed upload,
and the **duplicate** round trip for an image already in a body: the old app
duplicates the asset on edit so the new revision owns its picture, and the new
app has no duplicate call today. **The upload-failure notice and its
"exactly one notice" proof belong to CMT-010 in NEWFRONT-817**, which also has
to reconcile the still-red `bug:` scenario that asserts silence. **Slice for
duplicate-on-edit: NEWFRONT-812.**

### 2.4 Notices for comment operations (CMT-010)

Every comment operation resolves in **exactly one** visible notice of the
matching kind:

- create → success; update → success; delete → success;
- a failed create, update, delete or upload → error.

The oracle samples the notice region during the action and requires exactly
one new notice. `apps/web_new` already raises titled notices
(`Comment created successfully`, `Comment updated successfully`,
`Comment removed successfully` — _pinned_ by the oracle) plus error titles,
and the update/delete paths roll back their optimistic paint on failure. The
create path's failed-post draft behaviour is the deviation in §11.3. There
must never be two notices for one action (e.g. an attach failure after a
successful create must not double-toast the create).

**Verified gap.** Create/update/delete notices and rollbacks exist and are
unit-tested, and the failed-upload notice already exists (§2.3).
**Missing:** a test that an attach failure does not duplicate the create
notice, and the reconciliation of the upload-failure `bug:` scenario, which
still asserts that a failed upload is silent. The notice inventory and the
"exactly one notice" assertion are owned by CMT-010 in **NEWFRONT-817**; that
slice decides how the `bug:` scenario is resolved (the intended behaviour is
one notice, so the scenario must be updated to the fixed behaviour, not kept
red).

### 2.5 Keyboard behaviour (CMT-011)

In both the composer and the inline edit form:

- **Enter** with content posts/saves.
- **Shift+Enter** inserts a newline and does not post.
- **Ctrl+Enter** and **Cmd+Enter** are inert: they neither post nor insert a
  newline (the draft is byte-identical afterwards).
- **Enter on empty content** is a no-op.
- **Enter mid-upload** (submit disabled because an upload is in flight) is a
  no-op and the draft survives; once the upload settles, Enter posts again.
- IME composition commits with Enter; the composer must leave that Enter to
  the input layer.

The composer and edit form both already implement this shape (early return
on `shiftKey || ctrlKey || metaKey`, empty-body guard, composing guard). The
mid-upload rule is **not** implemented: both forms disable the submit/save
control on `sending`/`saving` and empty content only, and neither tracks an
upload in flight, so Enter during a stalled upload would post a body without
the image. **Verified gap:** the slice tracks pending uploads and disables
submit while any is in flight, then proves every keyboard branch, including
the mid-upload no-op and the post-settle post. **Slice: NEWFRONT-812.**

## 3. Comment cards — identity, edit, delete, git-sync, fold (CMT-003, 004, 005, 008, 012)

A card is one comment in the feed: a header (avatar, author name, relative
time with an exact-time hover detail, edited marker, fold control, visibility
mark, reaction picker, overflow menu), the body, and the reaction cluster.
The card is addressable by a stable element id `comment-<uuid>`; the oracle
scopes menu and body reads to that id. The overflow menu's entries carry
_pinned_ English titles that the driver maps to stable keys: **Edit**,
**Copy link**, **Switch to public comment** / **Switch to private comment**,
**Fold comment** / **Unfold comment**, **Delete**.

### 3.1 Identity (CMT-012)

The header shows:

- the author's display name and avatar (from the comment's `actor_detail`);
- a **bot** author renders a name that reads as a bot (the oracle only
  requires the author text to match `/bot/i`); the old client shows the
  account's first name followed by "Bot". The oracle also asserts that a
  **human** author does **not** match `/bot/i`, so a human named with "Bot"
  in their display name is rendered as stored, and a bot marker is only added
  for bot accounts. An **agent** comment additionally shows its speaker label
  (fallback "AI Agent"); human comments show none;
- the relative time (e.g. "3 minutes ago") with a tooltip carrying the exact
  date/time (the oracle checks the tooltip contains the stored creation
  year);
- an **edited** marker when `edited_at` is set.

**Verified gap.** `commentAuthorName` returns the speaker label for
`speaker_type === "agent"`, otherwise `actor_detail.display_name`. Bot
accounts (`actor_detail.is_bot`) therefore render their plain display name,
which the oracle would reject. **Fix in NEWFRONT-813:** when the actor is a
bot, render a bot-labelled name (a suffix that matches `/bot/i` and is not
applied to humans); keep the agent speaker label; keep the `UserLite.is_bot`
field as the signal. Time/edited/avatar already match.

### 3.2 Inline edit (CMT-003)

Edit is offered to the **author only** (not admins). The overflow "Edit"
opens an inline form prefilled with the current HTML, focused, with a save
control and a discard control. Discard restores the original body without any
write. Save sends
`PATCH /api/workspaces/{ws}/projects/{pid}/issues/{iid}/comments/{cid}/`
with `comment_html`, and the card shows the new body and an edited marker.

A pristine draft (no keystroke) leaves save **disabled** — only a real content
change can be saved through the UI. Saving identical HTML through the API
sets **no** `edited_at` (the server stamps only when `comment_html` differs),
and the card then shows no edited marker.

**Verified gap.** The pristine rule is **not implemented**: `CommentEditForm`
prefills the body and enables save whenever the prefilled body is non-empty,
with no comparison against the original, so the pinned `composer-edit.spec.ts`
step (save stays disabled until a keystroke) fails. The marker half matches:
`useUpdateComment` optimistically paints `edited_at` for a body patch and
reconciles from the response, and the server's value-changed check keeps an
identical patch unstamped. **NEWFRONT-813** adds the pristine comparison
(track the initial HTML and disable save until the body differs) and proves
the pristine, identical-patch and marker branches. The image-keeping
duplicate is 812 (§2.3).

### 3.3 Delete (CMT-004)

Delete is author-only. Choosing "Delete" removes the comment from the feed
and raises exactly one success notice. **No confirmation dialog** — the old
card deletes on the menu pick and the oracle assumes it. Today the card does
that (optimistic row removal, rollback on failure, `COMMENT_REMOVED_TITLE`).
**Verified gap:** none in structure; prove it. **Slice: NEWFRONT-813.**

### 3.4 Git-synced comment refusal (CMT-005)

When a comment is mirrored from a Git provider, the server refuses deletion
with **409** and `{"error": … "Unbind the project's repository to delete."}`.
The client must leave the comment in place, roll back its optimistic removal,
and tell the user that unbinding is required (the oracle checks the notice
text contains "fail" and the server body contains "unbind"). After the
repository is unbound, the same delete succeeds (204) and the card disappears.

**Verified gap.** The contract carries `is_synced`, and `useDeleteComment`
rolls back and shows the generic `Couldn't delete the comment` title with the
server message. **Two gaps.** First, the oracle requires the visible notice
text to contain _pinned_ **"fail"**; "Couldn't delete…" does not, so the
current title cannot pass. Second, there is no dedicated 409 branch: the
refusal must raise one error notice that reads as a failed removal (so its
text contains "fail"), carries the server's unbind instruction, and leaves
the card in place. The slice then proves 409 → rollback → unbind → 204 →
card gone. This is the refusal shape NEWFRONT-811 contracts.
**Slice: NEWFRONT-813.**

### 3.5 Fold (CMT-008)

Any viewer may fold/unfold, not just the author. The fold marker is the
literal label **`fold`** in the comment's `labels` array; toggling sends
`PATCH` with the full `labels` array (`withFold` keeps other labels). A folded
card hides its body **and its reactions** and shows a header affordance
("Folded comment · Click to expand" / "Click to collapse") plus the "Unfold
comment" menu entry. Folding persists across reloads (the label is on the
server). Unfolding removes the label and reveals body and reactions. The
folded state is a per-comment preference, not per-viewer state.

**Verified gap.** Implemented in `model.ts` (`isCommentFolded`, `withFold`),
`CommentCard` and the thread's update mutation; reactions are hidden because
the cluster lives inside the body branch. **Slice: NEWFRONT-813** proves the
label round-trip, the hidden reaction and the reload.

## 4. Activity feed (CMT-013, 014, 015, 016, 017)

### 4.1 Merged timeline (CMT-013)

One chronological feed merges property changes and comments on the detail
screen. Each row shows actor, a per-field sentence in the product's voice,
and a relative time with an exact-time hover. A brand-new item shows exactly
one creation row. The fields the issues spec §15/§16 lists render as
sentences (assignment, state, priority, estimate, dates, labels, cycle,
module, parent, relations, links, attachments, archive/restore, description,
title, creation, intake decisions) — with one known exception below. Links
inside entries navigate to the referenced entity (the oracle opens the first
entry's link and requires the URL to contain `profile`).

Reads: `GET /api/workspaces/{ws}/projects/{pid}/issues/{iid}/history/` with
`activity_type=issue-property` for activities and `activity_type=issue-comment`
for comments. **Do not call the history endpoint without `activity_type`**:
the old merged read returns 500 (NEWFRONT-128); the client merges. Intended
behaviour is that a merged read would return the combined list, but the
new app should keep the split reads until that bug is fixed, then a single
read is a permissible refactor behind the same model.

**Verified gap.** Built (`activity/api.ts` + `model.ts` `buildFeed`/`sortFeed`).
The exception: a **deletion** activity arrives with `field="issue"` and
`verb="deleted"` (the server stamps that field when an item is deleted), and
`activitySentence` has no `issue` case — it only handles a null field — so the
row is dropped instead of rendering "deleted the work item". The old renderer
treated a missing field as `issue`, which is also what a creation row carries.
**NEWFRONT-814** adds the `issue` field case (creation and deletion) so the
deletion row renders, and keeps it in the always-visible group in
`activityFilterOf` so narrowing the toggles cannot hide a deletion. The slice
otherwise owns the row's incremental/refresh behaviour and proves the merge.

### 4.2 Sort (CMT-014)

The header sort control flips oldest-first (default) and newest-first. The
composer sits **below** the feed in oldest-first and **above** it in
newest-first. The choice is persisted per browser as the JSON string `"desc"`
(or `"asc"`) under the localStorage key **`activity_sort_order`**; the server
order is untouched. Sorting is client-side over the merged feed.

**Verified gap.** Built (`DEFAULT_SORT`, `SORT_STORAGE_KEY`, `flipSort`,
prefs hook). **Slice: NEWFRONT-814** proves persistence and composer
placement.

### 4.3 Filter (CMT-015)

The header filter popover offers exactly four toggles, in this order, with
these _pinned_ labels: **Updates**, **Comments**, **State**, **Assignee**.
All are on by default. Turning one off hides its rows immediately; the
control marks itself (an accent dot) whenever the selection is narrowed.
**The last active filter cannot be removed** — toggling it is a no-op and the
stored selection is unchanged. Creation and deletion rows always show.
Persisted per browser as the stored categories in menu order under the key
**`issue_activity_filters`** (e.g. `["ACTIVITY"]` when only updates remain).
The server state is untouched by filtering.

**Verified gap.** Built (`FEED_FILTERS`, labels, `toggleFeedFilter`,
`isFilterNarrowed`, `FILTERS_STORAGE_KEY`). **Slice: NEWFRONT-814** proves the
last-filter guard and storage shape.

### 4.4 Loading and empty (CMT-016)

While either source read is in flight the section shows a **three-row
skeleton** (role `status`, busy, labelled as loading) and no entries. When
the reads resolve, entries replace it. An item whose history resolves empty
renders **no rows and no error surface** (no `role="alert"`) — the section
heading still renders. A read error shows a single error line, not the
skeleton forever.

**Verified gap.** Built (`FeedSkeleton`, three rows; empty renders nothing).
**Slice: NEWFRONT-814** proves both states.

### 4.5 Incremental pickup (CMT-017)

After the first load, the feed picks up new entries **without a full page
reload and without refetching the whole history**: every refetch of each
source carries a cursor, `created_at__gt=<newest known created_at for that
source>`, and the returned rows are appended if not already present. The
oracle watches the network: after a rename, at least one
`activity_type=issue-property&created_at__gt=…` request must carry exactly the
newest previously known property `created_at`; the URL must not change; a
comment posted through the composer joins the feed the same way. Each source
keeps its own cursor (a comment's timestamp is not a property cursor).

**Verified gap.** **Not built.** `activitiesQueryOptions` and
`commentsQueryOptions` refetch the full list on invalidation; the contracts
already accept `createdAfter` (`created_at__gt`) and
`listIssueComments`/`listIssueActivities` expose it, but nothing supplies it.
**Slice: NEWFRONT-814.** The behaviour: keep the last-seen `created_at` per
source in the query (or a ref), and on invalidation run a delta read for
that source, merging unique rows; a manual/full re-read may still reset the
cursor. Do not reload the page or reset the feed scroll.

## 5. Visibility, deep links, intake, editions (CMT-006, 007, 018, 025)

### 5.1 Visibility (CMT-006)

Projects that support external sharing carry an **anchor** (published
projects have a non-null `project.anchor`). On such projects:

- the composer shows an internal/external switch, internal by default;
- each card shows a corner visibility mark (icon only, no text) that reads
  as internal or public;
- the overflow menu offers **Switch to public comment** when the comment is
  internal and **Switch to private comment** when it is public; the switch is
  not author-gated;
- toggling sends `PATCH …/comments/{cid}/` with `access` (`INTERNAL` /
  `EXTERNAL`), paints the new mark optimistically and reconciles from the
  response.

On projects **without** an anchor there is no composer switch, no corner
mark, and no menu entry; the driver reads "hidden" for the badge in that case.
New comments start internal.

**Verified gap.** Built: `showAccess={published}` / `accessToggle: published`,
`toggledAccess`, badges. **Slice: NEWFRONT-817** proves the anchored and
non-anchored pairs and the persistence.

### 5.2 Deep link (CMT-007)

"Copy link" writes `/{workspaceSlug}/browse/{PROJECT}-{sequence}/#comment-{id}`
to the clipboard and raises a success notice containing "copied"; a clipboard
refusal raises an error notice. Opening such a URL scrolls the feed to that
card and **highlights** it temporarily (the oracle detects the highlight; the
new app's highlight is an 8-second accent ring). Highlighting is keyed on the
hash and also responds to in-page hash changes. The copy entry is **absent on
intake items** (§5.3).

**Verified gap.** Built (`commentDeepLink`, `commentIdFromHash`,
`COMMENT_HIGHLIGHT_MS`, thread highlight effect). **Slice: NEWFRONT-817**
proves clipboard text, notice, scroll/highlight, and the intake absence is
proved by 816.

### 5.3 Intake variant (CMT-018)

The intake item detail hosts the same feed and comment cards. Its variant:

- renders comments and property entries normally;
- offers **no "Copy link"** entry on any card;
- does not offer creation aids meant for triaged items (the intake screen's
  own chrome is the intake epic's; this area owns only the comment-surface
  variant).

The thread component already takes an `isIntake` flag that drops copy-link,
but **no intake route mounts it**: the only mount today is
`routes/$ws/_gated/browse/$issueKey.tsx`. The intake screen itself is built
by NEWFRONT-692/693 under the intake epic (NEWFRONT-80); this area's slice
wires the comment surface into that host when it exists and proves the
variant against the seeded intake row.

**Verified gap.** Flag exists, host absent. **Slice: NEWFRONT-816**, blocked
by NEWFRONT-692/693 (+819). Until the intake host lands, 816 must not fake a
second detail screen; it registers the thread in the intake detail's
activity slot, proven by `rules-intake.spec.ts`.

### 5.4 Edition sameness (CMT-025)

Comment, activity, mention and reaction behaviour is **edition-neutral**. The
old app's edition hooks in this area are inert: the cloud comment block only
wraps the core card with layout, and the cloud mention embed slot renders
nothing. No comment/activity endpoint, permission, storage key or keyboard
rule differs across OSS, cloud and desktop.

The new app has edition seams (`src/core/edition`) for routes, sidebar,
settings sections and flags, but **no** comment/activity flags; that is
correct. Rules:

- the thread, composer, cards, feed, mentions and reactions mount identically
  in every build; an edition may not hide or replace them;
- all reads/writes go to the same endpoints listed in §8;
- the desktop shell may wrap the web app, but a desktop-only comment feature
  must not appear, and a web feature must not disappear in desktop.

**Verified gap.** Structural: the surfaces are edition-agnostic, but nothing
proves it. **CMT-025 has no oracle scenario yet** — NEWFRONT-819 (stage 0,
under this epic) writes it; NEWFRONT-816's edition half is blocked by 819 and
consumes it. The scenario must run the same comment/feed/reaction flow in the
OSS and cloud stacks (`PARITY_ORACLE_EDITION=cloud`, separate namespace) and
in the desktop target against the same stack, asserting no extra or missing
surfaces. **Slice: NEWFRONT-816** (behaviour), scenario NEWFRONT-819.

**Seam to public boards (NEWFRONT-99).** Published boards render comments
through a separate public API, gated by whether comments are enabled and
by sign-in; anonymous visitors see a "Sign in to add your comment" card.
That surface is the public-boards epic's; this area owns only the comment
model and the `access` specifier that boards consume. No CMT row needs board
UI; CMT-006 is proven on the internal detail.

## 6. Mentions (CMT-019, 020, 021)

### 6.1 Autocomplete (CMT-019)

Typing `@` in the comment composer opens a suggestion list. Typing after the
trigger narrows it. The list is grouped under a section titled _pinned_
**"Users"**, shows avatars, returns **at most 5** people, and each row picks a
member. Choosing a suggestion replaces the typed `@query` with a
`mention-component` node carrying `entity_name="user_mention"`,
`entity_identifier` = member id, `id` = member id, and the display name in
`data-name`.

People search: `GET /api/workspaces/{ws}/entity-search/` with
`query`, `query_type=user_mention`, `project_id`, `count` (default 5). The
response envelope carries `user_mention: [{ member__id, member__display_name,
member__avatar_url }]`. The server excludes bots, orders by newest first, and
matches first/last/display name case-insensitively. The new contract
`searchMentionPeople` already models this and the **description** editor
already wires it (`onMentionQuery` → `insertMention`); the comment composer
does not. Suggestion sections are extensible (other entity kinds may be
added), but only the Users section is required.

**Verified gap.** Editor primitives and the search contract exist; the
composer does not pass `onMentionQuery`/`insertMention`, and — more than
wiring — the **comment editor has no mention node in its schema**: its
extension set is the description set plus a plain image node, while the
mention node is built only inside the description set. Inserting a mention
there would not survive serialization, so the picker would appear to work and
save text. **Slice: NEWFRONT-812** adds the mention node to the comment
extension set (the same node shape the description editor uses) and wires the
comment composer and inline edit form to the picker. The search envelope is
already contracted and tested (NEWFRONT-811 need not re-add it). The
render/save/notify half of mentions is NEWFRONT-815 (§6.2–6.3).

### 6.2 Render (CMT-020)

A stored mention renders as an identifiable reference showing the member's
name, and **linking to that member**: `/{workspaceSlug}/profile/{memberId}`.
The oracle collects visible references and requires the anchor's text to be
`@<displayName>` and its href to be exactly that path. The body renderer must
turn `mention-component` into a chip that carries the member id and a
profile link; unknown ids still render a chip rather than raw markup.

**Verified gap.** `renderableHtml` turns a mention into a chip whose text is
`@<name>`, but the chip is a `span` carrying `data-mention-id` — **no
`href`**, so it is not a link. The workspace slug is not passed to the
renderer today. **Slice: NEWFRONT-815** makes the `user_mention` chip an
anchor with the exact profile href (slug supplied by the card's context),
leaving other entity kinds as plain chips.

### 6.3 Save and notify (CMT-021)

The **server** owns mention semantics:

- on comment create/update it parses the body for
  `mention-component[entity_name=user_mention]`, records an `IssueMention` row
  per mentioned member, and subscribes new mentions to the item;
- on update it **diffs** old versus new mentions: newly added members get the
  mention notification, removed members lose their mention row (no stale
  source);
- a mentioned user receives an in-app notification with sender
  `in_app:issue_activities:mentioned`, entity = the issue, and an
  `isMentioned` marker; the inbox read is
  `GET /api/workspaces/{ws}/users/notifications/?mentioned=true`.

The client's obligations: store correct mention markup; on edit, removing a
mention must store a body without `mention-component`; never call a
mention-specific endpoint. The oracle verifies stored markup, the mention
notification appearing in the mentioned member's inbox, and unmention
clearing the link.

**Verified gap.** Nothing client-side beyond §6.1/6.2; the missing piece is
that the composer cannot produce mentions at all today (the comment schema
has no mention node), so the notify path is unproven end to end and the
scenario needs the editor flow. The "or description" half of this row refers
to the description editor: where a work item's **description** can be edited
from the detail, that is the description editor's own save path, not the
comment composer's, and the server applies the same diff and fan-out to its
markup. The oracle scenario exercises the comment flow only; 815 consumes
812's editor fix for the comment half and must state explicitly that the
description path is already covered by the description editor's existing
mention wiring, or extend the scenario if a run finds it is not.
**Seam to notifications NEWFRONT-76:** the notification row, inbox rendering
and unread semantics are the notifications epic's; this area proves the
notification exists with the right entity and sender. **Slice:
NEWFRONT-815.**

## 7. Reactions (CMT-022, 023, 024)

### 7.1 Work-item reactions (CMT-022)

The detail shows a reaction cluster for the item itself: existing reactions
as chips (emoji, count, highlighted when the viewer reacted, tooltip naming
the reactors) and a picker that offers an emoji shelf. Adding posts
`POST /api/workspaces/{ws}/projects/{pid}/issues/{iid}/reactions/` with
`reaction` = the decimal code-point key; clicking a chip the viewer already
owns deletes `…/reactions/{reaction}/`. Each successful toggle raises one
notice (**Reaction created successfully** / **Reaction removed successfully**
— _pinned_); a failure raises **Reaction creation failed**. The server state
is re-read after each toggle so counts and names come from the server. On an
**archived** item the reaction control is disabled (the composer is also
gone).

**Verified gap.** Built (`reactions/api.ts`, `IssueReactions`, model
encode/decode, notices). **Slice: NEWFRONT-817** proves toggle, tooltip,
server agreement and the archived disable.

### 7.2 Comment reactions (CMT-023)

The same behaviour scoped to one comment:
`POST`/`DELETE …/projects/{pid}/comments/{cid}/reactions/`. The cluster sits
with the card, is hidden entirely when the card is read-only (archived) and
while the comment is folded (§3.5). The picker is per card.

**Verified gap.** Built (`CommentReactions`, `CommentReactionPicker`).
**Slice: NEWFRONT-817.**

### 7.3 Duplicates (CMT-024)

One reaction per person per emoji. A duplicate add is refused with **400**
and a body whose `error` names the duplicate; the UI must keep a single chip
and raise one failure notice (**Reaction creation failed**), then re-read so
the count never double-counts the viewer. A refused duplicate is not a
successful add. The **comment** endpoint names the duplicate explicitly
(`Reaction already exists for the user`); the issue endpoint returns the
serializer's error without that sentence, and the oracle only requires a
client error with an `error` field there. The client branches on the status,
not on the wording.

**Verified gap.** The generic error path shows the failure notice and the
list re-reads on settle; there is no explicit duplicate parser, and no test
of the 400 shape. NEWFRONT-811 pins the comment endpoint's typed refusal
body; **NEWFRONT-817** proves the flow on both the issue and comment clusters.

## 8. API and contract reference (NEWFRONT-811)

All paths are under the API origin and are workspace-scoped. `{iid}` is an
issue UUID, `{cid}` a comment UUID.

| Purpose                  | Method and path                                                                                  | Query / body                                                                                                        | Success                                                                 |
| ------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Comment list             | `GET /api/workspaces/{ws}/projects/{pid}/issues/{iid}/history/`                                  | `activity_type=issue-comment`, `created_at__gt` (optional)                                                          | 200, array of comments                                                  |
| Property activities      | same                                                                                             | `activity_type=issue-property`, `created_at__gt` (optional)                                                         | 200, array                                                              |
| Merged read              | same, no `activity_type`                                                                         | —                                                                                                                   | **500 today** (NEWFRONT-128); do not use                                |
| Create comment           | `POST …/issues/{iid}/comments/`                                                                  | `comment_html`, `comment_json`, `access`, `labels` (no create-time `attachments`; images are linked after creation) | 201, comment; guest refusal 400 `{error}`                               |
| Update comment           | `PATCH …/issues/{iid}/comments/{cid}/`                                                           | `comment_html`, `comment_json`, `access`, `labels`                                                                  | 200, comment; synced body fields refused 400                            |
| Delete comment           | `DELETE …/issues/{iid}/comments/{cid}/`                                                          | —                                                                                                                   | 204; git-synced 409 `{error: …unbind…}`                                 |
| Issue reactions          | `POST …/issues/{iid}/reactions/`                                                                 | `reaction`                                                                                                          | 201; duplicate 400                                                      |
| Issue reaction removal   | `DELETE …/issues/{iid}/reactions/{reaction}/`                                                    | —                                                                                                                   | 204                                                                     |
| Comment reactions        | `POST …/projects/{pid}/comments/{cid}/reactions/` (not under `/issues/`)                         | `reaction`                                                                                                          | 201; duplicate 400 `{error: "Reaction already exists…"}` (comment path) |
| Comment reaction removal | `DELETE …/projects/{pid}/comments/{cid}/reactions/{reaction}/`                                   | —                                                                                                                   | 204                                                                     |
| Image upload             | `POST /api/assets/v2/workspaces/{ws}/projects/{pid}/` then byte upload then `PATCH …/{assetId}/` | asset payload / bytes                                                                                               | creates a parked asset                                                  |
| Attach images            | `POST /api/assets/v2/workspaces/{ws}/projects/{pid}/{cid}/bulk/`                                 | `{asset_ids: [...]}`                                                                                                | 204; skipped when the list is empty                                     |
| Duplicate image on edit  | `POST /api/assets/v2/workspaces/{ws}/duplicate-assets/{assetId}/`                                | `entity_id` (comment id), `entity_type=COMMENT_DESCRIPTION`, `project_id`                                           | 200 `{asset_id}` (NEWFRONT-812 gap)                                     |
| Mention search           | `GET /api/workspaces/{ws}/entity-search/`                                                        | `query`, `query_type=user_mention`, `project_id`, `count`                                                           | 200 `{user_mention:[…]}`                                                |
| Mentioned notifications  | `GET /api/workspaces/{ws}/users/notifications/?mentioned=true`                                   | —                                                                                                                   | 200, inbox rows with `isMentioned`                                      |

Comment fields the UI reads: `id`, `issue`, `actor`, `actor_detail`
(`display_name`, `avatar_url`, `is_bot`), `comment_html`,
`comment_stripped`, `access` (`INTERNAL`|`EXTERNAL`), `labels`,
`edited_at`, `created_at`, `is_synced`, `comment_reactions`,
`speaker_type` (`human`|`agent`|`system`|`integration`), `speaker_label`,
`speaker_agent_run_id`. `attachments` is a model array the response carries
for images already linked to the row; it is not a create-time field — images
arrive through the park/complete/bulk sequence above.

Activity fields: `id`, `field`, `verb`, `actor`, `actor_detail`,
`created_at`, `new_value`, `old_value`, `comment` (comment rows) and the
per-field payload.

Storage keys: `activity_sort_order` (`"asc"`/`"desc"`),
`issue_activity_filters` (array of `ACTIVITY`|`COMMENT`|`STATE`|`ASSIGNEE`),
comment fold marker label `fold`.

Contracts to pin (NEWFRONT-811): only the shapes that are genuinely absent
after 613/570 —

- the git-sync **409** body (`{error: …unbind…}` on comment delete) and the
  synced-body **400** field errors on update, as narrow typed parsers so the
  UI branches without string-matching;
- the duplicate comment-reaction **400** body
  (`{error: "Reaction already exists for the user"}`). The issue-reaction
  duplicate is not this table's shape: the issue view creates it from a
  serializer error, not the comment view's `IntegrityError` branch, so 811
  pins whatever the contract test observes there rather than assuming the
  same text;
- the **204 empty-body** delete shapes for comments and reactions, asserted in
  contract tests.

Already covered and not to be re-added: the mention-search envelope
(`entity-search.ts`, tested in `issue-modal-assets.test.ts`), comment
list/create/update/delete (`issue-comments.ts`), the asset upload/attach legs
(`comment-assets.ts`, including the `…/{cid}/bulk/` link) and both reaction
collections (`issue-reactions.ts`, exercised by `comments.contract.test.ts`).

## 9. Permissions matrix

| Action                 | Admin           | Member | Guest                                      | Guest (not own item, flag off)  | Author (any role) |
| ---------------------- | --------------- | ------ | ------------------------------------------ | ------------------------------- | ----------------- |
| Read feed/comments     | yes             | yes    | yes (may be barred from the item entirely) | item hidden                     | yes               |
| Post comment           | yes             | yes    | yes if own item or flag on                 | **400 refusal**, nothing stored | yes               |
| Edit own comment       | yes             | yes    | yes                                        | n/a (item hidden)               | yes               |
| Edit others' comment   | **no**          | **no** | **no**                                     | —                               | —                 |
| Delete own comment     | yes             | yes    | yes                                        | n/a                             | yes               |
| Delete others' comment | **no**          | **no** | **no**                                     | —                               | —                 |
| Fold/unfold            | yes             | yes    | yes                                        | n/a                             | yes               |
| Toggle access          | yes             | yes    | yes                                        | n/a                             | yes               |
| Copy deep link         | yes             | yes    | yes                                        | n/a                             | yes               |
| React (item/comment)   | yes             | yes    | yes                                        | n/a                             | yes               |
| Attach/post images     | same as posting | same   | same                                       | —                               | yes               |

Archived items: feed stays, composer gone, card menus gone, reactions hidden/
disabled. Intake items: no copy-link. Non-anchored projects: no access
switch or mark.

## 10. Edge cases and error states

- Empty feed after a successful read: no rows, no error.
- Read failure: one error line; retry by the usual query refresh.
- Optimistic fold/access/delete paints and rolls back on refusal; a rollback
  must restore the exact prior row.
- Comment still being edited when the item is archived: the edit surface
  disappears with the card menus.
- A comment created while the feed was open appears through the incremental
  path (CMT-017), once.
- A mention of a member who left: the chip still renders from stored markup.
- A reaction emoji outside the shelf: render from its code points; an
  unparseable key renders the key itself.
- IME composition Enter must not post.
- Clipboard denial on copy-link is an error notice, not a silent no-op.
- Git-synced comment: delete refused; edit of the body fields is also
  refused by the server (400).
- Duplicate reaction: single chip, one failure notice.
- Failed upload: one error notice, draft retained, retry possible (already
  built; CMT-010/817 proves it and reconciles the silent `bug:` scenario).
  A failed upload is not an attempted post, so it does not clear the draft.
- Failed create: one error notice and an **empty** composer (the oracle's
  rule, §11.3).

## 11. Recorded deviations (old bugs, linked issues)

### 11.1 NEWFRONT-145 — failed attachment upload is silent

Oracle `composer-attachments.spec.ts` locks: a failed upload surfaces **no**
notice, leaves a stuck 0%-progress node, and submit usually re-arms but
intermittently stays disabled until reload. Intended: one error notice,
draft kept, submit usable. `apps/web_new` already raises the one error
notice; the remaining work is the non-stuck draft node (812, §2.3) and the
scenario reconciliation, which belongs to CMT-010 in **NEWFRONT-817**. The
scenario currently asserts silence on both targets; 817 branches it on
`driver.target` so the old-app leg keeps the silent outcome, while the
`web_new` leg asserts exactly one error notice and a usable submit. It
cannot stay red on `web_new`.

### 11.2 NEWFRONT-147 — image-keeping edit never arms save

Oracle `composer-attachments.spec.ts` locks: opening the edit form on an
image comment leaves save disabled indefinitely. Intended: keep/duplicate the
image and save. The new edit form has no upload gating (save depends on
`saving` and empty content only), so that branch cannot reproduce here; the
real work is the **duplicate-on-edit** round trip (`POST
/api/assets/v2/workspaces/{ws}/duplicate-assets/{assetId}/`, §8), added by
**NEWFRONT-812**. That slice branches the scenario on `driver.target`: the
old-app leg keeps the stuck-save outcome, and the `web_new` leg asserts that
the edit arms save, saves, and keeps the image in the new body.

### 11.3 Failed-create draft (one rule: parity — the composer clears)

`composer-notices.spec.ts` pins that a **failed create clears the composer**
(`composerDraftText()` is `""` after the aborted POST). The oracle is the
behaviour reference, and `CommentComposer` today keeps the draft on a failed
create. **The rule is parity: every attempted post clears the draft, success
or failure**, and the composer's "keep the draft for a retry" comment goes.
There is no product fork and no `bug:` issue for this; a future keep-the-draft
scenario would be a separate product change with its own issue. Clearing the
draft must also clear the composer's parked image ids: the composer keeps
them in a ref and attaches every listed id on the next successful post, so
leaving them would bind a failed post's images to the next, unrelated
comment. The oracle does not observe that; 812 covers it with a unit test.
In the edit form, a failed save keeps the draft — a separate form the oracle
does not pin, and this rule does not apply to it.

### 11.4 NEWFRONT-128 — merged history read 500s

Split reads only (§4.1). Not a client bug.

## 12. Existing code, gap, and closing slice per row

| Row     | What exists in `apps/web_new` today                                                                               | Verified gap vs oracle                                                                                         | Closing slice                        |
| ------- | ----------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| CMT-001 | `CommentComposer` (submit gating, double-submit guard, access switch, image park/attach); `CommentRichTextEditor` | Paste-formatting unproven; no mention node/query (CMT-019); failed-create draft kept, oracle clears (§11.3)    | NEWFRONT-812                         |
| CMT-002 | No client rule (correct); `commentFailureDetail` surfaces server messages                                         | Refusal flow and flag-on success untested; missing-item state verified, owned by issues §21                    | NEWFRONT-812                         |
| CMT-003 | `CommentEditForm` (prefill, Enter, discard); `useUpdateComment` (optimistic `edited_at`, reconcile)               | **Pristine save not implemented** (save enabled on prefilled body); identical-patch/marker branches need proof | NEWFRONT-813                         |
| CMT-004 | Overflow delete, optimistic removal, rollback, success notice                                                     | Prove; the synced 409 is CMT-005                                                                               | NEWFRONT-813                         |
| CMT-005 | `is_synced` in the contract; generic error rollback                                                               | Generic title lacks "fail" (oracle-pinned); no 409/unbind branch or test                                       | NEWFRONT-813                         |
| CMT-006 | `showAccess`/`accessToggle`, `toggledAccess`, card marks                                                          | Anchored/non-anchored parity tests                                                                             | NEWFRONT-817                         |
| CMT-007 | `commentDeepLink`, hash highlight (8 s), copy notice                                                              | Clipboard text/notice/highlight proof                                                                          | NEWFRONT-817                         |
| CMT-008 | `withFold` label, card fold, hidden body & reactions                                                              | Label round-trip + reload proof                                                                                | NEWFRONT-813                         |
| CMT-009 | Upload-and-attach on create; attach-on-edit upload; failed-upload notice                                          | Non-stuck draft node on failure; **duplicate-on-edit** round trip; NEWFRONT-147 scenario reconciliation        | NEWFRONT-812                         |
| CMT-010 | Titled notices + rollback for create/update/delete; failed-upload notice                                          | No double notice on attach failure; one-notice proof; reconcile the silent-upload `bug:` scenario              | NEWFRONT-817                         |
| CMT-011 | Enter/Shift+Enter/modifier/empty guards in both forms                                                             | **No mid-upload disable** (no in-flight tracking); full keyboard branch coverage                               | NEWFRONT-812                         |
| CMT-012 | `commentAuthorName` (agent label), avatar, relative time + tooltip, edited marker                                 | Bot accounts render plain name (need bot label)                                                                | NEWFRONT-813                         |
| CMT-013 | `buildFeed`/`sortFeed`/`composeFeed` over split history reads                                                     | Deletion activity (`field="issue"`) dropped; merge proof; keep split reads                                     | NEWFRONT-814                         |
| CMT-014 | `DEFAULT_SORT`, `SORT_STORAGE_KEY`, composer placement                                                            | Persistence/reload proof                                                                                       | NEWFRONT-814                         |
| CMT-015 | Four filters, labels, last-filter guard, `FILTERS_STORAGE_KEY`                                                    | Storage shape and guard proof                                                                                  | NEWFRONT-814                         |
| CMT-016 | `FeedSkeleton` (3 rows), empty renders nothing, error line                                                        | Both branches proof                                                                                            | NEWFRONT-814                         |
| CMT-017 | Contracts accept `createdAfter`/`created_at__gt`; queries do not use it                                           | **No incremental read** — full refetch only                                                                    | NEWFRONT-814                         |
| CMT-018 | `isIntake` flag drops copy-link; `CommentThread` unmounted outside browse                                         | Intake host (692/693) absent; surface wiring                                                                   | NEWFRONT-816                         |
| CMT-019 | Editor `onMentionQuery`/`insertMention`, `searchMentionPeople` (used by description editor)                       | Comment editor **schema has no mention node**; composer/edit form also need the picker wiring                  | NEWFRONT-812                         |
| CMT-020 | Mention chip (`@name`, `data-mention-id`) in `renderableHtml`                                                     | Chip is a `span`, no profile `href`; slug not passed to the renderer                                           | NEWFRONT-815                         |
| CMT-021 | Server-side mention diff/fan-out; notifications contract                                                          | End-to-end mention → notify → unmention unproven                                                               | NEWFRONT-815                         |
| CMT-022 | `IssueReactions`, picker, toggles, notices, archived disable                                                      | Proof set                                                                                                      | NEWFRONT-817                         |
| CMT-023 | `CommentReactions` on card, hidden when folded/archived                                                           | Proof set                                                                                                      | NEWFRONT-817                         |
| CMT-024 | Generic error notice + re-read                                                                                    | No typed duplicate-400 parser (811) or duplicate-flow test                                                     | NEWFRONT-817 (contract 811)          |
| CMT-025 | Edition-neutral surfaces; no comment edition flags                                                                | No scenario proving edition sameness                                                                           | NEWFRONT-816 (scenario NEWFRONT-819) |

Every row is in exactly one slice; the contracts slice NEWFRONT-811 owns no
row and the area gate NEWFRONT-818 closes none on its own (it proves them).

## 13. Cross-epic seams

- **Issues NEWFRONT-71 (§21).** The detail screen mounts the thread; the
  issues slices own the mount, the shared detail chrome and the archived/
  published flags. This epic changes only the comment surface internals.
- **Intake NEWFRONT-80 (CMT-018).** NEWFRONT-692/693 build the intake detail
  and triage chrome; 816 plugs this area's thread into it with `isIntake`.
  Gate fixture opens the seeded intake row by its inbox-issue id.
- **Public boards NEWFRONT-99 (CMT-006).** Boards consume the `access`
  specifier and their own public comment API; no board UI here.
- **Notifications NEWFRONT-76 (CMT-021).** Mention rows, inbox filtering and
  unread state; this area proves the row exists with the right entity and
  sender.
- **Runners/agents (CMT-012).** Agent comments carry `speaker_type=agent` and
  a speaker label; runner chat is out of scope, only the card attribution is
  in scope.
- **Desktop/cloud (CMT-025).** Same endpoints and surfaces in every build;
  NEWFRONT-819's scenario is the proof.

## 14. Definition of done for the slices reading this spec

Each slice implements its rows as described, covers every changed line and
branch with unit tests, runs its package's typecheck/lint/tests plus
`check:boundaries`, `check:headers`, `check:deps`, `check:coexistence`, the
similarity check and the paths gate, and appends `built` to its rows'
Status cells. Slices never bring up the parity stack; the area gate
NEWFRONT-818 runs every comments scenario against `apps/web_new`, turns
passing rows into `new green`, and files fix issues for failures. A row whose
behaviour is not in the current split is a defect in the split, not a stub.
