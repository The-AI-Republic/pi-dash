# Feature inventory — Stickies (Phase 3)

- Area: Stickies — a personal, workspace-scoped board of rich-text quick notes, reached
  from the workspace sidebar at the `stickies` route: a masonry grid of coloured note cards
  the signed-in user can create, edit inline, recolour, reorder by drag, search and delete.
- ID prefix: `STK-`
- Editions: oss, cloud, desktop
- Status of this file: draft for human sign-off (H-signoff-3 via NEWFRONT-10)
- Method: read of the old sources listed in the coverage checklist — the `stickies` route
  (`layout.tsx`, `header.tsx`, `page.tsx`), the whole `core/components/stickies` tree, the
  sticky store (`core/store/sticky/sticky.store.ts`), the sticky service
  (`core/services/sticky.service.ts`), the `sticky-editor` used by the note body
  (`core/components/editor/sticky-editor/**` and its toolbar constants), the sidebar entry that
  links to the route, and the backend that serves it (`apps/api/.../app/views/workspace/sticky.py`,
  `db/models/sticky.py`, the sticky serializer and URL registration) to pin down server-side
  scoping, permissions and search. The old app was **not** run against a seeded stack in this
  pass (no local backend here); every row must still pass the oracle run (NEWFRONT-58) against the
  live old app before implementation starts, and the running-app pass may add rows here as comments.
  Descriptions are paraphrased; no old strings, code, class names or styles are reused.
- Ownership model (applies to every row unless noted): each sticky belongs to one owner
  (the user who created it) and one workspace; the list, reads, updates and deletes are all
  scoped server-side to `owner = request.user`, so a user only ever sees and edits their own
  stickies and never anyone else's. "Your stickies" is literal.
- Reachability note for the human reviewer: several files in `core/components/stickies` have **no
  reachable entry point** in the surveyed build — the floating action bar, the "all stickies" modal,
  the home widget, the truncated list and the drag-handle. They are recorded as dead code in the
  coverage checklist (with the grep evidence) rather than as rows, because they produce no
  observable behaviour in the current app. If the sign-off knows any of these is actually reachable
  (e.g. via a build/overlay not present here), it should come back as a comment and become rows.
- Negative rows (STK-021…STK-024) record load-bearing **absences** the new app must preserve
  (no real-time, no edition divergence, no per-sticky deep link, no export/import).

