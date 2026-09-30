# Feature inventory — Intake (Phase 3)

Area prefix: `INT-`. Editions covered: oss, cloud, desktop.

Sources surveyed: the project intake route
`apps/web/app/(all)/[workspaceSlug]/(projects)/projects/(detail)/[projectId]/intake/`
(`layout.tsx`, `page.tsx`); the component tree
`apps/web/core/components/inbox/` — the two-pane root (`root.tsx`), the status
helpers (`inbox-status-icon.tsx`, `inbox-issue-status.tsx`), the sidebar
(`sidebar/root.tsx`, `sidebar/inbox-list.tsx`, `sidebar/inbox-list-item.tsx`),
the detail pane (`content/root.tsx`, `content/issue-root.tsx`,
`content/inbox-issue-header.tsx`, `content/inbox-issue-mobile-header.tsx`,
`content/issue-properties.tsx`), the triage-action modals
(`modals/decline-issue-modal.tsx`, `modals/snooze-issue-modal.tsx`,
`modals/select-duplicate.tsx`, `modals/delete-issue-modal.tsx`), the create
dialog (`modals/create-modal/*`), and the filter/sort UI (`inbox-filter/*`);
the cloud-vs-OSS split point `apps/web/ce/components/inbox/source-pill.tsx` (an
empty OSS stub) plus the OSS de-dupe stubs it pulls in from
`apps/web/ce/components/de-dupe/*`; the shared constants and types
(`packages/constants/src/intake.ts`, `packages/types/src/inbox.ts`); the client
stores and services (`apps/web/core/store/inbox/*`,
`apps/web/core/services/inbox/*`); and the API
(`apps/api/pi_dash/app/views/intake/base.py`,
`apps/api/pi_dash/app/urls/intake.py`, the triage-state endpoint in
`apps/api/pi_dash/app/views/state/base.py`).

The intake screen header comes from
`apps/web/ce/components/projects/settings/intake/header.tsx` (imported by the
route layout) and the whole area is gated by the project's intake feature
toggle, which lives in Project settings (`settings/projects/[projectId]/features/intake`)
— that toggle screen is inventoried under Project settings; here we cover only
the gating effect it has on this screen.

Sweeps for desktop-only and OSS-only intake code found none beyond the CE
source-pill / de-dupe stubs (no `desktop-overlay` intake code exists); the
cloud overlay that supplies the real source pill, the duplicate-detection UI,
and forms/email ingestion is a private overlay not present in this checkout, so
its behavior is described from the OSS stubs, the shared types, and the core
code paths that branch on `source`. Behavior below is paraphrased from those
sources; no UI copy, code, class names or message text is reproduced.

Status vocabulary used throughout: an intake item carries a numeric status —
Pending, Snoozed, Accepted, Declined, or Duplicate. "Open" collects Pending and
Snoozed; "Closed" collects Accepted, Declined and Duplicate. Its source is one
of in-app, forms or email.

Row format follows the Parity page: ID, capability, who, edition, old entry
point, API, acceptance, parity test (empty until the oracle run), status.

