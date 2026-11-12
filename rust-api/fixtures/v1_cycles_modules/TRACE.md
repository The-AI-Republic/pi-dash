# TRACE — D-20 api-v1 cycles + modules fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(zero diff Ported-from→HEAD on all sources below, verified 2026-09-29 via
`git log --oneline <ported-from>..HEAD -- <paths>`, empty).
Reading material (not recorded, cross-checked): `rust-api/contract-tests/v1_cycles_modules/`
(`test_cycles.py`, `test_modules.py`, `conftest.py`; oracle PIDASHCONV-78, Done).
Generation method per file is noted in its `_method` key: structural values
(field lists, spans, Response literals, status codes, task call args, routes,
key sets) were extracted by running `python3 /tmp/gen_cycmod_fixtures.py`
(ast parsing, no Django settings, no DB); SQL skeletons transcribe the cited
`get_queryset` chains token-by-token; representative rows use fixed seed UUIDs.

## Models (FX-CYCMOD-01)

- `models/cycle.columns.json` — `db/models/cycle.py:16-57` (filter/display
  defaults), `:60-101` Cycle (Meta `:82-87`, save sort_order `:88-97`),
  `:104-127` CycleIssue (partial unique `:114-120`), `:130-157`
  CycleUserProperties (partial unique `:144-150`); shared base
  `db/models/project.py:302-311`, audit/soft-delete `db/mixins.py`, UUID pk
  `db/models/base.py`.
- `models/module.columns.json` — `db/models/module.py:14-55` (defaults),
  `:58-64` ModuleStatus (6 values), `:67-127` Module (Meta `:101-113`, save
  `:115-123`), `:130-149` ModuleMember, `:152-171` ModuleIssue, `:174-187`
  ModuleLink, `:190-217` ModuleUserProperties; archived_at guards recorded
  per file.

## Serializers (FX-CYCMOD-02, FX-CYCMOD-03)

- `serializers/cycle.golden.json` — `api/serializers/cycle.py:15-106`
  CycleCreate (Meta `:38-59`, validate `:61-106`), `:109-121` CycleUpdate,
  `:124-155` CycleSerializer (metrics `:132-140`), `:158-171`
  CycleIssueSerializer, `:174-184` CycleLiteSerializer, `:187-195`
  CycleIssueRequestSerializer, `:198-206` TransferCycleIssueRequestSerializer.
- `serializers/module.golden.json` — `api/serializers/module.py:21-121`
  ModuleCreate (Meta `:36-58`, validate `:60-81`, create `:83-121`),
  `:124-166` ModuleUpdate (Meta `:133-138`, update `:140-166`), `:169-206`
  ModuleSerializer (to_representation `:203-206`), `:209-230`
  ModuleIssueSerializer, `:233-258` ModuleLinkSerializer (create `:255-258`),
  `:261-271` ModuleLiteSerializer, `:274-285` ModuleIssueRequestSerializer.

## Queries (FX-CYCMOD-04, FX-CYCMOD-05)

- `queries/cycle.sql` + `queries/cycle.rows.json` — Q1 list
  `api/views/cycle.py:89-167` + archived filter `:197`; Q2 detail `:370-448`
  + `:469`; Q3 archived `:622-724` (estimates `:699-721`); Q4 issue list
  `:815-837` vs GET `:854-901`; Q5 issue detail `:1027-1114`; Q6 transfer
  `utils/cycle_transfer_issues.py:36-479` (delay `:462-478`).
- `queries/module.sql` + `queries/module.rows.json` — M1 list
  `api/views/module.py:85-171` + `:272`; M2 detail `:288-374` + `:473`; M3
  archived `:895-1077` (no estimates — asymmetry vs Q3); M4 issue list
  `:545-569` vs GET `:594-639`; M5 issue detail `:751-775`, `:800-849`,
  `:864-888` (delay `:879`).

## Guards (FX-CYCMOD-06)

- `guards/permissions.golden.json` — `app/permissions/project.py:85-116`
  ProjectEntityPermission (`:86-116`); `api/views/base.py:133-182`
  (handle_exception `:133-170`, dispatch `:172-182`); routes
  `api/urls/cycle.py:16-57`, `api/urls/module.py:15-51`; view-level delete
  rules `api/views/cycle.py:578-590`, `api/views/module.py:497-509`.

## Tasks (FX-CYCMOD-07)

- `tasks/enqueue.golden.json` — model_activity `api/views/cycle.py:338-346`
  (create), `:549-557` (update), `api/views/module.py:229-237` (create),
  `:439-447` (update); issue_activity `api/views/cycle.py:594-608`
  (delete), `:987-1002` (add/move), `:1086-1114` (remove),
  `api/views/module.py:512-527` (delete), `:714-728` (add/move), `:879-887`
  (remove), `utils/cycle_transfer_issues.py:462-478` (transfer).

## Handlers (FX-CYCMOD-08)

- `handlers/cycle.golden.json` — routes `api/urls/cycle.py:16-57` (8);
  handlers `api/views/cycle.py:80-1210` (list `:190-281`, post `:299-356`,
  detail get `:462-476`, patch `:494-561`, delete `:571-613`, archived get
  `:741-751`, archive `:763-783`, unarchive `:794-803`, issue get `:854-901`,
  issue post `:920-1010`, issue detail get `:1063-1076`, issue delete
  `:1086-1114`, transfer post `:1167-1210`).
- `handlers/module.golden.json` — routes `api/urls/module.py:15-51` (7);
  handlers `api/views/module.py:76-1077` (list get `:264-276`, post
  `:192-241`, detail get `:468-475`, patch `:402-450`, delete `:490-533`,
  issue get `:594-639`, issue post `:662-733`, issue detail get `:800-849`
  (unrouted → 405), issue delete `:864-888`, archived get `:1006-1018`,
  archive `:1034-1054`, unarchive `:1068-1077`).

## Bugs ported (translate, don't redesign — list for the layer PRs)

- `cycle_view=current` returns a bare list, not the paginated envelope
  (`api/views/cycle.py:201-210`).
- Archiving a null-`end_date` cycle raises TypeError → generic 500
  (`api/views/cycle.py:770`; 500 path `api/views/base.py:168,182`).
- Completed-cycle `sort_order` PATCH is a 200 no-op (view narrows payload at
  `api/views/cycle.py:512-515` but `CycleUpdateSerializer` has no
  `sort_order` field, `api/serializers/cycle.py:117-122`).
- Module-issue detail GET is defined (`api/views/module.py:800-849`) but not
  routed (`api/urls/module.py:31-35` DELETE only) → 405.
- `ModuleIssueListCreate.post` match loop compares `module_id` (UUID) to the
  `module_id` URL str and filters candidates by list membership
  (`api/views/module.py:683-688`).
- `ModuleLinkSerializer.create` duplicate message says "Issue"
  (`api/serializers/module.py:257`); unreachable — no link routes exist.
- Module duplicate-name create is 400 with code `MODULE_NAME_ALREADY_EXISTS`
  (`api/serializers/module.py:93-101`); cycles have no name uniqueness.
- Archived-module list has no estimate sums, archived-cycle list does
  (`api/views/module.py:895-982` vs `api/views/cycle.py:699-721`).
