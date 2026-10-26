# Pages area spec — collaborative documents (PAGE-001–090)

Build spec for inventory rows PAGE-001–090
(`.ai_design/new_frontend_implementation/parity/inventory/pages.md`). It is
written for the contracts issues (NEWFRONT-833, NEWFRONT-834), the fifteen
build slices (NEWFRONT-835–849), the nested-pages slice (NEWFRONT-850) and
the area gate (the last sub-issue of epic NEWFRONT-83). Every slice section
names the rows it owns; §12 maps each row ID to its section and the oracle
scenario that already proves it on `apps/web`.

Prose only. The old routes under
`apps/web/app/(all)/[workspaceSlug]/(projects)/projects/(detail)/[projectId]/pages/**`,
`apps/web/core/components/pages/**`, `apps/web/ce/components/pages/**`,
`packages/editor` (behaviour only) and the unchanged `apps/live` server were
read as **reference**. Nothing here is a translation of those files: no
component, hook, store, class name, style, icon or copy string is reused.
Two kinds of literal text appear on purpose:

- **Contract text** — API paths, query parameters, body and response fields,
  enum values, header names. These must match the server exactly and are
  named verbatim.
- **Parity labels** — a short user-visible string that an oracle scenario
  asserts literally (a menu entry, a tab name, a badge word, a toast). Each
  is quoted and marked _pinned_. §18 collects them in one table so a slice
  can check its copy in one place. Any other copy is described by meaning
  so slices write their own; where the oracle reads text by role or by
  position the spec says so.

Old sources consulted (reference only): the pages list route, its header,
tab strip, search, sort and filter controls, applied-filter chips, row list,
row item, row item action menu, empty and loading states, and loaders; the
create modal, quick-create action and duplicate path; the detail route and
its loader, error panel, breadcrumb header, searchable sibling switcher,
header action cluster, options menu and lock/visibility/archive/delete
dialogs; the page title and mark controls; the sticky formatting toolbar
and its typography, mark, alignment, list, quote, code, table, image and
colour entries; the editor surface built on the shared editor package
(behaviour only — extensions, slash menu, drag handles, embeds, mentions,
mark copy and character count); the navigation pane shell, its heading
preview, outline, info and assets panels; the version timeline, version
overlay and restore path; the export dialog; the assistant menu; the
community and cloud stub slots; the collaboration layer (socket lifecycle,
sync badge, offline pill, fallback persistence, versions and titles); the
API page views, serializers, URLs, permission classes, version endpoint and
the live server's health, move-server, conversion and printable-export
controllers. `apps/live` is an unchanged dependency: this area consumes its
protocols and never forks them.

Current-state source of truth for what is already built in `apps/web_new` is
§10 (reuse map), surveyed on `web-new-dev` at `3701e2fff`. The oracle files
`apps/web_new/e2e/parity/pages/*.spec.ts` (nine files, NEWFRONT-312–320) are
the behaviour record; §12 lists the scenario per row.

## 0. What this spec must settle, and where

The issue asks this file to resolve five items. Their homes:

1. **PAGE-036 row-text confirmation** — §5.3 item 8: the letter/wash colour
   control, a popover with one swatch row per kind plus a clear choice in
   each, applying to the current range, persisting across reload, and
   clearing back to plain text.
2. **PAGE-084–090 mapping and the `web_new`-only scenario convention** —
   §12.2 (mapping in order, as pinned in NEWFRONT-850) and §12.3 (file,
   driver placement, row tags, skip guard, how the gate runs them).
3. **Cross-epic seams** — §13: PAGE-009 (favourites nav), PAGE-043 (peek
   overlays), PAGE-050 (profile links and the deactivated fallback),
   PAGE-040 (community upgrade prompt vs licensed resolution), PAGE-074
   (the export dialog registration seam), pages entries in the command
   palette, and NEWFRONT-526's deferred SHELL legs. Each says what pages
   builds, registers and reuses.
4. **Reuse map** — §10: the survey of what already landed in
   `apps/web_new`, per slice, with what must be added and what must not be
   duplicated.
5. **Nested-pages API sketch** — §11.7: `parent=<id>` children listing,
   `parent_id` / `sub_pages_count`, parent validation, the backend's
   current state, and the probe a run uses to tell whether it has landed.

Rows are owned by no single slice here; slices mark their own rows `built`
(§17). This file changes no inventory Status.

## 1. Mental model

A **page** is a project-scoped collaborative document plus a small amount of
metadata. The metadata (name, mark, visibility, lock, archive stamp, parent,
owner, timestamps) is ordinary REST resource state. The **body** is a
conflict-free replicated document (`Yjs`) that the editor and every other
open client mutate over a websocket to `apps/live`; the server debounces
snapshots of it and writes all three stored forms (binary, HTML, JSON). The
REST description endpoint exists only as the fallback and as the primitive
that the live server itself uses; the browser does not write the body over
REST while the socket is healthy.

Consequences that shape every slice:

- **Two write paths to one document.** Title, mark and properties save over
  REST immediately. The body rides the socket. A body write through the REST
  fallback must carry all three forms at once, and the next socket snapshot
  wins if both run concurrently — the UI never runs both on purpose.
- **Occupancy is not ownership.** Locking is a cooperative freeze: a locked
  page renders read-only to _everyone_, including its owner, until unlocked.
  Visibility decides _who may open_ a page; lock decides _whether it may be
  edited_; archive decides _whether it is listed as live_.
- **Visibility is project-local.** A page belongs to one project (through
  the project–page link) and is visible to a project member when it is open,
  or to its owner when it is restricted. There is no per-page ACL beyond
  that pair.
- **Server is authoritative, the client hides.** The client never renders a
  disabled affordance for a missing permission (page creation and starring
  excepted, §2.5); it renders nothing and lets the server refuse the call if
  one is attempted.
- **Lists are one cached collection.** The project page collection is read
  once per workspace/project and partitioned client-side into the three
  tabs. With nested pages (D17) the default collection stays top-level only,
  so the tree fetches children per expanded parent (§11.7, §12.2).

The document's hash is the **mark** — a single emoji or a single vector
icon, never a cover image (PAGE-082).

## 2. Cross-cutting rules

### 2.1 Roles, ownership, and what each control needs (PAGE-018, PAGE-025)

Workspace roles are Admin 20, Member 15, Guest 5. A page's **standing** is
the higher of ownership and project role, and the client derives every
capability from it:

| Capability                                              | Needs                                                                      |
| ------------------------------------------------------- | -------------------------------------------------------------------------- |
| See the pages tab and open open pages                   | any active project member (guest included, subject to §2.2)                |
| Open a restricted page                                  | its owner only (an admin who does not own it is refused)                   |
| Create (quick or dialog), star, duplicate               | member or admin                                                            |
| Lock, unlock, change visibility, archive, restore, move | owner or admin                                                             |
| Delete                                                  | owner or admin, archived pages only                                        |
| Edit the body, title, mark                              | open _and_ unlocked _and_ unarchived, and either member/admin or the owner |
| Restore a version, apply an assistant reply             | edit permission                                                            |

The oracle's UI matrix (pinned in `gating-states-detail-shell.spec.ts`):
an owner has the full surface; a member sees only open-in-new-tab, copy-link
and duplicate on list rows; an admin on an open page matches the owner but
on a restricted page nobody but the owner gets in; a guest sees no rows
(§2.2), no create button, no star, and a reduced menu. Server truths pinned
in the same file and worth keeping in the spec: creation answers 201 for a
member and 403 for a guest; reading an open page is 200 for a member and 400
for a guest; a restricted read by a non-owner is refused; lock answers 204
even when the server allows it for a member the UI would hide it from;
archive answers 200; an access change by a non-owner is 400; deleting a live
page is refused because deletion requires an archived page.

### 2.2 Guest rows

A guest's collection read answers 200 with only the pages the guest owns,
unless the project's guest-all-features flag is on. The list therefore shows
the guest's own pages and nothing else — not an error panel. Direct links to
pages the guest does not own land on the not-found panel (§4.2).

### 2.3 Visibility, lock, archive — the three independent state flags

- **Access** is `0` (open) or `1` (restricted).
- **Lock** is the boolean `is_locked`. Locking is optimistic on the client
  with rollback and a notice on failure; the transitional phase between the
  old and new state is a brief visual state. Locking freezes editing for
  everyone, owner included.
- **Archive** is `archived_at` (a timestamp or null). Archiving is
  optimistic; restoring clears it. Archiving drops every user favourite row
  for that page. A page's archive marker reads as a dated line in its header
  (`Archived at <date>`, pinned shape: month, day, year).

Archive is the only state that also changes _listing_: archived pages leave
the first two tabs and appear in the shelved tab (§3.1). A shelved page
keeps its content and is rendered read-only with restore and delete
available in place of visibility and the sticky-toolbar toggle (PAGE-059).

### 2.4 Notices

Every mutation that can fail reports through the shared toast surface: a
success notice on completion and an error notice on failure, with the
existing shared timeouts. Where a mutation is optimistic (star, lock,
archive), a failed write also rolls the visible state back. Notices never
stack indefinitely on the same surface: a notice fired while another is
still showing is dropped, which is why the oracle quiesces between steps.

### 2.5 The two "rendered but disabled" cases

Only two controls render disabled instead of hiding: the list's empty-state
create action for a guest (§3.7), and the project feature-management
shortcut for a non-admin when the pages feature is off (PAGE-001). Every
other gated control is absent when not permitted.

### 2.6 URLs and query keys

- List: `/{workspaceSlug}/projects/{projectId}/pages` with one query key,
  `type`, whose values are the open tab (default, also the fallback for any
  unknown value), `private` and `archived` (§3.1). The list never carries
  search, sort or filter state in the URL (PAGE-021).
- Detail: `/{workspaceSlug}/projects/{projectId}/pages/{pageId}` with two
  optional query keys: `paneTab` (`outline`, `info`, `assets`) and
  `version` (a version id). Both are shareable and are what the sibling
  switcher and copy-link deliberately _drop_ (PAGE-080).
- The canonical page address copied anywhere is the detail path above with
  no query string (PAGE-011, PAGE-055).

### 2.7 Loading and failure shape (PAGE-019, PAGE-023, PAGE-079)

Four distinct async surfaces, each with its own loading shape:

1. **List first load** — a header skeleton plus placeholder rows; a refetch
   over cached rows shows the rows with a light non-blocking mutation
   indication and never re-skeletons.
2. **Detail record** — a centered brand spinner while the record resolves;
   the title area renders at its full height with a placeholder even before
   the name arrives.
3. **Body** — skeleton blocks until the document (socket or cache) is known
   to be empty-or-not; the body loader shows only before the document
   caches (§8.1).
4. **Version snapshot** — skeleton rows in the timeline and skeleton blocks
   inside the overlay while the snapshot read is in flight.

A failed list read falls through to the docs-flavoured empty content with
the create action enabled; it never strands a blank panel, and it never
substitutes stale rows for an error. A failed detail read renders the
not-found panel. A failed mutation reports by notice (§2.4).

## 3. List screen (PAGE-001–021)

Route `/{ws}/projects/{projectId}/pages`; the project shell's sub-navigation
owns the tab strip above it (shell area). The pages screen owns the header,
the list tabs, the search/sort/filter cluster, the rows and the empty,
loading and error states.

### 3.1 Feature gate and visibility tabs (PAGE-001, PAGE-002)

