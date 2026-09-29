# TRACE — D-21 api-v1 assets, stickies, intake fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(drift baseline; the domain gate PIDASHCONV-427 owns drift after this).
Each `fx-*.json` carries its own `trace` array; this file is the index.

## Serializers

- `fx-ser-asset.json` — `api/serializers/asset.py:13-42`
  (`UserAssetUploadSerializer`: name/type/size/entity_type, type default
  `image/jpeg`), `:45-53` (`AssetUpdateSerializer`: attributes JSON, not
  required), `:56-79` (`GenericAssetUploadSerializer`: name required, type
  optional, project_id/external_id/external_source optional), `:82-90`
  (`GenericAssetUpdateSerializer`: is_uploaded default True). Views bypass
  these serializers (read `request.data` directly) — recorded in-file.
- `fx-ser-sticky.json` — `api/serializers/sticky.py:12-34` (`StickySerializer`
  validate(): HTML sanitize pass-through, invalid HTML ->
  `{"error": "html content is not valid"}` `:21-27`, invalid binary ->
  `{"description_binary": ...}` `:29-32`; name not required `:17`;
  workspace/owner read-only `:16`); validators
  `utils/content_validator.py` (`validate_html_content`,
  `validate_binary_data`).
- `fx-ser-intake.json` — `api/serializers/intake.py:12-39`
  (`IssueForIntakeSerializer`: description <- description_json `:20`),
  `:42-54` (`IntakeIssueCreateSerializer`), `:57-80`
  (`IntakeIssueSerializer`: issue_detail, inbox <- intake.id `:65-66`),
  `:83-157` (`IntakeIssueUpdateSerializer`: validate() accept-guard
  `:113-136` -> `{"status": "Cannot accept intake issue: ..."}` `:132-134`,
  update() TRIAGE->default transition `:138-157`), `:160-170`
  (`IssueDataSerializer`: name max 255, priority choices+default none);
  `PRIORITY_CHOICES` `db/models/issue.py:108-114`; `StateGroup`
  `db/models/state.py:14-22`.

## Models

- `fx-model-fileasset.json` — `db/models/asset.py:28-100` (FileAsset columns
  `:45-62`, Meta+indexes `:64-74`, `asset_url` property `:79-100`,
  `get_upload_path` `:17-20`, dead `file_size` validator `:23-25`) plus
  `FileAssetSerializer` read shape `api/serializers/asset.py:93-123`
  (fields `__all__` + 16 read-only).
- `fx-model-sticky.json` — `db/models/sticky.py:16-57` (columns `:17-30`,
  Meta `:32-36`, save(): description_stripped via strip_tags incl.
  empty/None -> None `:38-44`, sort_order = max+10000 per workspace on
  create `:45-52`); `strip_tags` `utils/html_processor.py:28-31`.
- `fx-model-intake.json` — `db/models/intake.py:12-35` (Intake + Meta
  `:23-35`), `:38-39` (SourceType), `:42-47` (IntakeIssueStatus -2/-1/0/1/2),
  `:50-80` (IntakeIssue columns + Meta).

## Queries

- `fx-q-asset.json` — `api/views/asset.py:51-58` (`asset_delete`
  `filter(id).first()`), `:60-73` + `:258-271` (`entity_asset_delete` user
  avatar/cover lookups), `:210` / `:237` / `:366` / `:394`
  (`get(id, user_id)` patch/delete lookups), `:430` (generic
  `get(id, workspace__slug, is_deleted=False)`), `:534-540` (external
  dedupe filter), `:608` (generic patch lookup).
- `fx-q-sticky.json` — `api/views/sticky.py:30-37` (get_queryset:
  workspace__slug + owner + distinct), `:66-77`
  (`description_stripped__icontains` filter + `-created_at` ordering,
  paginate default 20).