| ID | Capability | Who | Edition | Old entry point | API | Acceptance | Parity test | Status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| INT-001 | Open a project's Intake screen as a two-pane layout: a scrollable request list on the left and a request detail on the right, under the project intake route | any project role (admin, member, guest) on a project with intake enabled | all | Project intake route; app header breadcrumb (project → Intake) | `GET /api/workspaces/{ws}/projects/{pid}/intake-issues/` | The route renders the sidebar list plus a detail area; the header shows the project→Intake breadcrumb; with no request selected the detail area shows a "pick an item" placeholder |  | not started |
| INT-002 | See a feature-disabled screen instead of the list when the project has intake switched off: an explanatory empty state with an action that jumps to the project's feature settings | any viewer (the jump action is enabled only for project admins) | all | Intake route when the project's intake feature is off | None (feature flag read from project details) | With intake off, no list loads; the empty state explains intake and offers a settings shortcut; the shortcut is enabled for admins and disabled for everyone else |  | not started |
| INT-003 | See the browser/page title carry the project name plus the Intake label | any viewer | all | Intake route document title | None | Page title reflects the current project name and the Intake area |  | not started |
| INT-004 | Switch between an Open tab and a Closed tab; the Open tab carries a live count of pending requests; switching refetches the matching requests and reflects the tab in the URL | any viewer | all | Sidebar tab bar | Same list endpoint with a status filter (`status`) | Open lists pending/snoozed, Closed lists accepted/declined/duplicate; the Open tab shows the pending total; switching updates both the list and the `currentTab` URL param |  | not started |
| INT-005 | Land with the first request in the active tab auto-selected: when no request is in the URL and the list is non-empty, the screen redirects to open the first item | any viewer | all | Sidebar on load / after tab switch | Same list endpoint | Opening the screen (or a tab) with items present navigates to the first item and puts its id in the URL; an empty tab selects nothing |  | not started |
| INT-006 | Read each request in the list at a glance: project-scoped id, status chip (hidden while pending), title, created date (with a tooltip), priority marker, and labels (four or more collapse to a count); the author avatar shows, with system/forms-generated requests attributed to an intake identity rather than a person | any viewer | all | Sidebar list row | Same list endpoint (per-item issue + intake fields) | Every row shows id, title, created date, priority and labels as described; a pending item shows no status chip; label overflow collapses to a count; a forms/system-authored request shows the intake identity instead of a member avatar |  | not started |
| INT-007 | Scroll the list to load more requests a page at a time, with placeholder rows while a page loads and a header "syncing" indicator during paging | any viewer | all | Sidebar list (infinite scroll) | Same list endpoint with cursor paging (ten per page) | Reaching the bottom loads the next page without duplicates until exhausted; skeletons show during load; the header shows a syncing hint while a page is fetching |  | not started |
| INT-008 | Land on the right empty state for each situation: filtered-to-nothing shows a no-results state; an empty Open tab shows guidance plus a create action; an empty Closed tab shows an informational state | any viewer | all | Sidebar list area when empty | Same list endpoint (empty result) | With filters and no matches, a no-results state appears; empty Open shows the create CTA; empty Closed shows its informational message |  | not started |
| INT-009 | Filter the list by status, priority, assignees, created-by, labels, created date and updated date from a searchable filter panel, with applied filters shown as removable chips and a running applied-count; changing a filter refetches | any viewer | all | Sidebar filter dropdown; applied-filter chip row | Same list endpoint with the chosen filter query params | Each filter narrows the list server-side; chips reflect and can remove active filters; the filter button shows how many are applied; clearing restores the full list |  | not started |
| INT-010 | Use quick date ranges (today, yesterday, last 7 days, last 30 days) as well as explicit ranges when filtering by created or updated date | any viewer | all | Filter panel, date filters | Same list endpoint with date range params | Selecting a preset filters to that window; the same field also accepts an explicit from/to range |  | not started |
| INT-011 | Order the list by created date, last-updated date or id, ascending or descending | any viewer | all | Sidebar order-by dropdown | Same list endpoint with `order_by` (field and direction) | Choosing a field and direction re-sorts the list server-side and the chosen option is marked in the menu |  | not started |
| INT-012 | See the Open tab default to pending/snoozed requests and the Closed tab to resolved ones even before any explicit status filter is applied | any viewer | all | Tab switch | Same list endpoint (default status when unspecified) | With no status filter, Open shows pending (and snoozed) and Closed shows accepted/declined/duplicate |  | not started |
| INT-013 | Create a new intake request from the header: a dialog with a required title, a rich-text description, and inline properties (intake state, priority, assignees, labels, due date); on save the item is created in the project's triage state, tagged as an in-app request, an activity entry is logged, and the screen opens the new item | admin, member, guest (all project roles) on an intake-enabled project | all | Intake header create control; create dialog | `POST /api/workspaces/{ws}/projects/{pid}/intake-issues/` (+ triage-state lookup `GET …/intake-state/`) | An eligible user opens the dialog, fills it, and saves; the request appears in the Open tab in the triage state as an in-app source; a creation activity is recorded; the detail opens on the new item |  | not started |
| INT-014 | Be guided by create-time validation: an over-long title (past 255 characters) blocks submission, an empty title is refused by the server, and an out-of-range priority is refused | anyone creating a request | all | Create dialog | Same create endpoint (name required; priority restricted to the allowed set) | Over-long title disables submit; empty title yields a server error and nothing is created; an invalid priority is rejected |  | not started |
| INT-015 | Keep a "create more" mode that, on save, clears the dialog for the next request instead of closing and navigating away | admin, member, guest | all | Create dialog footer toggle | Same create endpoint | With create-more on, saving resets the form and stays open; with it off, saving closes the dialog and opens the created item |  | not started |
| INT-016 | Be protected against losing work when dismissing the create dialog: closing or pressing escape is blocked with a notice while the editor is still processing, and otherwise closes | anyone creating a request | all | Create dialog close / escape | None | Attempting to close while the editor is mid-process shows a wait notice and keeps the dialog; once settled, close/escape dismisses it |  | not started |
| INT-017 | Attach files to a new request during creation, with the uploads committed to the created work item on save | admin, member, guest | all | Create dialog description/attachment area | Bulk asset status update against the new work item after create | Files added while composing are attached to the resulting request |  | not started |
| INT-018 | Be warned of likely duplicates while composing a request and while viewing one: a debounced check surfaces potential duplicate work items via a button/popover that opens a comparison view | admin, member, guest (composing); any viewer (detail) | cloud | Create dialog duplicate button; detail duplicate popover | Cloud duplicate-search endpoint (debounced on title/description) | With similar items present, a duplicate indicator appears and opens the comparison; in OSS/desktop no duplicate UI appears (stubs render nothing) |  | not started |
| INT-019 | Open a request's detail: editable title and rich-text description, reactions, attachments, and a properties panel; the intake state is shown read-only, while assignees, priority, due date and labels are editable | any viewer (editing gated per INT-026); guests read-only unless owner | all | Detail pane | `GET …/intake-issues/{issueId}/`; `PATCH …/intake-issues/{issueId}/` for field edits | Selecting a request shows its content; editable fields save inline for authorized users and are disabled otherwise; the state field is not directly editable here |  | not started |
| INT-020 | Accept a request into the project as a real work item: an "add to project" dialog opens the work-item form, and confirming moves the item to accepted, keeps it as a project work item, and advances the screen to the next/previous request | project admins only (the accept control is offered to admins and members, but a member is refused with a permission notice) | all | Detail header accept action | `PATCH …/intake-issues/{issueId}/` (status → accepted) plus the work-item update from the dialog | An admin accepting an open/pending request turns it into a project work item, marks it accepted, and moves focus to a neighbour; a member is blocked with a notice; the control is absent once the item is resolved |  | not started |
| INT-021 | Decline a request through a confirmation dialog that warns it cannot be undone; the request moves to declined and the screen advances | project admins only (offered to admin/member; member refused with a notice) | all | Detail header decline action; confirm dialog | `PATCH …/intake-issues/{issueId}/` (status → declined) | Confirming as admin marks the request declined and advances focus; cancelling leaves it; a member is refused |  | not started |
| INT-022 | Snooze a pending request until a chosen future date (past dates are not selectable) and un-snooze it later; a snoozed request shows a countdown and, once the date passes, reads as due | project admins only | all | Detail header overflow menu (snooze/un-snooze); date picker | `PATCH …/intake-issues/{issueId}/` (snoozed-till date; cleared to un-snooze) | Snoozing sets a future date and shows a days-remaining chip; the picker forbids past dates; un-snooze clears it; a lapsed snooze renders in the passed styling |  | not started |
| INT-023 | Mark a request as a duplicate of another project work item, chosen from a searchable picker; the detail then shows a "duplicate of" link to that work item | project admins only | all | Detail header overflow menu (mark as duplicate); search picker | `PATCH …/intake-issues/{issueId}/` (duplicate-of id); project issue search `…/issues/search/` for the picker | Choosing a target marks the request duplicate and shows a link to the chosen work item; searching filters candidate items and excludes the current one |  | not started |
| INT-024 | Copy a request's work-item link (with a confirmation toast) and, once a request is accepted or declined, open the underlying work item directly | any viewer for copy; open-work-item shown for accepted/declined | all | Detail header actions and overflow menu | None (client-side link build + clipboard) | Copy places the work-item URL on the clipboard and confirms; for resolved requests an open action navigates to the work item |  | not started |
| INT-025 | Delete a request through a confirmation dialog; deleting a non-accepted request removes both the intake entry and its underlying work item, while deleting an accepted request removes only the intake entry and keeps the work item | project admins, or the request's creator | all | Detail header overflow menu (delete); confirm dialog | `DELETE …/intake-issues/{issueId}/` | Confirming removes the request and advances/returns to the list; for pending/snoozed/declined/duplicate the work item is deleted too; for accepted the work item remains; an unauthorized attempt is refused with a permission notice |  | not started |
| INT-026 | Be limited by edit permissions: only a project admin or the request's creator may edit an intake item; a guest who is not the creator sees it read-only; a guest editing their own item is limited to title and description; and any accepted/declined/duplicate item is read-only | admin, creator, guest as stated | all | Detail pane (field enable/disable) | `PATCH …/intake-issues/{issueId}/` enforces the same limits server-side | Each actor can edit exactly the fields their role allows; resolved items are non-editable; unauthorized field writes are refused server-side |  | not started |
| INT-027 | Navigate between requests with the keyboard and header chevrons: up/down (and the prev/next buttons) move to the neighbouring request and wrap around, but do nothing while typing in the title or description | any viewer | all | Detail header prev/next; arrow keys | Same detail fetch on navigation | Arrow up/down and the chevron buttons move between requests and wrap at the ends; the shortcut is suppressed while editing text |  | not started |
| INT-028 | Deep-link to a specific request on a specific tab via the URL; opening a link to a request that is no longer available redirects back to the list; navigating within the screen keeps the URL in sync | any viewer with access to that request | all | Intake URL (`currentTab`, `inboxIssueId` params) | `GET …/intake-issues/{issueId}/` | A shared link opens that request on that tab; a stale/forbidden id redirects to the list; selecting, accepting, declining, deleting each update the URL |  | not started |
| INT-029 | View and restore earlier versions of a request's description | admin or creator (editable actors) | all | Detail description version control | `GET …/intake-work-items/{workItemId}/description-versions/[{versionId}]/` | The version list opens, a version can be viewed, and restoring it replaces the current description |  | not started |
| INT-030 | Use the screen on a narrow viewport: the request list collapses behind a toggle and the detail actions move into a mobile action header | any viewer | all | Mobile layout (sidebar toggle, mobile header) | Same endpoints | On small screens the list can be shown/hidden and the accept/decline/snooze/duplicate/delete actions remain reachable from the mobile header |  | not started |
| INT-031 | See intake update only on fetch — there is no live push: the list and detail refresh on tab switch, filter/sort change, paging, and explicit navigation, not spontaneously | any viewer | all | Whole screen | Same list/detail endpoints (no realtime channel; detail fetch does not revalidate on focus) | Changes made elsewhere appear after a refetch trigger, not in real time |  | not started |
| INT-032 | Restricted request visibility for guests: a guest sees only the requests they created unless the project allows guests to view all features, in which case they see all | guest (scoped); admin/member (all) | all | Sidebar list; detail fetch | List and retrieve endpoints scope guest results to own items unless the project's guest-view-all flag is set | A guest without the flag sees and can open only their own requests; with the flag they see all; admins/members always see all |  | not started |
| INT-033 | Have a project triage state guaranteed for intake: the intake state picker reads the project's triage state, and the first created request creates that state if it does not yet exist | admin, member, guest | all | Create dialog / detail state field | `GET …/intake-state/`; triage state auto-created on first create | The state field resolves to the project triage state; a project with no triage state gets one created on first intake creation |  | not started |
| INT-034 | Identical intake behavior in the OSS and desktop builds; the cloud edition adds source pills for forms/email requests, duplicate detection, and forms/email request ingestion | any viewer | oss/desktop identical; cloud adds source pill, de-dupe (INT-018), forms/email sources | Same entry points in every build | Source pill + de-dupe cloud endpoints (see INT-018) | An OSS or desktop run observes the same capabilities as web with no source pill and no duplicate UI; a cloud run additionally shows request sources and duplicate detection |  | not started |

