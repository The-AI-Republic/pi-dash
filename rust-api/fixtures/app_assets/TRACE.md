# TRACE — D-31 app:assets fixtures (PIDASHCONV-306)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from (drift baseline):
`01a93e17216faea7bfc156b0f864cbbe420d1c52` (drift owned by the domain gate
PIDASHCONV-420). Oracle: PIDASHCONV-89, suite at
`rust-api/contract-tests/app_assets/` (test_workspace_assets,
test_user_assets_v2, test_project_assets, test_downloads, test_duplicate,
test_legacy_v1, test_permissions, test_tenant_isolation,
test_static_restore_check, conftest, seed_assets) — every status/body in the
goldens below is cross-checked against that suite, which runs against live
Django. No Rust code in this issue.

18 routes: `app/urls/asset.py:26-114` (5 v1 + 13 v2 incl. restore).

## Models

- `models/fileasset.columns.json` — `FileAsset`
  `db/models/asset.py:45-62` (18 own fields `:45-62`; the issue body says 15
  but the source defines 18 — fixture follows the source); `Meta` `:64-74`
  (`db_table file_assets`, `ordering -created_at`, 4 indexes
  `asset_entity_type_idx` / `asset_entity_identifier_idx` / `asset_entity_idx`
  / `asset_asset_idx`); `__str__` `:76-77`; audit columns via
  `db/mixins.py:15-73` (`TimeAuditModel :15-23`, `UserAuditModel :26-40`,
  `SoftDeleteModel :46-73` incl. `objects` vs `all_objects`) and
  `db/models/base.py:17-22` (`BaseModel.id`); `EntityTypeContext` 10 values
  `:33-43`; touched FK columns `Workspace.logo_asset_id`
  `db/models/workspace.py:124-130`, `Project.cover_image_asset_id`
  `db/models/project.py:108-114`, `User.avatar_asset_id`
  `db/models/user.py:69-75`, `User.cover_image_asset_id`
  `db/models/user.py:78-84`. BUG: field `asset` `:46` has no `validators=`
  (migration `0075_alter_fileasset_asset.py` lists `FileExtensionValidator` +
  `file_size`, model dropped them) — runtime enforces neither.
- `models/asset_url.golden.json` — `asset_url` property
  `db/models/asset.py:79-100` (static group `:81-87`, `ISSUE_ATTACHMENT`
  `:89-90`, description group `:92-98`, `None` fallthrough `:100`).
- `models/upload_path.golden.json` — `get_upload_path`
  `db/models/asset.py:17-20` (workspace `:18-19`, user `:20`); `file_size`
  `db/models/asset.py:23-25` with `FILE_SIZE_LIMIT`
  `settings/common.py:428` (BUG: message hardcodes "5 MB").

## Serializers

- `serializers/fileasset.golden.json` — `FileAssetSerializer`
  `app/serializers/asset.py:9-13` (`fields = "__all__"`, `read_only`
  `created_by/updated_by/created_at/updated_at`); `id` declared on
  `BaseSerializer` `app/serializers/base.py:11`; 24-key output set pinned by
  the oracle (`test_legacy_v1.py` `V1_ROW_KEYS`); BUG: `UserAssetsEndpoint.get`
  serializes without `many=True` `app/views/asset/base.py:67` (500 on rows).

## Queries

- `queries/v1.golden.json` — legacy endpoints `app/views/asset/base.py:1-86`
  (`FileAssetEndpoint.get` `:23-33` incl. both-200 quirk, `.post` `:35-42`,
  `.delete` `:44-49` incl. is_deleted-only BUG, `FileAssetViewSet.restore`
  `:52-58`, `UserAssetsEndpoint.get` `:64-73` incl. missing-`many` BUG,
  `.post` `:75-80`, `.delete` `:82-86`); error envelopes
  `app/views/base.py:211-254`; auth defaults `app/views/base.py:189-194`.
