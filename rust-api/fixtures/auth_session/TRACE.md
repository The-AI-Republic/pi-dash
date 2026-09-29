# TRACE — D-16 authentication session/email/magic/password/CSRF fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
Out of scope (other epics, NOT recorded): `provider/oauth/*`, `adapter/oauth.py`,
`views/app|space/{google,github,gitlab,gitea}.py` (D-17); `views/cli/device.py`,
`services/cli_tokens.py`, `CLIDeviceCode` (D-22); `SocialLoginConnection` (D-17).
Reading material only (no fixtures): `tests/smoke/test_auth_smoke.py`,
`tests/contract/app/test_authentication.py`.

Probe environment: python 3.12 + Django 5.2 + DRF + zxcvbn + celery/kombu 5.x +
fakeredis, sqlite :memory: for ORM execution, scratch Postgres `fx279scratch`
(created + dropped on the local /tmp:5432 socket) for the compiler strings and
the ProjectMember IntegrityError reproduction. Real model/view/provider/adapter
class bodies, byte-identical; loaded under shim app labels `db`/`lic` for the
Django registry only. Config resolver (`get_configuration_value`) and network
(S3/avatar, SMTP) stubbed with stated values; celery `.delay` and cache/track
calls captured via mock as call sequences (per the issue). Live Django on
:8000 was NOT touched (shared).

## Errors (FX-AUTH-01)

- `FX-AUTH-01.errors.json` — `authentication/adapter/error.py:5-74` (56-code
  table), `:77-92` (`get_error_dict` incl. shared-mutable-default BUG),
  `authentication/adapter/exception.py:17-34`, `authentication/rate_limit.py:21-28,40-47`,
  settings `EXCEPTION_HANDLER` (`settings/common.py:119`); per-view 400-vs-302
  mapping from the view sources named under FX-AUTH-07/08.

## Redirects (FX-AUTH-02)

- `FX-AUTH-02.redirects.json` — `utils/path_validator.py:13-145`,
  `authentication/utils/host.py:16-67`, `authentication/utils/redirection_path.py:8-46`,
  reset targets `authentication/views/app/password_management.py:112-166` +
  `space/password_management.py:125-159`, space success branches
  `views/space/email.py:96-101,180-185` + `views/space/magic.py:98-103,156-161`.

## Models (FX-AUTH-03)

- `FX-AUTH-03.models.json` — `db/models/session.py:14-56`,
  `db/models/user.py:56-137` (auth columns), `:142-197` (avatar_url/full_name/save/get_display_name),
  `db/models/user.py:200-277` (Profile reference), `db/mixins.py:16-27,85+`,
  `settings/common.py:599` (SESSION_ENGINE).

## Queries (FX-AUTH-04)

- `FX-AUTH-04.queries.json` — `db/models/user.py:56-61`,
  `db/models/session.py:30-56`, `license/models/instance.py:22-50`,
  `authentication/adapter/base.py:297,303-328,220-234`,
  `authentication/utils/user_auth_workflow.py:8-9`,
  `authentication/utils/workspace_project_join.py:20-91`,
  `db/models/workspace.py:198-259`, `db/models/project.py:302-384`,
  `utils/cache.py:54-70`, `utils/analytics_events.py:5`.

## Guards (FX-AUTH-05)

- `FX-AUTH-05.guards.json` — `authentication/rate_limit.py:17-47`,
  `authentication/middleware/session.py:22-92`, `authentication/session.py:8-11`,
  `authentication/adapter/exception.py:17-34`, `settings/common.py:73-106,597-607`,
  permission declarations `views/common.py:28,47,99`,
  `views/app|space/check.py:29-32`, `views/app|space/magic.py:33-36,31-33`,
  `views/app|space/password_management.py:45-48`.

## Providers (FX-AUTH-06)

- `FX-AUTH-06.providers.json` — `authentication/provider/credentials/email.py:18-96`,
  `authentication/provider/credentials/magic_code.py:25-147`,
  `authentication/adapter/base.py:64-120,220-360`,
  `authentication/adapter/credential.py:8-18`, `config/registry.py:38-40,82-84`.

## Handlers email (FX-AUTH-07)

- `FX-AUTH-07.handlers_email.json` — `authentication/views/app/check.py:34-103`,
  `space/check.py:34-101`, `views/app/email.py:26-238`, `views/space/email.py:25-191`,
  `views/app/signout.py:16-28`, `views/space/signout.py:17-33`,
  `authentication/utils/login.py:14-28`, `authentication/urls.py:51-57,118-120`.

## Handlers magic+password (FX-AUTH-08)

- `FX-AUTH-08.handlers_magic_password.json` — `views/app/magic.py:33-196`,
  `views/space/magic.py:31-170`, `views/app/password_management.py:38-176`,
  `views/space/password_management.py:38-160`, `views/common.py:28-138`,
  `app/serializers/user.py:15-60`, `authentication/urls.py:59-78,122-139`.

## Tasks (FX-AUTH-09)

- `FX-AUTH-09.tasks.json` — publish call sites `views/app/magic.py:54`,
  `views/space/magic.py:50`, `views/app/password_management.py:87`,
  `views/space/password_management.py:99`, `adapter/base.py:230`; task defs
  `bgtasks/magic_link_code_task.py:23`, `bgtasks/forgot_password_task.py:23`,
  `bgtasks/user_activation_email_task.py:23` (bodies owned by D-07).

## Cross-cutting ported bugs (translate as-is)

- BUG-1: `AuthenticationException.__init__` shared mutable default `payload={}`
  (`adapter/error.py:82`) — mutations leak across instances (probe-verified).
- BUG-2: `is_signup = bool(user)` is inverted (`adapter/base.py:299`) —
  new-user signup reports False, existing-user login reports True.
- BUG-3: `ProjectMember` bulk_create omits `project_id`
  (`utils/workspace_project_join.py:76-87`) — Postgres IntegrityError on any
  signup with an accepted project invite (reproduced on real Postgres).
- BUG-4: magic-generate invalid/empty email raises uncaught django
  `ValidationError` → 500 (`views/app/magic.py:50`, `space/magic.py:46`).
- BUG-5: reset app malformed uidb64 raises uncaught UUID `ValidationError` →
  500 (`views/app/password_management.py:104-105`); reset space unknown id
  raises uncaught `DoesNotExist` → 500 (`space/password_management.py:116`).
- BUG-6: app success-login redirect drops the computed path
  (`get_redirection_path` returns slash-less paths rejected by
  `validate_next_path`) and drops explicit `next_path` when the APP host is not
  in allowed hosts — observed bare-base redirects.
- BUG-7: `throttle_failure_view` ×2 is dead code (zero callers); the live 429
  path is DRF default → `auth_exception_handler`.
- BUG-8: success `next_path` handling + `success=True` capital-T + space double
  slash + `email=False` missing-key payload + soft-deleted members still
  redirect (join carries no `deleted_at` filter) — quirks recorded in FX-02/07.
