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

- Routes: 528 Django rows — 502 OWNED, 26 MISSING (all listed below),
  5 Rust-only (all justified, in `RUST_ONLY_JUSTIFICATIONS`).
- Jobs: 85 tasks — 28 OWNED, 35 proxied by design, 22 MISSING;
  beat 26/26 OWNED.

## Routes

### G1 — axum auto-HEAD serves what Django 405s (0 rows; fixed)

Fix: PIDASHCONV-826 (landed). Axum auto-served HEAD wherever GET was
routed; Django has no HEAD arm, so HEAD 405s there. Every affected route
now carries an explicit HEAD proxy arm and its inventory row lists HEAD
as proxied, so the 247 rows reclassified to OWNED (signatures deleted
below). The two
magic-generate HEAD gaps stay under G1+G4 below (PIDASHCONV-829's
scope: their HEAD gap is `drf-405body-vs-axum-405`, not
`serves-but-django-405`).

```expected-routes
```

### G1+G4 — magic-generate routes: HEAD (G1) + the rest (G4) (2 rows)

Fix: PIDASHCONV-829 (all arms, including HEAD — PIDASHCONV-826 left
these two lines untouched). DRF POST-only routes: OPTIONS needs a proxy
arm (Django answers 200 metadata), the other methods need proxy arms for
DRF's 405 body shape.

```expected-routes
G1+G4 :: /auth/magic-generate/ :: GET:drf-405body-vs-axum-405;PUT:drf-405body-vs-axum-405;PATCH:drf-405body-vs-axum-405;DELETE:drf-405body-vs-axum-405;HEAD:drf-405body-vs-axum-405;OPTIONS:allows-but-405
G1+G4 :: /auth/spaces/magic-generate/ :: GET:drf-405body-vs-axum-405;PUT:drf-405body-vs-axum-405;PATCH:drf-405body-vs-axum-405;DELETE:drf-405body-vs-axum-405;HEAD:drf-405body-vs-axum-405;OPTIONS:allows-but-405
```

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

### G3 — DRF router artifacts: api-root index + format suffixes (4 rows)

Fix: PIDASHCONV-828. The `/api/v1/workspaces/{slug}/` api-root index
and the `.{format}` suffix variants Django's router generates; no Rust
route.

```expected-routes
G3 :: /api/v1/workspaces/{slug}/ :: no-rust-route
G3 :: /api/v1/workspaces/{slug}/invitations.{format}/? :: no-rust-route
G3 :: /api/v1/workspaces/{slug}/stickies.{format}/? :: no-rust-route
G3 :: /api/v1/workspaces/{slug}/{format} :: no-rust-route
```

### G4 — magic-link sign-in/up routes missing proxy arms (4 rows)

Fix: PIDASHCONV-829. Plain Django Views, POST-only in Rust: OPTIONS
(Django 200) has no proxy arm. (The two generate routes are under
G1+G4 above.)

```expected-routes
G4 :: /auth/magic-sign-in/ :: OPTIONS:allows-but-405;cbv-405:GET,PUT,PATCH,DELETE,HEAD
G4 :: /auth/magic-sign-up/ :: OPTIONS:allows-but-405;cbv-405:GET,PUT,PATCH,DELETE,HEAD
G4 :: /auth/spaces/magic-sign-in/ :: OPTIONS:allows-but-405;cbv-405:GET,PUT,PATCH,DELETE,HEAD
G4 :: /auth/spaces/magic-sign-up/ :: OPTIONS:allows-but-405;cbv-405:GET,PUT,PATCH,DELETE,HEAD
```

### G5 — DRF format suffixes on detail routes (2 rows)

Fix: PIDASHCONV-828 (with G3). The `{pk}.{format}` detail variants; no
Rust route.

```expected-routes
G5 :: /api/v1/workspaces/{slug}/invitations/{pk}.{format}/? :: no-rust-route
G5 :: /api/v1/workspaces/{slug}/stickies/{pk}.{format}/? :: no-rust-route
```

### G6 — handler-denial 405s pending contract proof (7 rows)

Fix: PIDASHCONV-830. Explicit `*_not_allowed` handlers replay the
view's auth prelude and answer DRF's 405 bytes; read-verified, but no
contract test pins the bytes yet, so the diff conservatively reports
`serves-but-django-405`.

