# TRACE — D-17 auth_oauth fixtures (AUTHOAUTH-F1..F12)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
Each JSON file also carries its own `trace` key with the same mapping.

## Models

- `F1_social_login_connection.columns.json` — `db/models/social_connection.py:1-43` (class 12-43; `Meta` 33-40; `__str__` 42-43); pk/audit shape `db/models/base.py:17-21` + `db/mixins.py` (`TimeAuditModel`/`UserAuditModel`/`SoftDeleteModel`).
- `F2_cli_device_code.columns.json` — `db/models/api.py:24-32` (generators), `:35-60` (`APIToken`, used subset), `:63-94` (`CLIDeviceCode`); goldens executed against CPython stdlib 2026-09-29 (`token_urlsafe(32)` → 43 chars; 28-char alphabet; `XXXX-XXXX` shape).

## Providers + error codes

- `F3_provider_auth_url.golden.json` — `authentication/provider/oauth/google.py:27-74`, `github.py:33-82`, `gitlab.py:25-77`, `gitea.py:24-86`; `urlencode` insertion order verified against CPython `urllib.parse.urlencode` 2026-09-29.
- `F4_provider_token_user_data.golden.json` — `authentication/adapter/oauth.py:75-103` (`get_user_token`, `get_user_response`, `set_user_data`); providers' `set_token_data`/`set_user_data` (+ `__get_email`) `google.py:76-115`, `github.py:84-182`, `gitlab.py:79-124`, `gitea.py:88-173`.
- `F5_error_codes.golden.json` — `authentication/adapter/error.py:41-50` (oauth rows; file order keeps 5122 before 5111/5112), `:77-92` (exception + `get_error_dict`); provider selector `authentication/adapter/oauth.py:49-59`.

## Queries

- `F6_account_upsert.before_after.json` — `authentication/adapter/oauth.py:105-136` (`OauthAdapter.create_update_account`); `Account` columns `db/models/user.py:280-302`.
- `F7_deactivate_token.before_after.json` — `authentication/services/cli_tokens.py:1-19` (full file); call site `authentication/views/cli/device.py:476-477`.
- `F8_device_helpers.golden.json` — `authentication/views/cli/device.py:69-129` (helpers); `DevMachine` `runner/models.py:331-377`; `MachineToken` `runner/models.py:811-869`; mint `runner/services/tokens.py:84-86`.

## Guards

- `F9_guards.golden.json` — `authentication/views/cli/device.py:54-66` (constants), `:132-143` (throttle + verification URI), `:145-511` (endpoint auth matrix); rate `settings/common.py:93-101`; OAuth routes `authentication/urls.py:66-125`; device routes `api/urls/auth.py:1-47`.

## Handlers

- `F10_initiate.golden.json` — `authentication/views/app/{google.py:28-62, github.py:30-64, gitlab.py:30-65, gitea.py:27-66}` + `views/space/{google.py:27-60, github.py:30-62, gitlab.py:30-63, gitea.py:28-70}`.
- `F11_callback.golden.json` — `authentication/views/app/{google.py:65-104, github.py:67-106, gitlab.py:68-107, gitea.py:69-107}` + `views/space/{google.py:63-102, github.py:65-104, gitlab.py:66-105, gitea.py:73-117}`.
- `F12_device_endpoints.golden.json` — `authentication/views/cli/device.py:145-188` (start), `:190-279` (approve), `:281-377` (token), `:379-402` (workspaces), `:405-483` (machine-token), `:485-511` (revoke).

## Cross-cutting ported bugs (translate as-is; layer issues must reproduce them)

- BUG-1: space google/github/gitlab callbacks 500 on every input — `base_host` session variable shadows the `base_host()` import, then every branch calls it (`views/space/google.py:63-102`, `github.py:65-104`, `gitlab.py:66-105`).
- BUG-2: app-gitea error/success `Location` is path-dropped or relative via `urljoin(base, ...)` (`views/app/gitea.py:27-66`, `:69-107`).
- BUG-3: gitlab `access_token_expired_at` sums `created_at + expires_in` with no `created_at` guard — `TypeError` when absent (`provider/oauth/gitlab.py:79-107`).
- BUG-4: github provider mutates the class-level `scope` attribute when an org id is set (`provider/oauth/github.py:59-60`) — leaks `read:org` into later instances in-process.
- BUG-5: `AuthenticationException` uses a mutable default `payload={}` (`authentication/adapter/error.py:77-92`).
- BUG-6: `OauthAdapter.create_update_account` swallows `DatabaseError`/`IntegrityError` via `log_exception` (`authentication/adapter/oauth.py:105-136`).