- `fx-q-intake.json` — `api/views/intake.py:63-83` (list get_queryset:
  snoozed Q + intake_view guard + select_related + order_by kwarg default
  `-created_at`), `:233-253` (detail, identical), `:173-184` (triage state
  get-or-create: name Triage, group TRIAGE, color #4E5355, sequence 65000),
  `:350-371` (patch ArrayAgg annotations: label_ids, assignee_ids with
  deleted/inactive guards); `TriageStateManager`
  `db/models/state.py:86-90`.

## Permissions / throttles

- `fx-perm.json` — `api/views/base.py:100-131` (APIKeyAuthentication +
  IsAuthenticated + ApiKeyRateThrottle/ServiceTokenRateThrottle switch),
  `:223-227` (BaseViewSet stack), `app/permissions/workspace.py:103-110`
  (WorkspaceUserPermission, sticky), `app/permissions/project.py:133-143`
  (ProjectLitePermission, intake), `api/rate_limit.py` (rates + headers),
  `api/views/intake.py:336-341` (patch role<=5 non-author denied),
  `:373-392` (guest whitelist + role>15 intake attrs),
  `:474-490` (delete matrix: status in [-2,-1,0,2] deletes issue too,
  else only admin role=20 or creator, else 403).

## Tasks

- `fx-task-asset.json` — `get_asset_object_metadata.delay(asset_id=...)`
  at `api/views/asset.py:214-215,370-371,614-615` (only when
  `storage_metadata` empty); body `bgtasks/storage_metadata_task.py:14-30`.
- `fx-task-intake.json` — `issue_activity.delay` created (post)
  `api/views/intake.py:186-216`, updated + `intake.activity.created`
  (patch) `:394-434`, incl. current_instance/requested_data encoding
  (description vs description_json key acceptance `:189,:375`);
  signature `bgtasks/issue_activities_task.py:1503-1514`.

## Handlers

- `fx-h-asset-user.json` — user-asset post (entity_type guard, mime
  allowlist, size_limit=min, key uuid-name, 200 upload_data/asset_id/
  asset_url) / patch (204, is_uploaded+attributes) / delete (204 soft +
  profile unlink): `api/views/asset.py:110-243`; routes
  `api/urls/asset.py:14-23`.
- `fx-h-asset-server.json` — same trio via server credentials
  (`S3Storage(is_server=True)` `:336`): `api/views/asset.py:282-400`;
  routes `api/urls/asset.py:24-33`. One closure with the user trio.
- `fx-h-asset-generic.json` — get (presigned download, not-uploaded 400,
  missing 404, 500 body) / post (name+size required,
  ATTACHMENT_MIME_TYPES guard, external 409 with asset_id/asset_url) /
  patch (204): `api/views/asset.py:419-621`; routes
  `api/urls/asset.py:34-43`; `ATTACHMENT_MIME_TYPES`
  `settings/common.py:652+`.
- `fx-h-sticky.json` — create 201 / list paginated default 20 with query
  filter / retrieve / partial_update / destroy 204:
  `api/views/sticky.py:48-113`; routes `api/urls/sticky.py:1-13`
  (DefaultRouter `stickies` under `workspaces/<slug>/`).
- `fx-h-intake.json` — list (paginated, snoozed/intake_view guards) /
  create 201 (name required, priority allowlist, triage auto-create) /
  retrieve / patch (guest whitelist, dual serializers, activity) / delete
  (conditional issue delete + 403): `api/views/intake.py:106-219`,
  `:272-494`; routes `api/urls/intake.py:14-23`.

## Cross-cutting ported bugs (translate as-is)

- BUG-1: `dispatch` returns the exception object, not the response
  (`api/views/base.py:180-182`; same shape as the license/app variants).
- BUG-2: user/server post `name=None` renders key `<hex>-None`
  (`api/views/asset.py:117,150,290,323`); non-numeric size -> 500, not 400
  (`:119,:292`).
- BUG-3: invalid-mime message claims 'Only JPEG and PNG' while 5 types
  pass (`api/views/asset.py:143,316`).
- BUG-4: `DRAFT_ISSUE_ATTACHMENT` has no `asset_url` branch -> None
  (`db/models/asset.py:79-100`).
- BUG-5: intake create guard is AND (`api/views/intake.py:156`): intake
  None + intake_view True -> 500 on `intake.id`.
- BUG-6: intake patch pops `issue` from `request.data`, so the
  intake.created activity never carries it (`:344` vs `:422`).
- BUG-7: intake delete skips the creator/admin guard for accepted
  (status 1) rows (`:475`); patch non-author denial is 400, not 403
  (`:337-341`).
- BUG-8: sticky destroy hard-deletes despite SoftDeleteModel
  (`api/views/sticky.py:112`); generic patch ignores `attributes`
  (`api/views/asset.py:600-619`).
