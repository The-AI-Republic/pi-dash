# Expected parity differences (PIDASHCONV-820)

Every row below is one live difference between Django and Rust that has
been reviewed and classified. The CI job
(`.github/workflows/rust-api-parity.yml`) runs

```sh
python rust-api/contract-tests/parity/route_inventory.py check --binary <pidash-api>
python rust-api/contract-tests/parity/job_inventory.py check --binary <pidash-api>
```

and fails when the live diff contains anything not listed here, or when
a listed row no longer appears live (a fix landed: delete its lines).
The ```` ```expected-routes ```` / ```` ```expected-jobs ```` fenced
blocks are machine-read (one `gap :: subject :: detail` signature per
line, sorted); the prose around them is for reviewers. Never add a
`G0`/`W0` line: unclassified rows must be resolved by hand, given a gap
id, and filed as a fix issue first.

Counts (2026-10-17, `pi_dash.settings.test`):

- Routes: 528 Django rows — 521 OWNED, 7 MISSING (all listed below),
  9 Rust-only (all justified, in `RUST_ONLY_JUSTIFICATIONS`).
- Jobs: 77 tasks — 77 OWNED, 0 PROXIED, 0 MISSING;
  beat 28/28 OWNED (same task, same cadence on both sides).

## Routes

### G1 — axum auto-HEAD serves what Django 405s (0 rows; fixed)

Fix: PIDASHCONV-826 (landed). Axum auto-served HEAD wherever GET was
routed; the premise was that Django 405s HEAD there for want of a HEAD
arm. PIDASHCONV-864 corrected the premise — Django's `View.setup`
binds `head = get`, so Django serves HEAD on plain GET views — but the
proxy arms stay: the proxy returns Django's own answer either way.
Every affected route now carries an explicit HEAD proxy arm and its
inventory row lists HEAD as proxied, so the 247 rows reclassified to
OWNED (signatures deleted below). The two magic-generate HEAD gaps
lived under G1+G4 below (their HEAD gap was
`drf-405body-vs-axum-405`, not `serves-but-django-405`);
PIDASHCONV-829's proxy arms resolve them, so that section is empty too.

```expected-routes
```

### G1+G4 — magic-generate routes (resolved by PIDASHCONV-829)

Both rows are gone: the proxy arms (including HEAD via axum's `get`
handling) make the generate routes match Django on every method.

### G2 — license instance-admin session/auth endpoints unported (0 rows; fixed)

Fix: PIDASHCONV-827 (landed). The 7 paths are Rust-owned (the 5 merged
`handlers_admin` routes were wired into `license::routes()`,
`sign-in/`/`sign-up/` were ported, every route carries an explicit
`.head(proxy)` arm so no HEAD gap remains). No live rows; the header
counts already reflect this.

```expected-routes
```

### G3 — DRF router artifacts: api-root index + format suffixes (0 rows; fixed)

Fix: PIDASHCONV-828 (landed). The `/api/v1/workspaces/{slug}/` api-root
index and the `.{format}` suffix variants Django's router generates had
no Rust route; all four now reclassify to OWNED via all-proxied
inventory rows (edge fallback) plus contract pins, so the rows are
deleted below.

```expected-routes
```

### G4 — magic-link sign-in/up routes missing proxy arms (resolved by PIDASHCONV-829)

All four rows are gone: the proxy arms make the sign-in/up routes
match Django on every method (OPTIONS 200 `Allow`, `Allow`-bearing
405s). The two generate routes were under G1+G4 above.

### G5 — DRF format suffixes on detail routes (0 rows; fixed)

Fix: PIDASHCONV-828 (landed, with G3). The `{pk}.{format}` detail
variants had no Rust route; both now reclassify to OWNED via
all-proxied inventory rows (dotted-pk proxy in the detail handlers)
plus contract pins, so the rows are deleted below.

```expected-routes
```

### G6 — handler-denial 405s, proven (OWNED-by-denial) (7 rows)

Fix: PIDASHCONV-830. Proven by
`contract-tests/app_workspace/test_denied_methods.py` (green on both
backends): every `serves-but-django-405` segment below is an
explicit `*_not_allowed` handler replaying the view's auth + permission
prelude — 401 anonymous, 403 or the exact 405 bytes authenticated, and
DRF's `Allow` on all three — instead of proxying a byte-identical
denial. Behaviorally OWNED; the diff reports it only because Rust
serves where the inventory says Django 405s.