## Coverage checklist

Every route file, top-level component folder and API endpoint in the nominated
sources is mapped to the row IDs that cover it. Items with no covering row are
explained instead.

### Route files

| Route file | Covering rows |
| --- | --- |
| `…/projects/(detail)/[projectId]/intake/layout.tsx` (app-header shell wrapping the intake header + content outlet) | INT-001 |
| `…/projects/(detail)/[projectId]/intake/page.tsx` (feature gate on the intake toggle, page title, mounts the two-pane root with URL tab/item params) | INT-001, INT-002, INT-003, INT-028 |

### Top-level component folders

| Component file | Covering rows |
| --- | --- |
| `core/components/inbox/root.tsx` (two-pane layout, initial fetch, init loader/error states, mobile sidebar toggle, "select an item" placeholder) | INT-001, INT-007, INT-030 |
| `core/components/inbox/inbox-status-icon.tsx` (per-status icon + color mapping) | INT-006, INT-022 |
| `core/components/inbox/inbox-issue-status.tsx` (status chip; hidden when pending; snoozed countdown / passed styling) | INT-006, INT-022 |
| `core/components/inbox/sidebar/root.tsx` (Open/Closed tabs, pending count, filter entry, applied filters, infinite scroll, empty states, auto-select first) | INT-004, INT-005, INT-007, INT-008, INT-009, INT-012 |
| `core/components/inbox/sidebar/inbox-list.tsx` (renders the list rows) | INT-001, INT-006 |
| `core/components/inbox/sidebar/inbox-list-item.tsx` (row content: id, source pill, status, title, date, priority, labels overflow, author/intake-identity avatar; links to the item) | INT-006, INT-028, INT-034 |
| `core/components/inbox/content/root.tsx` (detail fetch, editable/read-only + guest computation, disabled-when-resolved, redirect when item unavailable) | INT-019, INT-025, INT-026, INT-028, INT-031 |
| `core/components/inbox/content/issue-root.tsx` (title/description editing, reactions, attachments, description versions, activity, duplicate popover, forms-author label) | INT-017, INT-019, INT-029, INT-018, INT-034 |
| `core/components/inbox/content/inbox-issue-header.tsx` (accept/decline/snooze/duplicate/delete actions, permission toasts, copy/open link, keyboard + chevron navigation) | INT-020, INT-021, INT-022, INT-023, INT-024, INT-025, INT-027 |
| `core/components/inbox/content/inbox-issue-mobile-header.tsx` (same actions in the narrow-viewport header) | INT-030 |
| `core/components/inbox/content/issue-properties.tsx` (properties panel: read-only state, editable assignees/priority/due date/labels, duplicate-of link, accepted uses normal state) | INT-019, INT-023 |
| `core/components/inbox/modals/decline-issue-modal.tsx` (decline confirmation) | INT-021 |
| `core/components/inbox/modals/snooze-issue-modal.tsx` (snooze date picker, no past dates) | INT-022 |
| `core/components/inbox/modals/select-duplicate.tsx` (searchable duplicate-target picker over project work items) | INT-023 |
| `core/components/inbox/modals/delete-issue-modal.tsx` (delete confirmation, permission-error handling) | INT-025 |
| `core/components/inbox/modals/create-modal/create-root.tsx` (create form orchestration: submit, create-more, escape guard, attachments, redirect, duplicate button) | INT-013, INT-015, INT-016, INT-017, INT-018 |
| `core/components/inbox/modals/create-modal/modal.tsx` (create dialog shell) | INT-013 |
| `core/components/inbox/modals/create-modal/issue-title.tsx` (title field, 255-character guard) | INT-014 |
| `core/components/inbox/modals/create-modal/issue-description.tsx` (rich-text description, enter-to-submit, asset upload) | INT-013, INT-017 |
| `core/components/inbox/modals/create-modal/issue-properties.tsx` (create-time state/priority/assignees/labels/due date pickers) | INT-013, INT-033 |
| `core/components/inbox/inbox-filter/root.tsx` (filter dropdown + order-by entry) | INT-009, INT-011 |
| `core/components/inbox/inbox-filter/filters/*` (`filter-selection`, `status`, `priority`, `members`, `labels`, `date`, `state`) | INT-009, INT-010 |
| `core/components/inbox/inbox-filter/applied-filters/*` (chip row + per-type chips: `root`, `status`, `priority`, `member`, `label`, `date`, `state`) | INT-009 |
| `core/components/inbox/inbox-filter/sorting/order-by.tsx` (order-by field + direction) | INT-011 |
| `core/components/inbox/index.ts`, `content/index.ts`, `sidebar/index.ts`, `inbox-filter/index.ts`, `modals/create-modal/index.ts` (barrel re-exports; no behavior) | dead code for parity purposes: re-exports only, covered transitively by INT-001 |
| `ce/components/inbox/source-pill.tsx` (OSS stub: renders nothing; the cloud overlay supplies the real source pill) | INT-034 (edition split), INT-006 |

