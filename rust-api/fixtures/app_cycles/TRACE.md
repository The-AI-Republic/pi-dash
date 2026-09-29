# TRACE — D-27 app: cycles fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/app/`, models `apps/api/pi_dash/db/models/`, utils
`apps/api/pi_dash/utils/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(no drift — `git diff 01a93e17 -- <sources>` empty at record time).

Out of scope (not fixtured here): `app/views/workspace/cycle.py` (D-24 owns it);
`api/serializers/cycle.py` (D-20 owns it). No Celery tasks and no throttles exist
in this domain; `transfer_cycle_issues` is synchronous.

## Serializers

- `serializers.golden.json` — F-C27-01 — `app/serializers/cycle.py:15-106`
  (CycleWriteSerializer.validate :15-44 incl. "Start date cannot exceed end date"
  :22 + convert_to_utc rewrite :29-37; 22-field CycleSerializer :46-90;
  CycleIssueSerializer :92-100; CycleUserPropertiesSerializer :102-106);
  id rule `app/serializers/base.py:8-10`; convert_to_utc
  `utils/timezone_converter.py:40-94`; create XOR date rule lives in the view
  (`app/views/cycle/base.py:272-274`).

## Models

- `models/columns.json` — F-C27-02 — `db/models/cycle.py:16-157` (defaults
  :16-57; Cycle columns :60-81 + save sort_order min-10000 :88-97;
  CycleIssue :104-127 incl. partial unique
  `cycle_issue_when_deleted_at_null` :114-120; CycleUserProperties :130-157 incl.
  partial unique `cycle_user_properties_unique_cycle_user_when_deleted_at_null`
  :144-150); inherited columns `db/models/base.py:17-44`,
  `db/mixins.py:17-83`, `db/models/project.py:302-311`.

## Queries

- `queries_base.sql` + `.rows.json` — F-C27-03 —
  `app/views/cycle/base.py:69-182` (get_queryset: favorite Exists :70-76,
  total/completed/cancelled Counts :114-151, status Case :153-166, assignees
  ArrayAgg :168-178, order -is_favorite,name :179) + list override :183-268
  (effective order -is_favorite,-created_at :189; current-view fallthrough
  :236-237) + retrieve sub_issues/404 :410-459.
- `queries_archive.sql` + `.rows.json` — F-C27-04 —
  `app/views/cycle/archive.py:41-270` (archived-only filter :117; six group
  counts :137-207; six Cast-Float estimate subqueries :49-113 -> :234-266) +
  list/detail projections :271-584 + archive post date guard :586-604 +
  unarchive :606-611.
- `queries_issue.sql` + `.rows.json` — F-C27-05 —
  `app/views/cycle/issue.py:40-106` (get_queryset sub_issues_count :55-60 +
  filters :61-68; apply_annotations cycle_id/link/attachment/sub counts
  :77-106; ComplexFilterBackend :43 + IssueFilterSet :44 + filterset_fields :49)
  + list pipeline/group errors :108-221 + create move-vs-bulk_create :223-297 +
  destroy :299-324.

## Transfer

- `transfer.json` — F-C27-06 — `utils/cycle_transfer_issues.py:36-479`
  (completed-destination refusal :59-66; source-missing :145-149; estimate
  distributions :164-284; issue distributions :286-397; snapshot write :408-433;
  open-group move :435-459; activity :462-477) + endpoint
  `app/views/cycle/base.py:594-622` (new_cycle_id required :599-603).

## Permissions

- `perms.json` — F-C27-07 — `@allow_permission` matrix: base.py:183 (GUEST list),
  :270/:335/:411 (MEMBER write/read), :477-478 (ADMIN creator-only destroy),
  :521/:571/:581/:595 (MEMBER date-check/favorite/transfer),
  :626/:646/:659/:787 (GUEST userprops/progress/analytics); archive.py:271/:586/:606
  (MEMBER); issue.py:109/:223/:299 (MEMBER); mechanism
  `app/permissions/base.py:19-86`; roles `app/permissions/base.py:14-16`;
  PUT-update fallthrough (no `def update` in base.py; PUT mapped in
  `app/urls/cycle.py:29-38`).

## Progress / analytics

- `progress.json` — F-C27-08 — `app/views/cycle/base.py:658-783` (estimate
  aggregates :664-711; snapshot branch :712-718; live counts :720-765; envelope
  :767-783).
- `analytics.json` — F-C27-09 — `app/views/cycle/base.py:786-1049` (no-dates 400
  :807-811; snapshot branch :821-830; points branch :843-938; issues branch
  :940-1040; envelope :1042-1049).

## Misc

- `misc.json` — F-C27-10 — `app/views/cycle/base.py:520-557` (date-check overlap
  :539-547 + envelopes :548-556), :559-591 (favorite create/destroy),
  :625-657 (user-properties patch 201 :644 + get_or_create get :648-655).

## Routes (reference, no fixture file)

14 routes in `app/urls/cycle.py:21-106`: cycles list/create + detail
(get/put/patch/delete); cycle-issues list/create + detail; date-check post;
user-favorite-cycles list/create + destroy; transfer post; user-properties
(get/patch); archive get/post/delete (3 paths); archived-cycles list + detail;
progress get; analytics get.
