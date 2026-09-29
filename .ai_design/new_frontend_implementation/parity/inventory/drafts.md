# Feature inventory — Drafts (Phase 1)

Area prefix: `DRAFT-`. Editions covered: oss, cloud, desktop.
Sources surveyed: route `apps/web/app/(all)/[workspaceSlug]/(projects)/drafts/`
(`page.tsx`, `layout.tsx`, `header.tsx`), component folder
`apps/web/core/components/issues/workspace-draft/` (`root.tsx`,
`draft-issue-block.tsx`, `draft-issue-properties.tsx`, `quick-action.tsx`,
`empty-state.tsx`, `delete-modal.tsx`, `loader.tsx`), the draft creation and
conversion flow in `apps/web/core/components/issues/issue-modal/`
(`draft-issue-layout.tsx`, `form.tsx`, `base.tsx`) plus the discard
confirmation used when closing a new-item dialog, the client store
`apps/web/core/store/issue/workspace-draft/`, the hooks in
`apps/web/core/hooks/store/workspace-draft/`, the service
`apps/web/core/services/issue/workspace_draft.service.ts`, and the API layer
(draft viewset, draft serializers, workspace URL routes, draft model).
Sweeps for edition or shell differences (cloud overlay, desktop overlay,
command palette entries, keyboard shortcuts, list filters, realtime channels)
found no draft-specific code. Behavior below is paraphrased from those
sources; no UI copy, code, class names or message text is reproduced.

Row format follows the Parity page: ID, capability, who, edition, old entry
point, API, acceptance, parity test (empty until the oracle run), status.

