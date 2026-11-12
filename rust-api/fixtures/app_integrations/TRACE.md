# TRACE — D-33 app_integrations fixtures (PIDASHCONV-337)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

How values were produced: rows marked `"executed": true` ran against
verbatim-extracted logic via throwaway probe `/tmp/fx_d33_probe.py` (venv
python: real `ipaddress`, `hmac`/`hashlib`, `uuid4`, DRF `URLField`, Django
`ValidationError`; Django-model seams stubbed) plus a standalone DRF
`URLField.run_validation` check. DB/network branches are transcribed with
exact code refs; the domain gate (PIDASHCONV-455) re-verifies against live
Django via PIDASHCONV-91.

PORTED BUGS (translate as-is; listed for the record):
- B1 `db/models/webhook.py:27-31` — validate_domain compares `netloc`
  (host:port), so `http://localhost:8000/hook` PASSES. EXECUTED.
- B2 `app/views/webhook/base.py:84` — PATCH passes `context={request:
  request}` (request object as key), so the request-host domain is never
  appended on update.
- B3 `app/views/external/base.py:235` — Unsplash search URL has stray
  `page=${page}`; port byte-for-byte.
- B4 `bgtasks/webhook_task.py:313-321` — HMAC over `json.dumps` default
  separators, not the wire bytes (also recorded in D-08 FX-WEB-03).
- B5 `app/views/external/base.py:159,163` — `task`/`prompt` default to
  `False`, not `""`; LLM error detail swallowed to one generic 500 string.
- B6 `utils/github_client.py:298` — PR `state` maps to `closed` only on
  exact `"closed"`; `"merged"` renders as `open`.
- B7 `app/views/integration/github.py:1021-1029` — stale comment cites
  OneToOne uniqueness; field is FK. Behaviour (hard delete) is right.

## FX-WEB-01 webhook CRUD

- `fx-web-01-webhook-crud.json` — `app/views/webhook/base.py:20-108`
  (post `:22-36`, list `:39-58`, detail `:59-76`, patch `:78-102`,
  delete `:104-108`); soft delete `db/mixins.py:72-79`; roles
  `app/permissions/base.py:19-51`.

## FX-WEB-02 secret-regenerate

- `fx-web-02-secret-regenerate.json` — `app/views/webhook/base.py:111-118`
  (regenerate), `:121-126` (logs); `generate_token`
  `db/models/webhook.py:17-18` (EXECUTED shape).

## FX-WEB-03 webhook-log queries

- `fx-web-03-webhook-log-queries.json` — read query
  `app/views/webhook/base.py:124`; row shape `bgtasks/webhook_task.py:99-110`;
  columns `db/models/webhook.py:65-89`; ordering `:88`. Write path owned by
  D-08 (`tasks_webhooks/fx-web-01-save-webhook-log.json`).

## FX-WEB-04 WebhookSerializer golden I/O

- `fx-web-04-webhook-serializer.json` — field
  `app/serializers/webhook.py:20`; create `:22-55`; update `:57-90`;
  Meta `:92-102`; every ValidationError branch (`:27-28`, `:33-37`,
  `:41-42`, `:52-53`, `:62-63`, `:68-72`, `:76-77`, `:87-88`); DRF
  field messages EXECUTED.

## FX-WEB-05 SSRF/DNS guard

- `fx-web-05-ssrf-guard.json` — guard expr
  `app/serializers/webhook.py:39-42` + `:74-77` (EXECUTED matrix);
  domain rule `:52` + `:87` (EXECUTED); `validate_schema`
  `db/models/webhook.py:21-24` (EXECUTED); `validate_domain`
  `db/models/webhook.py:27-31` (EXECUTED incl B1).

## FX-MDL-01 column lists (15 models)

- `fx-mdl-01-model-columns.json` — Webhook/WebhookLog
  `db/models/webhook.py:34-89`; Integration/WorkspaceIntegration
  `db/models/integration/base.py:16-60`; GitProviderAccount/GitRepository/
  GitRepositoryBinding(+5 siblings) `db/models/integration/git.py:11-344`;
  Github* (8) `db/models/integration/github.py:15-262`; base cols
  `db/models/base.py:17-22`, `db/models/project.py:302-311`,
  `db/mixins.py:16-70`. D-05 structs
  `rust-api/crates/db/src/integrations/` (spot-verified GitProviderAccount).

