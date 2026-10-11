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

Counts (2026-10-10, `pi_dash.settings.test`):

- Routes: 528 Django rows — 514 OWNED, 14 MISSING (all listed below),
  5 Rust-only (all justified, in `RUST_ONLY_JUSTIFICATIONS`).
- Jobs: 85 tasks — 51 OWNED, 31 proxied by design, 3 MISSING;
  beat 26/26 OWNED.

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

### G2 — license instance-admin session/auth endpoints unported (7 rows)

Fix: PIDASHCONV-827. No Rust route at all.

```expected-routes
G2 :: /api/instances/admins/ :: no-rust-route
G2 :: /api/instances/admins/me/ :: no-rust-route
G2 :: /api/instances/admins/session/ :: no-rust-route
G2 :: /api/instances/admins/sign-in/ :: no-rust-route
G2 :: /api/instances/admins/sign-out/ :: no-rust-route
G2 :: /api/instances/admins/sign-up/ :: no-rust-route
G2 :: /api/instances/admins/{pk}/ :: no-rust-route
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

### Rust-only routes (5, all justified in-script)

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

## Jobs

Task dispositions: OWNED (Rust runs it), PROXIED (Rust forwards to the
Python worker and Django's outcome is preserved — each row says why),
MISSING (live registered task Rust forwards that would be lost at
switchover — one fix issue per register fn).

Note: 6 OWNED tasks are Rust-superset executions — called in Django
but unregistered there (dropped; PDASHOSS01-292), while the Rust
worker runs them: `copy_s3_objects_of_description_and_assets`,
`issue_description_version_task`, `process_logs`, `track_page_version`,
`get_asset_object_metadata`, `workspace_seed`. Deliberate (accepted by
their domain gates); recorded here and in the V-01 PR, for the V-05
ported-bug register to catalog.

### W-assistant (0 rows; sweep wired, turn reclassified to PROXIED)

Fix: PIDASHCONV-857 (hybrid-worker decision, not wiring: the Python
worker stays the owner post-switchover — see the `assistant.run_turn`
bullet under "Proxied by design". The sweep row is gone (wired by
PIDASHCONV-831: `register_sweep_handler` in `worker()`, live store);
`run_turn` needs a live `TurnSeam`/agent runtime and none exists, so
there is nothing to wire; the `W-assistant` gap id stays reserved in
`WIRE_GAPS`).

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

### W-mail (2 rows; notifications resolved by PIDASHCONV-834)

Fix: PIDASHCONV-865. The notifications row is gone (wired by PIDASHCONV-834:
`register_notifications_task` in `worker()`, pool-only handler). Send/stack
need live mail seams and none exist — that is PIDASHCONV-865, not wiring.

```expected-jobs
W-mail :: pi_dash.bgtasks.email_notification_task.send_email_notification :: must-wire (lost without python worker)
W-mail :: pi_dash.bgtasks.email_notification_task.stack_email_notification :: must-wire (lost without python worker)
```

### W-export (0 rows; reclassified to PROXIED)

Fix: PIDASHCONV-835 (hybrid-worker decision, not wiring: the Python
worker stays the owner post-switchover — see the `delete_old_s3_link`
bullet under "Proxied by design". No register fn exists and Rust has
no live S3 delete path, so there is nothing to wire; the `W-export`
gap id stays reserved in `WIRE_GAPS`).

```expected-jobs
```

### W-activity (1 row)

Fix: PIDASHCONV-836.

```expected-jobs
W-activity :: pi_dash.bgtasks.issue_activities_task.issue_activity :: must-wire (lost without python worker)
```

### Proxied by design (31 rows)

No fix issue: forwarding preserves Django's outcome on every row.

- `celery.*` (9): Celery canvas builtins; no canvas usage in the codebase.
- `django-drops-too` (17): called but unregistered in Django
  (PDASHOSS01-292) — the Python worker drops them; forwarding drops
  them identically. `track_event` is explicitly unregistered in Rust
  because an early-return handler would swallow the forward.
- `delete_old_s3_link`: hybrid worker (PIDASHCONV-835). Registered
  and beat-scheduled (twice daily), but the body deletes objects from
  a live S3 bucket and Rust has no live S3 delete path (`ObjectStore`
  has head/copy only; the worker wires `UnavailableObjectStore`;
  "Sinks, never live buckets"). A Rust handler would clear `url`
  without deleting objects, diverging from Django — so the Python
  worker stays the owner post-switchover (the AMQP broker stays too:
  the Rust scheduler keeps firing both beat entries and the Rust
  worker forwards to Python over Celery protocol). Decision owner:
  stage-8 switchover track (PIDASHCONV-835); revisit PIDASHCONV-858
  by 2027-04-10, or when Rust ships a live S3 delete path, whichever
  comes first.
- `assistant.run_turn`: hybrid worker (PIDASHCONV-857). Published on
  every assistant message POST (`messages.py:103`), but the body runs
  the pydantic-ai agent loop and Rust has no live `TurnSeam`/agent
  runtime (only `#[cfg(test)]` fakes; `Agent.run` has no Rust
  counterpart; the PIDASHCONV-254 rmcp/schemars/provider-client plan
  was never built). A Rust handler would fail or ACK every turn while
  running nothing, diverging from Django — so the Python worker stays
  the owner post-switchover (the AMQP broker stays too: the Rust
  handler enqueues into `rust_job_queue` and the Rust worker forwards
  to Python over Celery protocol). Decision owner: stage-8
  switchover track (PIDASHCONV-857); revisit PIDASHCONV-863 by
  2027-04-10, or when Rust ships a live TurnSeam, whichever comes
  first.
