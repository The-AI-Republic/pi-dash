# Feature inventory — Estimates (Phase 3)

Area prefix: `EST-`. Editions covered: oss, cloud, desktop.
Sources surveyed: project settings route
`apps/web/app/(all)/[workspaceSlug]/(settings)/settings/projects/[projectId]/estimates/`
(`page.tsx`, `header.tsx`), component folder
`apps/web/core/components/estimates/` (`root.tsx`, `estimate-list.tsx`,
`estimate-list-item.tsx`, `estimate-disable-switch.tsx`, `estimate-search.tsx`,
`empty-screen.tsx`, `loader-screen.tsx`, `radio-select.tsx`, `index.ts`,
`create/modal.tsx`, `create/stage-one.tsx`, `delete/modal.tsx`,
`points/create-root.tsx`, `points/create.tsx`, `points/update.tsx`,
`points/preview.tsx`, `points/index.ts`, `inputs/root.tsx`,
`inputs/number-input.tsx`, `inputs/text-input.tsx`, `inputs/index.ts`), the
edition variants in `apps/web/ce/components/estimates/`
(`estimate-list-item-buttons.tsx`, `helper.tsx`, `index.ts`,
`update/modal.tsx`, `update/index.ts`, `points/delete.tsx`,
`points/index.ts`, `inputs/time-input.tsx`, `inputs/index.ts`), the edition
store at `apps/web/ce/store/estimates/`, the client store
`apps/web/core/store/estimates/`, the hooks in
`apps/web/core/hooks/store/estimates/`, the service
`apps/web/core/services/estimate.service.ts`, and the API layer (estimate
view endpoints, estimate serializers, estimate URL routes, estimate models).
Sweeps for shell differences (desktop overlay, instance admin, public boards,
command palette entries beyond the work-item menu, keyboard shortcuts, list
filters, exports, realtime channels) found no dedicated estimate-settings
code. Behavior below is paraphrased from those sources; no UI copy, code,
class names or message text is reproduced.

Row format follows the Parity page: ID, capability, who, edition, old entry
point, API, acceptance, parity test (empty until the oracle run), status.

