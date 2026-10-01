# TRACE — D-23 api-v1 OpenAPI schema fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`
(zero diff Ported-from→HEAD on all sources below, verified 2026-10-01).

Disposition for this domain is minimal: utoipa-generated schema,
byte-identity NOT required, route coverage is. FX-OPENAPI-04 is the pinned
subset the doc builder renders; FX-OPENAPI-03 is the traceability inventory
of the annotation vocabulary (no served behaviour of its own).

Live captures ran against Django from this checkout on scratch Postgres
`pidash_525_test` (own cluster, `pi_dash.settings.test`) with
`ENABLE_DRF_SPECTACULAR=1`, except the flag-off section of FX-OPENAPI-05
(same server restarted with the flag unset). drf-spectacular 0.28.0,
djangorestframework 3.15.2, Django 4.2.30.

## FX-OPENAPI-01 doc meta goldens

- `FX-OPENAPI-01.meta.json` — `pi_dash/settings/openapi.py:11-279`
  (`SPECTACULAR_SETTINGS` verbatim: TITLE `:15`, DESCRIPTION `:16-20`,
  CONTACT `:21-25`, VERSION `:26`, LICENSE `:27-30`,
  SERVE_INCLUDE_SCHEMA `:34`, SCHEMA_PATH_PREFIX `:35`,
  SCHEMA_CACHE_TIMEOUT `:36`, PREPROCESSING_HOOKS `:40-42`,
  POSTPROCESSING_HOOKS `:43-45`, SERVERS `:49-52`, TAGS `:56-263` —
  all 14 names + descriptions in order, AUTHENTICATION_WHITELIST
  `:267-269`, COMPONENT_NO_READ_ONLY_REQUIRED `:273`,
  COMPONENT_SPLIT_REQUEST `:274`, ENUM_NAME_OVERRIDES `:275-278`).
  Method: imported `SPECTACULAR_SETTINGS` with Django configured and
  dumped verbatim. `openapi_version` "3.0.3" is the live-doc rendering
  (provenance: FX-OPENAPI-04 doc capture; drf-spectacular default).

## FX-OPENAPI-02 hook goldens