| ID | Capability | Who | Edition | Old entry point | API | Acceptance | Parity test | Status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| DRAFT-001 | Open the workspace drafts screen and see only drafts created by the signed-in user, newest first, under the workspace drafts route with its header and breadcrumb | any signed-in workspace role (each viewer sees only their own drafts) | all | Workspace drafts route, list area | `GET /api/workspaces/{ws}/draft-issues/` | Screen lists the viewer's drafts in reverse creation order; another user's drafts never appear; header breadcrumb and page title identify the screen |  | not started |
| DRAFT-002 | See the total draft count next to the screen title, shown only when at least one draft exists | any viewer of the screen | all | Drafts header, count marker | Same list response as DRAFT-001 (total from pagination info) | Empty workspace shows no count marker; with drafts present the marker matches the server total |  | not started |
| DRAFT-003 | Open the draft creation dialog from the header action, available only to workspace admins and members who belong to at least one project | admin, member with project membership (guests and project-less users get no working action) | all | Drafts header creation control | `POST /api/workspaces/{ws}/draft-issues/` on submit | Eligible users get an enabled control that opens the dialog; ineligible users see it disabled or absent |  | not started |
| DRAFT-004 | Fill in and save a new draft (project, title, description, work-item fields) through the same dialog used for work items in draft mode | admin, member | all | Creation dialog in draft mode | Same as DRAFT-003 | Saved draft appears at the top of the list; author gets a success notice; dialog closes and resets |  | not started |
| DRAFT-005 | Be protected against losing typed content: closing a new-item dialog with unsaved, non-empty content offers to keep it as a draft or throw it away, while empty content closes silently | anyone creating an item | all | New-item dialog close path, discard confirmation | Same as DRAFT-003 when the keep-as-draft choice is taken (no call on plain close or discard) | Closing with content shows the choice dialog; keep-as-draft stores a draft and closes everything; discard closes without storing; closing with no content never prompts |  | not started |
| DRAFT-006 | Leave the title blank when saving a draft and still get a usable draft: a generic placeholder title is applied automatically | admin, member | all | Creation dialog submit; keep-as-draft path | Same as DRAFT-003 (title defaulted client-side) | Submitted blank title stores a draft with the placeholder title instead of failing validation |  | not started |
| DRAFT-007 | Recognize each draft row by its project marker, work-type mark and title | any viewer of the screen | all | Draft row in the list | Same list response as DRAFT-001 | Every row shows which project it belongs to, what kind of item it will become, and its title |  | not started |
| DRAFT-008 | Open an existing draft for editing, via double-click on the row or the edit option in its menu | draft owner (admins per DRAFT-025) | all | Draft row double-click; row menu edit option | `PATCH /api/workspaces/{ws}/draft-issues/{id}/` on save | Dialog opens prefilled with the draft's values; saving updates the row in place |  | not started |
| DRAFT-009 | Reach row actions (edit, duplicate, delete) through the per-row menu and the hover quick-action control | draft owner (admins per DRAFT-025) | all | Draft row menu; hover quick actions | Varies per action (see DRAFT-008/010/011) | Menu and hover control expose the three actions; each one performs its own outcome |  | not started |
| DRAFT-010 | Duplicate a draft, producing a second draft whose title carries a copy marker and whose field values match the original | admin, member (own drafts) | all | Row menu duplicate option | `POST /api/workspaces/{ws}/draft-issues/` (values carried over client-side) | A new draft appears with the marked-up title and the same project, description and properties as the source |  | not started |
| DRAFT-011 | Delete a draft through a confirmation dialog; only the creator or a project admin may do it, everyone else is refused with a permission notice and the draft survives | creator or project admin (delete); anyone else is refused | all | Row menu delete option, confirmation dialog | `DELETE /api/workspaces/{ws}/draft-issues/{id}/` | Confirming as an authorized user removes the row and reports success; an unauthorized attempt reports a permission problem and the draft remains |  | not started |
| DRAFT-012 | Change a draft's workflow state inline from the row | draft owner (admins per DRAFT-025) | all | Row state picker | Same update endpoint as DRAFT-008 (`state` field) | New state renders on the row immediately and persists across reloads; a failed save rolls the row back with an error notice |  | not started |
| DRAFT-013 | Change a draft's priority inline from the row | draft owner (admins per DRAFT-025) | all | Row priority picker | Same update endpoint as DRAFT-008 (`priority` field) | New priority renders on the row immediately and persists across reloads |  | not started |
| DRAFT-014 | Change a draft's labels inline from the row, choosing only labels that belong to the draft's project | draft owner (admins per DRAFT-025) | all | Row labels picker | Same update endpoint as DRAFT-008 (`label` ids field) | Chosen labels render on the row and persist; labels from other projects are not offered |  | not started |
| DRAFT-015 | Change a draft's assignees inline from the row, choosing only members of the draft's project | draft owner (admins per DRAFT-025) | all | Row assignees picker | Same update endpoint as DRAFT-008 (`assignee` ids field) | Chosen assignees render on the row and persist; non-members are not offered |  | not started |
| DRAFT-016 | Change a draft's start and due dates inline from the row; a start date later than the due date is rejected | draft owner (admins per DRAFT-025) | all | Row date pickers | Same update endpoint as DRAFT-008 (date fields; order rule enforced server-side) | Valid dates render and persist; an inverted range is refused with a validation error and the old dates stay |  | not started |
| DRAFT-017 | Change a draft's estimate inline, offered only where the project has estimates switched on | draft owner on estimate-enabled projects | all | Row estimate picker (rendered only when enabled) | Same update endpoint as DRAFT-008 (`estimate` field) | Enabled projects offer the picker and the value persists; other projects show no estimate control |  | not started |
| DRAFT-018 | Attach a draft to a work cycle inline, offered only where cycle views apply to the project | draft owner on cycle-enabled projects | all | Row cycle picker (rendered only when applicable) | Same update endpoint as DRAFT-008 (cycle field) | Applicable projects offer the picker and the cycle persists; other projects show no cycle control |  | not started |
| DRAFT-019 | Attach a draft to work modules inline, offered only where module views apply to the project | draft owner on module-enabled projects | all | Row module picker (rendered only when applicable) | Same update endpoint as DRAFT-008 (`module` ids field) | Applicable projects offer the picker and the modules persist; other projects show no module control |  | not started |
| DRAFT-020 | Publish a draft into a real project work item: the draft's content, description, properties and attachments carry over, a creation activity entry is recorded, and the draft disappears from the drafts screen | admin, member | all | Draft edit dialog publish action | `POST /api/workspaces/{ws}/draft-to-issue/{draft_id}/` (plus cycle/module link creation where requested) | Published item appears in its project with the draft's content and files; activity history notes the creation; the drafts list no longer shows it |  | not started |
| DRAFT-021 | Be refused when publishing a draft that has no project, with a clear error and nothing created | anyone attempting the publish | all | Same publish action as DRAFT-020 | Same as DRAFT-020 (refusal path) | Attempt yields a missing-project error; no work item is created and the draft stays |  | not started |
| DRAFT-022 | Land on helpful empty states: with no drafts, an illustration plus a create action; with no projects at all, guidance plus a create-project action (gated to admins/members) instead of the list | any viewer (create-project action: admin, member) | all | List area when empty; no-project fallback | Same list response as DRAFT-001 (empty result) | Empty drafts show the illustration and working create action; project-less users get the create-project guidance whose action respects the role gate |  | not started |
| DRAFT-023 | See loading placeholders while drafts fetch, and page through large collections (fifty per page with a load-more control) | any viewer of the screen | all | List area while loading; list footer when more pages exist | Same list endpoint as DRAFT-001 with pagination parameters | Loading shows skeleton rows; scrolling/paging control appends the next page without duplicates until exhausted |  | not started |
| DRAFT-024 | Reach drafts at the workspace-scoped drafts address with a breadcrumb and page title; there is no per-draft address, so drafts cannot be deep-linked or shared by URL | any viewer | all | Workspace drafts route URL; browser address bar | None beyond DRAFT-001 (single-draft fetch endpoint exists for the API but the UI never routes to it) | The route renders the list with correct breadcrumb and title; no URL opens a single draft directly |  | not started |
| DRAFT-025 | Permissions matrix: every role lists only their own drafts; creation is open to all roles server-side but the header control stays disabled for guests and project-less users; edits, single-fetch and deletes are limited to admins and the draft's creator (so a guest may change or remove drafts they created); conversion to a work item requires admin or member even for one's own draft | guest / member / admin as stated | all | Whole screen plus row actions | List: all roles (owner-scoped); create: all roles (header control UI-disabled for guests); update/single fetch/delete: admin-or-creator; convert: admin+member only | Each role can do exactly its stated operations; unauthorized fetch/delete/convert calls are refused server-side and the UI gates or hides those controls, while guest creation and guest edits of own drafts succeed |  | not started |
| DRAFT-026 | Pinned non-behaviors: no keyboard shortcuts, no drag-and-drop reorder, no bulk operations, no list filters or search, no exports, and no live push — the list refreshes only by fetching (including on focus-safe fetch without refetch-on-focus) | any viewer | all | Whole screen (absence) | Same list endpoint as DRAFT-001 (plain fetch, no realtime channel) | No shortcut, drag, bulk, filter or export control exists on the screen; changes made elsewhere appear only after a fresh fetch |  | not started |
| DRAFT-027 | Identical drafts behavior in every build: OSS, cloud and desktop offer the same screen and rules with no edition-only or desktop-only additions | any viewer | all | Same entry points in every build | Same endpoints in every build | A scenario run against any edition observes the same capabilities and outcomes; no overlay or desktop draft code exists to diverge |  | not started |