| ID | Capability | Who | Edition | Old entry point | API | Acceptance | Parity test | Status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| EST-001 | Open the project estimates settings screen and see its header, breadcrumb and heading; viewers without project admin rights get a not-authorized view instead of the screen | project admin (view); non-admin members and guests are refused | all | Project settings estimates route | `GET /api/workspaces/{ws}/projects/{pid}/estimates/` | Admins see the screen with breadcrumb and heading; non-admins see the not-authorized view and no estimate controls |  | not started |
| EST-002 | See loading placeholders while the project's estimate systems are being fetched | anyone who can open the screen | all | Same screen while loading | Same list response as EST-001 | Skeleton rows show until the fetch resolves, then the real content replaces them |  | not started |
| EST-003 | Land on an empty state when the project has no estimate system yet, with an add action that opens the creation dialog | project admin (add action); other viewers see no working action | all | Screen with no active system | Same list response as EST-001 (empty result) | Empty project shows the illustration plus a working add action that opens the dialog; unauthorized viewers get no working action |  | not started |
| EST-004 | Switch estimates on and off for the project through the enable control; the switch is operable only by project admins | project admin (switch); others see it disabled | all | Boxed enable control on the screen | Project update carrying the active-system link or clearing it | Switch position matches whether the project currently has estimates on; flipping it toggles the project with a success notice; failures report an error and leave the state unchanged |  | not started |
| EST-005 | See the currently active estimate system with its name and every point value; duration-family values render in hours/minutes form | any project member viewer | all | Active-system section of the list | Same list response as EST-001 | Name plus all values visible; time-family values show humanized durations rather than raw numbers |  | not started |
| EST-006 | See previously replaced estimate systems in a read-only archived section with an explainer and a working outbound reference link | any project member viewer | all | Archived section below the active system | Same list response as EST-001 | Older unused systems listed without edit or delete controls; explainer plus outbound link present and the link opens externally |  | not started |
| EST-007 | Reach edit and delete actions for the active system only as a project admin; everyone else sees read-only rows | project admin (actions) | all | Active list-item controls | Varies per action (see EST-014/EST-016) | Admins get working row controls; non-admin rows expose no edit or delete affordance |  | not started |
| EST-008 | Walk a two-step creation dialog with a step counter, back navigation to the first step, and cancel that closes without saving; reopening starts over from the first step | project admin | all | Add-estimate action, creation dialog | None until submit (see EST-013) | Dialog opens on step one with a visible step counter; back returns to step one discarding entered values; cancel closes with nothing saved; reopening resets all inputs |  | not started |
| EST-009 | Choose the measurement family (points, categories, time) as a radio choice; families the current edition does not support render disabled with an explanatory marker | project admin | oss, cloud (time is cloud-only) | Creation dialog step one | None (client-side choice) | Supported families are selectable; the time family is disabled in OSS and selectable in cloud; disabled options cannot be submitted |  | not started |
| EST-010 | Start from a ready-made template whose card previews its values, or from scratch with starter values | project admin | all (templates exist per supported family) | Creation dialog step one | None (client-side choice) | Each template card shows the values it will load; picking one loads those values into step two; the scratch option starts from minimal starter values |  | not started |
| EST-011 | Arrange the value set inside the dialog: drag rows to reorder (positions renumbered), edit a value inline via double-click or its edit control, remove a value (control hidden once only two remain), add a new value (control hidden once six are present) | project admin | all | Creation dialog step two; saved-system edit forms where the edition supports them | None (client-side until submit; saved-system edits call point endpoints, see EST-014/EST-015) | Reorder changes the order and renumbers positions; inline edit opens a form for that row; removal refuses to go below two values; addition refuses beyond six |  | not started |
| EST-012 | Get per-value validation: numeric families accept only numbers above zero, the text family accepts only non-numeric text, duplicates are refused, empty values are refused, over-long input is capped, problems show inline markers with explanations, and submit is blocked until every row is clean | project admin | all | Value forms in step two and in saved-system edits | None client-side; server enforces a value-length cap (see EST-025) | Each rule refuses with an inline marker on the offending row; submitting with unresolved rows surfaces prompts instead of calling the API |  | not started |
| EST-013 | Submit the new system: it is stored as the project's in-use system with its values, a success notice confirms, the dialog closes and the screen shows it as active; failures report an error and keep the dialog open | project admin | all | Creation dialog confirm action | `POST /api/workspaces/{ws}/projects/{pid}/estimates/` (system plus its points) | New system appears as the active one with its values; success path notices and closes; failure path notices and preserves the dialog content |  | not started |
| EST-014 | Edit a saved system (its values, order and names) through the update dialog with per-row add/edit/remove and the same validation as creation, then save everything in one update | project admin | cloud (the OSS build ships a placeholder with no working edit dialog; its rows offer deletion only) | Active row edit action, update dialog | `PATCH /api/workspaces/{ws}/projects/{pid}/estimates/{eid}/` (system plus full point set; empty point sets refused) | Changes persist across reloads with a success notice; failures report an error; submitting with no values is refused server-side |  | not started |
| EST-015 | Delete a single saved value through a confirmation that offers moving affected work items to another value or clearing them; every affected item gets a history entry and remaining values are renumbered | project admin | cloud (the OSS build ships a placeholder with no confirmation) | Value-row delete control inside saved-system editing | `DELETE /api/workspaces/{ws}/projects/{pid}/estimates/{eid}/estimate-points/{ptid}/` (optional replacement value) | Choosing a replacement moves all affected items to it; clearing empties them; each affected item records a history entry; remaining values renumber without gaps |  | not started |
| EST-016 | Delete the whole estimate system through a confirmation warning that values are stripped from all work items permanently; deleting the active system also unlinks it from the project | project admin | all | Active row delete action, confirmation dialog | `DELETE /api/workspaces/{ws}/projects/{pid}/estimates/{eid}/` plus a project update clearing the link when the deleted system was active | Confirming removes the system, strips values from items and unlinks the project with a success notice; cancelling keeps everything; failures report an error |  | not started |
| EST-017 | Assign or clear an estimate on a work item from a searchable picker listing the active system's values (durations humanized), with the change recorded in the item's history | member+ on the item's project | all | Work-item estimate control (list properties, detail sidebar, creation/edit dialogs) | Work-item update carrying the chosen point id (or clearing it) | Offered values match the active system; assigning persists and adds a history entry; clearing removes the value and is also recorded |  | not started |
| EST-018 | See estimate controls appear and disappear project-wide with the enable switch: when estimates are off, no estimate control shows on work-item surfaces; when on, it appears wherever the layout includes it | any project member | all | Work-item surfaces (list, spreadsheet column, sidebar, dialogs, drafts rows, inbox creation) | Same list response as EST-001 (gating is client-side) | With estimates off, no surface offers an estimate control; switching on makes the control appear without any other configuration |  | not started |
| EST-019 | Group cycle progress by estimate totals versus plain counts where points data exists; the switch is hidden for text-family systems | member+ | all | Cycle analytics sidebar grouping control | Project estimate fetch plus cycle data | Numeric-family projects offer both groupings and totals change accordingly; text-family projects show counts only |  | not started |
| EST-020 | Break analytics charts down by estimate on numeric-family projects | member+ | all | Analytics axis selector | Same as EST-019 | Estimate axis is offered where meaningful and charts aggregate by estimate value; elsewhere it is absent |  | not started |
| EST-021 | Assign an estimate to a work item from the command palette, with the same values and outcomes as the inline picker | member+ | all | Palette work-item estimate menu | Same work-item update as EST-017 | Palette lists the active system's values; choosing one assigns it with the same persistence and history as the picker |  | not started |
| EST-022 | See an estimate value rendered read-only in peek, shared and public contexts with no editing affordance | any viewer including public board visitors | all | Read-only estimate spots and public boards | Same list/item responses (read paths) | Value is visible wherever the context shows it; no edit control is offered |  | not started |
| EST-023 | Read estimate changes in an item's history as set/removed entries naming the values and linking the item | anyone who can view the item's history | all | Item activity feed | Work-item update (EST-017) plus generated history entries | Assigning and clearing each render an entry with old/new values and an item link |  | not started |
| EST-024 | Permissions matrix: settings screen, enable switch and row actions require project admin; single-value endpoints require admin or member (guests refused); reads require project membership; guests cannot create, edit or delete anything | guest / member / admin as stated | all | Whole screen plus row/value actions | All estimate endpoints (list, create, single fetch, update, delete, point create/update/delete, project-points read) | Each role can do exactly its stated operations; disallowed calls are refused server-side and the UI gates or hides the corresponding controls |  | not started |
| EST-025 | Server-side rules and error paths: point creation requires both position and value, value text is length-capped, system updates with no values are rejected, omitted system names are generated, duplicate system names per project are refused | member+ (mutations) | all | API-level (surfaced as form errors and failure notices) | `POST`/`PATCH` point and system endpoints | Each violation returns a client error and nothing is stored; generated names produce a usable system; name clashes are refused |  | not started |
| EST-026 | Reach the screen at the single project-scoped settings address with the browser title tracking the project name; no address opens one specific estimate system | anyone who can open the screen | all | Settings route URL; browser tab title | None beyond EST-001 | The route renders the screen; there is no per-system deep link to share; the title follows the current project |  | not started |
| EST-027 | Fetch-based freshness with no live push: the screen loads through a per-workspace-and-project cache key and changes made elsewhere appear only after a fresh fetch | anyone who can open the screen | all | Whole screen (absence of live behavior) | Same list endpoint as EST-001 (plain fetch, no realtime channel) | No value updates itself without a fetch; reloading or revisiting shows the latest state |  | not started |
| EST-028 | Pinned non-behaviors: no keyboard shortcuts, no exports or imports, no bulk operations and no list filters or search on the estimates screen | any viewer | all | Whole screen (absence) | None | None of these controls exist on the screen |  | not started |
| EST-029 | Identical estimates behavior in every build except the stated edition split: OSS offers points and categories with creation and whole-system deletion; cloud additionally offers the time family, saved-system editing and single-value deletion with reassignment; desktop matches the web behavior of its edition with no desktop-only additions | any viewer | oss / cloud / desktop as stated | Same entry points in every build | Same endpoints in every build | A scenario run against any edition observes exactly its stated subset; no desktop-only estimate code exists to diverge |  | not started |