```expected-routes
G6 :: /api/users/me/workspaces/ :: POST:serves-but-django-405;PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspace-slug-check/ :: POST:serves-but-django-405;PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspaces/ :: PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspaces/{slug}/ :: POST:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspaces/{slug}/user-activity/{user_id}/export/ :: GET:serves-but-django-405;PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspaces/{slug}/workspace-themes/ :: PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspaces/{slug}/workspace-themes/{pk}/ :: POST:serves-but-django-405;PUT:serves-but-django-405;HEAD:serves-but-django-405
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

### W-assistant (1 row; sweep resolved by PIDASHCONV-831)

Fix: PIDASHCONV-857. The sweep row is gone (wired by PIDASHCONV-831:
`register_sweep_handler` in `worker()`, live store). `run_turn` needs a
live `TurnSeam` and none exists — that is PIDASHCONV-857, not wiring.

```expected-jobs
W-assistant :: assistant.run_turn :: must-wire (lost without python worker)
```

### W-cloud-agent (3 rows)

Fix: PIDASHCONV-832.

```expected-jobs
W-cloud-agent :: cloud_agent.run_agent_run :: must-wire (lost without python worker)
W-cloud-agent :: cloud_agent.scan_queued_runs :: must-wire (lost without python worker)
W-cloud-agent :: cloud_agent.sweep_stale_runs :: must-wire (lost without python worker)
```

### W-ticker + W-scheduler (4 rows)

Fix: PIDASHCONV-833 (one issue: the ticker and scheduler scan/fire
register fns are wired together).

```expected-jobs
W-ticker :: pi_dash.bgtasks.agent_ticker.fire_tick :: must-wire (lost without python worker)
W-ticker :: pi_dash.bgtasks.agent_ticker.scan_due_tickers :: must-wire (lost without python worker)
W-scheduler :: pi_dash.bgtasks.scheduler.fire_scheduler_binding :: must-wire (lost without python worker)
W-scheduler :: pi_dash.bgtasks.scheduler.scan_due_bindings :: must-wire (lost without python worker)
```

### W-mail (3 rows)

Fix: PIDASHCONV-834.

```expected-jobs
W-mail :: pi_dash.bgtasks.email_notification_task.send_email_notification :: must-wire (lost without python worker)
W-mail :: pi_dash.bgtasks.email_notification_task.stack_email_notification :: must-wire (lost without python worker)
W-mail :: pi_dash.bgtasks.notification_task.notifications :: must-wire (lost without python worker)
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

### W-automation (1 row)

Fix: PIDASHCONV-837.

```expected-jobs
W-automation :: pi_dash.bgtasks.issue_automation_task.archive_and_close_old_issues :: must-wire (lost without python worker)
```

### W-loop (2 rows)

Fix: PIDASHCONV-838.

```expected-jobs
W-loop :: pi_dash.bgtasks.loop.fire_loop_target :: must-wire (lost without python worker)
W-loop :: pi_dash.bgtasks.loop.scan_due_targets :: must-wire (lost without python worker)
```

### W-runner (7 rows)

Fix: PIDASHCONV-840.

```expected-jobs
W-runner :: runner.apply_agent_run_terminal_effects :: must-wire (lost without python worker)
W-runner :: runner.expire_stale_approvals :: must-wire (lost without python worker)
W-runner :: runner.mark_offline_runners :: must-wire (lost without python worker)
W-runner :: runner.reconcile_agent_run_terminal_effects :: must-wire (lost without python worker)
W-runner :: runner.reconcile_stalled_runs :: must-wire (lost without python worker)
W-runner :: runner.sweep_agent_chat_state :: must-wire (lost without python worker)
W-runner :: runner.sweep_chat_message_dedupe :: must-wire (lost without python worker)
```

### Proxied by design (35 rows)

No fix issue: forwarding preserves Django's outcome on every row.

- `celery.*` (9): Celery canvas builtins; no canvas usage in the codebase.
- `django-drops-too` (17): called but unregistered in Django
  (PDASHOSS01-292) — the Python worker drops them; forwarding drops
  them identically. `track_event` is explicitly unregistered in Rust
  because an early-return handler would swallow the forward.
- `dormant` (4): registered but never published; forward preserves
  non-execution.
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
- `managed_runner.expire_waiting_runs`: cloud-only app; the OSS beat
  entry fires into the void on both sides.
- `project_invitation`: never invoked (`invite.py:105` calls `.delay`
  on a list; PDASHOSS01-292).
- `recent_visited_task`: documented no-op (memory broker, no worker by
  design).
- `export_analytics_to_csv_email`: dead (defined, never referenced).

```expected-jobs
PROXIED :: celery.accumulate :: celery-builtin; no canvas usage
PROXIED :: celery.backend_cleanup :: celery-builtin; no canvas usage
PROXIED :: celery.chain :: celery-builtin; no canvas usage
PROXIED :: celery.chord :: celery-builtin; no canvas usage
PROXIED :: celery.chord_unlock :: celery-builtin; no canvas usage
PROXIED :: celery.chunks :: celery-builtin; no canvas usage
PROXIED :: celery.group :: celery-builtin; no canvas usage
PROXIED :: celery.map :: celery-builtin; no canvas usage
PROXIED :: celery.starmap :: celery-builtin; no canvas usage
PROXIED :: managed_runner.expire_waiting_runs :: cloud-only app; oss beat fires into the void on both sides
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
PROXIED :: runner.sweep_idle_sessions :: dormant (never published; forward preserves non-execution)
PROXIED :: runner.sweep_old_streams :: dormant (never published; forward preserves non-execution)
PROXIED :: runner.sweep_run_message_dedupe :: dormant (never published; forward preserves non-execution)
PROXIED :: runner.sweep_stale_runners :: dormant (never published; forward preserves non-execution)
```

## Beat

26/26 entries OWNED: same task, same cadence (the four
settings-backed intervals match Django's `pi_dash.settings.test`
values: 10/30/300/30s). Any beat drift fails the check structurally —
there is no "expected beat miss" list.