## FX-GIT-01 provider-account CRUD

- `fx-git-01-provider-account.json` — views
  `app/views/integration/git.py:34-130`; `_error_response` `:38-47`;
  `create_provider_account` `integrations/git/services.py:72-107`;
  `normalize_host_url` `:43-47` (EXECUTED); `serialize_provider_account`
  `:264-279`; `list_account_repositories` `:381-389`;
  `provider_payload` `integrations/git/registry.py:45-53`; adapter keys
  `adapters/github.py:51-53`, `adapters/gitlab.py:185-187`.

## FX-GIT-02 repo bind

- `fx-git-02-repo-bind.json` — views `app/views/integration/git.py:132-175`;
  `bind_repository` `services.py:297-351`; `get_binding` `:353-358`;
  `set_binding_sync_enabled` `:361-372`; `unbind_repository` `:374-379`;
  `serialize_binding` `:280-295`; `serialize_repository` `:232-245`;
  `parse_repository_url` `registry.py:29-34`; `parse_github_repo_url`
  `utils/github_client.py:236-254` (EXECUTED); account resolution
  `services.py:110-160`.

## FX-GHA-01 app-flow

- `fx-gha-01-app-flow.json` — install-start
  `app/views/integration/github.py:658-695`; callback `:730-808`;
  app-webhook `:860-956` (dedupe `:894-917`, routing `:919-955`);
  `_normalize_private_key` `utils/github_app_auth.py:54-60` (EXECUTED);
  `parse_github_datetime` `:106-109` (EXECUTED); HMAC verify `:201-210`
  (EXECUTED vectors); config `:33-73`; JWT/headers/token cache
  `:76-198` (transcribed).

## FX-GHA-02 workspace connect/disconnect/repos

- `fx-gha-02-workspace.json` — connect `:426-491`; disconnect `:494-537`;
  status `:540-562`; repos `:565-605` + `_serialize_repo` `:270-279`
  (EXECUTED); app-status `:611-655`; refresh `:698-727`; helpers
  `:90-137` (integration rows), `:170-188` (session cleanup), `:191-267`
  (refresh/upsert), `:282-352` (git account upserts).

## FX-GHA-03 project bind/status

- `fx-gha-03-project.json` — bind `:962-1082`; status get `:1093-1146`;
  patch `:1148-1172`; delete `:1174-1205`; `_upsert_git_binding_for_github_sync`
  `:355-420`; `_refresh_pr_links` `:811-857`; `parse_github_repo_url` +
  `pr_snapshot_from_payload` `utils/github_client.py:236-302` (EXECUTED).

## FX-EXT-01 LLM provider/model table + get_llm_config

- `fx-ext-01-llm.json` — provider tables `app/views/external/base.py:42-73`;
  `get_llm_config` `:76-120`; `get_llm_response` `:123-145`;
  project/workspace GPT endpoints `:148-212`; Unsplash `:215-243`
  (transcribed; network shapes recorded, not executed).

## FX-TSK-01 webhook-dispatch

- `fx-tsk-01-webhook-dispatch.json` — `webhook_activity`
  `bgtasks/webhook_task.py:377-461` (filter `:420-433`, enqueue `:435-451`,
  outer except `:453-460`); `model_activity` `:463-506`;
  `get_model_data` `:143-187`; HMAC wire quirk `:313-321` (EXECUTED,
  shared with D-08 `tasks_webhooks/fx-web-03-webhook-send-task.json`).

## FX-PERM-01 permission matrix (24 paths / 27 method rows)

- `fx-perm-01-permission-matrix.json` — routes
  `app/urls/integration.py:30-119` (17), `app/urls/webhook.py:14-31` (4),
  `app/urls/external.py:12-24` (3); roles
  `app/permissions/base.py:13-64`; view defaults
  `app/views/base.py:189-194`.