- `FX-OPENAPI-02.hooks.json` — `pi_dash/utils/openapi/hooks.py:14-23`
  (`preprocess_filter_api_v1_paths`, 19 accept/reject vectors), `:26-56`
  (`generate_operation_summary`, 18 vectors — NOTE dead code: defined
  here, re-exported `__init__.py:196,366`, zero call sites repo-wide;
  recorded for the verbatim port anyway), `:59-94`
  (`postprocess_project_id_dual_form`, before/after fragment).
  Method: imported hooks.py by file path (zero imports, no Django) and
  executed each function over the vectors (postprocessor over a synthetic
  doc fragment; generator/request/public args unused, passed None/None/True).
  Notes: the `PUT` exclusion is case-insensitive (`method.upper()`); the
  `server` substring clause is over-broad by construction (`observer`
  contains `server`) but currently drops nothing beyond the prefix rule —
  no served `/api/v1/` route contains `server` (assistant mcp-servers/*
  live under `/api/`; zero `server` paths in the golden route map).

## FX-OPENAPI-03 annotation-vocab inventory

- `FX-OPENAPI-03.vocab.json` — `pi_dash/utils/openapi/parameters.py:17-607`
  (38 `OpenApiParameter` constants: 18 path / 20 query; UUID 16, STR 18,
  BOOL 1, INT 3), `responses.py:17-490` (39 `OpenApiResponse` constants +
  `create_paginated_response` `:358-405`: 12-key envelope shape and example
  value, two executed calls), `examples.py:16-966` (57 `OpenApiExample`
  constants `:16-754`, 16 `SAMPLE_*` dicts + `SCHEMA_EXAMPLES` keys
  `:756-919` with the key→sample mapping, `get_sample_for_schema`
  `:921-938` with 8 vectors incl. `Paginated*` stripping and
  unknown-name → `SAMPLE_GENERIC` fallback, `PAGE_*` `:940-966`),
  `decorators.py:17-342` (`_merge_schema_options` `:17-30` with 4 merge
  vectors, all 21 `*_docs` factories `:33-342` with tags/parameters/
  responses defaults — NOTE `sticky_docs` `:286-299` is defined but NOT
  re-exported in `__init__.py`; imported directly by
  `api/views/sticky.py`), `__init__.py:17-367` (159-name `__all__`
  surface; defined-but-not-exported is exactly `sticky_docs` and
  `postprocess_project_id_dual_form`, the latter wired by dotted path in
  `settings/openapi.py:44`).
  Method: AST parse for exact definition line spans + runtime
  introspection of the live objects (Django configured, no DB);
  decorator defaults captured by running each factory with
  `extend_schema` patched in-module; merge/get_sample/paginated helpers
  executed over recorded vectors.
  Reading notes recorded in-file: decorator tag names outside the 14
  settings tags (`Workspaces`, `Stickies`, `Estimates`, `Estimate
  Points`, `Pages`, `Work Item Relations`); `utils/openapi/README.md` is
  stale (names `APITokenAuthenticationExtension` and
  `postprocess_assign_tags`, neither exists).

## FX-OPENAPI-04 live-doc essentials capture

- `FX-OPENAPI-04.doc_essentials.json` — live `GET /api/schema/` (+
  `?format=json`) against Django with `ENABLE_DRF_SPECTACULAR=1` (routes
  `pi_dash/urls.py:31-44`); per-operation essentials (operationId, tags,
  summary, parameters, responses — verbatim) for every path × method in
  the golden route map (121 paths, 189 operations); default content-type
  `application/vnd.oai.openapi; charset=utf-8` (YAML, no `+json`),
  `?format=json` content-type `application/vnd.oai.openapi+json`, parsed
  doc-equality proof (`parsed_equal: true`, 121 paths each), route-map ==
  golden assertion, `ApiKeyAuthentication` securityScheme block verbatim
  (rendered from `utils/openapi/auth.py:15-34`
  `APIKeyAuthenticationExtension.get_security_definition`).
  Method: httpx GETs; YAML parsed with `yaml.safe_load`, JSON with
  `r.json()`; equality asserted on the parsed docs.

## FX-OPENAPI-05 handler behaviour goldens

- `FX-OPENAPI-05.handlers.json` + `FX-OPENAPI-05.swagger_ui.normalized.html`
  + `FX-OPENAPI-05.redoc.normalized.html` — live HTTP fetches (routes
  `pi_dash/urls.py:31-44`; throttle `settings/common.py:92-94`
  `AnonRateThrottle` 30/minute over the default django-redis cache,
  observed key `:1:throttle_anon_127.0.0.1`; APPEND_SLASH
  `CommonMiddleware`; denial rewrite
  `authentication/adapter/exception.py:17-34` reading context, code table
  `authentication/adapter/error.py:71` `RATE_LIMIT_EXCEEDED = 5900`).
  `?format=xml` → 404 + YAML body bytes (`detail: {string, code}`
  ErrorDetail rendering); unsafe methods on all 3 endpoints → 405 + `Allow:
  GET, HEAD, OPTIONS` + body bytes per endpoint kind (YAML renderer on
  `/api/schema/`, `405 Method Not Allowed` TemplateHTMLRenderer exception
  fallback on the UI pages); `GET /api/schema` → 301 + `Location:
  /api/schema/` (relative) + empty body; swagger-ui (4653 bytes, 1 CSRF
  hole, 64-char token) + redoc (736 bytes, no hole) page bytes normalized
  with the contract-suite rule (`CSRFTOKEN"] = "[^"]*";` → `CSRFTOKEN"] =
  "";`); burst → clean allow-30-then-deny (first 429 at attempt 31,
  `Retry-After: 59`, per-renderer bodies: UI pages `429 Too Many
  Requests` fallback text, YAML `error_code: 5900 / error_message:
  RATE_LIMIT_EXCEEDED`, JSON 4-space `{"error_code": 5900,
  "error_message": "RATE_LIMIT_EXCEEDED"}`), unsafe-while-throttled → 429
  not 405 (throttle check runs before handler selection), window reset
  re-allows (200 after 65s); flag off → all three routes absent (Django
  URL-resolution 404, DEBUG technical page — length + sha + head recorded,
  full bytes environment-specific).
  Method: httpx fetches; private redis db 13 flushed before the denial
  section and before the burst; flag-off from the same server restarted
  with the flag unset.

## FX-OPENAPI-06 route table provenance

- `FX-OPENAPI-06.routes_golden.json` — byte copy of
  `rust-api/contract-tests/v1_openapi/routes_golden.json` from PIDASHCONV-81
  (`54254ec8`, PR #479): 121 paths, sha256
  `1177ecc6fc06fa7fd3f33bf89554f3aec214ec05ee23818416c961eb871d813f`.
  Regen rule (from `test_routes_golden.py`, never blanket-update):
  `REGEN_GOLDEN=1 pytest v1_openapi/test_routes_golden.py` + reviewed
  diff. The live capture in FX-OPENAPI-04 asserted route-map equality
  against this file at capture time.

## Out of scope (reading context, not ported here)

- `pi_dash/urls.py:31-44` (the 3 schema routes + `ENABLE_DRF_SPECTACULAR`
  gate) — served-surface context for the handlers sub-issue.
- `pi_dash/settings/common.py:92-94` (AnonRateThrottle 30/minute) and
  `:745-750` (spectacular enable block) — context for the throttles
  sub-issue.
- `pi_dash/api/urls/schema.py` — exists but unwired (not imported by
  `pi_dash/api/urls/__init__.py`): no `/api/v1/schema/*` routes; out of
  scope per the PIDASHCONV-81 client notes.
- `authentication/adapter/exception.py:17-34` + `error.py:71` — denial
  rewrite mechanism (reading context for FX-OPENAPI-05 bodies).