### Filter sub-note

The status filter (`inbox-filter/filters/status.tsx`) and state filter
(`filters/state.tsx`, `applied-filters/state.tsx`) are exercised through
INT-009; the status default behavior is INT-012. No filter is unmapped.

### API endpoints

| Endpoint | Covering rows |
| --- | --- |
| `GET /api/workspaces/{ws}/projects/{pid}/intake-issues/` (list; status filter; order-by; cursor paging; guest scoping) | INT-004, INT-006, INT-007, INT-009, INT-010, INT-011, INT-012, INT-032 |
| `POST /api/workspaces/{ws}/projects/{pid}/intake-issues/` (create; name required; priority validated; forces triage state; source in-app; logs activity) | INT-013, INT-014, INT-017, INT-033 |
| `GET /api/workspaces/{ws}/projects/{pid}/intake-issues/{issueId}/` (retrieve; guest own-only unless guest-view-all) | INT-019, INT-028, INT-032 |
| `PATCH /api/workspaces/{ws}/projects/{pid}/intake-issues/{issueId}/` (update issue fields and/or intake status; admin/creator; guest limited to name/description; status change admin-only; activity logged) | INT-019, INT-020, INT-021, INT-022, INT-023, INT-026 |
| `DELETE /api/workspaces/{ws}/projects/{pid}/intake-issues/{issueId}/` (delete; admin/creator; also deletes work item unless accepted) | INT-025 |
| `GET /api/workspaces/{ws}/projects/{pid}/intake-work-items/{workItemId}/description-versions/` and `…/{versionId}/` (list/retrieve description versions) | INT-029 |
| `GET /api/workspaces/{ws}/projects/{pid}/intake-state/` (project triage state) | INT-033 |
| `GET/POST /api/workspaces/{ws}/projects/{pid}/intakes/` and `GET/PATCH/DELETE …/intakes/{pk}/` (intake container: list/create/update/delete; pending-count annotation; default intake cannot be deleted) | list read backs the screen's single-intake lookup, covered by INT-001/INT-004; create/update/delete of intake containers are not exposed by any screen in the surveyed sources — no covering row: candidate cloud "multiple intakes / forms" feature (forms ingestion is cloud, INT-034), otherwise dead code for the OSS intake UI |
| Project issue search `…/issues/search/` (candidate list for the duplicate picker) | INT-023 |
| Cloud duplicate-detection search (debounced, private overlay endpoint) | INT-018 |
| Bulk project asset status update (`file.service` — commit uploads to the created work item) | INT-017 |

