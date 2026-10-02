# TRACE — D-22 v1_cli_auth runner-delete fixtures (V1CLIAUTH-F1..F4)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.
Each JSON file also carries its own `trace` key with the same mapping.

Scope: ONLY the unported unit `RunnerDeleteEndpoint` plus the models/queries/guards
its port needs. The domain's auth half (all six `api/v1/auth/*` routes) is already
ported by D-17 (PIDASHCONV-342 #808, PIDASHCONV-343 #817; fixtures AUTHOAUTH-F2/F9/F12)
and is NOT re-recorded here.

## Handler (V1CLIAUTH-F1)

- `V1CLIAUTH-F1.handler.golden.json` — `api/views/runner.py:29-57`
  (`RunnerDeleteEndpoint.delete` :44-57: by-pk 404 :45-47, view-guard 404 :48-49,
  manage-guard 403 :50-51, flag-parse 400 :52-55, delete + 204 :56-57);
  `api/urls/runner.py:9-14` (route `runners/<uuid:runner_id>/`, DELETE-only via
  `http_method_names=["delete"]` :12); `urls.py:24` (`api/v1/` include);
  `runner/services/runner_delete.py:146-163` (`parse_purge_local` —
  handler-observable query→status/body mapping only; canonical golden belongs to D-13).

## Models (V1CLIAUTH-F2)

- `V1CLIAUTH-F2.runner.columns.json` — `runner/models.py:382-498` (`Runner` fields
  :394-476, `Meta` :478-498: `db_table = "runner"` :479, ordering :480,
  `UniqueConstraint(pod, name)` :484-489, indexes :490-498); `:179-183`
  (`RunnerStatus`), `:186-187` (`Visibility.PRIVATE = 0`), `:190-204`
  (`RunnerProvisioning`); `:503-519` (`save()` pod auto-resolve, write-path note),
  `:521-536` (`project`/`project_id` `@property`, not columns), `:542+` (`revoke`,
  context only — D-13 owns delete internals). Plain `models.Model`: no `deleted_at`,
  no soft-delete manager.

## Queries (V1CLIAUTH-F3)

- `V1CLIAUTH-F3.lookup.sql` + `V1CLIAUTH-F3.lookup.rows.json` —
  `api/views/runner.py:45` (by-pk lookup `filter(pk=runner_id).first()` — no deleted
  filter, no join; `ORDER BY` + `LIMIT 1` from `Meta.ordering` + `.first()`, vacuous
  on a pk so the port uses plain `WHERE id = $1`); guard-facts read
  `runner/services/permissions.py:74-75` (view facts) and `:135-136` (manage facts)
  off the fetched row — no second query; `:137` (`is_workspace_admin`) unreachable.

## Guards (V1CLIAUTH-F4)

- `V1CLIAUTH-F4.guards.golden.json` —
  `runner/services/permissions.py:34-37` (`_authenticated_user_id`), `:40-54`
  (`runner_visible_to_user_q`, ORM sibling — handler does not use it), `:70-77`
  (`can_view_runner`), `:79-81` (`can_use_runner` alias), `:126-137`
  (`can_manage_runner`: view gate :133-134, PRIVATE owner-only :135-136, dead
  future-values branch :137); `runner/models.py:186-187` (`Visibility.PRIVATE = 0`).
  Handler-expected behavior only — the kernel already lives read-only in
  `rust-api/crates/auth/src/permissions/runner.rs`.

## Out of scope (not recorded here)

- `delete_runner` internals (pubsub frames, `revoke` cascade, row delete — D-13's
  service, D-13 owns `runner_delete.py`).
- Device-flow fixtures (already AUTHOAUTH-F*; do not duplicate).
- Auth/throttle spine (`APIKeyAuthentication`, `ApiKeyRateThrottle` — foundation,
  already ported; F1 notes only the handler-observable context).

## Ported bugs

- None observed on this path. The 403 branch (`api/views/runner.py:50-51`) and the
  `:137` owner-or-admin line are unreachable-but-by-design (kept so a future
  visibility value cannot silently mislabel access), not defects; the port keeps them.