## Coverage checklist

Every route file, top-level component folder and API endpoint in the
nominated sources is mapped to the row IDs that cover it. Items with no
covering row are explained instead.

### Route files

| Route file | Covering rows |
| --- | --- |
| `apps/web/app/(all)/[workspaceSlug]/(projects)/drafts/page.tsx` (screen entry, renders the list root with the page title) | DRAFT-001, DRAFT-024 |
| `apps/web/app/(all)/[workspaceSlug]/(projects)/drafts/layout.tsx` (shell: app header plus content wrapper around the screen) | DRAFT-001, DRAFT-024 |
| `apps/web/app/(all)/[workspaceSlug]/(projects)/drafts/header.tsx` (breadcrumb, count marker, gated creation control opening the draft dialog) | DRAFT-002, DRAFT-003 |

### Top-level component folders

| Component file | Covering rows |
| --- | --- |
| `apps/web/core/components/issues/workspace-draft/root.tsx` (fetch-on-mount list, skeleton, no-project fallback, empty state, row rendering, load-more paging) | DRAFT-001, DRAFT-007, DRAFT-022, DRAFT-023 |
| `apps/web/core/components/issues/workspace-draft/draft-issue-block.tsx` (row display, double-click edit, row menu: edit/duplicate/delete) | DRAFT-007, DRAFT-008, DRAFT-009, DRAFT-010, DRAFT-011 |
| `apps/web/core/components/issues/workspace-draft/draft-issue-properties.tsx` (inline pickers: state, priority, labels, assignees, dates, estimate, cycle, module) | DRAFT-012, DRAFT-013, DRAFT-014, DRAFT-015, DRAFT-016, DRAFT-017, DRAFT-018, DRAFT-019 |
| `apps/web/core/components/issues/workspace-draft/quick-action.tsx` (hover quick-action rendering of the row menu items) | DRAFT-009 |
| `apps/web/core/components/issues/workspace-draft/empty-state.tsx` (no-drafts illustration with create action) | DRAFT-022 |
| `apps/web/core/components/issues/workspace-draft/delete-modal.tsx` (confirmation dialog with creator-or-admin gate, success/permission notices) | DRAFT-011, DRAFT-025 |
| `apps/web/core/components/issues/workspace-draft/loader.tsx` (skeleton rows for initial and paged loads) | DRAFT-023 |
| `apps/web/core/components/issues/workspace-draft/index.ts` (barrel re-export; no behavior) | dead code for parity purposes: re-export only, covered transitively by DRAFT-001 |

### Client state, hooks and services