- `project_invitation`: never invoked (`invite.py:105` calls `.delay`
  on a list; PDASHOSS01-292).
- `recent_visited_task`: documented no-op (memory broker, no worker by
  design).
- `export_analytics_to_csv_email`: dead (defined, never referenced).

```expected-jobs
PROXIED :: assistant.run_turn :: hybrid worker (no live TurnSeam/agent runtime in Rust); forward preserves full python execution (PIDASHCONV-857)
PROXIED :: celery.accumulate :: celery-builtin; no canvas usage
PROXIED :: celery.backend_cleanup :: celery-builtin; no canvas usage
PROXIED :: celery.chain :: celery-builtin; no canvas usage
PROXIED :: celery.chord :: celery-builtin; no canvas usage
PROXIED :: celery.chord_unlock :: celery-builtin; no canvas usage
PROXIED :: celery.chunks :: celery-builtin; no canvas usage
PROXIED :: celery.group :: celery-builtin; no canvas usage
PROXIED :: celery.map :: celery-builtin; no canvas usage
PROXIED :: celery.starmap :: celery-builtin; no canvas usage
PROXIED :: pi_dash.bgtasks.analytic_plot_export.analytic_export_task :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.analytic_plot_export.export_analytics_to_csv_email :: dead (defined, never referenced)
PROXIED :: pi_dash.bgtasks.event_tracking_task.track_event :: django-drops-too (PDASHOSS01-292); rust explicitly unregistered (early-return would swallow forward)
PROXIED :: pi_dash.bgtasks.export_task.issue_export_task :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.exporter_expired_task.delete_old_s3_link :: hybrid worker (no live S3 delete in Rust); forward preserves full python execution (PIDASHCONV-835)
PROXIED :: pi_dash.bgtasks.forgot_password_task.forgot_password :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.magic_link_code_task.magic_link :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.page_transaction_task.page_transaction :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.project_add_user_email_task.project_add_user_email :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.project_invitation_task.project_invitation :: task never invoked (invite.py:105 calls .delay on a list; PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.recent_visited_task.recent_visited_task :: documented no-op (memory broker, no worker by design)
PROXIED :: pi_dash.bgtasks.user_activation_email_task.user_activation_email :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.user_deactivation_email_task.user_deactivation_email :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.user_email_update_task.send_email_update_confirmation :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.user_email_update_task.send_email_update_magic_code :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.webhook_task.model_activity :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.webhook_task.send_webhook_deactivation_email :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.webhook_task.webhook_activity :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.webhook_task.webhook_send_task :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.work_item_link_task.crawl_work_item_link_title :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.workspace_invitation_task.workspace_invitation :: django-drops-too (PDASHOSS01-292)
```

## Beat

26/26 entries OWNED: same task, same cadence (the four
settings-backed intervals match Django's `pi_dash.settings.test`
values: 10/30/300/30s). Any beat drift fails the check structurally —
there is no "expected beat miss" list.
