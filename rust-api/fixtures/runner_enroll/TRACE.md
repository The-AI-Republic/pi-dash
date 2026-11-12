# TRACE — D-13 runner enrollment/auth/machine fixtures (PIDASHCONV-578)

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`.
Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52` (drift baseline;
`git diff <base>..HEAD` over every D-13 source below is empty on 2026-10-01).

Method per fixture is recorded inside the file under `method`. Pure helpers
were executed against the real source offline (system python + Django 6.0.5,
`settings.configure()` with a documented fixed SECRET_KEY for tokens;
single-function AST extraction with stub settings for helpers in
Django-heavy modules; equivalent-DRF-field probes for framework message
strings and null-vs-omitted semantics). Generators live outside the repo
(`/tmp/d13_*.py`, not committed). SQL is hand-composed from the cited ORM
calls (no live Postgres on this runner — same precedent as the D-06
assistant and D-11 dispatch fixtures) with shape-derived example rows;
the one load-bearing ORM semantic (`save(update_fields=...)` /
`QuerySet.update()` vs `auto_now`) was verified empirically against the
installed Django (probe `/tmp/d13_save.py`).

## D13-F1 models

- `models/columns.json` — `runner/models.py:27-39`
  (`KNOWN_REVOKE_REASONS`, `_REVOKE_REASON_MAX_LEN`); `:42-49`
  (`PodManager`); `:52-176` (`Pod` incl `Meta` `:98-122`, save denorm
  `:145-159`, `default_for_project_id` `:173-176`); `:179-206`
  (`RunnerStatus`/`Visibility`/`RunnerProvisioning`); `:331-379`
  (`DevMachine`); `:382-499` (`Runner`, `MAX_PER_USER` `:392`) +
  `:503-519` (save auto-resolve) + `:538-540` (`mark_heartbeat`);
  `:765-786` (`RunnerForceRefresh`); `:811-870` (`MachineToken`, `revoke`
  `:865-870`). Read-only ref: `runner/services/matcher.py:363-372`
  (`MAX_PER_USER` enforcement site).

## D13-F2 serializers

- `serializers/shapes.golden.json` — `runner/serializers.py:27-33`
  (`RUNNER_NAME_CHARSET`); `:36-72` (`PodSerializer`, `get_runner_count`
  `:71-72`); `:75-101` (`RunnerLiveStateSerializer`, nested shape);
  `:104-130` (`DevMachineSerializer` incl annotated fields);
  `:133-142` (`PodMiniSerializer`); `:145-153`
  (`DevMachineMiniSerializer`); `:156-220` (`RunnerSerializer` incl
  `live_state` null); `:223-238` (`RunnerEnrollRequestSerializer`);
  `settings/common.py:361-362` (UTC datetimes). Mismatch note cites
  `runner/views/pods.py:226-228`.

## D13-F3 tokens

- `tokens/tokens.golden.json` — `runner/services/tokens.py:34-40`
  (prefixes + TTLs); `:43-54` (pepper/hash/fingerprint); `:57-92`
  (`MintedToken` shapes + minters); `:101-118` (`_key_ring` derived
  path); `:135-170` (`mint_access_token` claims); `:182-219`
  (`decode_access_token` error codes); `settings/common.py:480`
  (`ACCESS_TOKEN_TTL_SECS`), `:592` (`RUNNER_ACCESS_TOKEN_KEYS = []`).

## D13-F4 guards

- `guards/authn.golden.json` — `runner/authentication.py:46-53`
  (`_bearer`); `:56-180` (`RunnerAccessTokenAuthentication`, mt_
  branch `:120-162`, JWT branch `:85-118`); `:183-202`
  (refresh-token parse); `:205-240` (machine-token path);
  `:243-254` (`resolve_runner_for_run`);
  `runner/services/permissions.py:34-37,40-54,57-80,126-137`
  (visibility predicates); `core/permissions.py:28-34,61-64`
  (membership/admin); `api/middleware/api_authentication.py:20-76`
  (`APIKeyAuthentication`, consumed);
  `authentication/adapter/exception.py` +
  `authentication/adapter/error.py` (429 shape);
  `settings/common.py:91-94` (throttle defaults);
  `managed_runner/permissions.py` (`IsDesktopSession`, consumed).

## D13-F5 queries

- `queries/enroll_refresh.sql` + `queries/enroll_refresh.rows.json` —
  `runner/views/enrollment.py:284-358` (enroll tx), `:394-467`
  (refresh row-lock), `:629-680` (create-endpoint reads), `:691-744`
  (create tx body); `runner/services/tokens.py` (mint shapes, by ref).
- `queries/bootstrap.sql` (+ rows in `enroll_refresh.rows.json`) —
  `runner/views/enrollment.py:72-128` (dev-machine get-or-create),
  `:130-203` (bootstrap/rotate), `:57-69` (`_touch_dev_machine` write).