The 6 GET routes served HEAD all along: Django's `View.setup` binds
`head = get` (`django/views/generic/base.py`), so HEAD runs the GET
handler at runtime (Rust routes `.head(<get_handler>)` explicitly;
`test_head_serves` pins the served statuses). The checker used to miss
the binding and reported `HEAD:serves-but-django-405` artifacts on
those routes; PIDASHCONV-864 taught
`route_inventory._allowed_methods` the binding, so those segments are
gone from the fenced lines below. Only the POST-only export route
truly 405s HEAD, and that denial is proven like the rest.

```expected-routes
G6 :: /api/users/me/workspaces/ :: POST:serves-but-django-405;PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405
G6 :: /api/workspace-slug-check/ :: POST:serves-but-django-405;PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405
G6 :: /api/workspaces/ :: PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405
G6 :: /api/workspaces/{slug}/ :: POST:serves-but-django-405
G6 :: /api/workspaces/{slug}/user-activity/{user_id}/export/ :: GET:serves-but-django-405;PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspaces/{slug}/workspace-themes/ :: PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405
G6 :: /api/workspaces/{slug}/workspace-themes/{pk}/ :: POST:serves-but-django-405;PUT:serves-but-django-405
```

### Rust-only routes (9, all justified in-script)

New Rust-only surface fails the check until a justification is added to
`RUST_ONLY_JUSTIFICATIONS` in `route_inventory.py` (reviewed). Current:

- `/healthz` — foundation liveness probe; Django answers its 404 page
  here by design.
- `/api/schema`, `/api/schema/`, `/api/schema/redoc/`,
  `/api/schema/swagger-ui/` — served when the v1-openapi flags are on;
  Django only with `ENABLE_DRF_SPECTACULAR=1` (off in contract envs).
- `/api/v1/workspaces/{slug}/issues/{segment}/`,
  `/api/v1/workspaces/{slug}/work-items/{segment}/`,
  `/api/workspaces/{slug}/work-items/{tail}/` — single-param capture of
  Django's compound `{project_identifier}-{issue_identifier}` (the
  `ALIASES` join; dashless segments proxy so Django answers its own
  404 — behaviorally exact, hence not rust-only).
- The four v1 scheduler read routes (`.../schedulers/`,
  `.../projects/{project_id}/schedulers[/{scheduler_id}[/runs]]/`) —
  stage-8 drift (PIDASHCONV-1061): live on `origin/main` (04920ea3) but
  rust-dev's Django is frozen pre-drift, so the check sees them as
  Rust-only until PIDASHCONV-1055 syncs `main` into `rust-dev` (then
  they reclassify to OWNED and the justifications are deleted).

## Jobs

Task dispositions: OWNED (the Rust worker runs it) or MISSING (a live
Django task absent from the Rust registry). There is no PROXIED
disposition any more: PIDASHCONV-1108 removed the AMQP hand-off, so the
worker never forwards to a Python plane — an unknown name parks as
failed (`unknown task <name>`).

Note: 23 OWNED tasks are Rust-superset executions — called in Django
but unregistered there (dropped; PDASHOSS01-292), while the Rust
worker runs them: `copy_s3_objects_of_description_and_assets`,
`issue_description_version_task`, `process_logs`, `track_page_version`,
`get_asset_object_metadata`, `workspace_seed` (domain gates + V-01),
`user_activation_email`, `user_deactivation_email`,
`project_add_user_email`, `workspace_invitation`, `project_invitation`
(PIDASHCONV-1104), plus `track_event`, `page_transaction`,
`recent_visited_task` and `crawl_work_item_link_title` (PIDASHCONV-1107;
standalone-Rust rule — the Rust worker runs them, Django drops them),
plus the four auth mail tasks (`magic_link`, `forgot_password`,
`send_email_update_magic_code`, `send_email_update_confirmation`;
PIDASHCONV-1103), plus the four webhook tasks (`webhook_send_task`,
`webhook_activity`, `model_activity`, `send_webhook_deactivation_email`;
PIDASHCONV-1105; standalone-Rust rule — runs in Rust, Django drops it).
Deliberate (accepted by their domain gates);
recorded here and in the V-01 PR, for the V-05 ported-bug register to
catalog.