| ID | Capability | Who | Edition | Old entry point | API | Acceptance | Parity test | Status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| STK-001 | Open the Stickies screen from the workspace sidebar; it lists the signed-in user's own stickies for the current workspace | any workspace role (admin, member, guest) | all | Sidebar "Stickies" nav item (multiple-sticky icon) linking to the `stickies` route; route layout renders the app header + content shell | `GET /api/workspaces/{ws}/stickies/` (owner-scoped) | Navigating lands on the stickies route; the browser tab title reads as the user's stickies; only the current user's stickies for this workspace appear, newest/highest sort first; another user's stickies never appear | | not started |
| STK-002 | Sidebar entry is a personal, pinnable navigation item (can be shown/hidden and ordered among personal items) | any workspace role | all | Sidebar personal-items section | None (navigation preference; management owned by the workspace-shell area) | The Stickies entry appears in the sidebar's personal group and can be pinned/unpinned there; when present, activating it opens the stickies route. Pin management itself is verified in the shell inventory, not here | | not started |
| STK-003 | Grid presentation: stickies render as a responsive masonry of note cards, each showing its background colour and rich-text body | any workspace role | all | Stickies route body (infinite list → masonry layout) | Same as STK-001 | Cards lay out in columns that adapt to container width (roughly two on narrow through six on very wide); each card shows its coloured background and rendered note content; column count changes as the window resizes | | not started |
| STK-004 | Cards are ordered by an explicit sort order, most-recent/highest first, consistent with drag reordering (STK-014) | any workspace role | all | Stickies route body | `GET` list ordered by descending sort order server-side | The list order matches descending sort order; a newly created sticky appears first; reordering (STK-014) is reflected in the order on reload | | not started |
| STK-005 | Create a new sticky from the header "Add sticky" button: a blank note with a randomly chosen background colour is added at the top and becomes the active, editable card | any workspace role (admin, member, guest) | all | "Add sticky" button in the stickies route header | `POST /api/workspaces/{ws}/stickies/` | Activating Add inserts a new empty card at the top with a random background colour, focuses it for typing, shows a success notice, and the button shows a busy state while the create is in flight | | not started |
| STK-006 | Duplicate-empty guard: if the most recent sticky is still empty, "Add sticky" refuses to create another and warns instead | any workspace role | all | "Add sticky" button (header) | None (client guard before `POST`) | With an empty most-recent sticky present, pressing Add creates nothing and shows a warning that an empty sticky already exists; once that sticky has content, Add works again | | not started |
| STK-007 | Edit a sticky's body inline in a rich-text editor; edits autosave shortly after typing stops (no explicit save button) | owner of the sticky | all | The note card's inline editor | `PATCH /api/workspaces/{ws}/stickies/{id}/` (debounced) | Typing in a card updates it and persists a moment after typing pauses; reload shows the saved content; an empty body is stored as an empty paragraph rather than blank | | not started |
| STK-008 | Body formatting toolbar limited to bold, italic and a to-do (checkbox) list, with tooltips and keyboard shortcuts for the marks | owner of the sticky | all | Toolbar under the focused note editor | Same as STK-007 (persists via `PATCH`/`POST`) | The toolbar exposes exactly bold, italic and to-do list; toggling them changes the body and persists; the mark buttons carry their keyboard shortcuts and reflect active state at the caret | | not started |
| STK-009 | Enter behaviour in the note editor inserts a line break rather than submitting; notes are multi-line | owner of the sticky | all | Note editor key handling | Same as STK-007 | Pressing Enter adds a new line inside the note; it never "submits"/closes the card; content is retained and autosaved | | not started |
| STK-010 | Toolbar visibility follows focus: the formatting/colour/delete toolbar is shown for the focused card and collapses otherwise | owner of the sticky | all | Note card | None (presentation) | The toolbar is visible while the card is focused/active and hidden/collapsed when it is not, without losing content | | not started |
| STK-011 | Change a sticky's background colour from an eight-swatch palette opened via the toolbar's colour button; the change persists immediately | owner of the sticky | all | Colour button in the note toolbar → colour palette popover | `PATCH .../stickies/{id}/` with the chosen colour key | Opening the palette shows eight named background colours; picking one recolours the card at once and persists; clicking outside closes the palette | | not started |
| STK-012 | Delete a sticky from the toolbar trash action, guarded by a confirmation dialog | owner of the sticky | all | Trash button in the note toolbar → delete confirmation dialog | `DELETE /api/workspaces/{ws}/stickies/{id}/` | The trash action opens a confirm dialog; confirming removes the card and shows a success notice; cancelling keeps it; a failed delete shows an error notice and the card returns | | not started |
| STK-013 | Delete permission: only the sticky's owner (creator) may update or delete it; the server rejects others | owner only (server enforces creator) | all | Same edit/colour/delete controls | `PATCH`/`DELETE` guarded by creator permission | An update or delete attempted by a non-owner is refused server-side and the sticky is unchanged; because the list is owner-scoped, a user reaches only their own stickies in the UI | | not started |
| STK-014 | Reorder stickies by dragging one card onto another (drop above/below); order is saved via a fractional sort value | owner of the sticky | all | Drag a card within the stickies grid | `PATCH .../stickies/{id}/` with a recomputed sort value | Dragging a card and dropping above/below another repositions it; the new order persists across reload; dropping to make a card a child of another is not allowed (no nesting); a failed save reverts the position | | not started |
| STK-015 | Drag reordering is enabled only on the stickies route (not wherever a card might otherwise render) | owner of the sticky | all | Stickies route grid | Same as STK-014 | Cards are draggable on the stickies route; the same card rendered outside that route is not draggable | | not started |
| STK-016 | Search the user's stickies from the header search box; the query filters server-side and updates as you type (debounced) | any workspace role | all | Search control in the stickies route header | `GET .../stickies/?query=…` (matches the note's stripped text content) | Opening search reveals an input; typing narrows the list to stickies whose note text contains the term; the list refetches after a brief pause; note that despite a "search by title"-style prompt the match is against the note body text (flag as a copy/behaviour mismatch — see STK-025) | | not started |
| STK-017 | Search field interactions: Escape first clears a non-empty query then closes the empty field; a clear/close button empties the query and restores the full list | any workspace role | all | Header search control | Re-runs `GET` list on clear | Escape on a non-empty query clears it and reloads all stickies; Escape on an empty field closes it; the clear button empties the query, closes the field and reloads the full list; clicking outside an empty field closes it | | not started |
| STK-018 | Infinite scroll: more stickies load automatically as the user scrolls to the bottom, with a placeholder while the next page loads | any workspace role | all | Stickies route list (intersection sentinel near the end) | `GET .../stickies/?cursor=…&per_page=…` paginated | Scrolling to the end fetches and appends the next page without a manual action; a loading placeholder shows during the fetch; no further fetches once the last page is reached; already-loaded cards are not duplicated | | not started |
| STK-019 | Initial loading state shows a skeleton grid before the first page arrives | any workspace role | all | Stickies route body on first load | Same as STK-001 | Before the first page loads, placeholder skeleton cards are shown; they are replaced by real cards (or an empty state) once loading completes | | not started |
| STK-020 | Empty states: distinct art and messaging for "no stickies yet" (with an Add call-to-action) versus "search matched nothing" | any workspace role; the empty-state Add CTA is gated to workspace roles | all | Stickies route body when the list is empty | None (presentation; CTA triggers `POST` per STK-005) | With no stickies and no active search, a themed empty state with an "Add sticky" button appears (the button is disabled for a user without workspace-level permission); with an active search and no matches, a different "nothing matched your search" empty state appears instead | | not started |
| STK-021 | No real-time updates: sticky changes made elsewhere do not stream in; the list is fetched and locally cached, not live | any workspace role | all | Stickies route | None (plain fetch + client cache; no websocket/live channel) | A sticky created or changed in another session/tab does not appear until the stickies data is refetched (e.g. reload/re-navigation); the new app must not silently add a live channel that changes this without a row | | not started |
| STK-022 | No edition-specific stickies behaviour: the same capabilities exist identically in OSS, cloud and desktop, and stickies exist only in the workspace web build (not in god-mode or public boards) | any workspace role | all | Same entry points in every build | Same endpoints in every build | A scenario run against any edition observes the same stickies capabilities and outcomes; no extra or missing stickies rows appear per edition; god-mode and public-board surfaces expose no stickies | | not started |
| STK-023 | No per-sticky deep link / URL state: the route addresses the list only; search text and the open/active card are not encoded in the URL | any workspace role | all | Stickies route URL | None | The URL is the list route with no per-sticky path or query for the active card or search term; sharing the URL opens the list, not a specific sticky or a pre-filled search | | not started |
| STK-024 | No import/export and no bulk operations for stickies: there is no CSV/PDF export, no bulk select, and no multi-delete | any workspace role | all | Stickies route | None | The stickies screen offers no export, import, or multi-select/bulk actions; the new app must not lose this as an expectation (absence row) | | not started |
| STK-025 | bug: search prompt vs behaviour mismatch — the search affordance is presented as searching by title but the server matches the note's full stripped text | any workspace role | all | Header search control | `GET .../stickies/?query=…` (server matches stripped body text) | Recorded as an old-app behaviour to encode as a `bug:` scenario with a linked issue: the intended behaviour (search-by-title vs search-body) is a product decision for sign-off; the parity scenario should assert the current server behaviour (body-text match) and reference the bug issue | | not started |