### Backend supporting modules

| Module | Covering rows |
| --- | --- |
| `apps/api/pi_dash/app/views/intake/base.py` (`IntakeViewSet`, `IntakeIssueViewSet`, `IntakeWorkItemDescriptionVersionEndpoint`: permission gates, guest scoping, triage-state creation, source tagging, activity, cascade delete rules) | INT-013, INT-020, INT-021, INT-022, INT-023, INT-025, INT-026, INT-029, INT-032, INT-033 |
| `apps/api/pi_dash/app/urls/intake.py` (intake route registrations) | INT-001, INT-013, INT-025, INT-029 |
| `apps/api/pi_dash/app/views/state/base.py` → `IntakeStateEndpoint` (triage state read) | INT-033 |
| `packages/types/src/inbox.ts` (status/source enums, filter/sort types) | INT-004, INT-006, INT-009, INT-011, INT-034 |
| `packages/constants/src/intake.ts` (status metadata, order-by/sort options, date presets) | INT-006, INT-010, INT-011 |
| `apps/web/core/store/inbox/project-inbox.store.ts` (tabs, filters, sorting, paging, fetch/create/delete, applied-count) | INT-004, INT-007, INT-009, INT-011, INT-013, INT-025 |
| `apps/web/core/store/inbox/inbox-issue.store.ts` (per-item status/snooze/duplicate/field updates) | INT-020, INT-021, INT-022, INT-023, INT-019 |
| `apps/web/core/services/inbox/inbox-issue.service.ts`, `intake-work_item_version.service.ts`, `index.ts` (list/create/retrieve/update/delete + version endpoints) | INT-013, INT-019, INT-025, INT-029 |
| `apps/web/ce/components/de-dupe/*` (OSS stubs: render nothing) | INT-018, INT-034 |
| `apps/web/ce/components/projects/settings/intake/header.tsx` (intake screen header: breadcrumb, syncing hint, create control gated by the intake toggle + role) | INT-001, INT-003, INT-013 |

### Sweeps with no intake behavior (no rows; confirmed absent)

- `desktop-overlay/` and the desktop app: no intake-specific code — covered by INT-034.
- Real-time channels / websockets: none in the intake stores — covered by INT-031.
- The project intake feature toggle screen (`settings/projects/[projectId]/features/intake`): a separate area (Project settings inventory); only its gating effect on this screen is captured — INT-002.
- Empty-state image assets (`app/assets/empty-state/disabled-feature/intake-*`, `search/*`): presentational only, no behavior — covered by INT-002, INT-008.