| Module | Covering rows |
| --- | --- |
| `apps/web/core/store/issue/workspace-draft/issue.store.ts` (fetch/create/update/delete/convert, cycle/module attach, optimistic update with rollback, newest-first ordering, per-workspace draft count) | DRAFT-001, DRAFT-004, DRAFT-008, DRAFT-010, DRAFT-011, DRAFT-012–DRAFT-020, DRAFT-023 |
| `apps/web/core/store/issue/workspace-draft/filter.store.ts` (filter store exists but the drafts screen passes no filter parameters) | DRAFT-026 (no filters on this screen; store unused here) |
| `apps/web/core/hooks/store/workspace-draft/use-workspace-draft-issue.ts` + `index.ts` (hook binding the screen to the draft store) | DRAFT-001 |
| `apps/web/core/hooks/store/workspace-draft/use-workspace-draft-issue-filters.ts` (filter hook binding; unused by the screen) | DRAFT-026 (no filters on this screen) |
| `apps/web/core/services/issue/workspace_draft.service.ts` (list, single-fetch, create, update, delete, convert endpoints) | DRAFT-001, DRAFT-004, DRAFT-008, DRAFT-011, DRAFT-020, DRAFT-024 |

### Draft creation and conversion dialog

| Module | Covering rows |
| --- | --- |
| `apps/web/core/components/issues/issue-modal/draft-issue-layout.tsx` (discard-vs-keep prompt on close, blank-title defaulting, keep-as-draft save with notices) | DRAFT-005, DRAFT-006 |
| `apps/web/core/components/issues/issue-modal/form.tsx` (shared item form in draft mode; publish action requiring a project with an error otherwise) | DRAFT-004, DRAFT-020, DRAFT-021 |
| `apps/web/core/components/issues/issue-modal/base.tsx` (dialog shell routing between draft and regular submit paths with success/failure notices) | DRAFT-004, DRAFT-005 |
| `apps/web/core/components/issues/confirm-issue-discard.tsx` (shared discard confirmation used by the draft close path) | DRAFT-005 |

### API endpoints

| Endpoint | Covering rows |
| --- | --- |
| `GET /api/workspaces/{ws}/draft-issues/` (list own drafts, newest first, paginated) | DRAFT-001, DRAFT-002, DRAFT-023, DRAFT-025 |
| `POST /api/workspaces/{ws}/draft-issues/` (create; assignees/labels/state/cycle/module validated against the project; date order validated) | DRAFT-004, DRAFT-006, DRAFT-010, DRAFT-016, DRAFT-025 |
| `GET /api/workspaces/{ws}/draft-issues/{id}/` (single fetch, admin-or-creator) | DRAFT-024, DRAFT-025 |
| `PATCH /api/workspaces/{ws}/draft-issues/{id}/` (update own draft, admin+member) | DRAFT-008, DRAFT-012–DRAFT-019, DRAFT-025 |
| `DELETE /api/workspaces/{ws}/draft-issues/{id}/` (delete, admin-or-creator) | DRAFT-011, DRAFT-025 |
| `POST /api/workspaces/{ws}/draft-to-issue/{draft_id}/` (convert to project item; project required; carries fields/files, logs activity, removes draft) | DRAFT-020, DRAFT-021, DRAFT-025 |

### Backend supporting modules

| Module | Covering rows |
| --- | --- |
| `apps/api/pi_dash/app/views/workspace/draft.py` (viewset: permission gates, owner scoping, convert flow with cycle/module/activity handling) | DRAFT-001, DRAFT-020, DRAFT-021, DRAFT-025 |
| `apps/api/pi_dash/app/serializers/draft.py` (create/update validation: project-scoped relations, date order, content checks) | DRAFT-004, DRAFT-014, DRAFT-015, DRAFT-016, DRAFT-025 |
| `apps/api/pi_dash/app/urls/workspace.py` (the three draft route registrations) | DRAFT-001, DRAFT-020, DRAFT-025 |
| `apps/api/pi_dash/db/models/draft.py` (storage model; no user-visible behavior beyond what the rows state) | covered transitively by DRAFT-001, DRAFT-004, DRAFT-020 |

### Sweeps with no draft behavior (no rows; confirmed absent)

- Cloud overlay (`ee-overlay`): no draft-specific code — covered by DRAFT-027.
- Desktop overlay (`desktop-overlay/`): no draft-specific code — covered by DRAFT-027.
- Command palette / keyboard shortcuts: no draft entries or shortcuts — covered by DRAFT-026.
- List filters, search, drag-and-drop, bulk edit, exports/imports, realtime channels: none on the drafts screen — covered by DRAFT-026.
- Empty-state image assets (`app/assets/empty-state/workspace-draft/`, `app/assets/empty-state/draft/`): presentational only, no behavior — covered by DRAFT-022.