## Coverage checklist

Every route file, top-level component folder and API endpoint in the nominated sources
(`stickies` route and `core/components/stickies`) is mapped to the rows that cover it. Items
with no covering row are explained instead. "Dead code" below means: no reachable entry point
was found in the surveyed build (grep for every importer/mounting site returned nothing outside
the file itself or other dead files); such files produce no observable behaviour and so get no row.

### Route files

| Route file | Covering rows |
| --- | --- |
| `apps/web/app/(all)/[workspaceSlug]/(projects)/stickies/layout.tsx` (app header + content wrapper shell around the route) | STK-001 |
| `apps/web/app/(all)/[workspaceSlug]/(projects)/stickies/header.tsx` (breadcrumb, search, Add sticky) | STK-005, STK-016, STK-017 |
| `apps/web/app/(all)/[workspaceSlug]/(projects)/stickies/page.tsx` (page title + infinite list host) | STK-001, STK-003, STK-018 |

### Top-level component folders (`core/components/stickies`)

| Component / folder | Covering rows |
| --- | --- |
| `layout/stickies-infinite.tsx` (SWR fetch of first page + intersection-observer next-page loading) | STK-001, STK-018 |
| `layout/stickies-list.tsx` (masonry grid, responsive column count, loading/empty branching, drag-drop wiring) | STK-003, STK-004, STK-014, STK-019, STK-020 |
| `layout/stickies-loader.tsx` (initial skeleton grid) | STK-019 |
| `layout/sticky-dnd-wrapper.tsx` (per-card draggable + drop target; route-gated dnd; drag preview) | STK-014, STK-015 |
| `layout/sticky.helpers.ts` (translate drop payload → reorder-above/below; block make-child) | STK-014 |
| `sticky/root.tsx` (note card container, background colour, autosave debounce, delete-modal host) | STK-007, STK-010, STK-011, STK-012 |
| `sticky/inputs.tsx` (form wiring around the sticky editor; empty-body normalisation) | STK-007, STK-009 |
| `sticky/use-operations.tsx` (create/update/remove/reorder ops, toasts, duplicate-empty guard, name-length guard) | STK-005, STK-006, STK-007, STK-011, STK-012, STK-014 |
| `sticky/index.ts` | dead code: barrel re-export only |
| `delete-modal.tsx` (delete confirmation dialog + failure toast) | STK-012 |
| `modal/search.tsx` (`StickySearch`: expandable header search, debounce, Escape/clear behaviour) | STK-016, STK-017 |
| `sticky/sticky-item-drag-handle.tsx` (`StickyItemDragHandle`) | dead code: only usage is commented out in `sticky/root.tsx`; not rendered anywhere |
| `action-bar.tsx` (`StickyActionBar`: floating expandable bar with all-stickies/recent/add) | dead code: no importer/mount found anywhere in the tree (grep). Its buttons are the only openers of the all-stickies modal and the only consumers of the recent-sticky flow, so both are dead by extension |
| `modal/index.tsx` + `modal/stickies.tsx` (`AllStickiesModal`, `Stickies`) | dead code: opened only from `action-bar.tsx` (dead). The command-palette store still exposes an `allStickiesModal`/`toggleAllStickiesModal` seam, but nothing reachable calls it |
| `widget.tsx` (`StickiesWidget`: dashboard/home embed of the truncated list) | dead code: only reference is a design-doc note; not mounted. The home `my_stickies` dashboard widget is deliberately hidden (component is null and listed in the hidden-widget set, PDASHOSS01-7) |
| `layout/stickies-truncated.tsx` (`StickiesTruncated`: height-capped list with a "show all" link) | dead code: used only by `widget.tsx` and `modal/stickies.tsx`, both dead |

