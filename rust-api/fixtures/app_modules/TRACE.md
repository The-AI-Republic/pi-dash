# TRACE — D-28 app:modules fixtures (PIDASHCONV-294)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
Gate: PIDASHCONV-86 (`rust-api/contract-tests/app_modules/`) passes
byte-identical vs both backends; the port-existing bugs below are pinned
there and recorded here for the port (translate, don't redesign).

## Models (FX-MOD-01)

- `models/module.columns.json` — `db/models/module.py:58-100` (ModuleStatus
  `:58-64`, Module fields `:67-99`), Meta/constraints/save `:101-127`;
  audit cols `db/mixins.py:16-69`; UUID pk `db/models/base.py:17-21`;
  project/workspace FKs `db/models/project.py:302-311`.
- `models/module_member.columns.json` — `db/models/module.py:130-149`
  (+ audit/pk/FK refs as above); serializer write coupling
  `app/serializers/module.py:75-90,102-118`.
- `models/module_issue.columns.json` — `db/models/module.py:152-171`
  (+ audit/pk/FK refs); write coupling `app/views/module/issue.py:216-230,
  :256-270, :313, :336`, destroy coupling `app/views/module/base.py:744`.
- `models/module_link.columns.json` — `db/models/module.py:174-187`
  (+ audit/pk/FK refs); no-DB-unique note traces to
  `app/serializers/module.py:190-201`.
- `models/module_user_properties.columns.json` — defaults
  `db/models/module.py:14-55`, model `db/models/module.py:190-217`
  (+ audit/pk/FK refs).

## Serializers (FX-MOD-02)

- `serializers/module_write.golden.json` — `app/serializers/module.py:26-120`
  (Meta `:36-48`, to_representation `:50-53`, validate `:55-62`, create
  `:64-92`, update `:94-120`); wire shapes cross-checked against
  `contract-tests/app_modules/test_modules.py:28-65` and conftest
  `MODULE_ROW_KEYS`/`WRITE_MODULE_KEYS`.
- `serializers/module_flat_issue_detail.golden.json` — ModuleFlat
  `app/serializers/module.py:123-134`, ModuleIssue
  `app/serializers/module.py:137-153`, ModuleSerializer
  `app/serializers/module.py:206-254`, ModuleDetail
  `app/serializers/module.py:257-273`; retrieve envelope
  `app/views/module/base.py:395-649`.
- `serializers/module_link.golden.json` — `app/serializers/module.py:156-203`
  (to_internal_value `:170-176`, validate_url `:178-186`, create `:188-192`,
  update `:194-203`); wire cases cross-checked against
  `contract-tests/app_modules/test_module_links.py:34-105`.
- `serializers/module_userprops.golden.json` — serializer
  `app/serializers/module.py:276-280`, endpoint `app/views/module/base.py:
  825-855`, defaults `db/models/module.py:14-55`; wire cases
  cross-checked against `contract-tests/app_modules/
  test_favorites_properties.py:100-142`.

## Queries (FX-MOD-03)

- `queries/module_querysets.sql` (Q1-Q6, captured live via Django shell
  `str(queryset.query)`, params slug=`ws` project=`a0ee4576-…`)
  + `queries/module_querysets.rows.json` (result-row contract):
  Q1 `app/views/module/base.py:78-292`, Q2 `base.py:774-788`,
  Q3 `base.py:795-802` (live FieldError on `select_related('module')`
  recorded in rows.json), Q4 `app/views/module/archive.py:45-256`,
  Q5+annotations `app/views/module/issue.py:53-94`; manager base
  `db/models/issue.py:95-104` (IssueManager excludes triage/archived/
  archived-project/drafts inside every issue subquery).

## Guards (FX-MOD-04)

- `guards/permissions.golden.json` — `@allow_permission` role sets
  `app/views/module/base.py:294,353,395,651,723,763,793,826,846` and
  `app/views/module/issue.py:95,209,248,317`; mechanics
  `app/permissions/base.py:19-88` (role check + workspace-admin
  override + creator fast-path), `app/permissions/project.py:85-118`
  (ProjectEntityPermission) and `:133-143` (ProjectLitePermission);
  ROLE values `app/permissions/base.py:13-16`; 401 body is the DRF
  default (suite pins `test_modules.py:162-163`); PUT-has-no-gate
  quirk: no `update` method and no `get_permissions` override anywhere
  in `app/views/module/base.py` (cf role-gated `partial_update`
  `:651-721`).

## Tasks (FX-MOD-05)

- `tasks/enqueue_payloads.golden.json` — `model_activity.delay`
  `app/views/module/base.py:339-347` (create) and `:708-716`
  (partial_update, with pre-save `ModuleSerializer` snapshot `:668`);
  per-issue `issue_activity.delay` on destroy `base.py:728-741`;
  `recent_visited_task.delay` on retrieve `base.py:641-647`;
  `issue_activity.delay` on `create_module_issues` `app/views/module/
  issue.py:232-245`, on add `issue.py:272-285`, on remove
  `issue.py:294-313`, on destroy `issue.py:325-335`.

## Handlers (FX-MOD-06)

- `handlers/module_crud.golden.json` — `app/urls/module.py:19-35`
  (routes 1-2); handlers `app/views/module/base.py:294-759`; key sets
  cross-checked against conftest `MODULE_ROW_KEYS`/`MODULE_DETAIL_KEYS`/
  `WRITE_MODULE_KEYS` and `test_modules.py`.
- `handlers/module_issues_links.golden.json` — `app/urls/module.py:36-74`
  (routes 3-7); handlers `app/views/module/issue.py:94-337`,
  `app/views/module/base.py:762-788`, `app/serializers/module.py:170-203`;
  envelope/row keys cross-checked against `test_module_issues.py:20-33`
  (`LIST_KEYS`/`ISSUE_KEYS`) and `test_module_links.py:15-18`
  (`LINK_KEYS`); missing-pk 500s pinned in `test_module_issues.py:
  118-136`.
- `handlers/favorites_userprops_archive.golden.json` —
  `app/urls/module.py:75-104` (routes 8-13); handlers
  `app/views/module/base.py:791-855` and `app/views/module/
  archive.py:258-565`; 500s pinned in `test_favorites_properties.py:
  78-84` and `test_archive.py:58-64`; row keys cross-checked against
  `ARCHIVED_ROW_KEYS` (`test_archive.py:20-28`) and `PROPS_KEYS`.

## Port-existing bugs (translate, don't fix; all pinned by PIDASHCONV-86)

1. `GET modules/<id>/archive/` → 500: `get()` takes `pk`, the route
   passes `module_id` (TypeError → generic 500 handler
   `app/views/base.py:146-149`).
2. Module-issue detail `GET/PUT/PATCH` → 500: routes name kwargs
   `module_id`/`issue_id`, DRF defaults need `pk`.
3. `PATCH module-links/<pk>/` without `url` → 400 `{"error": "Invalid
   URL format."}`: `update()` validates unconditionally.
4. Duplicate-link `update` error says "…for this Issue" (verbatim).
5. `GET user-favorite-modules/` → 500: no `serializer_class`, and the
   queryset's `select_related("module")` raises FieldError first.
6. `PUT modules/<pk>/` is auth-only (no role gate, no archived guard);
   `PATCH` returns the annotated row while `PUT` returns the bare
   write shape.
7. `creator=True` on destroy is a fast-path OR, not a requirement: a
   MEMBER creator still 403s on the fall-through `[ADMIN]` role check.
8. `issue_activity` actor/project id str-vs-int inconsistencies and two
   None-unsafe `.first().module.name` lookups (see tasks fixture).
9. Archive queryset `member_ids` ArrayAgg lacks the
   `modulemember__deleted_at` filter the main queryset has.