### W-assistant (0 rows; fixed)

Fix: PIDASHCONV-863 (landed; revisits the PIDASHCONV-857 hybrid-worker
decision, whose 2027-04-10 backstop the Standalone Rust rule withdrew).
`register_turn_handler` is wired in `worker()` with the live
`LiveTurnSeam` — the D-11 provider lanes, the PIDASHCONV-1111/1112/1113
streaming/MCP/local-tool modules, the agent loop, and the ported SQL over
the worker pool (the sweep row went first, wired by PIDASHCONV-831). The
row reclassified to OWNED (signature deleted). The `W-assistant` gap id
stays reserved in `WIRE_GAPS`.

```expected-jobs
```

### W-cloud-agent (0 rows; wired)

Fix: PIDASHCONV-832 (wired `register_execute_tasks` in `worker()`:
scan/sweep/expire execute in Rust on bin-local live seams. The run
handler is overridden with an explicit forward — the model runtime
(pydantic-ai) has no Rust counterpart, so Python keeps executing
runs; PIDASHCONV-866 tracks true run ownership. The `W-cloud-agent`
gap id stays reserved in `WIRE_GAPS`).

```expected-jobs
```

### W-mail (0 rows; wired by PIDASHCONV-865)

Fix: PIDASHCONV-865 (landed). The notifications row went first (wired by
PIDASHCONV-834: `register_notifications_task` in `worker()`, pool-only
handler); the send/stack rows are gone here (live `RedisMail`/`MailDb`/
`Renderer`/`Mailer` seams plus a Rust-queue `StackOutbox` in
`tasks_mail::live`, wired via `register_email_notification_tasks` in
`worker()`). The `W-mail` gap id stays reserved in `WIRE_GAPS`.

```expected-jobs
```

### W-export (0 rows; fixed)

Fix: PIDASHCONV-858 (landed; revisits the PIDASHCONV-835 hybrid-worker
decision, whose 2027-04-10 backstop the Standalone Rust rule withdrew).
`register_expiry` is wired in `worker()` with the live S3 deleter (one
signed `DELETE` per truthy key over `pidash-storage` SigV4 + reqwest;
any S3 error fails the task like Django's uncaught `ClientError`, so
an outage parks instead of ack-and-clearing). The row reclassified to
OWNED (signature deleted). The `W-export` gap id stays reserved in
`WIRE_GAPS`.

```expected-jobs
```

### W-activity (0 rows; fixed)

Fix: PIDASHCONV-836 (landed). `register_activity_task` is wired in
`worker()` with the live `OriginRedis` (sync SETEX from `REDIS_URL`;
an unconfigured Redis aborts origin-bearing calls exactly like
Django's `redis_instance()` `RuntimeError` under the broad-except).
The row reclassified to OWNED (signature deleted).

```expected-jobs
```

### Non-Pi-Dash tasks (0 rows; dropped by PIDASHCONV-1108)

Ten names that are not PI Dash tasks are excluded from the comparison on
both sides (`job_inventory.py` `NON_PI_DASH_TASKS` and the Rust
inventory table), so they no longer appear as PROXIED rows:

- `celery.*` (9): Celery canvas builtins — `celery.accumulate`,
  `celery.backend_cleanup`, `celery.chain`, `celery.chord`,
  `celery.chord_unlock`, `celery.chunks`, `celery.group`, `celery.map`,
  `celery.starmap`. Pi Dash uses no canvas.
- `pi_dash.bgtasks.analytic_plot_export.export_analytics_to_csv_email`:
  dead — defined but never referenced.

```expected-jobs
```

## Beat

28/28 entries OWNED: same task, same cadence on both sides (the four
settings-backed intervals match Django's `pi_dash.settings.test`
values: 10/30/300/30s). No rust-ahead drift rows remain:
PIDASHCONV-1063's two offline-runner sweeps
(`runner-release-pins-on-offline-runners`,
`runner-fail-runs-on-offline-runners`) exist natively on both sides
since the PIDASHCONV-1055 main→rust-dev merge (2d029396);
PIDASHCONV-1087 deleted the `RUST_AHEAD_BEAT_ENTRIES` rows.

Any other beat drift fails the check structurally — there is no
"expected beat miss" list.
