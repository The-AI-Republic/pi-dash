# TRACE — D-01 license / instance-console fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/license/`. Supporting modules noted where behaviour
lives outside `license/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

## Models

- `models/instance.columns.json` — `license/models/instance.py:22-50` (class `Instance`, `Meta` 46-50); audit columns from `db/mixins.py:16-45` (`TimeAuditModel`), `db/mixins.py:26-38` (`UserAuditModel`), `db/mixins.py:57-69` (`SoftDeleteModel`); pk shape from `db/models/base.py:17-21` (`BaseModel.id` UUID pk); `InstanceEdition` 18-19; field defaults/choices `ROLE_CHOICES` 15.
- `models/instance_admin.columns.json` — `license/models/instance.py:53-69` (`Meta` 64-69, `unique_together` 65); audit columns as above.
- `models/instance_configuration.columns.json` — `license/models/instance.py:72-83` (`Meta` 79-83).
- `models/changelog.columns.json` — `license/models/instance.py:86-100` (`Meta` 96-100); import absence: `license/models/__init__.py:1-5` (imports `Instance, InstanceAdmin, InstanceConfiguration, InstanceEdition` only — `ChangeLog` NOT imported).

## Serializers

- `serializers/instance.golden.json` — `license/api/serializers/instance.py:11-17`; `primary_owner_details` field 12 with `source="primary_owner"` (missing on model — BUG-1, key absent); `UserAdminLiteSerializer` fields `app/serializers/user.py:156-170`; `read_only_fields` 17; base id field `license/api/serializers/base.py:8-9`; datetime `Z` rendering by DRF 3.15.2 `fields.py:431-456` (`SkipField` on missing attr).
- `serializers/instance_admin.golden.json` — `license/api/serializers/admin.py:36-42`; nested `user_detail` 37 (`UserAdminLiteSerializer` fields as above).
- `serializers/instance_admin_me.golden.json` — `license/api/serializers/admin.py:12-33` (note `is_email_verified` listed twice, 23 and 31 — BUG, output carries it once).
- `serializers/instance_configuration.golden.json` — `license/api/serializers/configuration.py:11-30`; decrypt branch 19-20; `source`/`is_managed` 25-28; registry sources `config/registry.py:36-66+`.
- `serializers/workspace.golden.json` — `license/api/serializers/workspace.py:15-41`; `owner` 16 (`api/serializers/user.py:9-11`), `logo_url` 17 (property `db/models/workspace.py:145-154`), `total_projects`/`total_members` 18-19 (annotations `api/views/workspace.py:41-56`), `validate_slug` 21-28, `Meta`/`read_only_fields` 30-41.

## Utils

- `utils/encryption.golden.json` — `license/utils/encryption.py:13-16` (`derive_key`), `:20-30` (`encrypt_data`), `:34-44` (`decrypt_data`); vectors executed against the real code.
- `utils/configuration_value.golden.json` — `license/utils/instance_value.py:14-18` (`_source`), `:28-54` (`get_configuration_value`); registry `config/registry.py:36-66+`.
- `utils/email_configuration.golden.json` — `license/utils/instance_value.py:57-74` (7 keys, call-time `os.environ.get` defaults).

## Queries

- `queries/instance_first.sql` + `.rows.json` — `.first()` call sites `api/views/instance.py:37,178,191`; SQL shape emitted by the Django compiler (soft-delete filter from `db/mixins.py:52-54`, ordering from `Meta.ordering`).
- `queries/admin_crud.sql` + `.rows.json` — `api/views/admin.py:56,64,66,78,85` (POST create, GET list, DELETE); `api/permissions/instance.py:17-18` (permission `role__gte=15` check); signup `admin.py:108,173,229`, signin `admin.py:294,335`.
- `queries/config_patch.sql` + `.rows.json` — `api/views/configuration.py:44` (`key__in` filter), `:57` (`bulk_update(..., ["value"], batch_size=100)`).
- `queries/disable_email.sql` + `.rows.json` — `api/views/configuration.py:69-80` (`Case(When(key="ENABLE_SMTP", then=Value("0")), default=Value(""))`).
- `queries/workspace_list.sql` + `.rows.json` — `api/views/workspace.py:41-69` (`Count` subqueries `:41-54`, `icontains` `:59-61`, `paginate(..., max_per_page=10, default_per_page=10)` `:63-69`; paginator `utils/paginator.py:654-694`); slug checks `workspace.py:31`, `serializers/workspace.py:26`.

## Guards

- `guards/instance_admin_permission.golden.json` — `license/api/permissions/instance.py:12-18`; anonymous branch 14-15; `role__gte=15` + instance scoping 17-18; applied in `api/views/base.py:42-43` (license `BaseAPIView`), `api/views/admin.py:45`, `api/views/configuration.py:33,64`, `api/views/workspace.py:20,38`; GET-PATCH split `api/views/instance.py:29-32`.

## Tasks

- `tasks/instance_traces.before_after.json` — `license/bgtasks/tracer.py:26-105` (`@shared_task` 26, `init_tracer` 29, instance-None 31-35, telemetry-off skips 37, `instance_details` span + 21 attributes 41-74, per-workspace `workspace_details` spans + 10 attributes 77-100, `shutdown_tracer` in `finally` 103-105).
- `tasks/configure_instance.golden.json` — `license/management/commands/configure_instance.py:20` (`DERIVED_FLAG_KEYS`), `:23-29` (`_is_db_sourced`), `:39-43` (SECRET_KEY mandatory), `:45-59` (seed loop, `get_or_create` + encrypt branch `:52-55`), `:61-170` (derived flags; `get_configuration_value` calls `:65-76,:90-100,:114-128,:142-156`; create-if-absent `:62`; per-key `if` chain, no `elif` — BUG-7).
- `tasks/register_instance.golden.json` — `license/management/commands/register_instance.py:24-26` (arg), `:28-38` (current version: env → package.json → `v0.1.0`), `:40-52` (latest: GitHub releases → fallback), `:53-90` (create `:61-77` with `secrets.token_hex(12)` + `IS_TEST` flag `:73`; update `:78-87`; `instance_traces.delay()` `:90`).

## Handlers

- `handlers/instance.golden.json` — `api/views/instance.py:28-199` (permissions `:29-32`; GET null-instance `:36-44`; 15-key derivation `:50-126`; `== "1"` coercions `:130-159`; `file_size_limit` float `:156`; base URLs `:162-167`; `workspaces_exist` `:170`; PATCH `:175-183`; signup-visited `:186-199`; cache `cache_response(7200, user=False)` + `cache_control(private, max_age=12)` `:34-35`, `invalidate_cache` `:175,189`; helpers `utils/cache.py:25-50,72-80`).
- `handlers/admins.golden.json` — `api/views/admin.py:44-86` (POST 400 `:53-54`, 403 `:57-61`, `User.DoesNotExist` → 500 BUG-2 `:64`, 201 `:66-68`; GET 403/200 `:71-80`; DELETE 204 `:83-86`); me `admin.py:361-366`; session `admin.py:369-379`; sign-out `admin.py:382-398`; error codes `authentication/adapter/error.py:5-71`; `get_error_dict` `:87-92`; `base_host` `authentication/utils/host.py:16-36`; `get_safe_redirect_url` `utils/path_validator.py:101-126`.
- `handlers/signup_signin.golden.json` — signup `admin.py:89-239` (redirect branches `:96-105,:108-117,:128-147,:153-169,:173-189,:192-208`; `zxcvbn<3` `:192`; create `:210-234`; success `general/` `:238`); signin `admin.py:242-358` (branches `:249-258,:265-275,:281-291,:297-307,:310-319,:322-332,:335-345`; success `:356-358`); password hashing `make_password` `:215`; login timestamps `:220-226,:347-353`.
- `handlers/configuration.golden.json` — GET `configuration.py:32-39`; PATCH `configuration.py:43-60` (strip `:49`, encrypt `:50-53`, `bulk_update` batch 100 `:57`); disable-email `configuration.py:63-86`; SMTP matrix `configuration.py:89-172` (receiver check `:92-96`; `int(EMAIL_PORT)` `:111`; `== "1"` TLS/SSL `:114-115`; 9 except branches `:131-172`).
- `handlers/workspace.golden.json` — slug-check `workspace.py:19-32` (400 `:25-29`, availability `iexact` OR restricted `:31`); POST `workspace.py:71-110` (400 name/slug `:78-82`, length `:84-88`, serializer errors `:100-103`, 409 `:105-110`, member create role 20 `:93-98`); restricted slugs `utils/constants.py:5-15`; IntegrityError fall-through BUG-3 `:105-110`.
- `handlers/base.golden.json` — `api/views/base.py` (TimezoneMixin `:34-39`; defaults `:42-51`; filter loop `:53-56`; exception matrix `:63-95`; dispatch quirk `:97-109` unreachable — `handle_exception` never re-raises so the `return exc` path never fires; `fields`/`expand` `:111-119`).

## Cross-cutting ported bugs (translate as-is)

- BUG-1: `primary_owner_details` always absent (`serializers/instance.py:12`; no `primary_owner` on `Instance`).
- BUG-2: `dispatch` returns `exc` not `response` (`license/api/views/base.py:107-109`; same in `app/views/base.py:161-163`) — non-DRF exceptions (e.g. `User.DoesNotExist`) become Django 500s.
- BUG-3: workspace POST non-"already exists" `IntegrityError` returns `None` (`api/views/workspace.py:105-110`) — Django 500.
- BUG-4: instance GET builds then discards `data`/`is_activated` (`api/views/instance.py:46-48` vs `:128`) — response is `{config, instance}` only.
- BUG-5: signup stores POST string/bool `is_telemetry_enabled` raw (`api/views/admin.py:125,233`) and `instance_name=company_name` which may be `""` (`:232`).
- BUG-6: signin admin check uses queryset truthiness not `.exists()` (`api/views/admin.py:335`).
- BUG-7: derived-flag seeding uses sequential `if`s not `elif` (`management/commands/configure_instance.py:64,88,112,140`) — harmless since keys distinct.