- **Feature off.** When the project's pages feature is off, the screen shows
  a guidance illustration with a short explanation and a shortcut into the
  project's feature management. The shortcut is an admin affordance: admins
  get an enabled link to the project's feature settings; everyone else sees
  the same control disabled (no navigation on click). No list, tabs, search
  or create control renders.
- **Feature on.** A sub-header tab strip splits the collection into three
  tabs: **Public** (open pages), **Private** (restricted pages) and
  **Archived** (shelved pages). _Pinned labels: the tabs read `Public`,
  `Private`, `Archived`._ The selection rides the `type` query value; a
  missing or unknown value lands on Public; re-selecting the active tab does
  nothing (no reload, no refetch). The strip only renders once at least one
  page exists in the collection — an empty project shows the empty state
  without a tab strip.

The split is client-side over the one collection read (PAGE-020): open
pages by access `0` and no archive stamp; restricted pages by access `1`
and no archive stamp; archived pages by a non-null archive stamp. A guest
only ever holds their own rows (§2.2), so the tabs partition that subset.

### 3.2 Search (PAGE-003)

The search control starts as a collapsed button in the cluster. Activating
it reveals a text field; typing filters rows by title immediately
(case-insensitive substring over the page name, untitled pages matched by
their fallback label). Escape first clears the text when there is any, and
collapses the control when the field is already empty. Clicking anywhere
outside collapses it only while it is empty. A search that matches nothing
switches the content area to the no-results empty variant (§3.7). Search is
client state and resets on project switch (PAGE-021).

### 3.3 Sort (PAGE-004)

The sort control is a single menu: one of **Name**, **Date created**,
**Date modified** (pinned labels), then a separator and **Ascending** /
**Descending** (pinned labels). Default is date modified, descending
(newest first). Changing either dimension re-sorts the visible rows
immediately, client-side, without a refetch. Name sorts with the locale's
collator, case-insensitively, and untitled pages sort by their fallback
label. Date sorts compare the `created_at` / `updated_at` timestamps. The
control's button shows the active key's label and a direction glyph. Sort
is client state and resets on project switch.

### 3.4 Structured filters and chips (PAGE-005)

The filter control offers three dimensions:

- **Favourites only** — a toggle that keeps only rows the caller has
  starred.
- **Creation date** — a relative-condition list plus a custom date. The
  relative values are the server's expression syntax
  `N_weeks;before;fromnow`, `N_weeks;after;fromnow`, `N_months;before;fromnow`,
  `N_months;after;fromnow` (the UI presents readable labels such as "1 week
  ago", "2 weeks from now"); the custom option is a calendar date and is
  applied as a `today`/`yesterday`/`last_7_days`-shaped custom expression or
  the picked date. Predicates are evaluated client-side against
  `created_at`.
- **Creator** — a searchable picker over the project's members (the shared
  member picker, §10); picking a person narrows to pages they own.

Each active filter renders as a removable chip in a row under the cluster,
plus a clear-all control that removes every filter at once. Removing one
chip removes only that filter; the other chips stay. Zero matches show the
no-results empty variant. Filters are client state and reset on project
switch.

### 3.5 Rows (PAGE-006)

Each row renders, left to right: the page **mark** (emoji or icon; the
fallback for a page with no mark is a neutral document glyph), the **title**
(untitled pages show a visual fallback label, not an empty line), the
**owner avatar**, a **visibility glyph**, a **creation-date detail**, the
favourite **star** (member and above only) and the overflow menu. The
visibility glyph and the date are revealed on row hover together with the
owner; a restricted row's glyph reveals a tooltip reading `Private` (pinned
word) and an open row's glyph reveals `Public` — the tooltip must actually
open (see §15 for the old bug). Clicking the row opens the detail view;
right-click anywhere on the row opens the same overflow menu at the pointer
(the browser context menu is suppressed). The row menu is described in
§3.8.

The visible order is always the client sort, defaulting to modification date
descending. The server returns favourites first and then newest-created
first, but that order is not what the list shows once the default sort
applies — the oracle asserts the full row order per key and direction, and
favourites are not pinned on top. Name comparison lowercases both names
before comparing; dates compare the ISO timestamps.

### 3.6 Create (PAGE-007, PAGE-008)

**Quick create.** The list header's primary action creates a page with no
form: name `Untitled`, no mark, access inherited from the active tab (open
tab → access `0`, private tab → access `1`, archived tab → open, i.e. `0`,
because an archived page cannot be created). On success the client
navigates straight to the new page's detail. On failure it stays on the
list and shows an error notice. The action is member and above; guests see
no create control.

**Create dialog.** A secondary path opens a modal with: an optional mark
picker (the same emoji/icon picker as the detail title, §5.2), a required
title (trimmed; the submit is blocked while empty), an inline validation
when the title exceeds **255** characters, and an open-or-restricted
choice. Cancel discards the draft (no request). Submit creates the page
(`POST` §11.1) and closes; when the caller asked to continue to the page,
the client navigates to the new id, otherwise it stays on the list with the
row present. Errors keep the dialog open with its input for retry (§11.3;
PAGE-008 has no pinned bug — the "keep input on failure" rule is the
general mutation rule in §2.4).

### 3.7 Empty and loading variants (PAGE-019)

Five mutually exclusive content states:

1. **First-load skeleton** — a header skeleton plus 3–6 placeholder rows,
   each with mark, title, avatar and menu placeholders.
2. **Empty collection / empty tab** — a docs-flavoured illustration with
   guidance copy and a create action labelled `Create your first Page`
   (pinned) for admins and members (guests get it disabled, §2.5). The copy
   differs slightly between "no pages in this project yet" and "no pages in
   this tab".
3. **Empty shelved tab** — the same illustration family but with **no**
   action (nothing can be created into the archive).
4. **No results** — filters or search matched nothing: an illustration whose
   heading is `No matching results.` (pinned) with clear-filters /
   clear-search actions, and no create action.
5. **Feature off** — §3.1 guidance.

A failed collection read renders variant 2 with the create action enabled
(never a dedicated error panel, never stale rows).

### 3.8 Row overflow menu (PAGE-010, PAGE-011, PAGE-012, PAGE-013, PAGE-014, PAGE-015, PAGE-016, PAGE-017)

One menu, opened from the row's overflow button or a right-click. The
canonical owner-level order (pinned by the oracle):

`Open in new tab`, `Copy link`, `Make a copy`, `Lock` (or `Unlock`),
`Make private` (or `Make public`), `Archive` (or `Restore`) — all pinned.

- **Open in new tab** — every viewer; opens the canonical detail address in
  a new browser tab.
- **Copy link** — every viewer; writes the canonical address to the
  clipboard (toast `Page link copied to clipboard.`, pinned).
- **Make a copy** — member and above; `POST .../duplicate/`; the copy
  appears in the collection (toast `Page duplicated successfully.`,
  pinned).
- **Lock / Unlock** — owner or admin; toggles through the lock endpoint and
  the collaboration channel; the lock indicator updates optimistically
  (toasts `Page locked successfully.` / `Page unlocked successfully.`;
  failure `Page could not be locked.` — all pinned).
- **Make private / Make public** — owner or admin; writes access `1`/`0`;
  the row migrates between the Private and Public tabs (toast ends in
  `marked private` / `marked public`; pinned suffix). The entry is hidden
  while the page is archived.
- **Archive / Restore** — owner or admin; shelve or restore; the row moves
  to or out of the Archived tab and archiving drops the favourite (toasts
  `Page archived successfully.` / `Page restored successfully.`, pinned).
- **Delete** — owner or admin, **only** on an archived page, behind a
  confirmation dialog that names the page and warns the action is permanent
  and cannot be undone; the confirm button shows a busy state while the
  request runs. Cancel writes nothing. Deleting from the detail navigates
  back to the list after success.
- **Move to project…** — owner or admin, **cloud edition only**; the
  community build carries no entry and the server route is a no-op stub
  (PAGE-017, §11.1 and §11.2). The licensed behaviour is a picker over
  the projects the caller can write to; confirming relocates the page, it
  leaves the current project's collection and appears in the target's.

A member's menu holds only the first three entries; a guest gets no rows at
all. List rows deliberately do **not** carry Export (PAGE-083).

### 3.9 Data loading and caching (PAGE-020)

The collection is read with one request per workspace/project (a shared
query key scoped by workspace). Tab switches, sort, search, filter, and
opening a row do not refetch. Back-navigation within the cache window holds
the cached rows while revalidating once. There is no separate favourites
collection read: the favourites-only filter is a client predicate on the
same rows (`is_favorite` on each page). The dedicated favourites and
archived collection endpoints answer 404 in this build and are never
called — do not add contracts for them.

### 3.10 URLs and client state (PAGE-021)

The URL carries the active tab only (§2.6). Search text, sort key and
direction, and the three filter dimensions live in client state and reset
when the project changes. Row links carry the page id, so a pasted row
address opens that page; quick create navigates to the new id.

## 4. Detail shell (PAGE-022, PAGE-023, PAGE-024, PAGE-025, PAGE-079, PAGE-080)

Route `/{ws}/projects/{projectId}/pages/{pageId}`.

### 4.1 Load (PAGE-022)

One record read by id with visit tracking on
(`GET .../pages/{pageId}/?track_visit=true`, §11.1), plus a collection read
for the sibling switcher (cached, shared with the list). While unresolved,
the shell shows its centered spinner (§2.7). The browser tab title follows
the page name, including the fallback for an empty name, and updates as the
name changes. The document binary is cached client-side per page so a
return to a page paints from cache before the socket syncs.

A record that the server reports as deleted (present but stamped deleted,
or absent when the shell expected it) shows the not-found panel on a direct
load. A viewer sitting on a page that is deleted underneath them meets the
same panel; the old app never broadcast a delete to other clients, so the
new app does not invent a push — the panel appears on the next load,
refetch or navigation.

### 4.2 Not accessible or missing (PAGE-023)

A forbidden id (restricted page, non-owner), a missing id, and a guest's
foreign id all render the same explanatory panel: the heading `Page not
found` (pinned), copy explaining that the page may be private or no longer
exist, and a recovery link labelled `View other Pages` (pinned) back to the
pages list. While the record is unresolved the shell shows the centered
brand spinner (pinned shape). The editor never mounts behind it. A
forbidden id must not flash the editor first.

### 4.3 Breadcrumb cluster and header (PAGE-024)

Across the top of the detail: the project context (name with its mark, a
link back to the project), a back-to-list control, the **sibling switcher**
(a searchable menu over the sibling pages the collection holds — same
project, not archived, that the caller can see; selecting one navigates to
it and drops the `paneTab` and `version` keys), the **sync indicator**
(§8.2), and the **header action cluster** (§7: favourite, link copy, and
whatever edition slots register). The cluster always leaves a path back to
the list.

### 4.4 Read and edit gating (PAGE-025)

Read: an open page for any active member (guest included); a restricted
page for its owner only. Edit: read plus member-or-above or ownership, and
the page unlocked and unarchived. A locked or archived page renders the
full shell with the body and title as static read-only content and no
editable region, no mark control, no toolbar and no destructive body
actions; the header still shows the archive marker and the restore/delete
actions their owners may use.

### 4.5 Loading and deep links (PAGE-079, PAGE-080)

- Record spinner before the record resolves; skeleton body blocks until the
  document is known; the title area never collapses and shows a
  placeholder when empty; the version timeline shows skeleton rows while
  its read is in flight.