- `queries/v2_user_workspace.golden.json` — `UserAssetsV2Endpoint`
  `app/views/asset/v2.py:29-198` (`asset_delete` `:32-39`,
  `entity_asset_save` `:41-78`, `entity_asset_delete` `:80-107`, `post`
  `:109-168`, `patch` `:170-189`, `delete` `:191-198`) and
  `WorkspaceFileAssetEndpoint` `app/views/asset/v2.py:201-429`
  (`get_entity_id_field` `:204-234`, `asset_delete` `:236-245`,
  `entity_asset_save` `:247-284`, `entity_asset_delete` `:286-312` incl. dead
  `None` check after `.get` `:290-291`, `post` `:314-377`, `patch` `:379-398`,
  `delete` `:400-407`, `get` `:409-429`); publishers `:177`,`:386` (payloads
  in `tasks/file_asset.golden.json`); tenant gap (no membership gates on
  `:109`,`:170`,`:191`,`:314`,`:379`,`:400`,`:409`) pinned by the oracle.
- `queries/v2_project.golden.json` — `StaticFileAssetEndpoint`
  `app/views/asset/v2.py:432-465` (`AllowAny` `:435`), `AssetRestoreEndpoint`
  `app/views/asset/v2.py:468-477`, `ProjectAssetEndpoint`
  `app/views/asset/v2.py:480-627` (`get_entity_id_field` `:483-510` incl.
  extra `DRAFT_ISSUE_DESCRIPTION`, `post` `:512-577` incl. `PROJECT_COVER`
  duplicate-kwarg 500 BUG `:554-563`, `patch` `:579-593`, `delete`
  `:595-604`, `get` `:606-627` incl. `pk=pk` quirk `:609`),
  `ProjectBulkAssetEndpoint` `app/views/asset/v2.py:630-688`
  (`save_project_cover` `:631-634`, per-entity branches `:657-686` incl.
  `IntegrityError` swallows), `AssetCheckEndpoint`
  `app/views/asset/v2.py:691-697`, `DuplicateAssetEndpoint`
  `app/views/asset/v2.py:700-780` (`throttle_classes` `:701`,
  `get_entity_id_field` `:703-734`, `post` `:736-780` incl. `entity_id` (not
  `entity_identifier`) quirk `:739` and create-before-copy `:761-778`),
  `WorkspaceAssetDownloadEndpoint` `app/views/asset/v2.py:783-807`,
  `ProjectAssetDownloadEndpoint` `app/views/asset/v2.py:810-835`; publisher
  `:587` (payload in `tasks/file_asset.golden.json`).

## Guards

- `guards/permissions.golden.json` — `AllowAny` static (`v2.py:435`);
  `@allow_permission([ADMIN, MEMBER, GUEST], level="WORKSPACE")` on restore
  (`v2.py:471`), check (`v2.py:694`), duplicate (`v2.py:736`), ws-download
  (`v2.py:786`); entity-level `[ADMIN, MEMBER, GUEST]` (default `PROJECT`) on
  project post/patch/delete/get (`v2.py:512,579,595,606`) and bulk
  (`v2.py:636`); `level="PROJECT"` on project-download (`v2.py:813`); no
  per-method gates on user-v2 (`v2.py:109,170,191`), workspace-v2
  (`v2.py:314,379,400,409`) or v1 (`base.py:16,52,61`); decorator semantics
  `app/permissions/base.py:19-88`, `ROLE` `:13-16`; `AssetRateThrottle`
  `throttles/asset.py:8-15` (scope `:9`, key `:11-15`), rate
  `settings/common.py:93-95`, 429 envelope
  `authentication/adapter/error.py:71` + `authentication/rate_limit.py:24,43`.

## Tasks

- `tasks/file_asset.golden.json` — `delete_unuploaded_file_asset`
  `bgtasks/file_asset_task.py:20-26` (cutoff `:24` incl. string-default `"7"`
  BUG, soft-delete via `SoftDeletionManager` `db/mixins.py:46-73`);
  `get_asset_object_metadata` body `bgtasks/storage_metadata_task.py:14-30`
  (shared — publishers only here); in-scope publishers
  `app/views/asset/v2.py:177,386,587`; out-of-scope call sites listed as
  reference (`space/views/asset.py:148`,
  `app/views/issue/attachment.py:227`, `api/views/issue.py:2507,2649`,
  `api/views/asset.py:215,371,615`).
