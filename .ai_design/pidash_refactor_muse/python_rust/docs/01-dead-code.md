# Dead code identification (Stage 0)

Status: **draft for review** (Stage 0 identification). Analysis only — nothing
deleted. Companion to `00-inventory-and-slices.md` (measured at OSS `main`
`ddaef0c2`, re-verified 2026-09-23).
Staging copy: Rich uploads the accepted version to the Pi Dash wiki as the
Stage 0 dead-code page; the `skip` rows in §6/§6b of the inventory doc mirror
it for filing.

Rule: an area is listed here only with no-reference evidence (zero imports,
zero registrations, zero route/signal/task wiring). Candidates needing a
product decision are listed separately and are NOT dead.

## Confirmed dead — do not port (disposition `skip`)

### 1. `analytics/` app — empty but installed

- `apps/api/pi_dash/analytics/` holds `__init__.py` + `apps.py` only (13 LOC).
  No routes, views, serializers, models, signals, tasks.
- Nothing imports it: the only references to `pi_dash.analytics` repo-wide are
  its own `apps.py` (`name = "pi_dash.analytics"`) and its
  `INSTALLED_APPS` entry (`settings/common.py:50`).
- Action: do not port. Nothing is deleted in this project (parent rule 1): the file stays in place, unrouted and untested, and removal belongs to the post-switchover follow-up plan. (That plan also drops the `INSTALLED_APPS` entry.)
- Exception — `tests/unit/analytics/test_advance_analytics.py` is a **live test
  in a misleading directory**: it exercises `Project`/`ProjectMember` and
  `AgentRun`/`Pod`, not the analytics app. Relocate it (project/runner tests),
  do not delete it with the app.

### 2. `runner/admin.py` — Django admin registrations with no admin installed

- Registers `DevMachine`, `Runner`, `RunnerSession`, `RunnerForceRefresh`,
  `MachineToken`, `AgentRun`, `AgentRunEvent`, `ApprovalRequest`,
  `AgentChatSession`, `AgentChatMessage`, `AgentChatEvent`,
  `AgentChatApprovalRequest` via `django.contrib.admin`.
- `django.contrib.admin` appears nowhere in `settings/` or `urls.py`; no
  `autodiscover()` call; nothing imports `pi_dash.runner.admin`.
- Action: do not port. Nothing is deleted in this project (parent rule 1): the file stays in place, unrouted and untested, and removal belongs to the post-switchover follow-up plan. The runner surfaces it pretends to administer are covered by REST endpoints (D-13…D-15).

### 3. `prompting/admin.py` — same, with one stale claim

- Registers `PromptTemplate` / `PromptSectionOverride` the same way; equally
  uninstalled and unimported.
- Its comment says "the admin is currently the only surface that edits
  templates, so this is where `updated_by` gets set until a dedicated REST
  endpoint lands" — that surface does not exist in this codebase. Template
  editing today goes through shell/management (`reseed_*`) or the template
  admin UI; the `updated_by` audit path for templates must be re-decided in
  slice D-04, not inherited from this file.
- Action: do not port. Nothing is deleted in this project (parent rule 1): the file stays in place, unrouted and untested, and removal belongs to the post-switchover follow-up plan.

### 4. Control-plane WebSocket — already a reject stub

- `runner/routing.py` (`ws/runner/`), `runner/consumers.py` (stub rejects all
  traffic with close code 1008), `runner/urls.py` comment. No producers or
  consumers of the WS path exist, but the stub is **not import-free**:
  `pi_dash/asgi.py` imports `pi_dash.runner.routing.websocket_urlpatterns`
  into `ProtocolTypeRouter`, so it must stay until the follow-up removal
  plan edits `asgi.py` in the same change.
- Action: do not port. Nothing is deleted in this project (parent rule 1): the file stays in place, unrouted and untested, and removal belongs to the post-switchover follow-up plan. The replacement realtime path (D-14)
  must preserve the close-code-1008 behavior old runners rely on.

### 5. `debug_toolbar` wiring — dev-only

- `urls.py` `DEBUG` branch (`__debug__/`), `settings/local.py`
  (`INSTALLED_APPS` + `MIDDLEWARE` additions).
- Action: do not port (re-add natively in Rust dev tooling if wanted).

### 6. Unused dependencies — zero imports repo-wide

| Package                        | Manifest                   | Evidence                                                         |
| ------------------------------ | -------------------------- | ---------------------------------------------------------------- |
| `slack-sdk==3.27.1`            | `requirements/base.txt:58` | no `import slack` / `from slack` anywhere                        |
| `jsonmodels==2.7.0`            | `requirements/base.txt:32` | no `jsonmodels` reference anywhere                               |
| `django-celery-results==2.5.1` | `requirements/base.txt:22` | no `django_celery_results` / `celery_results` reference anywhere |

- Action: remove from requirements (ops tail, D-37). No code change needed —
  nothing imports them.

## Claimed dead, actually live — corrections

- **Plane leftovers: none found.** The only matches for `plane` in Python code
  are "control-plane" prose in `runner/` (sessions, pubsub, consumers, urls).
  No `plane.io` / self-hosted-Plane references in code or config. The earlier
  "Plane leftovers" note is void.
- **Importer stubs: none found.** `db/models/importer.py`,
  `app/serializers/importer.py`, `bgtasks/export_task.py`,
  `bgtasks/exporter_expired_task.py`, `app/views/exporter/` are a live
  import/export feature — see candidates below.
- **`license/api/views/admin.py`, `license/api/serializers/admin.py` are live
  REST endpoints** (instance console), not Django admin. Unaffected by item 3.
- **`bgtasks/scheduler.py` has no signal receivers.** The receivers live in
  `bgtasks/github_signals.py` (+ `scheduler/`, `orchestration/`, `runner/`
  `signals.py`, `db/models/user.py`). Inventory §1/§2 wording corrected
  accordingly.

## Not dead — product decisions (candidates, disposition as noted)

- **Exporter/importer porters** (`app/urls/exporter.py`, `bgtasks/export_task.py`,
  `utils/porters/`) — live but low-traffic. D-35 carries the exporter
  `minimal` pending open question 3. Either a late small slice or removal.
- **`croniter`** — used lazily by one throwaway ops command
  (`db/management/commands/dry_run_scheduler_migration.py`, which itself says
  it is written to survive croniter's removal). Removable together with that
  command in the ops tail (D-37), not independently dead.
- **Legacy non-FTS search contract** (`include_comments=False` callers in
  `app`) — keep behavior through slice D-29; do not carry the flag forward
  past it.

## Filing note

- The `skip` rows above map to the `skip` row in the §6 slice table and the
  `Tests today` exclusions; no epic is ever filed for them (§9 of the inventory
  doc). Stage 0 files no code PR — acceptance is Rich approving this page.