- `paneTab` opens the navigation pane on outline, info or assets. `version`
  opens the version overlay directly on load for that snapshot. The
  timeline selection writes `version`; the history shortcut (in the options
  menu) writes `paneTab=info`; closing the overlay removes `version`; the
  switcher and the copy-link control drop both keys. A shared
  `?paneTab=info&version=<id>` URL reproduces exactly that view on a fresh
  machine.

## 5. Title, mark and toolbar (PAGE-026, PAGE-027, PAGE-028, PAGE-029, PAGE-030, PAGE-031, PAGE-032, PAGE-033, PAGE-034, PAGE-035, PAGE-036)

### 5.1 Title (PAGE-026)

The title is an in-place editable heading at the top of the canvas. Typing
saves over `PATCH .../pages/{pageId}/` immediately (debounced keystrokes,
outside the body path). The cap is **255** characters with a live count
that is visible only while the field is focused. Enter does not insert a
newline: it moves the caret to the start of the body. Read-only pages render
the title as static heading text with the fallback label for an empty name.
The old build's defects — accepting over-255 titles without a count
(NEWFRONT-354), and its related title-count badge — are fixed here: input
past the cap is refused and the count communicates the limit.

### 5.2 Mark picker (PAGE-027)

A mark affordance beside the title reveals on hover and stays pinned while
the title is empty (so a new page can find it). It opens a picker with two
tabs, **Emoji** and **Icon** (vector icons from the shared icon set),
searchable; picking persists over `PATCH .../pages/{pageId}/` — the mark is
the page's `logo_props` (`{in_use: "emoji" | "icon", ...}`), which is the
only field the picker writes (`color` is a separate, unrelated column and
the mark never goes through it) — and the mark appears in the list row, the
breadcrumb cluster and the sibling switcher. The picker offers a clear action that removes the
mark; clearing persists an empty mark and the fallback glyph returns (the
old build refused the clear with a 400 — NEWFRONT-355). The whole surface
is absent for read-only viewers.

### 5.3 Sticky formatting bar (PAGE-028, PAGE-029, PAGE-030, PAGE-031, PAGE-032, PAGE-033, PAGE-034, PAGE-035, PAGE-036)

A sticky bar above the body, editable pages only. It hides entirely for
read-only viewers and when the user's sticky-toolbar preference is off
(PAGE-035); when hidden, the editor's floating formatting affordance (the
selection toolbar) is still reachable, and the pane toggle in the header
remains available either way.

Controls, in reading order:

1. **Typography selector** (PAGE-028) — Plain text and six heading levels;
   shows the block kind under the caret and tracks caret movement;
   applying restyles the block.
2. **Mark buttons** (PAGE-029) — bold, italic, underline, strikethrough;
   active highlight for the current selection; tooltips name the action and
   its key hint.
3. **Alignment buttons** (PAGE-030) — left, centre, right, with active
   highlight and key hints.
4. **List buttons** (PAGE-031) — numbered, bulleted, checkable lists, same
   tooltip/highlight rules.
5. **Quote and inline code** (PAGE-032) — restyle the selection; persist
   through save and reload.
6. **Table** (PAGE-033) — inserts a table at the caret with extended
   keyboard navigation between cells once inside; the table saves and
   reloads with the document.
7. **Image** (PAGE-034) — opens the file picker, uploads through the file
   pipeline (§11.4) and inserts the image inline at the caret; the image
   appears in the assets pane (§6.7).
8. **Colour** (PAGE-036, confirmed text) — the letter/wash colour control.
   Its button previews the current letter colour and text-wash colour of
   the selection. It opens a popover with **one swatch row per kind** —
   one row of text colours ("letter") and one row of highlight/wash
   colours — and each row carries its own **clear** choice. Choosing a
   swatch applies that colour to the current range; the choice persists
   with the body across reload; clearing removes that colour from the
   range and the text returns to plain styling. Applied colours show in the
   button preview and as active swatch states.

The bar holds **14** icon-only controls (pinned: the oracle counts the
icon buttons); the typography selector is the one text control. Every one
of these is a formatting command on the document, saved with the body; none
of them writes REST itself. The colour control has no separate
storage: letter and wash colours are marks on the body.

## 6. Pane, blocks and embeds (PAGE-037, PAGE-038, PAGE-039, PAGE-040, PAGE-041, PAGE-042, PAGE-043, PAGE-044, PAGE-045, PAGE-046, PAGE-047, PAGE-048, PAGE-049, PAGE-050)

### 6.1 Block catalog and editor surface (PAGE-037–039, PAGE-042, PAGE-044)

The body editor is the shared rich-text editor configured for pages. Its
block catalog, reachable from the slash-command palette and a drag handle
on hovered blocks, contains: plain text, the six heading levels, numbered,
bulleted and checkable lists, tables, quotes, fenced code blocks, inline
code, callouts, dividers, images, person mentions, links, underline,
alignment, emoji, letter colour and wash colour, markdown paste and a live
character count. Every entry inserts, saves with the document and reloads.

- **Fenced code** (PAGE-038) is a block alongside the inline code mark; both
  persist in the payload.
- **Uploads** (PAGE-039) resolve through paired handlers: an upload attaches
  a fresh asset id, and a duplicate/paste from another page clones the
  existing asset onto this page rather than pointing at a foreign asset;
  the editor reports asset changes so the assets pane stays current.
  Removing an image from the document removes its assets row.
- **Body emoji** (PAGE-042) come from the editor's own picker, distinct from
  the page mark picker; they persist with the body.
- **Drag handle** (PAGE-037) exposes move-up/move-down, duplicate and
  delete of a block; all persist.
- **Markdown paste** (PAGE-037) renders pasted markdown into blocks.
- **Copy body as markup** (PAGE-044) — the options menu's copy-markdown
  entry writes the body's markup representation to the clipboard and shows
  a success notice. It reads the live editor content and is available to
  editors only.

### 6.2 Work-item embeds (PAGE-040)

An embed is an atomic, selectable block carrying workspace, project, entity
id and a display label. The block serializes all of those into the document
(`type: "issue-embed-component"` with those attributes in the JSON form),
so a licensed client can resolve it, and it renders as a single unit that
selects and deletes as a whole. In the community edition the embed slot
renders an upgrade prompt in place of the live card, with an outbound link
to the upgrade surface (shared `UpgradePill` and the kit's outbound-link
styling, §10.1). The old community build rendered nothing at all
(NEWFRONT-352); the intended behaviour is the prompt, and the gate proves
the prompt on `web_new` while treating the old "nothing renders" assertion
as target/no-oracle. In a
licensed edition the slot renders the referenced item's card and its
peek-overlay (§13.2, PAGE-043 peer).

### 6.3 Links and entity previews (PAGE-043)

Inline links are editable in place: clicking a link while editing moves the
caret, and a link editing affordance edits its target text in place.
Pasting a bare address inserts a link with that address (linkified). Known
entity references render as rich previews — a card that names the entity
and opens the entity's peek overlay on click — driven by the shared
content-parsing layer that rewrites custom nodes for display, copy and
export. The old community build linkified pasted addresses but rendered no
preview card (pinned as the current behaviour); the new app builds the
preview for recognised entity references it has a resolver for, and the
gate proves the link-editing and linkify halves plus the preview on the
entity kinds the shell resolver knows. The pages slice does not own the
peek overlay: it emits the entity reference and lets the shell's peek
open it (§13.2).

**Storage contract.** The reference's stored element is
`entity-reference-component` with its identifiers and `data-label`. The
tag is on the API content allowlist
(`apps/api/pi_dash/utils/content_validator.py`) and the live document
schema parses it, so a save round-trip through the server preserves the
node. The same holds for `issue-embed-component` (PAGE-040): its
identifiers persist in `description_html` as well as the structured
`description_json` snapshot. PAGE-040's and PAGE-043's save/reload
clauses are claimed as proven, and the pages area gate proves them.

### 6.4 Mentions (PAGE-041)

Typing the mention trigger opens person search across the workspace,
scoped to the current project (`GET /entity-search/` with
`query_type=user_mention`, `project_id`, `count`, §11.5). Suggestions are
custom rows (avatar plus display name). Picking one inserts a mention chip
that stores the member id and a display name; at render the chip resolves
the current display name from the member directory, falling back to the
stored name when the person is gone. Chips survive save and reload. A
mention of the current user is visually distinguished, as the editor
configures it.

### 6.5 Navigation pane shell (PAGE-045, PAGE-046)

A right-hand pane slides in and out with the standard motion, at a fixed
width, and the canvas reflows around it (a fixed readable measure, about
720px with the pane closed and about 1152px with it open — pinned by the
oracle as the two measures the full-width toggle chooses between). It hosts
three tabs in order: **Outline**, **Info**, **Assets** (pinned labels). The
active tab rides `paneTab`; "temporarily replaced by extension content"
is the edition pane slot (§9.4). While the pane is closed, a slim
heading-density preview floats at the canvas edge, summarising the
document's heading structure; hovering expands it, and selecting a heading
opens the pane on Outline and scrolls there. The preview reads the live
editor headings, so adding a heading updates it without reload.

### 6.6 Outline (PAGE-047)

Lists first-, second- and third-level headings in document order, live as
the user types. Clicking an entry scrolls the body to that heading. With no
headings, a themed empty illustration prompts the author to add headings.

### 6.7 Assets (PAGE-048)

Lists the document's attachments in document order: image rows show a
thumbnail, the file name, and a download action revealed on hover, using
the workspace asset view and download URL builders (asset id, project id,
workspace slug, §11.4). Non-image attachment types delegate to an extension
renderer (the edition asset slot; community shows nothing for them beyond
their name). Uploading an image adds a row; removing it from the document
removes the row. Plain links are never rows. With no attachments, a themed
empty illustration shows.

### 6.8 Info — statistics (PAGE-049)

Live counts of words, characters and paragraphs, plus an estimated reading
time that updates while typing. Reading time reads in seconds under a
minute and minutes above it (the oracle asserts both ranges). The counts
subscribe to editor changes, not to reloads.

### 6.9 Info — attribution and versions (PAGE-050, PAGE-051, PAGE-052, PAGE-053, PAGE-054)

Attribution rows: **last edited by** with a relative timestamp, and
**created by** with a calendar date. Each name links to that person's
profile (§13.3). A deactivated or missing member renders a textual fallback
(e.g. "Deactivated user", pinned shape: the row stays intact, the name is
replaced by a deactivated-user label, and it is not a broken link).

Below them, the **version timeline**: a current-version row first, then
dated entries newest-first, each with timestamp and author (the snapshot's
owner, named from the workspace directory, with the deactivated-user
fallback of §6.9), the active entry highlighted. The first row is
labelled `Current version` (pinned), points at no snapshot (an id-less
entry, never a stored snapshot id) and is the active row on the live
document; selecting it (or closing the overlay) leaves any snapshot
behind. Selecting a dated entry writes `version` to the URL and opens the
version overlay.