### Supporting sources outside `core/components/stickies` (consulted for behaviour; rows only, no checklist obligation)

- `core/store/sticky/sticky.store.ts` — list/next-page fetch, create/update/delete/reorder actions, optimistic update + rollback, owner-scoped caching: STK-004, STK-005, STK-007, STK-012, STK-014, STK-018.
- `core/services/sticky.service.ts` — the five endpoint calls (see API table). `getSticky(id)` (single retrieve) is defined but has no reachable caller in the surveyed build → dead code at the service level, endpoint still mapped below.
- `core/components/editor/sticky-editor/**` (`editor.tsx`, `toolbar.tsx`, `color-palette.tsx`, constants `TOOLBAR_ITEMS.sticky`) — bold/italic/to-do toolbar, colour palette, delete action, enter-key disabled: STK-008, STK-009, STK-010, STK-011, STK-012.
- Sidebar entry (`core/components/workspace/sidebar/sidebar-item.tsx` static item + `ce/.../sidebar/helper.tsx` icon) and personal-nav preferences: STK-001, STK-002 (pin management owned by the shell inventory).
- `core/components/home/widgets/empty-states/stickies.tsx` (`StickiesEmptyState`) — rendered only by `stickies-list.tsx`'s non-stickies-route branch, which is reached only through the dead truncated/modal/widget path → effectively unreachable; not a stickies-route row. Noted here so the sign-off can confirm.

