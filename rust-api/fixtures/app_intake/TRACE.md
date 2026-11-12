# TRACE — D-32 app:intake fixtures (PIDASHCONV-278)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from (drift baseline): `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
Gate: PIDASHCONV-90 (`rust-api/contract-tests/app_intake/`) passes byte-identical
vs both backends; key sets cross-checked against
`contract-tests/app_intake/test_intake.py` (`INTAKE_KEYS`, `INTAKE_ISSUE_KEYS`,
`INTAKE_ISSUE_DETAIL_KEYS`) and `test_intake_access.py`.
DRF key order verified empirically against the installed DRF
(`ModelSerializer.get_fields`: explicit `Meta.fields` renders in list order;
`__all__` renders `[pk] + declared(base-first) + concrete fields + forward
relations in model order`; probe `/tmp/order_probe.py`).

## Models

- `models/intake.columns.json` — `Intake` `db/models/intake.py:12-35`
  (own fields `:13-17`; `__str__` `:19-21`; `Meta` `:23-35`); audit columns via
  `db/mixins.py:15-83` (`TimeAuditModel`, `UserAuditModel`, `SoftDeleteModel`)
  and `db/models/base.py:17-22` (`BaseModel.id`); project/workspace FKs via
  `db/models/project.py:302-311` (`ProjectBaseModel`, incl. the
  `save()` workspace-backfill `:309-311`).
- `models/intake_issue.columns.json` — `IntakeIssue` `db/models/intake.py:50-84`
  (own fields `:51-74`; `Meta` `:76-80`); `SourceType` `:38-39`;
  `IntakeIssueStatus` `:42-47`; same audit/project base as above.

## Serializers

- `serializers/intake.golden.json` — `IntakeSerializer`
  `app/serializers/intake.py:17-24` (declared `:18-19`, `Meta` `:21-24`);
  `id` declared on `BaseSerializer` `app/serializers/base.py:11`.
- `serializers/intake_issue.golden.json` — `IntakeIssueSerializer`
  `app/serializers/intake.py:27-90` (`Meta` `:30-41`, `validate` `:43-66`,
  `update` `:68-84`, `to_representation` `:86-90`); nested
  `IssueIntakeSerializer` `app/serializers/issue.py:1021-1036`.
- `serializers/intake_issue_detail.golden.json` — `IntakeIssueDetailSerializer`
  `app/serializers/intake.py:93-117`; nested `IssueDetailSerializer` and
  `IssueIntakeSerializer` owned by other domains (referenced, not recorded).
- `serializers/intake_issue_lite.golden.json` — `IntakeIssueLiteSerializer`
  `app/serializers/intake.py:120-124`.
- `serializers/issue_state_intake.golden.json` — `IssueStateIntakeSerializer`
  `app/serializers/intake.py:127-139` (incl. the `exclude = ["workpad"]`
  note `:137-139`).

## Queries

- `queries/intake_list.sql.json` — `IntakeViewSet.get_queryset` + `list`
  `app/views/intake/base.py:60-75` (annotate `:68`, `select_related` `:69`).
- `queries/intake_issue_queryset.sql.json` — `IntakeIssueViewSet.get_queryset`
  `app/views/intake/base.py:100-174` (tenant filter `:102-105`,
  `select_related`/`prefetch_related` `:106-113`, `cycle_id` `:114-118`,
  `link_count` `:119-124`, `attachment_count` `:125-133`,
  `sub_issues_count` `:134-139`, array aggregates `:140-173`).
- `queries/intake_issue_list.sql.json` — `IntakeIssueViewSet.list`
  `app/views/intake/base.py:176-219` (404 `:179-180`, `issue_filters` `:183`,
  label_ids annotate `:188-197`, `order_by` `:198`, status CSV filter
  `:200-202`, guest scoping `:204-214`, `BasePaginator.paginate` `:215-219`);
  paginator envelope `utils/paginator.py:654-~694`.
- `queries/description_versions.sql.json` —
  `IntakeWorkItemDescriptionVersionEndpoint` `app/views/intake/base.py:569-637`
  (`process_paginated_result` `:570-576`, guest gate `:583-597`, single
  `:599-608`, `required_fields` `:612-623`, queryset `:625-627`,
  `paginate` `:629-636`); cursor envelope `utils/global_paginator.py:33-78`.

## Guards

- `guards/permissions.matrix.json` — role x action matrix for all 10 routes
  (`app/urls/intake.py:15-66`); decorators `app/views/intake/base.py`
  (`:72`, `:77`, `:81`, `:176`, `:221`, `:328`, `:502`, `:549`, `:578`);
  `allow_permission` semantics `app/permissions/base.py:13-110`;
  `BaseViewSet` auth `app/views/base.py:84-97`; `handle_exception`
  `app/views/base.py:110-150`; `ROLE` values `20/15/5`.
- `guards/guest_scoping.golden.json` — guest scoping in `list` `:204-214`,
  `retrieve` `:531-545`, versions `get` `:583-597`; GUEST issue_data narrowing
  `partial_update` `:398-403`.
- `guards/default_intake_delete.golden.json` — default-intake delete guard
  `app/views/intake/base.py:81-91` (incl. the missing-`None`-check bug `:83-85`).

## Tasks

- `tasks/intake_create.before_after.json` — `IntakeIssueViewSet.create`
  `app/views/intake/base.py:221-326` (name check `:223-224`, priority check
  `:227-234`, triage-state ensure `:239-250`, `IssueCreateSerializer` `:253-262`,
  `IntakeIssue` row `:267-272`, `issue_activity.delay` `:274-285`,
  `issue_description_version_task.delay` `:287-292`, re-fetch annotate
  `:293-322`, detail response `:323-324`); task payloads
  `bgtasks/issue_activities_task.py`, `bgtasks/issue_description_version_task.py`.
- `tasks/intake_update.before_after.json` — `partial_update`
  `app/views/intake/base.py:328-500` (gates `:355-368`, issue branch `:378-416`,
  intake branch `:422-427`, saves `:430-471`, re-fetch `:474-498`, response
  `:499-500`); `destroy` cascade rule `:549-566` appended as the delete case.
