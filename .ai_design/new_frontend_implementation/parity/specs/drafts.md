# Drafts — area spec (DRAFT-001–027)

Status: implementation spec for `apps/web_new`. This is prose, not a translation of the old screen. The old drafts route, row components, modal, stores and services are behavior references only. The new surface must be written independently, may reuse the Issues primitives listed below, and may not import or copy old frontend code, styles, assets, strings or structure.

Inventory: `.ai_design/new_frontend_implementation/parity/inventory/drafts.md` — every row `DRAFT-001` through `DRAFT-027`, all editions (`oss`, `cloud`, `desktop`).
Oracle: `apps/web_new/e2e/parity/drafts/` — 13 files including `support.ts`; every row is tagged in at least one scenario.
Issue ownership: this spec belongs to NEWFRONT-781; slices NEWFRONT-782–787 build it and extend NEWFRONT-643; the area gate NEWFRONT-788 proves it.

## 1. Product model

A draft is a private, workspace-scoped, half-finished work item. It is not a project list item and does not appear in project issue lists, boards, searches, intake or archives. It belongs to the user who created it, may optionally name a project, and may hold the same authoring fields as a work item: title, rich description, state, priority, labels, assignees, start and due dates, cycle, modules and estimate. Descriptions and attachments are stored against the draft until it is promoted.

The drafts screen is a personal workspace surface at `/{ws}/drafts`. It shows only the signed-in user's drafts. Its purpose is to let that user finish, duplicate, discard or publish their own unfinished work. It is not a multi-user review queue, and it has no sharing or per-draft URL.

A draft is promoted by creating a real work item in the draft's project, carrying the draft's fields and description assets, recording a creation activity entry, and deleting the draft. Promotion is only available to workspace members and admins. A draft with no project can be edited, duplicated, moved and deleted, but cannot be promoted until it is assigned a project.

Two kinds of literal text appear deliberately below:

- **Contract text** — API paths, request/response fields, enum values, URL keys and cache keys. These must match the server exactly and are named verbatim.
- **Parity labels** — a short user-visible string the oracle asserts literally (a document title, a button name, a heading, a menu entry, a toast). Each is quoted and marked _pinned_ in §1.1. These strings are test contract, not copied product copy: everything else — layout, wording, icons, artwork, styles — is written new in the product's voice.

## 1.1 Parity labels (pinned)

The oracle reads these strings through user-visible selectors (accessible names, `page.title()`, visible text). A slice must render them exactly as quoted; equivalent wording elsewhere is fine, but these particular spots are contract. A new redesign may restyle the control, but must keep the accessible name/text.