### API endpoints

| Endpoint | Covering rows |
| --- | --- |
| `GET /api/workspaces/{ws}/stickies/` (owner-scoped list; `order_by(-sort_order)`; `?query=` matches `description_stripped`; cursor pagination) | STK-001, STK-003, STK-004, STK-016, STK-018, STK-025 |
| `POST /api/workspaces/{ws}/stickies/` (create; roles admin/member/guest; server assigns sort_order = max+step, sanitises `description_html`) | STK-005, STK-006, STK-020 |
| `GET /api/workspaces/{ws}/stickies/{id}` (single retrieve) | no reachable caller in the surveyed frontend (dead at the service level); endpoint exists and is owner-scoped |
| `PATCH /api/workspaces/{ws}/stickies/{id}/` (update body/colour/sort; creator-only; HTML sanitised) | STK-007, STK-008, STK-011, STK-013, STK-014 |
| `DELETE /api/workspaces/{ws}/stickies/{id}/` (delete; creator-only) | STK-012, STK-013 |

### Backend supporting sources (for server-side rules asserted above)

| Source | Covering rows |
| --- | --- |
| `apps/api/pi_dash/app/views/workspace/sticky.py` (owner-scoped queryset; create/list roles = admin/member/guest; update/destroy = creator; list search on stripped text) | STK-001, STK-005, STK-013, STK-016 |
| `apps/api/pi_dash/db/models/sticky.py` (owner + workspace scope; `description_stripped` derived on save; new-row sort_order = max+10000) | STK-004, STK-013, STK-016 |
| sticky serializer (fields; `description_html` sanitised/validated → 400 on invalid) | STK-005, STK-007 |
| `apps/api/pi_dash/app/urls/workspace.py` (list/create and detail route registration) | STK-001, STK-005, STK-007, STK-012 |

### Sweeps with no stickies behaviour (no rows; confirmed absent)

- Desktop overlay (`desktop-overlay/`): no stickies code — covered by STK-022.
- `ce/` tree and instance admin (`apps/admin/`) / public boards (`apps/space/`): no stickies code — covered by STK-022.
- Real-time channels for stickies: none (plain fetch + client cache) — covered by STK-021.
- Exports/imports and bulk operations: none — covered by STK-024.
- Per-sticky deep link / URL-encoded search or active card: none — covered by STK-023.
- Empty-state image assets (`app/assets/empty-state/stickies/*.webp`): presentational only, no behaviour — covered by STK-020.
- `logo_props` and `color` (vs `background_color`) fields on the model/type: not set or read by the stickies UI (only `background_color` is used) — no row.
- Client-side 100-character name guard in `use-operations.tsx`: dormant — the UI never edits the `name` field (only the body), so the guard is currently unreachable; noted, no row.