## Coverage checklist

Every route file, top-level component folder and API endpoint in the
nominated sources is mapped to the row IDs that cover it. Items with no
covering row are explained instead.

### Route files

| Route file | Covering rows |
| --- | --- |
| `apps/web/app/(all)/[workspaceSlug]/(settings)/settings/projects/[projectId]/estimates/page.tsx` (screen entry, admin gate with not-authorized view, project-name title, dimmed wrapper fallback) | EST-001, EST-026 |
| `apps/web/app/(all)/[workspaceSlug]/(settings)/settings/projects/[projectId]/estimates/header.tsx` (breadcrumb plus settings header shell) | EST-001 |

### Top-level component folders (core)

| Component file | Covering rows |
| --- | --- |
| `apps/web/core/components/estimates/root.tsx` (fetch-on-mount screen, loader, empty state, enable switch section, active list, archived list, CRUD dialog wiring) | EST-001, EST-002, EST-003, EST-004, EST-005, EST-006, EST-008 |
| `apps/web/core/components/estimates/estimate-list.tsx` (renders one row per given system id; renders nothing for an empty set) | EST-005, EST-006 |
| `apps/web/core/components/estimates/estimate-list-item.tsx` (row title plus joined value preview with humanized durations; delegates controls to the edition buttons) | EST-005, EST-007 |
| `apps/web/core/components/estimates/estimate-disable-switch.tsx` (enable toggle bound to the project's active-system link, admin-gated, success/failure notices) | EST-004, EST-024 |
| `apps/web/core/components/estimates/create/modal.tsx` (two-step dialog shell, step counter, back/cancel/reset, per-row error aggregation, submit with success/failure notices) | EST-008, EST-012, EST-013 |
| `apps/web/core/components/estimates/create/stage-one.tsx` (family radio with edition gating markers, scratch starter and template cards with value previews) | EST-009, EST-010 |
| `apps/web/core/components/estimates/delete/modal.tsx` (whole-system confirmation with permanent-removal warning, project unlink when active, notices) | EST-016 |
| `apps/web/core/components/estimates/points/create-root.tsx` (sortable value list with renumbering, add/remove/update plumbing, min/max control visibility) | EST-011 |
| `apps/web/core/components/estimates/points/create.tsx` (new-value form: type-specific rules, duplicate/empty/length checks, inline error markers, create-then-add-new chaining) | EST-011, EST-012 |
| `apps/web/core/components/estimates/points/update.tsx` (edit-value form: same rules excluding self from duplicate check, unchanged-value fast path, unsaved-changes guard, notices on saved systems) | EST-011, EST-012, EST-014 |
| `apps/web/core/components/estimates/points/preview.tsx` (value row: drag handle, double-click edit, edit/delete controls with min-count guard, delete-toggle routing to edition dialog) | EST-011, EST-015 |
| `apps/web/core/components/estimates/points/index.ts` (barrel re-export; no behavior) | dead code for parity purposes: re-export only, covered transitively by EST-011 |
| `apps/web/core/components/estimates/inputs/root.tsx` (dispatches numeric / text / time entry by family) | EST-011, EST-012 |
| `apps/web/core/components/estimates/inputs/number-input.tsx` (numeric entry with capped length) | EST-011, EST-012 |
| `apps/web/core/components/estimates/inputs/text-input.tsx` (text entry with capped length) | EST-011, EST-012 |
| `apps/web/core/components/estimates/inputs/index.ts` (barrel re-export; no behavior) | dead code for parity purposes: re-export only, covered transitively by EST-011 |
| `apps/web/core/components/estimates/radio-select.tsx` (generic radio group used for the family choice) | EST-009 |
| `apps/web/core/components/estimates/loader-screen.tsx` (skeleton rows for the initial load) | EST-002 |
| `apps/web/core/components/estimates/empty-screen.tsx` (illustrated empty screen with add action) | dead code: no imports anywhere; the screen uses the compact empty state in `root.tsx` instead (covered by EST-003) |
| `apps/web/core/components/estimates/estimate-search.tsx` (placeholder rendering only) | dead code: no imports anywhere and renders no working control (absence covered by EST-028) |
| `apps/web/core/components/estimates/index.ts` (barrel re-export; no behavior) | dead code for parity purposes: re-export only, covered transitively by EST-001 |

### Top-level component folders (edition variants)

| Component file | Covering rows |
| --- | --- |
| `apps/web/ce/components/estimates/estimate-list-item-buttons.tsx` (OSS row buttons: delete only, admin- and editable-gated) | EST-007, EST-016, EST-029 |
| `apps/web/ce/components/estimates/helper.tsx` (edition gate: points and categories enabled, time disabled in OSS) | EST-009, EST-029 |
| `apps/web/ce/components/estimates/update/modal.tsx` (OSS placeholder: renders nothing) | EST-014, EST-029 (no working edit in OSS) |
| `apps/web/ce/components/estimates/update/index.ts` (barrel re-export; no behavior) | dead code for parity purposes: re-export only, covered transitively by EST-014 |
| `apps/web/ce/components/estimates/points/delete.tsx` (OSS placeholder: renders nothing) | EST-015, EST-029 (no confirmation in OSS) |
| `apps/web/ce/components/estimates/points/index.ts` (barrel re-export; no behavior) | dead code for parity purposes: re-export only, covered transitively by EST-015 |
| `apps/web/ce/components/estimates/inputs/time-input.tsx` (OSS placeholder: renders nothing) | EST-009, EST-029 (no time entry in OSS) |
| `apps/web/ce/components/estimates/inputs/index.ts` (barrel re-export; no behavior) | dead code for parity purposes: re-export only, covered transitively by EST-009 |
| `apps/web/ce/components/estimates/index.ts` (barrel re-export; no behavior) | dead code for parity purposes: re-export only, covered transitively by EST-007 |

### Client state, hooks and services

| Module | Covering rows |
| --- | --- |
| `apps/web/core/store/estimates/project-estimate.store.ts` (workspace/project fetch, create, delete; active system derived from the in-use flag per project; archived ordered newest first; enabled check reads the project's link) | EST-001, EST-004, EST-005, EST-006, EST-013, EST-016, EST-018 |
| `apps/web/core/store/estimates/estimate-point.ts` (single saved-value update with local sync) | EST-014 |
| `apps/web/ce/store/estimates/estimate.ts` (edition estimate entity: ordered value ids, value lookup, in-store value creation) | EST-005, EST-011, EST-014 |
| `apps/web/core/hooks/store/estimates/use-project-estimate.ts` + `index.ts` (hook binding screens to the project estimate store) | EST-001 |
| `apps/web/core/hooks/store/estimates/use-estimate.ts` (hook binding a screen to one system: ordered value ids plus lookup) | EST-005, EST-011 |
| `apps/web/core/hooks/store/estimates/use-estimate-point.ts` (hook binding a form to one saved value) | EST-014 |
| `apps/web/core/services/estimate.service.ts` (workspace list, project list, single fetch, system create, system delete, value create, value update) | EST-001, EST-013, EST-014, EST-015, EST-016 |

### API endpoints

| Endpoint | Covering rows |
| --- | --- |
| `GET /api/workspaces/{ws}/projects/{pid}/project-estimates/` (values of the project's active system; admin/member only) | EST-017, EST-024 |
| `GET /api/workspaces/{ws}/projects/{pid}/estimates/` (all systems with values, project members) | EST-001, EST-003, EST-005, EST-006, EST-024 |
| `POST /api/workspaces/{ws}/projects/{pid}/estimates/` (create system with values; generated name fallback; member+) | EST-013, EST-024, EST-025 |
| `GET /api/workspaces/{ws}/projects/{pid}/estimates/{eid}/` (single system fetch) | EST-024, EST-026 |
| `PATCH /api/workspaces/{ws}/projects/{pid}/estimates/{eid}/` (update system plus full value set; empty sets refused) | EST-014, EST-025 |
| `DELETE /api/workspaces/{ws}/projects/{pid}/estimates/{eid}/` (delete whole system) | EST-016 |
| `POST /api/workspaces/{ws}/projects/{pid}/estimates/{eid}/estimate-points/` (add one value; position and value required) | EST-014, EST-025 |
| `PATCH /api/workspaces/{ws}/projects/{pid}/estimates/{eid}/estimate-points/{ptid}/` (edit one value; admin/member) | EST-014, EST-024 |
| `DELETE /api/workspaces/{ws}/projects/{pid}/estimates/{eid}/estimate-points/{ptid}/` (delete one value with optional reassignment, per-item history, renumbering) | EST-015 |

### Backend supporting modules

| Module | Covering rows |
| --- | --- |
| `apps/api/pi_dash/app/views/estimate/base.py` (project-points read, bulk system list/create/retrieve/update/delete, value create/update/delete with reassignment history and renumbering) | EST-001, EST-013, EST-014, EST-015, EST-016, EST-024, EST-025 |
| `apps/api/pi_dash/app/serializers/estimate.py` (value length cap; read serializers nesting values) | EST-012, EST-025 |
| `apps/api/pi_dash/app/urls/estimate.py` (the five estimate route registrations) | EST-001, EST-013, EST-014, EST-015, EST-016, EST-017 |
| `apps/api/pi_dash/db/models/estimate.py` (storage models; system name unique per project; no user-visible behavior beyond what the rows state) | covered transitively by EST-013, EST-025 |

### Related consumers outside the nominated sources (rows only, no checklist obligation)

- Work-item estimate picker and gating (`dropdowns/estimate.tsx`, issue properties, detail sidebar, spreadsheet column, drafts rows, inbox creation) — EST-017, EST-018.
- Cycle grouping and analytics (`cycles/dropdowns/estimate-type-dropdown.tsx`, cycle analytics sidebar, analytics axis selector) — EST-019, EST-020.
- Command-palette assignment (`power-k/.../work-item/estimates-menu.tsx`) — EST-021.
- Read-only rendering (`readonly/estimate.tsx`, public board properties) — EST-022.
- History entries (`issue-activity/.../actions/estimate.tsx`) — EST-023.

### Sweeps with no estimate-settings behavior (no rows; confirmed absent)

- Desktop overlay (`desktop-overlay/`): no estimate code — covered by EST-029.
- Instance admin (`apps/admin/`): no estimate code — covered by EST-029.
- Keyboard shortcuts, exports/imports, bulk operations, list filters/search on the settings screen: none — covered by EST-028.
- Realtime channels for estimates: none (plain fetch) — covered by EST-027.
- Empty-state image assets (`app/assets/empty-state/project-settings/estimates-*`, `app/assets/empty-state/estimates/`): presentational only, no behavior — covered by EST-003.