- `queries/machines_runners.sql` + `queries/machines_runners.rows.json` —
  `runner/views/runners.py:52-59` (control-online subquery), `:73-125`
  (machine list + annotations), `:132-147` (scope helpers), `:150-170`
  (single-machine serialize), `:189-246` (revoke/rotate/delete writes),
  `:289-333` (runner list + visibility), `:342-425` (detail/patch
  reads + busy-guard), `:455-492` (revoke read-then-revoke).
- `queries/pods_projects_desktop.sql` +
  `queries/pods_projects_desktop.rows.json` —
  `runner/views/pods.py` (CRUD + delete guards + Issue sweep),
  `runner/views/projects.py:31-77` (serialize) + `:97-124`
  (auth-mode dispatch), `runner/views/desktop.py:107-149`
  (enroll) + `:168-194` (delete),
  `runner/views/machine_commands.py:63-109,208-242` (command reads).
- `queries/ticket_redis.json` — `runner/views/enrollment.py:799-932`
  (ticket/redeem Redis ops; JSON, not SQL).
- `queries/revoke_cascade.sql` + `queries/revoke_cascade.rows.json` —
  `runner/models.py:592-685` (revoke cascade SQL S1-S4 + on_commit hooks).

## D13-F6 services

- `services/flows.golden.json` —
  `runner/views/enrollment.py:57-69` (`_touch_dev_machine`),
  `:227-255` (`_managed_cap_error`), `:541-542` (`_RUNNER_NAME_RE`),
  `:613-623` (invalid-name body), `:790-796`
  (`_next_auto_runner_name`);
  `runner/services/runner_delete.py:47-143` (delete order-of-ops),
  `:146-163` (`parse_purge_local`);
  `runner/services/pod_naming.py` (validators + consts);
  `runner/services/validation.py:48-182` (`validate_run_creation` +
  `_resolve_pod`); `runner/views/desktop.py:37-62`
  (`_version_is_allowed`); `runner/views/machine_commands.py:51-73`
  (consts + `_scoped_machine`), `:128-157` (create message shape);
  `runner/models.py:542-685` (revoke cascade steps).

## D13-F7 handlers

- `handlers/endpoints.golden.json` — `pi_dash/urls.py:23,26`
  (mounts); `runner/urls.py` + `runner/web_urls.py` (routes);
  `runner/views/enrollment.py` (enroll, refresh, self-revoke,
  create, ticket, redeem, invite/revive 410s);
  `runner/views/register.py` (health);
  `runner/views/runners.py` (machines, runners, revoke);
  `runner/views/pods.py` (pods); `runner/views/projects.py:80-124`
  (3 auth modes); `runner/views/desktop.py` (post/delete);
  `runner/views/machine_commands.py` (create/status/result).

## D13-F8 external (READ-ONLY pins — trace, never port)

- `external/wire_pins.json` — `runner/services/matcher.py:54-66`
  (`NON_TERMINAL_STATUSES`), `:242-252` (`drain_pod_by_id`);
  `runner/services/pubsub.py:39-163` (verb arg shapes);
  `runner/services/machine_outbox.py:336-368` (cmd-result key/TTL/JSON);
  `runner/views/machine_sessions.py:54-66` (`_auth_dev_machine`);
  `runner/services/agent_run_finalization.py:48-88`
  (`finalize_agent_run`); `runner/services/outbox.py:113-114,562-580`
  (stream-cleanup zset); `orchestration/service.py:644`
  (`complete_project_move_handoff`);
  `authentication/services/cli_tokens.py:12-19`
  (`deactivate_api_token`); `settings/redis.py:64-117`
  (`redis_instance` None semantics);
  `ee/authentication/desktop.py:25` (`request_is_desktop`).

## Ported quirks (translate as-is, listed in the PR)

- `get_runner_count` counts revoked runners; `pods.py:228` comment
  claims it matches the guard that excludes them — the comment is wrong.
- Unknown `?pod=` name on create falls through to the default pod (no 404).
- `?workspace=` membership probe on the projects list (and its
  unfiltered variant) omits `is_active=True`, unlike every other D-13 check.
- Health reports `protocol_version` 3; enroll/create bodies report 4.
- Ticket `{"error": "workspace not found"}` (lowercase+space) vs
  `workspace_not_found` everywhere else; ticket docstring URL is stale.
- Ticket still 201s when Redis is `None` (unredeemable ticket).
- Pod create is admin-only; rename/delete are admin-or-creator.
- Pod demote (`is_default: false`) may leave a project with no default.
- Pod-create `(project, name)` `IntegrityError` is uncaught (500).
- Machine rotate kills sessions but leaves runner rows un-revoked.
- `close_runner_session`'s `code` param is accepted and ignored.
- `redis_instance()` never returns `None` in production (raises
  without `REDIS_URL`); all `None` branches are mock-driven.
- `send_runner_revoke` frames carry no `runner_id`/`mid`; `remove_runner`
  frames carry `runner_id` but no `mid` (only `send_to_*` envelopes).
- `save(update_fields=[...])` without `updated_at` (enroll, refresh,
  revoke cascade, `MachineToken.revoke`, `mark_heartbeat`) leaves
  `updated_at` untouched — verified against Django 6.0.5.