**Version overlay** (PAGE-052–054): a full-canvas overlay above the editor
showing the snapshot's timestamp with a read-only marker, the frozen
content rendered from the snapshot (structured JSON preferred, HTML
fallback), skeleton blocks while the snapshot read is in flight, and a
load-failure panel with a retry action. The structured form is preferred
exactly when the whole stored document uses nodes the app renders (doc,
paragraph, heading, text, hard break, bullet/ordered lists with items,
quote, code block, divider); otherwise the stored markup renders, so an
unfamiliar node degrades to the server's own rendering rather than to a
blank body. A retry asks for the snapshot again and the skeleton returns
while the new read flies. Restore appears only with edit permission: it
writes the snapshot into the live editor in place — the document is
replaced through the live document, the user stays on the page, and a
success notice confirms; failures report by notice. Read-only
viewers can view snapshots and never see restore. Closing the overlay
removes `version` from the URL and returns to the live document.

The restore control sits in the overlay header beside the close control,
appears only once the snapshot is loaded (a failed read shows the error
panel and nothing else), and writes the same body the overlay is
showing — the structured render when the document is known, the stored
markup otherwise. The write is the editor's own whole-document
replacement, reported as an ordinary edit, so the draft the save path
persists, the live outline and statistics all follow it, and the address
does not change. The overlay then closes (the `version` key is dropped)
and the history read is refreshed, because the restore itself is a new
save. A refused or impossible write (no live editor to take it) shows the
failure notice and leaves the overlay open. Pinned copy: `Restore`,
`Page version restored.`, `Page version could not be restored.`

## 7. Header actions and options menu (PAGE-055, PAGE-056, PAGE-057, PAGE-058, PAGE-059, PAGE-060)

### 7.1 Link copy (PAGE-055)

Writes the canonical address (§2.6) to the clipboard, swaps to a
confirmation state for roughly a second (the oracle accepts 300–3000ms),
then reverts. Pasting the address reopens the same page.

### 7.2 Favourite (PAGE-056)

A star in the header, member and above. It writes the favourite through
`POST/DELETE .../favorite-pages/{pageId}/` (no collection read exists for
it), flips filled/empty optimistically, rolls back with an error notice on
failure, and — on the first favourite — asks the sidebar favourites section
to open (§13.1). It coordinates with the list star through one shared
favourite write helper and two distinct driver method names, never two
implementations.

### 7.3 Visibility, lock and archive in detail (PAGE-057, PAGE-058, PAGE-059)

- **Visibility** lives only in the options menu (`Make private` / `Make
public`, pinned labels), not as a separate header control. It is hidden
  from non-privileged viewers and while the page is archived (archive shows
  Restore instead). Selecting it writes access `1`/`0` and notices.
- **Lock** toggles through resting → locked, with a brief transitional
  state on the way (about 600ms in the old build; the new build may choose
  its own short duration but must show it). It is optimistic with rollback.
  The oracle pins a notable consequence: locking freezes editing for the
  owner too. The detail options menu does **not** carry a lock entry — the
  list row menu owns Lock/Unlock (PAGE-014); the header lock control is the
  detail's own affordance.
- **Archive** in the detail freezes editing, marks the header with the
  dated line (§2.3), and surfaces Restore and Delete while hiding visibility
  and the sticky-toolbar toggle. Restore clears the marker, unfreezes
  editing and returns the page to its tab. Delete is the same confirmation
  flow as the list (§3.8) and navigates back to the list after success.

The old build has a stale-resolve race (NEWFRONT-369) where the load-time
record read resolves after a mutation and overwrites it. The new app must
not reproduce it: a mutation invalidates or writes through the record cache
so the menu and editor reflect server truth without a reload.

### 7.4 Options menu (PAGE-060)

The detail's options menu gathers, in the owner order pinned by the oracle:
**Full width**, **Sticky toolbar**, **Copy markdown**, **Version history**,
**Make a copy**, **Archive** (or **Restore**), **Make private** (or **Make
public**), **Export**. The last entry is the export registration seam's
mount point (§9.6): the menu shows it only when a registration exists, and
the pages side places it after the pages-owned entries, which is where the
oracle pins `Export` (after `Make private`, PAGE-060). Full width toggles
the canvas measure (§6.5) and persists; sticky toolbar toggles the bar
(§5.3) and persists; Copy markdown is PAGE-044; Version history opens the
pane on its info tab; Make a copy is PAGE-012; Archive is §7.3; visibility
is §7.3; Export opens the export dialog registered by the exports area.

The menu degrades by role: a member sees Full width, Sticky toolbar, Copy
markdown, Version history, Make a copy, Export; the owner/admin entries
(Archive, private/public) are absent. A guest never reaches the detail.
The menu deliberately has **no** new-tab, copy-link, lock or move entry —
the list row menu owns those. Delete appears only on an archived page
(§7.3).

### 7.5 What the header does not carry (PAGE-076–078, PAGE-082, PAGE-083)

- No share control in the community build (cloud slot, PAGE-076).
- No collaborator avatar strip (cloud slot; the server has no presence
  roster, PAGE-077).
- No extra header buttons or pane tabs beyond the registered edition slots,
  which contribute nothing on community (PAGE-078).
- No cover image anywhere: neither the list row nor the detail header has a
  cover upload, crop or display path (PAGE-082).
- No favourites tab in the list, no export action on list rows, no global
  shortcut cheat sheet, no merge-conflict dialog (PAGE-083). The list's tabs
  are exactly Public/Private/Archived; the row menu is exactly the six
  entries in §3.8; export exists only in the detail options menu; shortcuts
  appear only as toolbar tooltips; concurrent edits converge silently.

## 8. Collaboration, sync and persistence (PAGE-061, PAGE-062, PAGE-063, PAGE-064, PAGE-065, PAGE-066, PAGE-067, PAGE-068, PAGE-069, PAGE-070, PAGE-071)

`apps/live` is an unchanged dependency; this area mounts a client against
its protocol. No slice forks the server.

### 8.1 Socket-first persistence and fallback (PAGE-064, PAGE-065)

- The editor joins a Yjs document over the collaboration socket
  (`ws(s)://<live-host>/live/collaboration?documentType=project_page&projectId=<id>&workspaceSlug=<slug>`,
  §11.6) and sends keystrokes over it. The server debounces snapshots
  (~10s observed), persists all three forms (binary, HTML, JSON) and a
  title manager syncs the name; legacy documents without a title are
  migrated by that manager. While connected there are **zero** browser
  REST description writes.