| Where | Pinned string | Oracle |
| --- | --- | --- |
| Document title of the drafts route | `Workspace Draft` | `list.spec.ts`, `non-behaviors.spec.ts` |
| Header/breadcrumb text | contains `Drafts` | `list.spec.ts`, `non-behaviors.spec.ts` |
| Header create control (accessible name) | `Draft a work item` | `create.spec.ts`, driver `draftsHeaderCreateState` |
| Empty-state heading | `Half-written work items` | `empty-loading.spec.ts`, `support.ts` |
| Empty-state primary action | `Create draft work item` | driver `draftsEmptyCreateState` |
| No-project heading | `No project` | `empty-loading.spec.ts`, `support.ts` |
| No-project primary action | `Start your first project` | driver `draftsNoProjectCreateState` |
| Create dialog title placeholder | `Title` | driver `fillCreateTitle` |
| Discard guard heading | `Save this draft?` | `discard.spec.ts`, `create.spec.ts` |
| Discard guard actions | `Save to Drafts`, `Discard`, `Cancel` | `discard.spec.ts`, driver `confirmSaveDraft` |
| Create success toast | contains `Draft created` | `create.spec.ts` |
| Delete dialog heading | `Delete draft` | driver `confirmDraftDelete` |
| Delete dialog actions | `Delete`, `Cancel` | driver `confirmDraftDelete` |
| Delete success toast | contains `Draft deleted` | `duplicate-delete.spec.ts`, `permissions.spec.ts` |
| Publish success toast | contains `Draft published to project.` | `publish.spec.ts` |
| Publish control in the edit dialog | `Publish issue` | driver `publishDraft` |
| Move-mode action | `Add to project` | `edit.spec.ts` |
| Row menu entries, in order | `Edit`, `Make a copy`, `Move to project`, `Delete` | `edit.spec.ts`, DRAFT-009 |
| Priority trigger text | `High`, `Medium`, `None` (the picker's own labels) | driver `draftPriorityText` |

`Untitled` (the blank-title placeholder) and `Project is required` (the promote refusal) are also contract strings; they are server/storage values rather than UI copy and are covered in §4.1 and §8.6.

**Driver key mapping.** The parity driver's `draftRowMenuClick(name, entry)` is called with the *old* i18n keys `edit`, `make_a_copy`, `move_to_project`, `delete`, not with the pinned labels. The `web_new` driver must map those keys to its own menu entries (by order or by a key→label table); the screen itself renders the pinned labels above.

## 2. Screen, shell and route

### 2.1 Route and deep links (DRAFT-024)

The only drafts address is the workspace-scoped collection route `/{ws}/drafts`. The document title is the pinned `Workspace Draft`, and the shell breadcrumb's current item is the pinned drafts label under the workspace shell (§1.1). The route is guarded by the normal workspace gate; an unauthenticated visitor is sent through sign-in, and a visitor without workspace membership is handled by the shell's access gate.

There is no `/{ws}/drafts/{draftId}` route and no per-draft editor page. A URL with a draft id after the collection path must resolve as an unknown route (the app's not-found surface), not as a one-draft view. Drafts therefore cannot be deep-linked, bookmarked individually or shared by URL. Opening a draft is an in-page dialog action.

The document head and the visible collection heading are distinct strings: the head carries the pinned `Workspace Draft`, while the visible heading and breadcrumb carry the pinned `Drafts`. The shell also owns the workspace header and the personal navigation entry for drafts; these are shell behavior, not a drafts-local copy. The command palette entry `shell.go-drafts` (shortcut `gj`) navigates here and belongs to the shell command registry, not to a screen-level shortcut.

### 2.2 Header and count (DRAFT-001, DRAFT-002, DRAFT-003)

The page header has three responsibilities:

- identify the collection with the breadcrumb/heading;
- show the server's total draft count as a compact chip only when the total is greater than zero;
- offer the primary create action, accessible name `Draft a work item`, when the viewer can reach the create flow.

The count chip is hidden at zero and shows the list response's total, not the number of currently loaded rows. It therefore includes drafts on later pages and drafts that the screen can render as project-less rows. It must never count drafts the viewer cannot own: the list is always owner-scoped, so the chip is the viewer's own total. After a successful create, promotion or delete, the chip and list must update consistently; a project-less or unjoined-project draft still counts and still has a row under the fixed behavior in §7.

The create control is visible only when the viewer belongs to at least one non-archived project that they can create work in. It is enabled for workspace admins and members. For a workspace guest who does belong to a project, the control stays visible but disabled. For a workspace member with no joined/visible project, the control is absent; the no-project empty state in §5 owns the guidance instead. The header create control and the empty-state create-project action use the same permission facts but have different visibility: the former is absent without a usable project, the latter is present but disabled for guests.

### 2.3 Page composition and loading (DRAFT-023)

The route shell composes the workspace header/breadcrumb and the drafts content. The screen owns only the collection body, count chip and its actions. It reads the projects the viewer may use and the drafts list independently; while the projects read is still unresolved, the screen shows its loading skeleton rather than briefly asserting the no-project state. Once both reads settle, the state order is:

1. no usable project → no-project state;
2. usable project and zero drafts → empty-drafts state;
3. drafts present → list (with the count chip).

The initial list load shows a skeleton of row placeholders. A held/delayed list request continues to show that skeleton. A page transition or workspace change remounts the view under the new workspace scope and must not show the previous workspace's drafts.

The list is cursor-paginated at fifty drafts per page and ordered newest first by the server. The first page is fetched on mount. If the response says another page exists, the footer offers a load-more control; clicking it appends the next page in order. While the next page is in flight, the footer shows a single-row loading placeholder instead of the control. Loaded pages are flattened in server order and de-duplicated by draft id; the total remains the server total. Reaching the end removes the load-more affordance.

There is no infinite-scroll sentinel, drag reorder, list filter, search box, bulk-selection UI or export control on this screen. Loading more is explicit (DRAFT-023), and the list is not a virtualized or reorderable collection.

## 3. Row anatomy and inline properties

### 3.1 A row (DRAFT-007, DRAFT-008, DRAFT-009)

Each draft is a full-width list row with:

- a project marker when the draft has a project: the project's short identifier, rendered as a compact monospace-style button that copies the identifier to the clipboard and confirms the copy;
- an optional work-type mark for the draft's `type_id`, placed with the project marker (fixed behavior, NEWFRONT-301);
- the draft title, truncated with a hover/focus reveal when long; a null or empty title is presented as the stored placeholder title rather than a blank row;
- the inline property strip described below;
- a row-actions control that is visible as the ellipsis quick action on hover/focus and opens the same menu as right-clicking the row.

A row with no project has no project marker and no click-to-copy affordance; it still has a title, its inline properties that are possible without a project, and row actions. It is a real row, not a silently omitted record (fixed behavior, NEWFRONT-305). A draft in a project the viewer has not joined likewise remains visible as a row with an unresolved/empty project presentation, because the list is owner-scoped and the draft belongs to the viewer; the screen must not make a counted draft unreachable.

Double-clicking anywhere on the row body (but not on an interactive property control or menu item) opens the edit dialog. The row's context menu opens on right-click and on the ellipsis control. Both openers must show the same four translated entries, in this order: Edit, Make a copy, Move to project, Delete. They must never render raw i18n keys (fixed behavior, NEWFRONT-298). A menu entry is a real action, not a label with no outcome.

Row actions are visible/gated by ownership. The list is owner-scoped, so a foreign draft is not normally on the screen; nevertheless every mutating route must enforce ownership/gating server-side, and the UI must not expose a working action that the server will always refuse. The precise role matrix is in §8.

### 3.2 Inline properties (DRAFT-012–019)

The property strip presents the editable values in a stable compact row, in this order: State, Priority, Labels, Start date, Due date, Assignees, and then the project-feature properties in a fixed order: Modules, then Cycle, then Estimate. Only the enabled ones render, but the relative order is fixed so the parity driver's single conditional slot resolves correctly. Each control:

- reads its value from the draft/list payload and project reference data;
- is a dropdown/picker using the shared Issues pickers, not a new drafts-only picker;
- writes one field at a time through `PATCH /api/workspaces/{ws}/draft-issues/{id}/`;
- paints the picked value immediately (optimistic), then keeps it if the write succeeds;
- on failure, restores the prior value and reports a visible error notice; no silent rollback (fixed behavior, NEWFRONT-303);
- stops row double-click/context actions from firing when the user is interacting with the picker.

The controls and their rules:

- **State (DRAFT-012).** Offers workflow states from the draft's project, using project-scoped state data. The trigger shows the draft's stored state, and for a draft that has no stored state yet it may show the project's default state, but the overdue calculation reads only the stored state. A pick patches `state_id`. If the project or project information is unresolved, the control is disabled/locked rather than offering workspace-wide states.
- **Priority (DRAFT-013).** Offers the standard priority levels plus the “none”/no-priority state. The trigger renders the priority label/icon from the shared picker; a pick patches `priority`.
- **Labels (DRAFT-014).** Offers only labels belonging to the draft's project. Multi-select. The trigger shows the selected labels as chips/compact text; a pick patches the complete `label_ids` set.
- **Assignees (DRAFT-015).** Offers only active project members with at least member role, not arbitrary workspace members and not guests. Multi-select. A pick patches the complete `assignee_ids` set.
- **Start date and due date (DRAFT-016).** Two single-day pickers using the shared date picker and the user's week start. Start cannot be after due and due cannot be before start: the calendars disable out-of-range days on each side, and the client validates the range before writing. The server only compares the two when both arrive in the same request, so a date write that could invert the range sends both dates together (§8.4). Clearing a date patches `null`. Valid dates persist across reloads. An inverted range is refused with a visible validation/error notice, and the prior dates stay.
- **Modules (DRAFT-019).** Rendered only when the draft's project has module views enabled. Offers that project's modules, multi-select; a pick patches the complete `module_ids` set.
- **Cycle (DRAFT-018).** Rendered only when the draft's project has cycle views enabled. Offers that project's cycles; a pick patches `cycle_id`, clearing patches `null`.
- **Estimate (DRAFT-017).** Rendered only when the draft's project has an estimate system enabled and that system exposes selectable points. The trigger renders the selected estimate point; a pick patches `estimate_point` with the point id and clearing patches `null`. A project without estimates has no estimate control at all. The fixed behavior is that an enabled project must actually offer the picker; do not repeat the old defect where the screen only had the project-list payload and could not see the estimate link (NEWFRONT-300).

The feature controls must be driven by the same project feature facts the Issues surface uses (`cycle_view`, `module_view`, and the project's estimate link/system). They must not infer enablement from the presence of any workspace-wide records. Project-less drafts have no project-scoped pickers; their state/priority/labels/assignees/dates/cycle/modules/estimate controls are unavailable/locked until a project is chosen through the move/publish flow. The row still renders.

### 3.3 Overdue signal

A due date is shown with the overdue treatment only when the stored state's workflow group is neither completed nor cancelled and the due day is strictly before today in the user's calendar. A draft with no stored state does not read overdue merely because the trigger displays a project default. The date display must remain readable without colour alone (for example an accessible label/tone in addition to colour).

## 4. Create, edit, copy, move and delete

All modal flows use the shared `WorkItemCreateDialog` seam (§6). Drafts must not create a second work-item form or a drafts-only copy of the picker/editor stack.

### 4.1 Create a draft (DRAFT-003, DRAFT-004, DRAFT-006)

The header create control opens the shared create dialog in draft mode. The dialog heading and submit label identify saving a draft rather than filing a work item. The form offers the same authoring fields as the work-item create form, subject to the draft's project choice: project, title, description, state, priority, labels, assignees, dates, cycle, modules, estimate, parent and the shared advanced fields where applicable. Any project choice is restricted to projects the user can create work in; a draft may also have no project (see §7/§8), but the header entry point is absent for a user with no usable project.

On direct submit, a non-empty title is trimmed and stored. A blank or whitespace-only title is not a validation failure in draft mode: it stores the canonical placeholder title `Untitled` (fixed behavior, NEWFRONT-302). The same defaulting applies to the keep-as-draft close path. The create request carries the fields the user actually set, omitting empty values; `project_id` is omitted when no project is chosen. A successful create:

- writes `POST /api/workspaces/{ws}/draft-issues/`;
- shows a success notice containing `Draft created` (§1.1);
- closes the dialog and resets it, so reopening starts blank;
- adds/refreshes the row at the top of the list (newest first) and updates the count.

A failed create keeps the dialog open, shows an error, and does not add a phantom row. A draft created through the dialog uses the draft asset scope for description attachments.

The header create dialog is an entry point only; it is not opened by a screen-level keyboard shortcut. The shell's global create command may open the broader work-item create flow, but the drafts page itself has no custom shortcut beyond the shell's navigation command.

### 4.2 Edit a draft (DRAFT-008)

Double-click or the row menu's edit action opens the same dialog in stored-draft mode, seeded from the draft row (including description and memberships) and its project. The dialog title/submit label identify an update of the draft, not a new issue. Changing fields and submitting patches the draft through `PATCH /api/workspaces/{ws}/draft-issues/{id}/`; the server answers no content, so the page must re-read the list or update its cached row before showing the saved values. On success the row updates in place and remains in list order; on failure the dialog stays open with an error and the row is unchanged.

Editing an existing draft never shows the unsaved-new-item discard prompt merely because the dialog was closed: the draft already exists, so closing without changes is harmless. A subsequent explicit save is what persists edits. The project of an existing draft is not silently replaced by the first available project; the draft keeps its stored project unless the user uses the move/publish flow to choose another.

### 4.3 Keep-as-draft and discard (DRAFT-005, DRAFT-006)

Closing a _new_ work-item dialog that contains unsaved, non-empty work opens the shared discard guard, headed `Save this draft?` with `Discard`, `Cancel` and `Save to Drafts` (§1.1). Non-empty means the sanitized form has meaningful content: a title, non-empty description beyond editor scaffolding, or a changed property/membership. Empty/whitespace-only title and empty editor content do not count by themselves. The guard offers:

- **Keep as draft:** store the current form as a draft, defaulting a blank title to `Untitled`, then close the guard and the create dialog. The saved draft shows the normal create/keep confirmation and appears in the list.
- **Discard:** close the guard and the create dialog without a write. Nothing is stored.
- **Cancel:** dismiss the guard and return to the form with its content intact.

Closing an untouched create dialog with no meaningful content closes silently, with no guard and no request. Closing an existing draft editor does not open this guard. A failed keep-as-draft write keeps the guard/form available with an error; it must not close as if saved or leave the user unaware.

The draft mode submit itself (direct save) and the guard's save path share the same payload construction and blank-title rule. A guest may create through the API, but the drafts screen's header create control stays disabled for guests; keep-as-draft from a flow the guest can reach must obey the server's role rules.

### 4.4 Make a copy (DRAFT-010)

The copy action opens the shared create dialog in draft mode, prefilled from the source row and with a copy marker appended to the title. The marker is the canonical ` (copy)` suffix required by the oracle; new UI copy for the action label itself is written new. The seed carries the source's project, description, state, priority, labels, assignees, dates, cycle, modules, estimate and parent where present. Saving creates a new draft through `POST /api/workspaces/{ws}/draft-issues/`; it does not mutate the source. The copy then appears at the top of the list with the same project marker and property values as the source. Duplicating must not drop cycle/module/estimate values merely because they are project-feature fields.

The prefilled copy is still a new draft: its close behavior follows the create/keep/discard rules, not the stored-draft edit rule. A copy inherits the source's project; it must not be re-homed silently.

### 4.5 Move to project and publish (DRAFT-020, DRAFT-021)

The row menu's move-to-project action opens the shared edit dialog for the stored draft in move mode. It shows a distinct add-to-project action labeled `Add to project` (not the normal draft save label) and lets the user choose the target project where allowed (§1.1). The edit dialog's regular publish action, labeled `Publish issue`, is also available for a stored draft with a project. Both paths end in `POST /api/workspaces/{ws}/draft-to-issue/{draftId}/` with the draft's current field payload.

Promotion is atomic from the user's perspective: the server creates the real work item in the draft's project, carries title, description HTML, state, priority, labels, assignees, start/due dates, estimate point, cycle and modules, moves description file assets from the draft to the issue, records a creation activity entry, and deletes the draft. The response is the created issue shape (including its id, project, sequence and stored fields). On success the dialog closes, a success notice containing `Draft published to project.` identifies that the draft was published to the project (§1.1), the row disappears, and the count decreases by one. The promoted item is then visible through the normal project issue surfaces, not the drafts list.

No project: the UI must not silently publish into the first project or no-op. It must refuse with a clear missing-project error and leave the draft intact. The API also refuses with HTTP 400 and the canonical error text `Project is required`; no work item is created. A project-less draft must still have a row (fixed behavior, NEWFRONT-305) with an empty project slot and a working move/edit path so the user can assign a project. A draft in an unjoined project is likewise still rendered; the user may be unable to use some project pickers until they can access the project, but the draft is not invisible.

A failed promotion keeps the dialog open with an error and keeps the draft row. Do not remove the row optimistically before the server confirms.

### 4.6 Delete (DRAFT-011)

The row menu's delete action opens a confirmation dialog headed `Delete draft` (§1.1), naming the draft and warning that the action cannot be undone; its actions are `Delete` and `Cancel`. The primary action confirms removal; cancel/Escape closes without a request. While deleting, both actions are locked and the primary shows a deleting state.

The standing is creator-or-workspace-admin. The draft's creator may delete it at any role, including guest; a workspace admin may delete any draft. A non-creator non-admin is refused. Because the list is owner-scoped, a foreign draft is normally unreachable from the UI; the UI must still avoid offering an action the server will refuse, and the server must enforce the same rule. The current 643 client gate uses project-admin role for this check; that is wrong for drafts and must be corrected by this flow to workspace-admin (a creator still qualifies). A project-only admin who is not a workspace admin must not be treated as authorized.

On success: remove the row, decrement the count, show a success notice containing `Draft deleted` (§1.1), and refresh/invalidate the list. On a 403 or the server's creator-or-admin refusal, show a distinct permission notice and leave the row. On any other failure, show a generic delete-failed notice and leave the row. The delete request is `DELETE /api/workspaces/{ws}/draft-issues/{id}/` and answers no content.

## 5. Empty, no-project and error states

### 5.1 Empty drafts (DRAFT-022)

When the viewer has a usable project and the list has settled with zero drafts, render a dedicated empty state: a drafts-specific illustration (newly drawn, not an old asset), the pinned heading `Half-written work items` (§1.1), a newly written supporting sentence (not copied from the old app), and a primary action named `Create draft work item`. The action opens the same draft create dialog as the header. It is enabled for workspace members/admins and disabled for guests, matching the header gate. The count chip is absent.

The empty state must not be used while the list is loading, while the projects read is unresolved, or when there are drafts that cannot currently be rendered as rows. Under the fixed NEWFRONT-305 behavior, a counted draft always has a row, so “empty” means a true zero total.

### 5.2 No project

When the viewer has no usable (joined, non-archived) project, the list body shows a no-project state: a project-oriented illustration/mark, the pinned heading `No project` (§1.1), a newly written supporting sentence explaining that creating or joining a project is required (not copied from the old app), and a primary action named `Start your first project`. The action opens the workspace create-project dialog. It is enabled for workspace admins/members and disabled for guests. The header create control is absent in this state. The no-project state takes precedence over the empty-drafts state only when the list is at zero. If the viewer still owns drafts (project-less, or in projects they have not joined), those drafts keep their rows and the screen shows the guidance without hiding them: a counted draft is never invisible (NEWFRONT-305). In practice this means the body renders rows whenever the total is nonzero, and the no-project guidance sits above or beside them rather than replacing the list.

“Usable project” follows the viewer's own project visibility: members typically see all workspace projects, guests see only joined projects, archived projects do not count. The state must not be driven by a workspace-wide project count that ignores the viewer's membership.

### 5.3 Error behavior

A failed initial list fetch must not masquerade as an empty drafts collection. Show a retryable error/empty-with-retry surface that keeps the shell and distinguishes a load failure from a true zero-draft state. A failed next-page fetch keeps the already loaded rows and leaves the load-more control available to retry; it must not discard pages or duplicate rows. A failed project read leaves the screen in loading/error rather than wrongly asserting no project.

## 6. NEWFRONT-643 overlap (binding)

NEWFRONT-643 (“Issues slice: drafts”, ISS-137–141) merged as squash `351d37ea1`. It built the drafts page under `apps/web_new/src/features/issues/drafts/` and the route `apps/web_new/src/routes/$ws/_gated/drafts/`. The Drafts epic slices must extend that code; no row may be built twice and no component may be forked. The table below is the ownership/overlap decision for every DRAFT row. “Built” means 643 has the basic behavior and the Drafts slice is a verification/extension task; “partial” means a working base exists but a listed gap remains; “missing” means 643 has no implementation of the capability. Each row is assigned to exactly one Drafts epic slice.

| Row       | What 643 already provides                                                                                                                                     | Remaining work / slice decision                                                                                                                                                                            | Owning slice                                      |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| DRAFT-001 | Owner-scoped, newest-first list; 50/page cursor query; workspace route; basic heading and shell entry                                                         | Verify project count, breadcrumb/title composition and no cross-user leakage against the area oracle; retain the list/query implementation                                                                 | NEWFRONT-782                                      |
| DRAFT-002 | Total-count chip hidden at zero, rendered above zero                                                                                                          | Verify total is server total (not loaded count), updates after create/promote/delete, and includes project-less rows; add any missing unit branches                                                        | NEWFRONT-782                                      |
| DRAFT-003 | Header create, enabled for members/admins with projects, disabled for guests, absent without a project                                                        | Verify the visibility/enablement matrix and that the control opens the draft dialog; no new component                                                                                                      | NEWFRONT-782                                      |
| DRAFT-004 | Draft-mode create through the shared dialog; success notice; closes/resets; list refresh                                                                      | Verify top-of-list placement, reset, failure handling and project/field payload; extend tests only                                                                                                         | NEWFRONT-784                                      |
| DRAFT-005 | Shared create-dialog discard guard with keep/discard/cancel; blank-form silent close                                                                          | Verify dirty detection, no-write discard, keep-as-draft write and failure state; extend the shared dialog tests rather than forking                                                                        | NEWFRONT-784                                      |
| DRAFT-006 | Blank direct submit defaults to `Untitled`; keep-as-draft path already defaults                                                                               | Verify both direct submit and guard save for empty/whitespace titles; this is the NEWFRONT-302 fixed behavior                                                                                              | NEWFRONT-784                                      |
| DRAFT-007 | Project marker and title; click-to-copy identifier                                                                                                            | Add the work-type mark from `type_id` (NEWFRONT-301), including null type handling; verify marker/title composition                                                                                        | NEWFRONT-783                                      |
| DRAFT-008 | Double-click edit and row-menu edit; prefilled stored draft; PATCH; list refresh                                                                              | Verify both opener paths, stored project retention and save/failure behavior across the shared dialog                                                                                                      | NEWFRONT-783                                      |
| DRAFT-009 | Ellipsis and context menu with the four actions; translated labels in new code                                                                                | Verify both openers expose identical labels/order/actions and never raw keys (NEWFRONT-298); ensure every action routes to its flow                                                                        | NEWFRONT-785                                      |
| DRAFT-010 | Copy seed carries project, description, state, priority, labels, assignees, dates, cycle, modules, estimate, parent; suffixed title; POST creates a new draft | Verify marking/carry-over, especially project-feature fields, and add coverage for any missing field                                                                                                       | NEWFRONT-785                                      |
| DRAFT-011 | Confirmation dialog, creator/project-admin client gate, permission/generic errors, success toast and row removal                                              | Correct the client gate to creator-or-workspace-admin as part of this flow; verify server refusal and guest-created deletion; NEWFRONT-787 independently proves the same rule in the matrix                | NEWFRONT-785                                      |
| DRAFT-012 | Inline state picker; optimistic PATCH; rollback; error toast in new code                                                                                      | Verify rollback + visible error (NEWFRONT-303), project-scoped state options, default-display rule and persistence                                                                                         | NEWFRONT-783                                      |
| DRAFT-013 | Inline priority picker; optimistic PATCH; persistence                                                                                                         | Verify options, immediate paint and reload persistence; add any missing branch tests                                                                                                                       | NEWFRONT-783                                      |
| DRAFT-014 | Inline project-scoped label picker; complete label set PATCH                                                                                                  | Verify only source-project labels are offered and selected labels persist                                                                                                                                  | NEWFRONT-783                                      |
| DRAFT-015 | Inline project-member assignee picker; complete assignee set PATCH                                                                                            | Verify member+ active project members only, guests excluded, and persistence                                                                                                                               | NEWFRONT-783                                      |
| DRAFT-016 | Start/due date pickers, min/max and range disabling, PATCH, overdue tone                                                                                      | Verify valid save/reload, inverted-range refusal and old values retained, plus week-start behavior                                                                                                         | NEWFRONT-783                                      |
| DRAFT-017 | Conditional estimate picker; reads project estimate link and estimate points                                                                                  | Verify enabled projects actually show/persist the picker (NEWFRONT-300) and disabled projects show none; fix any project-detail data-source gap                                                            | NEWFRONT-786                                      |
| DRAFT-018 | Conditional cycle picker in the property strip                                                                                                                | Verify cycle-view gating, project cycles only, attach/clear persistence                                                                                                                                    | NEWFRONT-786                                      |
| DRAFT-019 | Conditional module picker with multi-select                                                                                                                   | Verify module-view gating, project modules only, complete-set persistence                                                                                                                                  | NEWFRONT-786                                      |
| DRAFT-020 | Edit-dialog publish/move paths; promote write; success toast; row/count removal; asset hand-off                                                               | Verify full field/asset/activity carry-over and the project issue result; extend tests but do not fork the dialog                                                                                          | NEWFRONT-784                                      |
| DRAFT-021 | Project-less draft row exists; UI refuses publish without project; server 400 is surfaced                                                                     | Verify the refusal leaves the draft and creates nothing; ensure move-to-project can assign a project                                                                                                       | NEWFRONT-784                                      |
| DRAFT-022 | Empty and no-project states with role-gated actions; project creation dialog                                                                                  | Verify state precedence, artwork is original, guest/member action states, and loading does not flash the empty state                                                                                       | NEWFRONT-782                                      |
| DRAFT-023 | Cursor list, 50/page, explicit load-more control, footer loader, skeleton                                                                                     | Verify delayed skeleton, 51-row append/no duplicates, end-of-list behavior and failed-next-page retry                                                                                                      | NEWFRONT-782                                      |
| DRAFT-024 | `/{ws}/drafts` route with a document head; no single-draft route                                                                                                | Change the document title to the pinned `Workspace Draft` (643 ships `Drafts` today, which fails the oracle); verify breadcrumb/header text, no per-draft deep link, and unknown-id URL behavior | NEWFRONT-782                                      |
| DRAFT-025 | Base list/create/edit/delete/publish and basic role facts                                                                                                     | Verify the complete matrix; backend creator bypass for retrieve/PATCH is still NEWFRONT-299, and the corrected delete standing must agree with the workspace-admin rule; do not claim green until verified | NEWFRONT-787                                      |
| DRAFT-026 | Standard list with explicit load-more and no filter/bulk/export features; app query defaults disable focus refetch                                            | Verify absence of shortcuts/drag/bulk/filter/export/realtime, focus-safe no-refetch and outside changes appearing only on fresh read                                                                       | NEWFRONT-782                                      |
| DRAFT-027 | Default surfaces use the same route/components/endpoints; no edition-specific branch                                                                          | Verify in OSS, cloud and desktop, plus no overlay-only divergence; no edition-specific drafts code is allowed                                                                                              | NEWFRONT-782 (smoke), proved by NEWFRONT-788 gate |

Rules for extending 643:

- Add exports to the existing drafts feature barrel/module; do not copy its files into a second drafts feature.
- Extend `DraftProperties`, `DraftBlock`, `DraftsPage`, `DeleteDraftDialog`, `DraftEmptyStates`, `api.ts`, `model.ts` and the route in place.
- Shared modal changes go in `WorkItemCreateDialog` and the create/discard modules owned by NEWFRONT-630/631/632; drafts call them through props and helpers.
- Preserve the existing query key family (`["ws", ws, "issue-drafts", "list"]` or its current equivalent) and extend cache updates rather than introducing a parallel drafts cache.
- If a row’s implementation turns out to be missing rather than partial, the owning slice adds it to the existing files. It does not create a competing implementation.

## 7. Fixed behavior against old bugs

These are target requirements, not oracle-current behavior. The oracle scenarios named “bug:” pin the old behavior until the linked fix lands; the new app must implement the intended column below. The owning slice must include tests for the fixed behavior.

| Issue        | Old behavior                                                                                                            | Required behavior in `apps/web_new`                                                                                                                                                                                                                                                       | Owning slice                                                   |
| ------------ | ----------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| NEWFRONT-298 | Row menus render raw i18n keys (`edit`, `make_a_copy`, `move_to_project`, `delete`)                                     | Both menu openers show translated labels in the required order: Edit, Make a copy, Move to project, Delete; no raw keys or missing labels                                                                                                                                                 | NEWFRONT-785                                                   |
| NEWFRONT-299 | Backend creator bypass points at work items, so a guest/member is refused an owned draft PATCH or GET                   | Admin-or-creator may retrieve and PATCH a draft; a guest may edit/fetch/delete their own draft; cross-user requests remain refused. The API must be fixed and the UI must use the correct ownership/role facts. Do not claim DRAFT-025 green on the old API behavior                      | NEWFRONT-787                                                   |
| NEWFRONT-300 | Estimate picker never renders on estimate-enabled projects because the screen's project payload lacks the estimate link | An estimate-enabled project offers the estimate picker and persists the chosen point; a project without estimates shows no control. Resolve the project-detail/estimate system data needed by the row                                                                                     | NEWFRONT-786                                                   |
| NEWFRONT-301 | Rows carry no work-type mark because the old type-identifier renders an empty fragment                                  | A row with `type_id` shows the corresponding work-type mark beside the project marker; a row without one shows no false mark; the title remains present                                                                                                                                   | NEWFRONT-783                                                   |
| NEWFRONT-302 | Direct blank-title submit fails title validation and stores nothing                                                     | Direct submit of an empty/whitespace title stores a draft with the placeholder title `Untitled`; the keep-as-draft path does the same; no title-required error in draft mode                                                                                                              | NEWFRONT-784                                                   |
| NEWFRONT-303 | A rejected inline save snaps the value back with no notice                                                              | A rejected inline save restores the prior value and shows a visible error notice; the server's stored value stays unchanged                                                                                                                                                               | NEWFRONT-783                                                   |
| NEWFRONT-305 | Project-less or unjoined-project drafts count toward the total but render no row                                        | Every counted draft renders a row. A project-less row has an empty project slot and working move-to-project/edit actions; an unjoined-project draft remains visible to its owner. Total and rendered reachability never disagree. If a project marker cannot resolve, do not drop the row | NEWFRONT-782 (row existence), NEWFRONT-784 (move/publish path) |

These fixes may require changes outside the drafts feature: NEWFRONT-299 is an API permission fix, and NEWFRONT-300 may need a project-detail query or a contract field. Those changes belong to the named slices/issues, not to this spec issue. This spec records the target so a slice cannot “pass” by enshrining the old bug.

## 8. API surface

All paths are under the API base URL and use the workspace slug (`{ws}`) unless stated. All authenticated calls require an active workspace membership at the stated level. The list and detail responses are the `DraftIssue` contract in `packages/api-client/src/contracts/issue-drafts.ts`; writes use `DraftIssueCreateInput`. The response carries, at minimum: `id`, `name`, `project_id`, `description_html`, `state_id`, `priority`, `parent_id`, `assignee_ids`, `label_ids`, `start_date`, `target_date`, `cycle_id`, `module_ids`, `estimate_point`, `type_id`, `sort_order`, `created_at`, `updated_at`, `created_by`, `updated_by`, `completed_at`. The screen reads `type_id` for the work-type mark, `project_id` for the marker/project features, `description_html` for edit/copy, module/cycle ids for memberships and estimate point for the estimate control. The list envelope additionally carries `total_count`, `next_cursor`, `prev_cursor`, `next_page_results`, `prev_page_results`, `count` and `results`.

### 8.1 `GET /api/workspaces/{ws}/draft-issues/`

- **Who:** workspace admin, member or guest. The query is always `created_by = the authenticated user`, regardless of role.
- **Parameters:** cursor pagination (`cursor`, `per_page`; the screen uses `per_page=50`). Ordered `-created_at`. The endpoint forwards the request's query parameters through the shared issue-filter parser, so filters such as `project`, `state`, `priority`, labels and assignees are accepted; owner scoping is applied afterwards as `created_by = the authenticated user`, so no filter can widen the result to another user's drafts. The drafts screen sends no filters and must not add filter UI.
- **Response:** the paginated envelope above, with each draft serialized as the list projection. `next_page_results` plus `next_cursor` decide whether the load-more control appears.
- **Errors:** unauthenticated or non-member → the standard auth/permission error; an invalid cursor/per_page → a 400 validation error. The screen treats a transient failure as retryable, not as an empty list.
- **Reads used by the screen:** owner scoping, newest-first order, total, next page, row identity, `type_id`, project, memberships/estimate, and description for the row/edit flows.

### 8.2 `POST /api/workspaces/{ws}/draft-issues/`

- **Who:** workspace admin, member or guest at the API level. The screen's header action additionally disables guests and hides for project-less users, but that is a UI gate, not an API permission replacement.
- **Body:** `DraftIssueCreateInput`. `project_id` is optional; a draft may be project-less. `name` is optional at the contract; the UI supplies `Untitled` for a blank/whitespace draft title. `description_html`, state, priority, assignees, labels, dates, cycle, modules, estimate and parent are optional. The create endpoint does not take a separate membership bridge; cycle/modules/estimate ride the create payload.
- **Validation:** state, labels, assignees, estimate points and parent must belong to the supplied project; assignees must be active project members with at least member role. `cycle_id` and `module_ids` are stored as given and are *not* project-checked by the server, so the UI must offer only the draft project's cycles and modules; do not describe the server as enforcing that scoping. A start date after target date is rejected. Description HTML is sanitized; invalid content/assets are rejected. A project-less payload cannot carry project-scoped relationships meaningfully and must not create cross-project links.
- **Response:** HTTP 201 with the stored draft row (including `type_id`, project, description, memberships and estimate). The screen uses the id to refresh and, for dialog saves, resets/opens the new row. Failure is 400 with field errors; the dialog stays open and the row is not added.

### 8.3 `GET /api/workspaces/{ws}/draft-issues/{id}/`

- **Who:** workspace admin or the draft's creator. This is the intended admin-or-creator rule (NEWFRONT-299); a non-creator member/guest is refused. The endpoint is not used for a per-draft page; it exists for detail hydration/edit support.
- **Parameters:** the draft UUID in the path. Owner scoping is also enforced in the query for non-admin callers; an absent/foreign draft must not leak existence beyond the standard permission/not-found behavior.
- **Response:** the full `DraftIssue` including `description_html` and all fields the edit dialog needs (type_id, project, module/cycle ids, estimate point).
- **Errors:** 403/404 as above. The UI must not depend on this call for the owner-scoped list to work, especially while NEWFRONT-299 is unresolved; list/edit can hydrate from the list row.

### 8.4 `PATCH /api/workspaces/{ws}/draft-issues/{id}/`

- **Who:** workspace admin or the draft's creator (intended; see NEWFRONT-299). A guest may patch their own draft; a non-creator cannot patch another user's draft. The implementation must use draft rows for the creator bypass, not issue rows.
- **Body:** any subset of `DraftIssueCreateInput`, including `project_id` when the move flow assigns a project. Inline writes send one field: `state_id`, `priority`, `label_ids`, `assignee_ids`, `start_date`, `target_date`, `cycle_id`, `module_ids` (complete set) or `estimate_point` (`null` clears). Labels/assignees/state/estimate/cycle/modules are validated against the resulting project. The server compares `start_date` and `target_date` only when both are present in the same request, so a one-field date write is not range-checked against the stored counterpart. The UI therefore validates the range itself before writing and sends both dates together whenever a change could invert the range; the delete/clear path may send `null` for one side. Do not rely on server validation for a partial date write. Description HTML is sanitized.
- **Response:** HTTP 204, no body. The screen must apply the successful value to its cache or re-read; a 400/403/500 restores the prior value and shows an error notice. A missing/foreign draft is 404/403 and must not be presented as a successful save.

### 8.5 `DELETE /api/workspaces/{ws}/draft-issues/{id}/`

- **Who:** workspace admin or the draft's creator. A guest may delete their own draft; a member/guest cannot delete another user's draft; a project-only admin does not qualify unless also a workspace admin.
- **Parameters:** the draft UUID. Deletion is a soft delete: the draft's `deleted_at` is set and related draft rows are soft-deleted, so the default manager hides it from the list and detail reads. The UI confirms first.
- **Response:** HTTP 204 with no body. The UI removes the row, decrements the count and refreshes. 403 → permission notice; other failures → generic delete failure; in both cases the row remains.
- **Note:** the current backend delete gate is draft-model creator-or-admin; the UI must mirror workspace-admin, not project-admin.

### 8.6 `POST /api/workspaces/{ws}/draft-to-issue/{draftId}/`

- **Who:** workspace admin or member. Guests are refused even for their own draft. The source draft must belong to the caller's workspace. The server's draft lookup is workspace-scoped rather than owner-filtered, so a member/admin can promote any draft in the workspace by id; the owner-scoped UI never exposes a foreign draft, and cross-user promotion is not a user-facing capability. Do not widen the UI to it. Any future ownership tightening must match the other draft writes.
- **Body:** the issue fields to create, built from the draft's current values: `name`, `description_html`, `state_id`, `priority`, `label_ids`, `assignee_ids`, `start_date`, `target_date`, `estimate_point`, `cycle_id`, `module_ids` and `parent_id`. Project is taken from the draft; a client must not smuggle a different workspace/project. The description assets attached to the draft are moved to the created issue.
- **Validation/refusal:** if the draft has no `project_id`, return HTTP 400 with an error containing `Project is required`; create nothing and keep the draft. All issue validations (project-scoped state/labels/assignees, date order, estimate/parent) apply. A guest/foreign caller is refused with the standard permission error.
- **Response:** HTTP 201 with the created issue's create-serializer shape: `id`, `name`, `project_id`, `sequence_id`, `state_id`, `priority`, `sort_order`, `parent_id`, `description_html`, timestamps. The server also records the creation activity and moves assets before deleting the draft. The screen uses project/id to invalidate project surfaces, removes the draft row and count, and shows the pinned publish success notice (§1.1).
- **Failure:** 400/403 keeps the draft and row; the dialog stays open with a clear error. There is no silent success or optimistic row removal.

### 8.7 Project reference reads used by the screen

The screen needs the viewer's project list/details to resolve project marker, project role, feature flags (`cycle_view`, `module_view`), estimate enablement/system, and the project's states/labels/members/cycles/modules. These are shared reference reads owned by the Issues/Projects layers and must follow the same query keys and cache policy as the rest of `apps/web_new`. Drafts must not define competing picker endpoints or fetch all workspace records to fake project scoping.

## 9. Permissions matrix (DRAFT-025)

| Viewer                  | List            | Create via API | Header create control                                           | Edit own draft               | Edit others' draft                                            | Single fetch own | Delete own | Delete others'        | Promote own                | Promote others'                                                    |
| ----------------------- | --------------- | -------------- | --------------------------------------------------------------- | ---------------------------- | ------------------------------------------------------------- | ---------------- | ---------- | --------------------- | -------------------------- | ------------------------------------------------------------------ |
| Workspace admin         | Own drafts only | Yes            | Enabled if a usable project exists                              | Yes                          | Intended admin bypass; UI normally cannot reach a foreign row | Yes              | Yes        | Yes (workspace admin) | Yes                        | Yes only if the draft is accessible under the same workspace rules |
| Workspace member        | Own drafts only | Yes            | Enabled if a usable project exists                              | Yes (intended, NEWFRONT-299) | No                                                            | Yes (intended)   | Yes        | No                    | Yes                        | No                                                                 |
| Workspace guest         | Own drafts only | Yes (server)   | Visible but disabled if they have a project; absent without one | Yes (intended, NEWFRONT-299) | No                                                            | Yes (intended)   | Yes        | No                    | No (guest convert refused) | No                                                                 |
| Non-member / signed-out | No              | No             | Not rendered                                                    | No                           | No                                                            | No               | No         | No                    | No                         | No                                                                 |

Project role never widens draft ownership except that a workspace admin is also authorized. A project admin who is only a workspace member does not get cross-user draft fetch/edit/delete. A guest who created a draft may edit/fetch/delete it under the intended API fix, but may not promote it. The UI's disabled header control for guests must not be implemented by removing the guest's ability to manage drafts they already created; the rows still expose their permitted own-draft actions.

The response to an unauthorized cross-user request is a permission refusal (403) and must not reveal/edit/delete the draft. The permissions slice NEWFRONT-787 must prove the fixed server behavior for owned PATCH/GET (NEWFRONT-299) and verify the client delete gate is workspace-admin rather than project-admin; it must not simply preserve the oracle's bug behavior.

## 10. Reuse seams

Every seam below is owned by an Issues slice. Drafts call the existing component/helper; it does not fork or duplicate the behavior.

| Seam                                               | Owner                                                    | What drafts calls                                                                                                                         | What the Issues slice owns                                                                                                                   | What drafts must not duplicate                                                     |
| -------------------------------------------------- | -------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `WorkItemCreateDialog`                             | NEWFRONT-630 (create modal core, ISS-117–123)            | Create in draft mode; stored-draft edit; copy seed; move/publish mode; publish action                                                     | Dialog shell, form fields, project selector, title rules, create-more, discard-to-draft plumbing, parent attach, submit/loading/error states | A second form, a drafts-only title rule, draft pickers, or a cloned publish button |
| Modal context/edit/description/advanced            | NEWFRONT-631                                             | Edit target/snapshot, description editor/asset scope, advanced fields, publish for stored drafts                                          | Modal shell, edit prefill/diff/save, description editor and attachment handling, context/advanced sections                                   | A drafts-only editor, a snapshot diff engine, or an independent asset flow         |
| Modal extras / archive / delete / move-to-project  | NEWFRONT-632                                             | The shared move-to-project affordance/dialog pattern and success/failure semantics; delete-style confirmation primitives where applicable | Archive/delete/move dialogs, creator-or-admin behavior, destination picker, success/error handling                                           | A second move-to-project dialog or a duplicate delete confirmation                 |
| State/priority/label/member/cycle/module dropdowns | NEWFRONT-615                                             | Inline property pickers and their project-scoped option reads                                                                             | Picker primitives, option selection, search/multi-select, disabled/empty states, accessible keyboard behavior                                | Drafts-specific dropdowns or a parallel option-query layer                         |
| Date range/pickers                                 | NEWFRONT-616                                             | Start and due date controls, min/max ordering, week-start, clear behavior                                                                 | Date/date-range primitives and calendar behavior                                                                                             | A drafts-only calendar or date parsing rules                                       |
| Estimate picker                                    | NEWFRONT-744 (estimate slice; Contracts C / picker seam) | Conditional estimate control and point persistence                                                                                        | Estimate picker, point options, project estimate-system reads                                                                                | A second estimate picker or a local “enabled” heuristic based on other projects    |
| Draft list/query/cache                             | NEWFRONT-643 (extended by 782–787)                       | Infinite list, cache paint/rollback, row/count removal, invalidation                                                                      | Draft query keys, cursor paging, optimistic patches, create/delete/promote mutations                                                         | A parallel drafts query/cache or a second pagination implementation                |

The shared dialog needs draft-specific entry props (a draft seed/copy payload, an edit target kind `draft`, a move-to-project mode and a caller-owned publish/copy save). Those props and their tests belong to 643/the dialog owners; Drafts slices extend them. The dialog's publish write may be caller-owned on the drafts page so the page can update its cache and toast consistently; the dialog must not also write a competing result.

## 11. App behavior, query conventions and design

### 11.1 Shell/breadcrumb and page title

The route is inside the workspace `_gated` shell. The shell owns the workspace header, workspace switcher/context, personal Drafts navigation entry, command registry and breadcrumb infrastructure. The drafts page supplies its current crumb/title and document head. It must not construct a second app header or hardcode workspace identity. The visible heading and breadcrumb read the pinned `Drafts`; the document title is the pinned `Workspace Draft` (§1.1). The shell route gate handles sign-in, workspace access and onboarding redirects before the page mounts.

### 11.2 Permissions and profile reads

Components read roles through `core/session` (`usePermissions(workspaceSlug)` and `useMe()`), never raw role numbers in the drafts feature. `usePermissions` exposes `isMember`, `canEdit` (member+), and `isAdmin` derived from the workspace role. Project-specific role facts come from the shared project details/query; they never replace the workspace role for draft ownership. User/timezone/week-start preferences come from the shared session/profile surfaces. Drafts must not duplicate session/auth state or read the token.

### 11.3 TanStack Query keys and pagination

The list is one `useInfiniteQuery`/equivalent under the workspace-scoped drafts key family, e.g. `draftKeys.list(ws) = [...workspaceScope(ws), "issue-drafts", "list"]`. `workspaceScope(ws)` is `["ws", ws]`; workspace changes and sign-out invalidate the whole scope. The page requests `per_page=50`; `initialPageParam` is the empty cursor; `getNextPageParam` returns `next_cursor` only when `next_page_results` is true and the cursor is nonempty. Flatten pages in order and de-duplicate by id. The total comes from the server envelope's total count on the latest page; it is not the flattened row count.

Writes update cached pages optimistically where safe:

- inline field write: patch the one draft in every cached page holding it; on error restore a snapshot and show an error;
- delete/promote: remove the draft from every page and decrement the total only after the server confirms (or perform the cache removal in `onSuccess`); then invalidate the list to reconcile;
- create: on success invalidate/re-read the list so the newest draft joins the top; do not insert a client-only row that the server did not store.

Reference data (projects, states, labels, members, cycles, modules, estimates) uses the shared picker/reference queries and policies, with project-scoped keys. Keep `refetchOnWindowFocus: false` and `refetchOnReconnect: false` from the app QueryClient (the parity non-behavior): external writes appear only on a fresh read/remount, while the owner's own mutations may refetch. The list stale time follows the shared list policy; do not add a realtime subscription or polling loop.

### 11.4 Count chip, empty-state and visual treatment

The count chip is a small neutral badge adjacent to the heading, shown only above zero; it is not a button and does not imply a filter. It shows the server total and updates with successful mutations. Use the kit badge/token styling, not old CSS.

Empty and no-project states use the kit `EmptyState`/equivalent, original artwork built from neutral shapes/icons and tokens, a concise new explanation, and one clear primary action. Do not copy the old illustrations, copy, or class strings. The empty state action and the header action open the same draft dialog; the no-project action opens the shared create-project dialog. All states must be keyboard reachable and have visible focus.

The page uses the dense/neutral app visual language (tokens, 4px spacing grid, 13px base text, 28/32px row/control density, hairline borders, indigo accent). The row is a flat list item with a hover surface, not a card grid. The menu/popovers use kit Menu/Popover; the property strip wraps at narrow widths without hiding controls. Dark mode, focus rings and reduced motion follow the app-wide rules.

## 11.5 Parity driver contract

The oracle specs call a fixed surface on `apps/web_new/e2e/parity/drivers/web-new.ts`. Every drafts method there is currently a `todo()` throw (`draftsPageTitle` through `draftsMainCheckboxCount`). The slices implement the methods their rows need, in place, from the semantics below — never by changing a method signature or loosening a spec assertion.

What the scenarios require the screen to render:

- A `main` landmark whose first child is the header block carrying the breadcrumb text, the count chip's digits and the create control.
- One list row per draft, findable by the draft's exact title text; the project marker is the row's first button and exposes the project identifier as its text.
- The inline property strip in the fixed order State, Priority, Labels, Start, Due, Assignees, then at most one project-feature control (Modules, then Cycle, then Estimate). The driver resolves the feature control by position (index 6), so only the enabled one may render.
- Priority is read from the trigger's icon tone (`urgent`/`high`/`medium`/`low`/`none`) and surfaced as `Urgent`/`High`/`Medium`/`Low`/`None`.
- A hover/focus ellipsis button inside the row opening a menu with four buttons; right-click opens the same four entries.
- Date pickers expose disabled out-of-range days through the shared calendar.
- Row and header actions are buttons by accessible name, using the pinned labels in §1.1.

Driver mapping trap: `draftRowMenuClick(name, entry)` receives the old keys `edit`, `make_a_copy`, `move_to_project`, `delete`. Map them to this screen's entries (order is stable: Edit, Make a copy, Move to project, Delete).

## 12. Non-behaviors and URL rules (DRAFT-024, DRAFT-026)

The following are intentionally absent and must stay absent unless a new inventory row is added:

- no keyboard shortcuts specific to drafts; pressing ordinary letter/slash/question-mark keys does not open a create dialog or change the route. The only related command is the shell's `gj` navigation command;
- no drag-and-drop reorder; the order is server newest-first;
- no bulk selection/checkbox operations, bulk edit, archive, transfer or multi-delete;
- no list filters, search, grouping, display options, saved views or layout switch;
- no CSV/PDF/export/import action;
- no realtime push/websocket, polling or focus refetch on the list; external changes appear only after a fresh read (reload/remount or explicit retry);
- no per-draft URL, share link, browser back/forward draft editor, or deep-link to a draft;
- no edition-specific controls, overlay-only screens, desktop-only draft behavior, or separate cloud/desktop endpoints.

Counts and rows must never disagree: if the total is nonzero, the screen either renders rows or shows an explicit error/retry for the records it cannot load; it must not show an empty state over counted invisible drafts (NEWFRONT-305).

## 13. Editions

No DRAFT row is edition-specific: every row is tagged `all` (OSS, cloud and desktop). The drafts route, list, row properties, create/edit/copy/publish/delete flows, permissions, empty states and API endpoints are identical in all three builds. There is no cloud overlay drafts code and no desktop-overlay drafts code; a new edition may change packaging or host wiring but must not fork the drafts surface.

The area gate NEWFRONT-788 must still prove this explicitly:

- **OSS:** run the full drafts oracle suite against `apps/web_new` with the OSS build and confirm every row green.
- **Cloud:** run the edition-neutral leg (the DRAFT-001/027 smoke scenarios, as NEWFRONT-788 defines it) against the cloud build/edition wiring, including the cloud shell/auth path, and confirm no cloud-only control or missing drafts behavior. No cloud-specific drafts row exists, so the gate is proving absence of divergence, not a separate feature.
- **Desktop:** run the drafts scenarios in the desktop build/edition wiring and confirm the same route, permissions and outcomes; no desktop-only draft additions or omissions.
- **Cross-check:** the gate also verifies the entry traffic is the expected read-only draft traffic and that no edition/overlay route shadows `/{ws}/drafts`.

The gate moves each row to “new green” only after all applicable edition legs pass; it does not waive the route migration or the shared Issues gate.

## 14. Row coverage index

This index is for reviewers/slices; each row is specified above by its detailed section and assigned exactly once.

| Row       | Primary section(s) | Owning slice                      |
| --------- | ------------------ | --------------------------------- |
| DRAFT-001 | §2.1–2.3, §8.1     | NEWFRONT-782                      |
| DRAFT-002 | §2.2, §11.4        | NEWFRONT-782                      |
| DRAFT-003 | §2.2, §4.1, §9     | NEWFRONT-782                      |
| DRAFT-004 | §4.1, §8.2         | NEWFRONT-784                      |
| DRAFT-005 | §4.3               | NEWFRONT-784                      |
| DRAFT-006 | §4.1, §4.3, §7     | NEWFRONT-784                      |
| DRAFT-007 | §3.1, §7           | NEWFRONT-783                      |
| DRAFT-008 | §3.1, §4.2, §8.4   | NEWFRONT-783                      |
| DRAFT-009 | §3.1, §4.4–4.6, §7 | NEWFRONT-785                      |
| DRAFT-010 | §4.4, §8.2         | NEWFRONT-785                      |
| DRAFT-011 | §4.6, §8.5, §9     | NEWFRONT-785                      |
| DRAFT-012 | §3.2, §7, §8.4     | NEWFRONT-783                      |
| DRAFT-013 | §3.2, §8.4         | NEWFRONT-783                      |
| DRAFT-014 | §3.2, §8.4         | NEWFRONT-783                      |
| DRAFT-015 | §3.2, §8.4         | NEWFRONT-783                      |
| DRAFT-016 | §3.2–3.3, §8.4     | NEWFRONT-783                      |
| DRAFT-017 | §3.2, §7           | NEWFRONT-786                      |
| DRAFT-018 | §3.2               | NEWFRONT-786                      |
| DRAFT-019 | §3.2               | NEWFRONT-786                      |
| DRAFT-020 | §4.5, §8.6         | NEWFRONT-784                      |
| DRAFT-021 | §4.5, §7, §8.6     | NEWFRONT-784                      |
| DRAFT-022 | §5.1–5.2           | NEWFRONT-782                      |
| DRAFT-023 | §2.3, §11.3        | NEWFRONT-782                      |
| DRAFT-024 | §2.1, §12          | NEWFRONT-782                      |
| DRAFT-025 | §8, §9             | NEWFRONT-787                      |
| DRAFT-026 | §12                | NEWFRONT-782                      |
| DRAFT-027 | §13                | NEWFRONT-782 (gate: NEWFRONT-788) |

## 15. Acceptance for this spec

This spec is complete when:

- all DRAFT-001 through DRAFT-027 are covered in prose and cited by ID;
- the six endpoint contracts, permissions, owner scoping, validation and promote response are resolved, with the server's actual validation limits stated honestly;
- every user-visible string the oracle pins is listed verbatim as a parity label, and the parity-driver contract says what markup the slices must render;
- the NEWFRONT-643 overlap table assigns every row and forbids duplicate/forked builds;
- the shared dialog/picker/action seams and their owners are explicit;
- the shell/session/TanStack Query/count/empty-state behavior is specified;
- all seven target bug issues have intended behavior and an owning slice;
- the edition story and gate obligations are explicit;
- the similarity and paths gates pass for the spec-only change.