- A REST fallback covers socket loss: it fetches the latest description
  binary (`GET .../description/`, octet-stream), reconciles it with local
  state, and patches binary + HTML + JSON together (`PATCH
.../description/`) on roughly a 30s interval. The standard save gesture
  (the platform's save shortcut) forces one patch early (debounce ~500ms)
  and resets the interval. A body patch that 500s leaves the client wedged
  in the old build (NEWFRONT-359): the new app surfaces the error and keeps
  retrying without wedging.
- Title, mark and property changes save over the record endpoint
  immediately, outside the description path; they never wait for the
  fallback interval.
- Concurrent editors converge with no merge dialog. Reload shows the
  merged body and title.

### 8.2 Session identity and sync badge (PAGE-066, PAGE-061)

The socket handshake carries an auth token whose payload holds the current
user's id, display name and a deterministically derived cursor colour
(hashed from the id), so every client renders the same colour for the same
person. Socket stages map onto the header sync indicator:

- **Connecting** and **Syncing** show the badge in a transitional state
  (`Syncing...` is the pinned wording).
- **Synced** hides the badge shortly after confirmation (the exact delay is
  not pinned; a short beat is expected). Every page switch resets to
  Syncing.
- **Connection lost** is a stable fault badge; its tooltip explains that
  background saves continue (the old wording promised a 10-second save
  while the fallback actually ran every 30s — the new copy states the
  real cadence).

Normal editing never shows a persistent badge. A refused initial
connection must recover (the old client never retried a refused socket; the
new one retries with backoff and a terminal fault state).

### 8.3 Offline pill (PAGE-062)

On an editable page, when the browser reports offline, a separate **Offline**
pill appears in the header cluster after a short beat while the sync badge
hides. Typing continues locally. On reconnect the pill clears after a short
beat and the offline typing reaches the server. The pill is absent on
read-only pages.

### 8.4 Reconnect lifecycle (PAGE-067)

An explicit state machine: initial → connecting → awaiting-sync → synced →
reconnecting (with attempt count) → disconnected (with a typed reason).
A transient drop reconnects and re-syncs without user action. The sync
loader (centered brand mark) shows only when the local cache is known
empty and unsynced; a socket that can never connect exhausts retries into
the Connection-lost badge, and the client then fetches the binary fallback
separately and renders it.

### 8.5 Handshake/auth gating (PAGE-068)

The live server accepts a socket only when the token's user id matches the
session cookie the main API validates (`GET /api/users/me/`); a mismatch
terminates the session, and missing credentials open no socket. Every call
is scoped by cookie plus workspace and project ids. Privileged admin HTTP
routes on the live server additionally require a shared-secret header; the
community build wires no privileged admin route, so the secret-header gate
is unprovable live (the oracle pins a generic 404 with and without the
header). Do not invent an admin route to prove it.

### 8.6 Structured error events and force-close (PAGE-069, PAGE-063)

Load and persist failures reach the client as structured frames carrying a
readable message, a phase (load or persist), a machine code and a
disconnect flag. The UI subscribes to them and surfaces them (an error
notice for a persist failure; a load-failure state for a load failure) —
the old build never subscribed, so every failure was silent (NEWFRONT-338).
Oversized content additionally triggers a cross-server force-close: the
client receives a notice frame first, then connections close with a
dedicated code (4001 / document-too-large) and the document unloads.

**Oversized strip** (PAGE-063): when the backend rejects an update as too
large, a strip appears under the toolbar advising the author to split the
content across new or nested pages, because live sync is paused. It fires
once per incident with an optional dismiss. The old strip never rendered
(NEWFRONT-338); the new build renders it from the error frame.

### 8.7 Multi-instance fan-out (PAGE-070)

The live server shares state over a queue layer so queued commands survive
reconnection and a client can land on any instance. The client only has to
be instance-agnostic: it keeps its local document across reconnects and
does not assume a sticky server. The seeded single-instance stack cannot
prove cross-instance fan-out; the oracle proves the reconnection half
(typing while disconnected survives the reconnect with nothing lost or
duplicated), and the gate accepts that as the row's proof, recording the
cross-instance half as an environment gap.

### 8.8 Live operations endpoints (PAGE-071)

The live server exposes three operations the page area (and the export
path) rely on:

- `GET /live/health/` → `{status: "OK", timestamp: <iso>, version: <string>}`.
- `POST /live/convert-document/` with `{description_html, variant,
description_binary?}` → all three forms; malformed input (empty or
  markup-less HTML, unknown variant, bad base64) is a 400.
- `POST /live/pdf-export/` with `{pageId, workspaceSlug, projectId, title?,
noAssets?}` → a `%PDF` stream; unauthenticated 401, invalid body 400,
  missing page 404/502.

## 9. Preferences, assistant, editions and negative rows (PAGE-072, PAGE-073, PAGE-075, PAGE-076, PAGE-077, PAGE-078, PAGE-081, PAGE-082, PAGE-083)

### 9.1 Display preferences (PAGE-072)

Four editor display preferences: compact versus large type, typeface family,
full-width canvas, and sticky toolbar. They persist per user in local
storage (per-user keys via the shared preference storage), apply on mount
after a short beat, reflow the editor immediately, and never touch saved
content. Only canvas width and sticky toolbar ship visible toggles (in the
options menu); size and face follow the persisted configuration and have no
settings switcher in this area — do not add one.

### 9.2 Toolbar key map and tooltips (PAGE-073)

Toolbar tooltips advertise the ten-command map: four basic marks (bold,
italic, underline, strikethrough), three alignments, three list kinds. The
standard save gesture drives the manual fallback save (§8.1). Enter moves
from title to body; Escape collapses the empty list search. There is no
global shortcut map, no cheat sheet, no page-specific chord outside the
shared command registry. Tooltips name each hint; every hinted combination
fires its command with the editor focused.

### 9.3 Assistant menu (PAGE-075)

Editable pages, cloud edition: the editor's assistant menu operates on the
current text selection through the editor handle. A compact menu expands
while a task runs; one conversational task sends the selected text plus a
free-text instruction to the workspace-scoped editor-task endpoint
(`POST /api/workspaces/{slug}/ai-assistant/` with `{task, prompt}`;
`task` names the editor task) and optional tone scores that adjust
formality. The reply renders read-only with controls to apply it in place
(replacing the selection), append it below, or regenerate; loading and
failure render inline; empty selections are a no-op; closing resets the
task state. Gating is workspace member-or-admin with assistant access
configured.

Community: **no** assistant surface renders (no toolbar entry, no slash
entry). The editor-task endpoint is unrouted (404) and the root assistant
endpoint answers 400 "model not configured". The oracle pins exactly that;
the licensed reply/apply/append/regenerate/preset behaviour is a recorded
gap and must not be invented on the community stack. The contracts issue
(NEWFRONT-834) lands the request shape so the cloud build can enable it.

### 9.4 Editions and desktop (PAGE-076–078, PAGE-081)

- The header share slot and collaborator strip are cloud slots; community
  renders nothing (§7.5).
- Extra header actions and pane panels are edition slots; community adds
  nothing, and the cloud editor registries add zero block entries beyond
  the community catalog (the slash catalog is exactly the families in
  §6.1; pane tabs are exactly Outline/Info/Assets).
- Desktop is the same pages experience as web: no desktop-only branch,
  shortcut or layout variant. Pages code must not read a platform flag for
  behaviour.

### 9.5 Negative rows (PAGE-082, PAGE-083)

Do not invent: no cover image, no favourites tab, no list-row export, no
shortcut cheat sheet, no merge-conflict dialog. The list tab count, the row
menu length and the presence of export only in the detail menu are pinned
by the oracle and must stay true.

### 9.6 Export dialog seam (PAGE-074)

The export dialog is built by the exports area (NEWFRONT-770, EXP-016–021);
this area only mounts its entry. The contract between the two sides:

- The pages detail exposes a typed registration seam — a module in
  `features/pages/detail/` exporting `registerPageExportEntry({ id, label,
Component })` (called once by whoever owns the dialog) and a hook
  `usePageExportEntry()` that returns the registered entry or `null`. The
  entry's `Component` renders the dialog; the pages detail renders it when
  the menu entry is chosen.
- The options menu renders the entry, at the position the registration
  declares (the pages side mounts it last, after the pages-owned entries,
  matching the oracle's `Export`-last pin for PAGE-060), only when a
  registration exists. With no registration the menu has no Export entry
  and no placeholder.
- The pages slice never imports the exports feature and never reimplements
  its dialog. The exports slice never imports pages internals; it imports
  the seam module only.
- The seam is ungated in every edition (PAGE-074's row is "ungated in the
  detail menu"); it is not a permission gate.
- Registration happens where the shell mounts features for the workspace
  (an app-level entry module), not inside the pages route, so a page
  detail opened directly also finds it.

Behaviour of the dialog is the exports spec's (formats, scope, paper size,
file names, reset, notices). Pages owns only the menu position and the
mount.

## 10. Reuse map — what `apps/web_new` already provides (survey, `3701e2fff`)

This is a survey of the **new** tree. For each slice: what exists, what it
must add, and what it must not duplicate.

### 10.1 Kits, shared modules, contracts

- **`@pidash/kit`** provides the primitives this area needs: `Button`,
  `IconButton`, `Input`, `Textarea`, `Combobox`, `Menu`, `ContextMenu`,
  `Dialog`, `Popover`, `Tooltip`, `Avatar`, `Badge`, `Kbd`,
  `Switch`, `VirtualList`, `EmptyState` (four tiers:
  illustrated/marketing/detailed/section, each with or without actions),
  `Skeleton`, `Spinner`, `Breadcrumbs`, `PreviewCard`, `UpgradePill`,
  `Markdown`, `Toast`. Do not add a page-specific duplicate of any of these;
  a new pattern belongs in the kit via a kit issue.
- **`shared/editor`** provides lazy `RichTextEditor` / `RichTextView`,
  `loadDescriptionExtensions` / `loadIssueDescriptionExtensions` /
  `loadCommentExtensions`, sanitisation and the mention node. It does **not**
  yet provide tables, task lists, code highlighting, colours, callouts,
  dividers, embeds, slash menu or drag handles; the pages slices extend the
  editor set (`loadPageExtensions`, new — a lazy loader, Tiptap and any
  Yjs/ProseMirror binding reached only through `import()` per
  `check-boundaries.mjs`).
- **`shared/shell`** provides `AppShell`, `Sidebar` +
  `SidebarFavoritesGroup`, `TitleBar`, `TopBar`, `CommandPalette` with the
  registry (`registerCommands`, `registerPalettePages`) and its entries,
  `PeekPanel`, `clipboard`, `fileUrls.resolveFileUrl`, `preferences` +
  `preferenceStorage`, `toaster`, `favorites` (the
  `web_new.favorites.open` flag + `setFavoritesSectionOpen`), the shortcuts
  registry, `document` (fullscreen portal), and the contextual palette
  selector `contextual/pages.ts` (`selectPageContext`, SHELL-091).
- **`shared/pickers`** provides `MemberPicker`, `ProjectPicker`,
  `ProjectMark` and friends. Reuse them for the creator filter and the
  cloud move picker.
- **`core/query`** provides `workspaceScope`, optimistic helpers and the
  query client; **`core/session`** provides `useWorkspace`,
  `usePermissions` and `selectPermissions` (Admin 20 / Member 15 / Guest 5)
  — the only place role numbers are read; **`core/edition`** provides the
  edition slots; **`core/platform`** the desktop bridge (pages must not use
  it, PAGE-081).
- **`packages/api-client/src/contracts/pages.ts`** already carries the
  thin shell slice: `PageUpdate` (id, name, access, color, is_locked,
  archived_at, project_ids), `updatePage`, `lockPage`, `unlockPage`,
  `setPageAccess`, `archivePage` (returns `{archived_at}`), `restorePage`,
  `addPageFavorite`, `removePageFavorite`, and the
  `PAGE_ACCESS_PUBLIC/PRIVATE` constants. `entity-search.ts` provides
  `searchMentionPeople`; `assistant-editor.ts` provides the editor-task
  request. The pages area's full CRUD, description, versions, duplicate and
  the nested fields are **not** present yet — NEWFRONT-833/834 add them.
- Every request and response shape lives in
  `packages/api-client/src/contracts/`; contract text is §11.

### 10.2 Per slice

| Slice                                                      | Exists today                                                                                                                                     | Must add                                                                                                                                                                                       |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 834 contracts (core REST)                                  | `pages.ts` thin shell slice                                                                                                                      | full page list/detail/create/patch/delete, access/lock/archive/restore/duplicate/description/versions contracts + tests                                                                        |
| 834 contracts (live + assistant)                           | `assistant-editor.ts` request                                                                                                                    | live health/convert/pdf-export request shapes and error mapping; page assistant task contract                                                                                                  |
| 835 list shell (001–006)                                   | route placeholder `routes/$ws/_gated/projects/$projectId/pages/index.tsx` (`TabAreaMount`), EmptyState tiers, `usePermissions`, the query client | list screen, tabs, search/sort/filter cluster + chips, row + menu primitives, feature gate                                                                                                     |
| 836 create + star + row actions (007–012)                  | kit Dialog/Menu, toaster, `addPageFavorite`/`removePageFavorite`, `favorites.ts`                                                                 | quick create, create dialog, star wiring (shared favourite helper), open-in-new-tab, copy link, duplicate                                                                                      |
| 837 visibility/lock/shelve/delete/move (PAGE-013–PAGE-017) | `setPageAccess`, `lockPage`/`unlockPage`, `archivePage`/`restorePage`, kit `Dialog`, `ProjectPicker` (cloud move)                                | row-menu actions, confirmation dialog, cloud move dialog                                                                                                                                       |
| 838 gating + list states + URLs (018–021)                  | `selectPermissions`, EmptyState tiers, route search params                                                                                       | gating matrix, state variants, URL/cache rules                                                                                                                                                 |
| 839 detail shell + deep links (022–025, 079–080)           | `Breadcrumbs`, `Spinner`, `Skeleton`, `PeekPanel` (not used here), `resolveFileUrl`                                                              | detail route (new file), record query, not-found panel, header cluster, sibling switcher, sync/pane/version query keys                                                                         |
| 840 title + mark + toolbar core (026–032)                  | `shared/editor` lazy loader, `Textarea`/`Input`, `Popover`                                                                                       | page title control, mark picker, sticky bar with typography/marks/alignment/lists/quote/code, key hints                                                                                        |
| 841 toolbar tables/images/colours (033–036)                | editor image extension (issue set), file upload plumbing (`shared` upload helpers)                                                               | table extension, toolbar image insert, letter/wash colour control + popover                                                                                                                    |
| 842 blocks + embeds + mentions (037–044)                   | mention node + `searchMentionPeople`, `RichTextView`, sanitizer                                                                                  | `loadPageExtensions` with callouts, dividers, task lists, code blocks, colors, embed node, slash menu, drag handle, markdown paste, char count, copy-markdown                                  |
| 843 side pane + outline/assets/info (045–050)              | `resolveFileUrl`, `useWorkspace`, `Avatar`; no shared tabs (only feature-local tab sets)                                                         | pane shell + tab pattern (new: no Tabs in kit — kit issue or pane-local), heading preview, outline, assets, stats, attribution (profile links per `parity/specs/profile.md` §3 and §13.3 here) |
| 844 version timeline + restore (051–054)                   | none                                                                                                                                             | versions contracts wiring, timeline, overlay, restore through the live editor, failure/retry                                                                                                   |
| 845 header actions + options menu (055–060)                | `clipboard`, toaster, favourite/access/lock/archive contracts                                                                                    | link copy, star, options menu, archive/restore/delete in detail, full-width/sticky toggles                                                                                                     |
| 846 sync/save + export entry (061–064, 074)                | toaster, `preferenceStorage`, the `check-boundaries` heavy list already names `yjs` and `jspdf`                                                  | sync badge, offline pill, fallback persistence, oversized strip, export seam                                                                                                                   |
| 847 realtime collaboration client (065–071)                | none; no Yjs dependency yet                                                                                                                      | Yjs + websocket provider in a lazy chunk, state machine, identity token, error frames, health/convert/pdf clients                                                                              |
| 848 prefs + keymap + negatives (072–073, 081–083)          | `preferenceStorage`, `commands` shortcuts registry                                                                                               | editor prefs, tooltip key map, negative-row unit proofs                                                                                                                                        |
| 849 assistant + cloud stub slots (075–078)                 | `assistant-editor.ts`, edition slots                                                                                                             | community absence + edition slot mount points, cloud-enable path                                                                                                                               |
| 850 nested (084–090)                                       | `parent` in the container model read; no tree UI                                                                                                 | tree, breadcrumbs, move, archive/delete semantics in the UI, child search, `nested.spec.ts`                                                                                                    |

Every row in the table above is about files the new tree owns; none of it
authorises reusing a class string, a component name or a copy line from the
old tree.

## 11. API surface and contracts the slices build against

All paths are under the workspace slug. `BASE` below is
`/api/workspaces/{slug}/projects/{projectId}`. Contract issues put these
shapes in `@pidash/api-client` with zod schemas and contract tests; slices
import the contracts, never call raw fetch.

### 11.1 Pages REST

| Method and path                                 | Body / query                                                                                                                                     | Response                                                                                             | Rows          |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------- | ------------- |
| `GET BASE/pages/`                               | — (the collection; the UI sends no filter params — filtering is client-side)                                                                     | array of page objects                                                                                | 001–021       |
| `POST BASE/pages/`                              | `name` (string), `access` (0/1), `parent` (id or null), `logo_props` (mark), optional `description_html`/`description_json`/`description_binary` | created page object                                                                                  | 007, 008, 084 |
| `GET BASE/pages/{pageId}/`                      | `track_visit=true` (default true)                                                                                                                | page object, plus `issue_ids` when tracking                                                          | 022           |
| `PATCH BASE/pages/{pageId}/`                    | any subset of `name`, `logo_props`, `access`, `parent`                                                                                           | updated page object; `400 "Page is locked"` while locked                                             | 026, 027, 087 |
| `DELETE BASE/pages/{pageId}/`                   | —                                                                                                                                                | 204; `400 "The page should be archived before deleting"` on a live page; 403 for non-owner non-admin | 016, 089      |
| `POST BASE/pages/{pageId}/access/`              | `{access: 0/1}` (0 open, 1 restricted)                                                                                                           | 204; 400 when the caller does not own the page                                                       | 013, 057      |
| `POST`/`DELETE BASE/pages/{pageId}/lock/`       | —                                                                                                                                                | 204                                                                                                  | 014, 058      |
| `POST BASE/pages/{pageId}/archive/`             | —                                                                                                                                                | 200 `{archived_at}`; archives the whole subtree; drops favourites                                    | 015, 059, 088 |
| `DELETE BASE/pages/{pageId}/archive/`           | —                                                                                                                                                | 204; a page whose parent is still archived is detached to the top level first                        | 015, 059, 088 |
| `POST BASE/pages/{pageId}/duplicate/`           | —                                                                                                                                                | the copy                                                                                             | 012, 060      |
| `POST BASE/pages/{pageId}/move/`                | `{new_project_id}`                                                                                                                               | cloud only; community route absent/no-op                                                             | 017           |
| `GET BASE/pages/{pageId}/description/`          | —                                                                                                                                                | `application/octet-stream` binary                                                                    | 064, 067      |
| `PATCH BASE/pages/{pageId}/description/`        | binary + `description_html` + `description_json`                                                                                                 | updated page; `400` with `PAGE_LOCKED`/`PAGE_ARCHIVED` codes on a frozen page                        | 063, 064      |
| `GET BASE/pages/{pageId}/versions/`             | —                                                                                                                                                | array of `{id, created_at, created_by/author, ...}`                                                  | 051           |
| `GET BASE/pages/{pageId}/versions/{versionId}/` | —                                                                                                                                                | one snapshot with its stored content forms                                                           | 052, 054      |
| `POST/DELETE BASE/favorite-pages/{pageId}/`     | —                                                                                                                                                | 204                                                                                                  | 009, 056      |

**No restore route exists.** There is no server-side "restore version"
endpoint in this build; restoring is a write into the live editor (§6.9,
PAGE-053). Do not add a contract for one. Likewise:

- The favourites and archived **collection** endpoints answer 404 and are
  never called (PAGE-020); do not contract them.
- `GET BASE/pages-summary/` exists and answers
  `{public_pages, private_pages, archived_pages}` but no row consumes it.
  It is recorded here as an unrowed endpoint; no slice calls it.
- `move/` is a cloud stub: in the community tree the route is not served
  and the UI has no affordance (PAGE-017 oracle pins exactly that).

Page object fields the UI reads: `id`, `name`, `owned_by`, `access`,
`logo_props`, `parent`, `is_favorite`, `is_locked`, `archived_at`,
`created_at`, `updated_at`, `created_by`, `updated_by`, `label_ids`,
`project_ids`, and (in nested builds) `parent_id`, `sub_pages_count`
(§11.7). `name` is free text up to 255; `logo_props` is the mark object
(`{in_use: "emoji" | "icon", emoji: {value}, icon: {name, color}}`) and is
optional/clearable. `color` is a separate free-text column the pages UI does
not read for the mark; do not use it as the mark. (Mark clearing is PAGE-027's
old bug NEWFRONT-355; see §15.)

### 11.2 Permissions and refusals to preserve

- Lock/access/archive: owner or admin; where the server is looser (lock
  answers 204 for a member), the client still hides the control (PAGE-018).
- Delete: archived only; owner or admin.
- Access change by a non-owner: 400 (the UI never offers it).
- A guest's collection read: 200, own pages only unless the project's
  guest-all flag is on.
- A restricted page read by a non-owner: refused (400 in the pinned
  matrix); the UI shows the not-found panel, not a permission error.

### 11.3 Request/response validation the UI relies on

- Over-255 titles are refused client-side (§5.1); the server also trims and
  rejects blanks on create.
- A duplicate keeps body, mark and visibility; it is a standalone page with
  no parent.
- Archive returns the stamp used for the header marker; restore answers
  204 with no body.
- The record patch and the description patch are separate; a title save
  never triggers a description write and vice versa.

### 11.4 Files and assets

Uploads go through the platform file pipeline (upload returns an asset
record; the description patch/insert references its id). The assets pane
resolves display and download URLs from the workspace asset builders keyed
by asset id, project id and workspace slug via `resolveFileUrl` +
`fileUrls` helpers. Image thumbnails use the same resolved URL. Duplication
clones existing assets rather than sharing foreign ids (PAGE-039).

### 11.5 Entity search (mentions, links, embeds)

`GET /api/workspaces/{slug}/entity-search/?query=<term>&query_type=user_mention&project_id=<id>&count=<n>`
returns one array per entity kind; people arrive as
`{member__id, member__display_name, member__avatar_url}`. The contract
(`searchMentionPeople`) keeps the people section and ignores the rest.
Entity-link resolution uses the same endpoint with the relevant
`query_type`; the pages slice does not add kinds the shell resolver does
not already understand.

### 11.6 Live server

- Socket: `ws(s)://<live>/live/collaboration` with query
  `documentType=project_page`, `projectId=<id>`, `workspaceSlug=<slug>`;
  auth token payload `{id, name, color}`. Yjs update/awareness frames per
  the unchanged server protocol.
- `GET /live/health/` → `{status, timestamp, version}`.
- `POST /live/convert-document/` → `{description_html, variant,
description_binary?}` ⇒ `{description_json, description_html,
description_binary}`; 400 on malformed input.
- `POST /live/pdf-export/` → `{pageId, workspaceSlug, projectId, title?,
noAssets?}` ⇒ PDF stream; 401 unauthenticated, 400 invalid, 404/502 for
  a missing page.

### 11.7 Nested pages (D17) — what contracts and NEWFRONT-850 build against

The web app's page endpoints today hide children (`parent__isnull=True` in
the session view set), and the serializers carry `parent` but **no**
`parent_id` or `sub_pages_count`. NEWFRONT-104 is the backend design and
PDASHOSS01-270 is the backend issue; neither is a blocker for NEWFRONT-850
(relates_to only). The contracts and the nested slice build against this
target:

- **List:** the default (no query params) stays top-level only, so the
  existing list and the old app are unchanged. `GET BASE/pages/?parent=<pageId>`
  returns that page's children with the same visibility, ordering and
  favourite-first rule as the collection. When a search or filter parameter
  is present, results include child pages (so PAGE-090 search finds them).
- **Fields:** every page object in list and detail carries `parent`
  (container id or null) and `parent_id` (same value, flat, for clients
  that do not expand; `parent` is the serializer's nested field and
  `parent_id` is the flat one, both emitted), plus `sub_pages_count`
  (visible, non-archived children the caller can see). Counts never
  include children the caller cannot see.
- **Archived subtrees:** because `sub_pages_count` counts live children, a
  cascaded archive leaves an archived parent reporting `0` even though its
  shelved subtree still exists. The Archived tab must still reach that
  subtree: an archived row keeps its expand control, `GET pages/?parent=<id>`
  supplies the shelved children, and the count and delete confirmation use
  the children actually loaded. A live row never gets an expand control
  from this rule, and a page the caller cannot see is still absent from the
  child read.
  The same blindness hides an archived child under a *live* parent, which
  also reports `0`. Discovery therefore probes every row with
  `GET pages/?parent=<id>` rather than trusting the count: the Archived
  tab, search, structured filters, favourites, the command palette and the
  detail header's ancestor trail all walk the whole forest that way, seeded
  from the one cached top-level collection read, so PAGE-020's single
  top-level read still holds. Child reads are not collection reads. The
  trail and the discovery surfaces render once their walk settles, so a
  pending read never flashes the restricted fallback or a partial list.
- **Detail:** a child page retrieves by id under the same visibility rule;
  the client no longer gets a "not found" for a page it can see.
- **Create/update:** `parent` (id or null) is accepted on `POST` and
  `PATCH`; validation: the parent must be visible to the caller, in this
  project, not archived, and — on update — not the page itself or one of
  its descendants (`400`). Archive cascades to descendants; unarchive under
  an archived parent detaches to the top level (and the UI says so);
  delete re-homes children to the top level (and the confirmation says how
  many).
- **Detecting that the backend has landed** (a run convention, so a slice
  never guesses): probe with a seeded project — create a page with a
  child through the API, then `GET BASE/pages/?parent=<parent>`; the
  backend is landed when (a) the child list contains the child, (b) the
  child detail answers 200 for its owner, (c) the parent object carries
  `parent_id: null` and `sub_pages_count: 1`, and (d) a `PATCH` moving a
  page under its own descendant answers 400. Record the exact probe
  responses in the slice's workpad and PR. If the probe fails, land the UI
  and zod schemas against the sketch above, record the live-API probe, and
  park only the live contract-test remainder on PDASHOSS01-270 via
  `waiting_on_external` — never invent nested behaviour the API does not
  serve.
- **Cross-project move (PAGE-017) is a different feature** from nested move
  (PAGE-087): PAGE-017 relocates a page to another project in the cloud
  build; PAGE-087 reparents within a project. They never share a dialog.

## 12. Row index and web_new-only scenario convention (PAGE-084–090)

### 12.1 Row → section → oracle scenario

| Rows                                  | Section          | Oracle file                                                                               |
| ------------------------------------- | ---------------- | ----------------------------------------------------------------------------------------- |
| PAGE-001–PAGE-006                     | §3.1–§3.5        | `pages/list-discovery.spec.ts`                                                            |
| PAGE-007–PAGE-012                     | §3.6, §3.8       | `pages/create-row-actions.spec.ts`                                                        |
| PAGE-013–PAGE-017                     | §3.8, §7.3       | `pages/create-row-actions.spec.ts`                                                        |
| PAGE-018–PAGE-021                     | §2.1, §3.1–§3.10 | `pages/gating-states-detail-shell.spec.ts`                                                |
| PAGE-022–PAGE-025                     | §4               | `pages/gating-states-detail-shell.spec.ts`                                                |
| PAGE-026–PAGE-036                     | §5               | `pages/title-toolbar.spec.ts`                                                             |
| PAGE-037–PAGE-044                     | §6.1–§6.4        | `pages/blocks-embeds.spec.ts`                                                             |
| PAGE-045–PAGE-050                     | §6.5–§6.9        | `pages/pane-versions.spec.ts`                                                             |
| PAGE-051–PAGE-054                     | §6.9             | `pages/pane-versions.spec.ts`                                                             |
| PAGE-055–PAGE-060                     | §7               | `pages/header-sync-export.spec.ts`                                                        |
| PAGE-061–PAGE-064                     | §8               | `pages/header-sync-export.spec.ts`                                                        |
| PAGE-065–PAGE-071                     | §8               | `pages/collab-live.spec.ts`                                                               |
| PAGE-072, PAGE-073, PAGE-081–PAGE-083 | §9               | `pages/prefs-cloud-misc.spec.ts`                                                          |
| PAGE-074                              | §9.6             | `pages/header-sync-export.spec.ts` (old) and `exports/page-export.spec.ts` (NEWFRONT-770) |
| PAGE-075–PAGE-078                     | §9.3–§9.4        | `pages/prefs-cloud-misc.spec.ts`                                                          |
| PAGE-079, PAGE-080                    | §4.5             | `pages/gating-states-detail-shell.spec.ts`                                                |
| PAGE-084–PAGE-090                     | §12.2–§12.3      | **none — web_new only**                                                                   |

Every one of PAGE-001–083 has at least one `specTitle([...])` scenario in
the nine files (checked against `web-new-dev` at `3701e2fff`). Two things
the gate must double-check rather than assume:

- **PAGE-040** has only the `bug:` scenario ("embed identifiers persist in
  snapshots while the community renders no upgrade card"), which asserts
  the old absence. The intended behaviour (upgrade prompt on community,
  resolved card when licensed) has no passing scenario of its own; the gate
  should treat PAGE-040 as target/no-oracle for the prompt and prove it on
  `web_new`, or re-pin if a non-bug scenario exists by then.
- **PAGE-065** is tagged in `collab-live.spec.ts` as
  `specTitle(["PAGE-065"], "concurrent keystrokes converge ...")` and the
  row status is `oracle green`; the gate should confirm the tag while
  collecting results, since the row's inventory summary mentions a scenario
  title that matches it only by the row tag.

### 12.2 Mapping pinned for PAGE-084–090 (NEWFRONT-850)

In order, the rows are exactly:

- **PAGE-084 — sub-page create.** An **Add sub-page** action in the page
  header and in the overflow menu (member and above; no guest) creates a
  child through the same quick-create path as PAGE-007 with `parent` set to
  the source page, no other inherited field, and opens the child. The
  parent's child count increases.
- **PAGE-085 — tree with counts.** The list shows pages that have children
  with an expand control and a visible child count; expanding loads and
  indents that page's children through `GET pages/?parent=<id>`,
  recursively. Expand state survives in-session navigation (a route change
  and back; not a full reload). Children use the list's active sort and
  keep favourite-first. A page the caller cannot see never appears, and
  counts exclude it.
- **PAGE-086 — breadcrumbs.** A child opens by URL or from the tree and
  its header shows breadcrumbs of every ancestor from the top, each
  clickable. An ancestor the caller cannot see renders as the literal
  fallback `Restricted page` (pinned) with no link. An archived middle
  ancestor under a live parent resolves through the same deep walk
  (§11.7); the trail waits for the walk so it never shows the fallback
  for an ancestor that is merely not loaded yet.
- **PAGE-087 — move picker and drag.** **Move to…** in the overflow menu
  (owner or admin) and drag-and-drop in the tree put a page under another
  page or back to the top level, carrying its whole subtree. Refused with a
  clear message: under itself, under a descendant, under an archived page,
  or under a page the caller cannot see. The picker search excludes the
  page and its descendants.
- **PAGE-088 — archive cascade and restore-to-top.** Archiving a page
  archives its descendants, and the whole subtree appears in the Archived
  tab; since an archived parent's `sub_pages_count` is 0, its subtree is
  reached by expanding the archived row, and its visible count reflects the
  shelved children once loaded. Restoring a page whose parent is still
  archived detaches it to the top level and says so in a notice, including
  when the parent exists only as a loaded child in the archived tree.
- **PAGE-089 — delete re-homes children.** Deleting a page moves its
  children to the top level with their content intact; the delete
  confirmation states how many sub-pages will move. (The server already
  does this; the UI adds the count.)
- **PAGE-090 — search across children.** Search, filters, favourites and
  the command palette find child pages as well as top-level ones; results
  show the parent path (the breadcrumb trail as a secondary line). This
  needs the backend's search-including-children behaviour (§11.7). The
  Archived tab is part of the same list, so discovery also reaches a
  shelved child: the forest walk treats an archived row as potentially
  expandable (its `sub_pages_count` is 0 even with a subtree), and the
  palette reads the same forest without dropping archived rows.

### 12.3 web_new-only scenario convention (rows 084–090)

There is no oracle for these rows (the old app cannot show nested pages;
D17 / PDASHOSS01-270). They follow the Parity page's "New features" rule
and use the existing harness, with these binding details:

1. **Spec file.** One new file,
   `apps/web_new/e2e/parity/pages/nested.spec.ts`, imported from
   `../fixtures` and `../helpers/tags` like its siblings. Do not add a
   second nested file and do not put these scenarios in an oracle file.
2. **Titles and tags.** Every test uses `specTitle(["PAGE-08x"], "…")` and
   `{ tag: specTags(["PAGE-08x"]) }`; a test proving two rows names both
   IDs. One test per behaviour and row at minimum; a row with several
   distinct acceptance clauses (085 expand + counts + visibility; 087
   picker + drag + refusals) may use several tests, all tagged with its
   row.
3. **Skip guard.** Each test's callback receives `testInfo` as its second
   argument and starts with
   `test.skip(testInfo.project.name !== "new", "PAGE-08x is new (D17); no oracle")`.
   The skip is evaluated inside the callback, not as a static
   `test.skip(condition)` at module scope, so the `new` project runs the
   test and the `oracle` project records it as skipped for the stated
   reason. Never remove the guard to make an oracle leg "pass"; the gate
   reads the skip reason.
4. **Driver placement.** Extend the one interface: add the nested methods
   to `drivers/parity-driver.ts` next to the existing `pages*` groups, with
   a comment naming PAGE-084–090. Implement them fully in
   `drivers/web-new.ts` (real selectors and server reads through
   `helpers/api`). Implement them in `drivers/web.ts` as explicit
   "not applicable on the oracle (D17)" throws — the throw is only
   reachable if the skip guard is bypassed, which is itself a defect. The
   web driver never carries a fake success for an un-runnable row. What the
   web driver carries for un-runnable rows is therefore exactly: a method
   that throws with the D17 reason. Any server setup/cleanup helpers go in
   `helpers/api.ts` and are used by the `new` leg only.
5. **What the tests may drive.** Only the `driver`, the `seed` facts and
   the API helpers (server-state assertions), never page internals. Each
   test creates its own scratch project/pages through the helpers and
   cleans up after itself, so order and reruns do not matter.
6. **How the gate runs them.** The pages area gate runs, against
   `apps/web_new` only:
   `pnpm test:parity:new pages/nested.spec.ts` for the new-only rows, and
   the full pages area (`pnpm test:parity:new pages/`) for the oracle-backed
   rows. It never runs the oracle project for `nested.spec.ts`. A row turns
   `new green (target, no oracle)` when all its `new` tests pass; a skipped
   or missing test is not green. The gate adds the pages prefixes to the
   migrated-route list (F-09) when every row is green, and files fix
   sub-issues for failures.
7. **Inventory status.** The nested slice appends `built` to PAGE-084–090
   (e.g. `new (D17), built`) as it lands; no slice in this area moves any
   status to `new green` — only the gate does.

## 13. Cross-epic seams (binding on both sides)

Each seam names what the pages area **builds**, what it **registers**, and
what it **reuses**. The owning story is the other area's issue; this spec
pins the contract so both sides can land independently.

### 13.1 PAGE-009 — first favourite expands the favourites nav (shell)

- **Pages builds/registers:** the list star and the detail star call the
  shared write helper (`POST/DELETE .../favorite-pages/{pageId}/`) and, on
  the first successful favourite _for this user in this session_, call
  `setFavoritesSectionOpen(true)` from `shared/shell/favorites.ts`
  (key `web_new.favorites.open`, value `"1"`).
- **Reuses:** the shell's `SidebarFavoritesGroup`, which reads
  `isFavoritesSectionOpen()` for its initial open state and refreshes the
  favourites tree through `sidebarKeys.favorites` / user-favorites
  contracts. Pages never renders the sidebar group.
- **Proof:** `create-row-actions.spec.ts` pins the row star path; the
  detail star path is pinned in `header-sync-export.spec.ts`.

### 13.2 PAGE-043 — entity peek overlays (peek ownership)

- **Pages builds:** the embed/link node emits a normalised entity reference
  (`entity_name`, `entity_identifier`, project/workspace ids, display
  label) and renders the inline preview affordance.
- **Registers:** the click handler calls the shell's peek entry point with
  that reference; it does not import issue detail, cycle detail or any
  other area's data.
- **Reuses:** `shared/shell/PeekPanel` and the issues area's peek data and
  commit path. If the entity kind has no registered peek, the preview
  renders as a plain link (no dead affordance).

### 13.3 PAGE-050 — profile links (deactivated-user fallback)

- **Pages builds:** the info-tab attribution rows with two links.
- **Registers:** links resolve to the profile area's route
  (`/{ws}/profile/{userId}/` per `parity/specs/profile.md`; the legacy
  `/profile/<id>` addresses forward to the settings tabs, not to a member
  profile — do not link there). When the profile route is not yet mounted,
  the name renders as plain text and the link is omitted rather than
  pointing at a 404. The fallback label for a departed member is
  `Deactivated user` (pinned).
- **Reuses:** the member directory read (`useWorkspace`/members queries)
  for names and avatars. A missing or deactivated member renders the
  deactivated-user label (§6.9), never a broken avatar or link.

### 13.4 PAGE-040 — work-item embeds (community prompt vs licensed resolution)

- **Pages builds:** the embed node and its serialised attributes; the
  community renderer (an `UpgradePill` plus an outbound link to the upgrade
  surface) and the slot call for a licensed resolver.
- **Registers:** the embed renderer consults the edition slot (e.g.
  `page.embed.workItem`); community leaves it absent and renders the
  prompt; a licensed edition fills it with a component that resolves the
  referenced item and opens its peek.
- **Reuses:** kit `UpgradePill`, the shell's external-link handling, and
  the peek path from §13.2. The page never fetches the work item itself on
  community.

### 13.5 PAGE-074 — export dialog (NEWFRONT-770 seam)

Defined in §9.6: pages exposes `registerPageExportEntry` /
`usePageExportEntry`; exports registers the dialog; the options menu
renders it when present. Pages builds the mount point, exports builds the
dialog. Neither imports the other's internals.

### 13.6 Pages in the command palette (shell registry)

- **Pages registers:** page commands through `registerCommands` — at
  minimum the existing contextual group (`selectPageContext`, SHELL-091)
  mounted on the detail route with the real handlers: lock/unlock,
  make public/private, archive/restore, favourite, copy URL. With nested
  pages, a page's contextual entries also carry the page's parent path in
  the chip/subject.
- **Reuses:** the shell registry, the palette chrome and the global
  navigation commands. Pages does not add a second palette or a page-owned
  key binding; every shortcut comes from the registry.
- **Search:** PAGE-090's palette search over pages uses the shell's palette
  search against the pages collection/search; the pages area supplies the
  rows (with parent path), the shell supplies the ranking and activation.

### 13.7 NEWFRONT-526 deferred SHELL legs (needs page routes)

NEWFRONT-526 holds six `web_new` skips in
`shell/palette-search/palette-entity-contextual.spec.ts` (SHELL-091:
cycle, module, page) and
`shell/palette-search/shared-empty-states.spec.ts` (SHELL-104: cycles
list/detail/modal); only the SHELL-091 page leg needs this area's routes
and contextual entries.

- **Pages builds:** the list route (835) and detail route (839) and the
  contextual `selectPageContext` mount with real handlers (SHELL-091).
  (SHELL-104's legs drive cycles routes — the cycles area's, not this
  area's; pages' own `EmptyState` usage on the list is PAGE-019's.)
- **NEWFRONT-526 does:** removes its page-related `driver.target ===
"web_new"` skips once these routes land and runs those legs green; the
  cycle/module halves are that issue's own.
- **Order:** this area does not wait for NEWFRONT-526; 526 runs after the
  routes exist. The pages gate proves the pages rows; 526 proves the shell
  rows.

### 13.8 NEWFRONT-850 bridge

- **Pages (837/839) builds:** the base row actions and breadcrumb cluster.
- **850 extends:** the subtree-aware actions (move, archive cascade
  guidance, delete count) and ancestor breadcrumbs, plus `nested.spec.ts`.
- **Reuses:** the shared favourite, access, lock, archive, delete and move
  contracts; 850 adds only the nested deltas to `@pidash/api-client` if the
  contracts run did not.

## 14. Gate adjudication

At the pages area gate:

- Run every pages scenario on `apps/web_new` for the editions the rows are
  tagged with: OSS for the whole area, desktop for PAGE-081, cloud for
  PAGE-017, 075–078 (and the licensed halves of 040 and 043). Bring the
  parity stack up with `apps/web_new/e2e/parity/stack/parity-up.sh`; tear
  down happens automatically at run end.
- An oracle-backed row turns `new green` when its scenario(s) pass on
  `web_new`. A row with only a `bug:` scenario (PAGE-040, PAGE-063,
  PAGE-069's bug steps are additional to passing ones) is adjudicated per
  §15's bug markers: the intended behaviour is proven on `web_new`, and
  the old assertion is target/no-oracle — do not make the old bug pass.
- PAGE-084–090 turn `new green (target, no oracle)` when their `new`
  tests in `pages/nested.spec.ts` pass (§12.3).
- PAGE-070's cross-instance half and PAGE-068's secret-header half are
  environment gaps: record them, prove the provable half, and do not file
  a "fix" for the environment.
- A cloud leg that cannot run is recorded `blocked: <reason>`, files one
  fix sub-issue under the epic, and does not hold the rest of the area.
- On 100% green: add the pages path prefixes to the migrated-route list
  (F-09), append `new green` (or `new green (target, no oracle)`) to each
  row, and move the epic on per its own rule. On a real failure, file fix
  sub-issues and wait.

## 15. Bug markers (observed → intended)

Each old bug the oracle pins stays pinned as a `bug:` scenario; the new
app builds the intended behaviour and the gate proves that.

- **NEWFRONT-362 (PAGE-006)** — a private row's visibility glyph never
  opens its tooltip because the icon does not forward the trigger ref.
  Intended: hover reveals `Private` on private rows and `Public` on open
  ones.
- **NEWFRONT-354 (PAGE-026)** — titles over the 255 cap persist and no
  count renders. Intended: cap enforced with a focused-only live count.
- **NEWFRONT-355 (PAGE-027)** — clearing the mark is refused (API 400).
  Intended: clearing persists and the fallback glyph returns.
- **NEWFRONT-351 (PAGE-039)** — pasting an image from another page 500s
  instead of cloning the asset. Intended: the paste clones the asset onto
  this page.
- **NEWFRONT-352 (PAGE-040)** — community renders no embed at all.
  Intended: upgrade prompt with outbound link; licensed resolves.
- **NEWFRONT-338 (PAGE-063, PAGE-069)** — the client never subscribes to
  socket error broadcasts, so load/persist failures and the oversized strip
  are silent. Intended: structured error frames surface as notices/strip.
- **NEWFRONT-359 (PAGE-064)** — a 500 on the fallback description patch
  wedges the client. Intended: error notice plus continued retry.
- **NEWFRONT-360 (PAGE-064, PAGE-074)** — an editor-reference race exports/
  copies empty content on some loads. Intended: read the loaded document
  and wait for sync before export/copy.
- **NEWFRONT-368 (PAGE-056)** — a star toggled within ~1s of sync-ready is
  stomped stale by the post-sync revalidation. Intended: optimistic flip
  survives revalidation (write-through/invalidate ordering).
- **NEWFRONT-369 (PAGE-057, PAGE-059)** — a slow load-time record read
  overwrites a mutation, so the menu/editor shows stale state until reload.
  Intended: mutations win over an in-flight stale read.
- **NEWFRONT-356 (PAGE-074)** — the markdown "text only" scope keeps
  markdown image syntax. Intended: the scope drops images. This is the
  exports area's bug and fix; pages only mounts the seam.
- **NEWFRONT-470/471 (PAGE-074)** — relative-URL pictures dropped and
  pickers outside the tab order. Exports area's, same note.

Also recorded: PAGE-016's revisited-deleted-page behaviour (the page keeps
its address and shows the not-found panel; there is no pushed delete
signal), PAGE-061's stale wording about a 10-second save (the fallback runs
every 30s), PAGE-058's lock freezing the owner too, and PAGE-074's
client-side-only export (no export endpoint serves this path).

## 16. Open and deliberately-unrowed items

- **Assistant error path:** `rephrase-grammar/` exists in the old service
  but no row consumes it. The assistant slice (849) lands only the
  conversational task; a new row would be needed before building anything
  else. Do not invent it.
- **`pages-summary/`** exists and is unused (§11.1). No row, no call.
- **Cloud move picker** (PAGE-017) is a recorded confirmation, not proven
  on the OSS stack; the community pin is the absence.
- **Cross-instance fan-out** (PAGE-070) and **secret-header admin gate**
  (PAGE-068) are environment gaps, recorded, not invented.
- **Licensed embed resolution** (PAGE-040) and **assistant reply paths**
  (PAGE-075) need the licensed build; the community half is pinned.

## 17. Slice checklist snapshot (what each run owes)

Each slice reads its section above, cites this file in its PR, and:

1. Builds the slice's rows without importing anything from the old tree
   (`check:boundaries`, import-ban lint and `check-similarity` must pass).
2. Covers every line and branch it changes with a unit test (vitest),
   including the gated/absent branches and failure/rollback paths.
3. Runs, from the repo root: `pnpm install --frozen-lockfile`, then the
   changed packages' `typecheck`, `lint`, `test`; for `web_new`, also
   `check:boundaries`, `check:headers`, `check:deps`, `check:coexistence`,
   `check:similarity` and `size`; plus the paths gate.
4. Appends `built` to its rows' Status cells and does **not** move rows to
   `new green` and does **not** bring up the parity stack.
5. Records the head SHA, each command with its exit code, and the changed
   line/branch coverage in the workpad.

## 18. Pinned labels (copy slices must match)

The oracle reads these strings literally. Everything else is described by
meaning. A slice that needs new copy writes it fresh; it must not copy a
label from this table that it does not assert.

| Group                            | Pinned text                                                                                                                                                                                                                                                         | Rows                         |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------- |
| List tabs                        | `Public`, `Private`, `Archived`                                                                                                                                                                                                                                     | PAGE-002                     |
| Feature gate                     | `Manage features`                                                                                                                                                                                                                                                   | PAGE-001                     |
| Empty collection                 | `Create your first Page`                                                                                                                                                                                                                                            | PAGE-019                     |
| No results                       | `No matching results.`                                                                                                                                                                                                                                              | PAGE-003, PAGE-005, PAGE-019 |
| Sort menu                        | `Name`, `Date created`, `Date modified`, `Ascending`, `Descending`                                                                                                                                                                                                  | PAGE-004                     |
| Visibility tooltip               | `Private`, `Public`                                                                                                                                                                                                                                                 | PAGE-006                     |
| Row menu                         | `Open in new tab`, `Copy link`, `Make a copy`, `Lock`, `Unlock`, `Make private`, `Make public`, `Archive`, `Restore`                                                                                                                                                | PAGE-010–PAGE-016            |
| Row toasts                       | `Page link copied to clipboard.`, `Page duplicated successfully.`, `Page locked successfully.`, `Page unlocked successfully.`, `Page could not be locked.`, `Page archived successfully.`, `Page restored successfully.`; `marked private` / `marked public` suffix | PAGE-010–PAGE-015, PAGE-057  |
| Favourite toasts                 | `Page added to favorites.`, `Page removed from favorites.`, `Page could not be added to favorites.`                                                                                                                                                                 | PAGE-009, PAGE-056           |
| Undefined title                  | `Untitled`                                                                                                                                                                                                                                                          | PAGE-006, PAGE-026, PAGE-079 |
| Not found                        | `Page not found`, `View other Pages`                                                                                                                                                                                                                                | PAGE-016, PAGE-023           |
| Detail menu                      | `Full width`, `Sticky toolbar`, `Copy markdown`, `Version history`, `Make a copy`, `Archive`, `Restore`, `Make private`, `Make public`, `Export`                                                                                                                    | PAGE-060                     |
| Detail toasts                    | `Markdown copied to clipboard.`, `Page exported successfully.`                                                                                                                                                                                                      | PAGE-044, PAGE-074           |
| Pane tabs                        | `Outline`, `Info`, `Assets`                                                                                                                                                                                                                                         | PAGE-045                     |
| Version timeline                 | `Current version` (the id-less first row; its link never names a snapshot)                                                                                                                                                                                           | PAGE-051                     |
| Archive marker                   | exactly `Archived at Mon D, YYYY`                                                                                                                                                                                                                                   | PAGE-059                     |
| Attribution fallback             | `Deactivated user`                                                                                                                                                                                                                                                  | PAGE-050                     |
| Sync badge                       | `Connection lost`, `Syncing...`, both read from the badge as it appears; the badge disappears once synced                                                                                                                                                           | PAGE-061, PAGE-067           |
| Offline pill                     | `Offline`                                                                                                                                                                                                                                                           | PAGE-062                     |
| Mark picker tabs                 | `Emoji`, `Icon`                                                                                                                                                                                                                                                     | PAGE-027                     |
| Toolbar icon count               | 14 icon-only controls in the sticky bar (the oracle counts them; the mark/alignment/list/quote/code/table/image/colour entries above account for the set)                                                                                                           | PAGE-028–PAGE-036            |
| Nested (spec-defined, no oracle) | `Add sub-page`, `Move to…`, `Restricted page`                                                                                                                                                                                                                       | PAGE-084, PAGE-086, PAGE-087 |
