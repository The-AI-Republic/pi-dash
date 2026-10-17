# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-08 oracle, Rust-side replay (PIDASHCONV-243).

Executable form of the tasks_webhooks oracle against the Rust worker.
The Django oracle (``test_webhook_tasks.py``) publishes jobs in Celery
protocol v2 to ``CELERY_BROKER_URL`` and probes the worker with Celery
broadcast inspect — only a Celery (Django) worker can execute or answer
those. The Rust worker consumes the Postgres queue ``rust_job_queue``
instead, so this module replays the same oracle by seeding
``rust_job_queue`` rows carrying the exact Celery v2 payloads
(see ``_harness/rust_queue.py``) with the Django worker stopped.

Run with the Django worker (and beat) STOPPED and the Rust worker
running against the same DATABASE_URL::

    export DATABASE_URL=postgresql://...  # your own scratch database
    export CELERY_BROKER_URL=amqp://...   # same broker the Rust worker uses
    pidash-api worker &  # Rust worker; Django worker stopped
    pytest -p no:django tasks_webhooks/test_rust_replay.py -v

What maps where (``celery_wire`` -> ``rust_queue``):

- ``celery_wire.publish(task, args, kwargs)`` ->
  ``rust_queue.publish(...)`` (same task name, same args/kwargs).
- ``broker_probe.wait_for_queue_drain`` -> ``rust_queue.wait_for_settled``
  per job (the Postgres row is acked/deleted once the worker forwards
  it) plus a ``broker_probe.queue_depth`` delta proving the message
  arrived on the broker ``celery`` queue for the Python plane.
- ``test_worker_registration`` (Celery inspect) ->
  ``test_rust_worker_liveness`` below: a no-op track_event job must be
  consumed from ``rust_job_queue``, proving a Rust worker (not Django)
  is serving this database.

What is NOT duplicated here: the static parity tests (wire payload,
retry options, beat entries, fan-out call sites) are
backend-independent — they assert on Django source text and the
in-process wire builder, so they already pass with no worker at all.
Only the live worker tests have a replay form.

Ownership split (mirrors the worker registry on rust-dev): 7 of the
11 D-08 task names are Rust-owned — ``process_logs`` (PIDASHCONV-260),
``archive_and_close_old_issues`` (PIDASHCONV-837),
``issue_activity`` (PIDASHCONV-836), plus ``track_event``,
``page_transaction``, ``recent_visited_task`` and
``crawl_work_item_link_title`` (PIDASHCONV-1107 wires
``register_sink_tasks``, ``register_visit_page_handlers`` and
``register_link_crawl_handler`` in ``register_oss_handlers``).
``crates/jobs/src/tasks_webhooks/mod.rs``
keeps the webhook send/fan-out path Python-owned deliberately
(registering a local send handler would steal live traffic while the
webhook-row fetch, HTTP send and SMTP send still live on Python;
PIDASHCONV-1105 ports those four names).
Every test below except the owned-cases (seven tasks, nine tests:
``test_rust_process_logs_executes_locally``,
``test_rust_archive_and_close_executes_locally``,
``test_rust_issue_activity_executes_locally``,
``test_rust_track_event_executes_locally``,
``test_rust_track_event_assigns_workspace_role``,
``test_rust_recent_visited_executes_locally``,
``test_rust_page_transaction_executes_locally``,
``test_rust_page_transaction_url_src_inserts_nothing`` and
``test_rust_crawl_link_executes_locally``)
therefore asserts observable AMQP forwarding — the
``rust_job_queue`` row is acked AND the broker ``celery`` queue grows
while the Django worker is stopped — instead of local DB effects, plus
a no-local-row pin proving the Rust worker never executes these jobs
itself. No skipped or weakened assertions.

process_logs note (PIDASHCONV-242, decided EXECUTE; PIDASHCONV-260
registers it): canonical Django (settings.local, the CI worker-plane
env) executes ``process_logs`` at worker boot, so the Rust worker owns
``PROCESS_LOGS_TASK`` too and ``test_rust_process_logs_executes_locally``
asserts the local ``api_activity_logs`` row diff instead of forwarding.

issue_activity note (PIDASHCONV-836 registers it): the Rust worker owns
``ISSUE_ACTIVITY_TASK`` and ``test_rust_issue_activity_executes_locally``
asserts the local ``issue_activities`` row diff instead of forwarding.

archive_and_close note (PIDASHCONV-837 registers it):
``test_rust_archive_and_close_executes_locally`` pins local execution.

standalone note (PIDASHCONV-1107 registers track_event, page_transaction,
recent_visited_task and crawl_work_item_link_title): the forward pins
for those four are gone; the ``*_executes_locally`` tests below assert
the same DB effects and sink captures the Django oracle pins. The
liveness probe moved to ``WEBHOOK_SEND`` (still Python-owned); the
PostHog sink assertions need the replay job's ``POSTHOG_*`` env.

Beat note: D-08 owns no beat entries (``test_no_beat_entries_owned`` —
cleanup owns the webhook-log delete; the daily
``check-every-day-to-archive-and-close`` entry names
``issue_automation_task.archive_and_close_old_issues``, which that
static test does not count as owned). Beat pins are source-text, so
there is no replay form.
"""

import json
import uuid
from datetime import datetime, timedelta, timezone

from _harness import broker_probe, rust_queue
from _harness import seed as seed_helpers
from _harness.db import diff as _diff
from _harness.db import insert_row
from _harness.db import snapshot, wait_for_condition as wait_for

M = "pi_dash.bgtasks"

WEBHOOK_SEND = f"{M}.webhook_task.webhook_send_task"
DEACTIVATION_EMAIL = f"{M}.webhook_task.send_webhook_deactivation_email"
WEBHOOK_ACTIVITY = f"{M}.webhook_task.webhook_activity"
MODEL_ACTIVITY = f"{M}.webhook_task.model_activity"
ISSUE_ACTIVITY = f"{M}.issue_activities_task.issue_activity"
ARCHIVE_AND_CLOSE = f"{M}.issue_automation_task.archive_and_close_old_issues"
CRAWL_LINK_TITLE = f"{M}.work_item_link_task.crawl_work_item_link_title"
RECENT_VISITED = f"{M}.recent_visited_task.recent_visited_task"
PAGE_TRANSACTION = f"{M}.page_transaction_task.page_transaction"
PROCESS_LOGS = f"{M}.logger_task.process_logs"
TRACK_EVENT = f"{M}.event_tracking_task.track_event"


def _publish_and_observe_forward(task_name, args=None, kwargs=None):
    """Publish one job; assert the Rust worker forwarded it to Python.

    Returns the celery id. The ``rust_job_queue`` row must be settled
    (acked on AMQP publish) and the broker ``celery`` queue must grow —
    it grows because the Django worker is stopped. Growth is a lower
    bound, not an exact delta: the worker's scheduler loop also
    forwards its own unregistered entries over the same broker, so
    concurrent scheduler traffic can only add messages. Settlement of
    our row is the precise per-job proof; a requeued (unforwarded) job
    would never settle.
    """
    baseline = broker_probe.queue_depth()
    celery_id = rust_queue.publish(task_name, args=args, kwargs=kwargs)
    rust_queue.wait_for_settled(celery_id, what=f"{task_name} forwarded")
    arrived = broker_probe.queue_depth()
    assert arrived >= baseline + 1, (
        f"{task_name} was consumed from rust_job_queue but no message "
        f"arrived on the broker (depth {baseline} -> {arrived})"
    )
    return celery_id


def test_rust_worker_liveness(db_conn, broker_url):
    """Rust inspect-equivalent: a still-forwarded job is consumed.

    Replaces ``test_worker_registration`` (Celery broadcast inspect,
    which only a Django worker answers). Settlement of the
    ``rust_job_queue`` row proves the Rust worker owns the queue; a
    Django worker never reads that table. The probe is a webhook-send
    job (still Python-owned since PIDASHCONV-1107 owns track_event).
    """
    hook = _seed_hook(db_conn)
    _publish_and_observe_forward(
        WEBHOOK_SEND, kwargs=_send_kwargs(hook, "rust-replay-liveness")
    )


def _seed_hook(db_conn):
    # No webhook_sink fixture here: nothing POSTs while the Django
    # worker is stopped, so the URL is never fetched — it only has to
    # be a well-formed row.
    owner = seed_helpers.user(db_conn, "rust-replay-hook-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplayhook", owner["id"])
    url = f"https://example.com/hook/{uuid.uuid4().hex}"
    hook = seed_helpers.webhook(db_conn, workspace["id"], url, issue=True)
    hook = dict(hook)
    hook["workspace_slug"] = workspace["slug"]
    hook["owner_id"] = owner["id"]
    return hook


def _send_kwargs(hook, probe_id):
    return {
        "webhook_id": str(hook["id"]),
        "slug": hook["workspace_slug"],
        "event": "issue",
        "event_data": {"id": probe_id},
        "action": "POST",
        "current_site": "example.com",
        "activity": None,
    }


def test_rust_webhook_send_forwards_to_python(db_conn, broker_url):
    """Send path, Rust side: forward, never execute locally.

    Mirrors ``test_webhook_send_success`` (same payload incl. the
    header-spaces bug surface): the sink POST and the ``webhook_logs``
    row are Python-plane effects, so the replay pins forwarding plus a
    no-local-row pin — the Rust worker must not write the log row
    itself.
    """
    hook = _seed_hook(db_conn)
    before = snapshot(db_conn, ["webhook_logs"])
    _publish_and_observe_forward(
        WEBHOOK_SEND, kwargs=_send_kwargs(hook, "rust-replay-probe")
    )
    assert _diff(before, snapshot(db_conn, ["webhook_logs"])) == {}


def test_rust_webhook_failure_forwards_without_local_log_row(db_conn, broker_url):
    """First-attempt failure, Rust side: same forward, no local row.

    Mirrors ``test_webhook_send_failure_logs_retry_count``: with no
    sink in the loop there is no 500 to observe — the payload is
    byte-identical to the success path apart from the probe id — so the
    replay pins that the job still forwards exactly once and writes no
    local ``webhook_logs`` row. Retry timing stays pinned statically
    (``test_retry_options_parity``).
    """
    hook = _seed_hook(db_conn)
    before = snapshot(db_conn, ["webhook_logs"])
    _publish_and_observe_forward(
        WEBHOOK_SEND, kwargs=_send_kwargs(hook, "rust-replay-fail")
    )
    assert _diff(before, snapshot(db_conn, ["webhook_logs"])) == {}


def test_rust_webhook_redelivery_forwards_twice(db_conn, broker_url):
    """Redelivery: two publishes → two broker messages, nothing local.

    Mirrors ``test_webhook_redelivery_posts_twice_with_identical_bodies``:
    Django intends distinct per-execution delivery ids, but the
    ``X-Pi Dash-Delivery`` header never reaches the wire, so parity is
    two identical forwards. Both ``rust_job_queue`` rows settle and the
    broker grows by at least two (lower bound — scheduler traffic can
    only add); no local ``webhook_logs`` rows appear.
    """
    hook = _seed_hook(db_conn)
    kwargs = _send_kwargs(hook, "rust-replay-redelivery")
    before = snapshot(db_conn, ["webhook_logs"])
    baseline = broker_probe.queue_depth()
    celery_ids = [
        rust_queue.publish(WEBHOOK_SEND, kwargs=dict(kwargs)) for _ in range(2)
    ]
    for celery_id in celery_ids:
        rust_queue.wait_for_settled(celery_id, what="redelivered webhook forwarded")
    assert broker_probe.queue_depth() >= baseline + 2
    assert _diff(before, snapshot(db_conn, ["webhook_logs"])) == {}


def test_rust_activity_chain_entry_forwards_to_python(db_conn, broker_url):
    """Chain entry, Rust side: model_activity forwards; chain runs on Python.

    Mirrors ``test_activity_chain_fans_out_to_sink`` (same seed, same
    positional payload): the fan-out (model → webhook_activity → send →
    sink POST) executes on the Python plane, so the replay pins the
    entry-point forward plus no local ``webhook_logs`` row.
    """
    # model_id must be a real issue, mirroring the oracle: the payload
    # must be executable when the Python plane eventually runs it.
    chain = seed_helpers.issue_chain(db_conn, "rustreplaychain")
    owner, workspace, issue = chain["owner"], chain["workspace"], chain["issue"]
    url = f"https://example.com/hook/{uuid.uuid4().hex}"
    seed_helpers.webhook(db_conn, workspace["id"], url, issue=True)
    before = snapshot(db_conn, ["webhook_logs"])
    _publish_and_observe_forward(
        MODEL_ACTIVITY,
        args=[
            "issue",
            str(issue["id"]),
            {},
            None,
            str(owner["id"]),
            workspace["slug"],
            "example.com",
        ],
    )
    assert _diff(before, snapshot(db_conn, ["webhook_logs"])) == {}


def test_rust_webhook_activity_forwards_to_python(db_conn, broker_url):
    """Fan-out midpoint, Rust side: forwards with the oracle arg shape."""
    chain = seed_helpers.issue_chain(db_conn, "rustreplayfanout")
    owner, workspace, issue = chain["owner"], chain["workspace"], chain["issue"]
    _publish_and_observe_forward(
        WEBHOOK_ACTIVITY,
        kwargs={
            "event": "issue",
            "verb": "created",
            "field": None,
            "old_value": None,
            "new_value": None,
            "actor_id": str(owner["id"]),
            "slug": workspace["slug"],
            "current_site": "example.com",
            # Required by the Python signature (webhook_task.py:378-390);
            # same values model_activity passes on the created path, so the
            # forwarded payload stays executable on the Python plane.
            "event_id": str(issue["id"]),
            "old_identifier": None,
            "new_identifier": None,
        },
    )


def test_rust_deactivation_email_forwards_to_python(db_conn, broker_url):
    """Deactivation mail, Rust side: forward; SMTP sends on Python.

    Mirrors ``test_deactivation_email_delivers`` (same positional
    payload): the SMTP receipt is a Python-plane effect, so the replay
    pins the forward.
    """
    owner = seed_helpers.user(db_conn, "rust-replay-deact-owner")
    receiver = seed_helpers.user(db_conn, "rust-replay-deact-receiver")
    workspace = seed_helpers.workspace(db_conn, "rustreplaydeact", owner["id"])
    hook = seed_helpers.webhook(
        db_conn, workspace["id"], "https://example.com/hook",
        created_by_id=str(owner["id"]),
    )
    _publish_and_observe_forward(
        DEACTIVATION_EMAIL,
        args=[str(hook["id"]), str(receiver["id"]), "example.com", "contract reason"],
    )


def _added_rows(conn, table, before):
    after = snapshot(conn, [table])[table]
    new_keys = set(after) - set(before[table])
    return [after[k] for k in new_keys]


def test_rust_process_logs_executes_locally(db_conn, broker_url):
    """process_logs, Rust side: EXECUTE locally (PIDASHCONV-260, FX-LOG-01).

    Revisits ``test_rust_process_logs_forwards_to_python``: EXECUTE
    parity per PIDASHCONV-242 means the Rust worker owns
    ``PROCESS_LOGS_TASK``, so this mirrors
    ``test_process_logs_writes_postgres_row`` (same probe payload,
    ``path == "/contract/probe"``) and asserts the local
    ``api_activity_logs`` row diff instead of forwarding. Settlement of
    the ``rust_job_queue`` row plus the new row is the proof: with the
    Django worker stopped, a forward could never produce a local row.
    No broker-depth assertion: the scheduler loop forwards other
    domains' entries over the same broker, so only the row diff is
    attributable to this job.
    """
    before = snapshot(db_conn, ["api_activity_logs"])
    celery_id = rust_queue.publish(
        PROCESS_LOGS,
        args=[
            {
                "token_identifier": "rust-replay-probe",
                "path": "/contract/probe",
                "method": "GET",
                "response_code": 200,
            },
            {},
        ],
    )
    rust_queue.wait_for_settled(celery_id, what=f"{PROCESS_LOGS} executed")
    after = wait_for(
        lambda: _added_rows(db_conn, "api_activity_logs", before) or None,
        what="api_activity_logs row",
    )
    assert len(after) == 1
    assert after[0]["path"] == "/contract/probe"


def _seed_automation_project(db_conn):
    """Project with both automation windows on, plus stale issues.

    One stale closed issue (archive phase stamps ``archived_at``) and
    one stale open issue (close phase moves it to the project's
    default state). ``updated_at`` 60 days back against a 1-month
    window (30 days, QUIRK-1) leaves a wide margin either side of the
    worker's cutoff; no cycle/module/intake links, so the guards pass.
    """
    owner = seed_helpers.user(db_conn, "rust-replay-arch-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplayarch", owner["id"])
    project = seed_helpers.project(
        db_conn, workspace["id"], archive_in=1, close_in=1
    )
    closed_state = seed_helpers.state(
        db_conn, project["id"], workspace["id"], group="completed"
    )
    open_state = seed_helpers.state(
        db_conn, project["id"], workspace["id"], group="backlog"
    )
    close_target = seed_helpers.state(
        db_conn, project["id"], workspace["id"], group="cancelled"
    )
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET default_state_id = %s WHERE id = %s",
            (str(close_target["id"]), str(project["id"])),
        )
    stale = (datetime.now(timezone.utc) - timedelta(days=60)).isoformat()
    base_issue = {
        "workspace_id": str(workspace["id"]),
        "project_id": str(project["id"]),
        "name": "rust replay automation probe",
        "description_json": {},
        "description_html": "<p></p>",
        "priority": "none",
        "complexity_score": 0,
        "is_draft": False,
        "sort_order": 65535,
        "git_work_branch": "",
        "workpad": "",
        "updated_at": stale,
    }
    closed_issue = insert_row(
        db_conn,
        "issues",
        {**base_issue, "state_id": str(closed_state["id"]), "sequence_id": 1},
    )
    open_issue = insert_row(
        db_conn,
        "issues",
        {**base_issue, "state_id": str(open_state["id"]), "sequence_id": 2},
    )
    return {
        "project": project,
        "close_target": close_target,
        "closed_issue": closed_issue,
        "open_issue": open_issue,
    }


def _issue_snapshot_row(conn, issue_id):
    return snapshot(conn, ["issues"])["issues"].get(str(issue_id))


def test_rust_archive_and_close_executes_locally(db_conn, broker_url):
    """archive_and_close, Rust side: EXECUTE locally (PIDASHCONV-837).

    Revisits the ``ARCHIVE_AND_CLOSE`` forward case: 837 wired
    ``register_automation_handlers`` in ``worker()``, so the Rust
    worker owns ``ARCHIVE_AND_CLOSE_TASK`` and runs both phases
    itself. Mirrors the Django entry
    (``issue_automation_task.py:23-26``): the stale closed issue gets
    ``archived_at`` stamped to today, the stale open issue moves to
    the project's default state. Settlement of the ``rust_job_queue``
    row plus the two diffs is the proof: with the Django worker
    stopped, a forward could never produce local row changes. No
    broker-depth assertion: the scheduler loop forwards other
    domains' entries over the same broker, so only the row diffs are
    attributable to this job.
    """
    seeded = _seed_automation_project(db_conn)
    closed_id = str(seeded["closed_issue"]["id"])
    open_id = str(seeded["open_issue"]["id"])
    # Midnight-UTC guard: the worker stamps its own today; bound it
    # instead of asserting one exact date.
    earliest = datetime.now(timezone.utc).date()
    celery_id = rust_queue.publish(ARCHIVE_AND_CLOSE, args=[], kwargs={})
    rust_queue.wait_for_settled(celery_id, what=f"{ARCHIVE_AND_CLOSE} executed")

    def _stamped():
        row = _issue_snapshot_row(db_conn, closed_id)
        return row if row and row["archived_at"] is not None else None

    archived = wait_for(_stamped, what="archived_at stamp")
    assert earliest <= archived["archived_at"] <= datetime.now(timezone.utc).date()

    def _moved():
        row = _issue_snapshot_row(db_conn, open_id)
        return row if row and row["state_id"] != open_state_id else None

    open_state_id = str(seeded["open_issue"]["state_id"])
    moved = wait_for(_moved, what="close state move")
    assert moved["state_id"] == str(seeded["close_target"]["id"])


def test_rust_track_event_executes_locally(db_conn, broker_url, posthog_sink):
    """track_event, Rust side: EXECUTE locally (PIDASHCONV-1107).

    Mirrors ``test_track_event_posts_capture_to_posthog`` (same probe
    payload): the Rust worker POSTs the same /batch/ capture itself.
    Settlement plus the sink receipt is the proof: with the Django
    worker stopped, a forward could never produce a local POST. No
    broker-depth assertion: the scheduler loop forwards other
    domains' entries over the same broker, so only the sink receipt is
    attributable to this job.
    """
    import os

    owner = seed_helpers.user(db_conn, "rust-replay-evt-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplayevt", owner["id"])
    posthog_sink.clear()
    celery_id = rust_queue.publish(
        TRACK_EVENT,
        args=[str(owner["id"]), "contract.probe", workspace["slug"], {"source": "contract"}],
    )
    rust_queue.wait_for_settled(celery_id, what=f"{TRACK_EVENT} executed")
    (capture,) = posthog_sink.wait_for_count(1, what="posthog /batch/ capture")
    assert capture["path"] == "/batch/"
    # Header names are case-insensitive on the wire (RFC 9110 §5.1):
    # compare lowercased so the port (reqwest) and the SDK (requests)
    # pin the same header set despite different capitalisation.
    headers = {key.lower(): value for key, value in capture["headers"].items()}
    assert headers.get("content-type") == "application/json"
    assert headers.get("user-agent") == "posthog-python/3.5.0"
    body = capture["json"]
    assert body["api_key"] == os.environ["POSTHOG_API_KEY"]
    assert body["sentAt"]
    assert body["historical_migration"] is False
    (msg,) = body["batch"]
    assert msg["event"] == "contract.probe"
    assert msg["distinct_id"] == str(owner["id"])
    assert msg["context"] == {}
    assert msg["timestamp"]
    assert msg["properties"]["source"] == "contract"
    assert msg["properties"]["$groups"] == {"workspace": workspace["slug"]}
    assert msg["properties"]["$lib"] == "posthog-python"
    assert msg["properties"]["$lib_version"] == "3.5.0"
    assert msg["properties"]["$geoip_disable"] is True


def test_rust_track_event_assigns_workspace_role(db_conn, broker_url, posthog_sink):
    """Role events, Rust side: same owner/admin/unknown stamp as Django."""
    owner = seed_helpers.user(db_conn, "rust-replay-role-owner")
    member = seed_helpers.user(db_conn, "rust-replay-role-member")
    workspace = seed_helpers.workspace(db_conn, "rustreplayrole", owner["id"])
    missing_slug = f"missing-ws-{uuid.uuid4().hex[:8]}"
    posthog_sink.clear()
    cases = [
        (str(owner["id"]), workspace["slug"], "owner"),
        (str(member["id"]), workspace["slug"], "admin"),
        (str(owner["id"]), missing_slug, "unknown"),
    ]
    for user_id, slug, role in cases:
        celery_id = rust_queue.publish(
            TRACK_EVENT,
            args=[user_id, "user_invited_to_workspace", slug, {"case": role}],
        )
        rust_queue.wait_for_settled(celery_id, what=f"{TRACK_EVENT} executed")
    captures = posthog_sink.wait_for_count(3, what="role capture batch")
    roles = {
        capture["json"]["batch"][0]["properties"]["case"]: capture["json"]["batch"][0][
            "properties"
        ]["role"]
        for capture in captures
    }
    assert roles == {"owner": "owner", "admin": "admin", "unknown": "unknown"}


def test_rust_recent_visited_executes_locally(db_conn, broker_url):
    """recent_visited_task, Rust side: EXECUTE locally (PIDASHCONV-1107).

    Mirrors ``test_recent_visited_writes_row`` (same payload): create
    inserts the visit row with NULL audit (QUIRK-5); a repeat visit
    touches only visited_at. Settlement plus the row diffs is the
    proof: with the Django worker stopped, a forward could never
    produce local rows.
    """
    owner = seed_helpers.user(db_conn, "rust-replay-visit-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplayvisit", owner["id"])
    project = seed_helpers.project(db_conn, workspace["id"])
    entity_id = str(uuid.uuid4())
    before = snapshot(db_conn, ["user_recent_visits"])
    celery_id = rust_queue.publish(
        RECENT_VISITED,
        args=["issue", entity_id, str(owner["id"]), str(project["id"]), workspace["slug"]],
    )
    rust_queue.wait_for_settled(celery_id, what=f"{RECENT_VISITED} executed")
    (created,) = wait_for(
        lambda: _added_rows(db_conn, "user_recent_visits", before) or None,
        what="user_recent_visits row",
    )
    assert created["entity_name"] == "issue"
    assert created["entity_identifier"] == entity_id
    assert created["user_id"] == str(owner["id"])
    assert created["project_id"] == str(project["id"])
    assert created["workspace_id"] == str(workspace["id"])
    assert created["visited_at"] is not None
    assert created["created_by_id"] is None
    assert created["updated_by_id"] is None

    celery_id = rust_queue.publish(
        RECENT_VISITED,
        args=["issue", entity_id, str(owner["id"]), str(project["id"]), workspace["slug"]],
    )
    rust_queue.wait_for_settled(celery_id, what=f"{RECENT_VISITED} re-executed")

    def _touched():
        row = snapshot(db_conn, ["user_recent_visits"])["user_recent_visits"].get(
            str(created["id"])
        )
        return row if row and row["visited_at"] != created["visited_at"] else None

    touched = wait_for(_touched, what="visited_at touch")
    assert touched["visited_at"] > created["visited_at"]
    live = [
        row
        for row in snapshot(db_conn, ["user_recent_visits"])["user_recent_visits"].values()
        if row["user_id"] == str(owner["id"]) and row["deleted_at"] is None
    ]
    assert len(live) == 1
    assert touched["updated_at"] == created["updated_at"]


def _seed_page(db_conn, workspace_id, owner_id):
    return insert_row(
        db_conn,
        "pages",
        {
            "name": "Contract tx page",
            "description_json": {},
            "description_html": "<p></p>",
            "owned_by_id": str(owner_id),
            "access": 0,
            "workspace_id": str(workspace_id),
            "color": "",
            "is_locked": False,
            "view_props": {"full_width": False},
            "logo_props": {},
            "is_global": False,
            "sort_order": 65535,
            "created_by_id": str(owner_id),
        },
    )


def _live_page_logs(db_conn, page_id):
    return [
        row
        for row in snapshot(db_conn, ["page_logs"])["page_logs"].values()
        if row["page_id"] == str(page_id) and row["deleted_at"] is None
    ]


def test_rust_page_transaction_executes_locally(db_conn, broker_url):
    """page_transaction, Rust side: EXECUTE locally (PIDASHCONV-1107).

    Mirrors ``test_page_transaction_writes_page_logs`` (same payload):
    new components insert PageLog rows (the mention extract drops the
    entity_type attribute); removed ones soft-delete. Settlement plus
    the row diffs is the proof: with the Django worker stopped, a
    forward could never produce local rows.
    """
    owner = seed_helpers.user(db_conn, "rust-replay-tx-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplaytx", owner["id"])
    page = _seed_page(db_conn, workspace["id"], owner["id"])
    mention_id, image_id, entity_id = (str(uuid.uuid4()) for _ in range(3))
    html = (
        f'<mention-component id="{mention_id}" entity_identifier="{entity_id}" '
        f'entity_name="ISS-1" entity_type="issue"></mention-component>'
        f'<image-component id="{image_id}" src="{entity_id}"></image-component>'
    )
    celery_id = rust_queue.publish(
        PAGE_TRANSACTION,
        args=[html, "", str(page["id"])],
    )
    rust_queue.wait_for_settled(celery_id, what=f"{PAGE_TRANSACTION} executed")
    rows = wait_for(
        lambda: _live_page_logs(db_conn, page["id"]) or None,
        what="page_logs rows",
    )
    assert len(rows) == 2
    by_tx = {row["transaction"]: row for row in rows}
    mention = by_tx[mention_id]
    assert mention["entity_name"] == "ISS-1"
    assert mention["entity_type"] is None
    assert mention["entity_identifier"] == entity_id
    assert mention["workspace_id"] == str(workspace["id"])
    image = by_tx[image_id]
    assert image["entity_name"] == "image"
    assert image["entity_type"] is None
    assert image["entity_identifier"] == entity_id

    celery_id = rust_queue.publish(
        PAGE_TRANSACTION,
        args=["", html, str(page["id"])],
    )
    rust_queue.wait_for_settled(celery_id, what=f"{PAGE_TRANSACTION} re-executed")
    wait_for(
        lambda: True if _live_page_logs(db_conn, page["id"]) == [] else None,
        what="page_logs soft-delete",
    )
    deleted = [
        row
        for row in snapshot(db_conn, ["page_logs"])["page_logs"].values()
        if row["page_id"] == str(page["id"])
    ]
    assert len(deleted) == 2
    assert all(row["deleted_at"] is not None for row in deleted)


def test_rust_page_transaction_url_src_inserts_nothing(db_conn, broker_url):
    """URL image src, Rust side: same poisoned batch, no rows (PIDASHCONV-1107).

    Mirrors ``test_page_transaction_url_src_inserts_nothing``: the UUID
    column rejects the raw src URL, the handler logs and acks, and the
    mention row in the same batch is lost too. Settlement proves the
    handler ran (unlike the Django oracle, no marker job is needed —
    the queue row settles only after the handler returns).
    """
    owner = seed_helpers.user(db_conn, "rust-replay-txbug-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplaytxbug", owner["id"])
    page = _seed_page(db_conn, workspace["id"], owner["id"])
    html = (
        f'<mention-component id="{uuid.uuid4()}" entity_identifier="{uuid.uuid4()}" '
        f'entity_name="ISS-1" entity_type="issue"></mention-component>'
        f'<image-component id="{uuid.uuid4()}" src="https://cdn.example/logo.png">'
        f"</image-component>"
    )
    before = snapshot(db_conn, ["page_logs"])
    celery_id = rust_queue.publish(
        PAGE_TRANSACTION,
        args=[html, "", str(page["id"])],
    )
    rust_queue.wait_for_settled(celery_id, what=f"{PAGE_TRANSACTION} poisoned run")
    assert _added_rows(db_conn, "page_logs", before) == []


def test_rust_issue_activity_executes_locally(db_conn, broker_url):
    """issue_activity, Rust side: EXECUTE locally (PIDASHCONV-836).

    Revisits ``test_rust_issue_activity_forwards_to_python``: this issue
    wires ``register_activity_task`` in ``worker()``, so the Rust worker
    owns ``ISSUE_ACTIVITY_TASK``. There is no behavioral Django oracle
    test to mirror (wire + registration pins only), so this publishes a
    realistic ``issue.activity.created`` payload in the real call-site
    shape — kwargs, JSON-string ``requested_data``, real uuids, numeric
    epoch (e.g. ``api/views/cycle.py:594``) — and asserts the local
    ``issue_activities`` row diff instead of forwarding.
    ``requested_data`` carries no ``assignee_ids`` key, so exactly one
    row (``created the issue`` / ``created``) is written, with the actor
    taken from the issue row itself (``issue_activities_task.py:577-579``);
    the caller actor is a different user to pin the override. Settlement
    of the ``rust_job_queue`` row plus the new row is the proof: with the
    Django worker stopped, a forward could never produce a local row.
    No broker-depth assertion: the scheduler loop forwards other
    domains' entries over the same broker, so only the row diff is
    attributable to this job.
    """
    chain = seed_helpers.issue_chain(db_conn, "rustreplayactivity")
    issue, owner, project = chain["issue"], chain["owner"], chain["project"]
    caller = seed_helpers.user(db_conn, "rust-replay-activity-caller")
    # The create builder overwrites the caller actor with the issue's
    # creator (:577-579); set it explicitly so the pin is deterministic.
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET created_by_id = %s WHERE id = %s",
            (str(owner["id"]), str(issue["id"])),
        )
    db_conn.commit()
    epoch = 1728500000
    before = snapshot(db_conn, ["issue_activities"])
    celery_id = rust_queue.publish(
        ISSUE_ACTIVITY,
        kwargs={
            "type": "issue.activity.created",
            "requested_data": json.dumps({"name": "contract issue"}),
            "current_instance": None,
            "issue_id": str(issue["id"]),
            "actor_id": str(caller["id"]),
            "project_id": str(project["id"]),
            "epoch": epoch,
        },
    )
    rust_queue.wait_for_settled(celery_id, what=f"{ISSUE_ACTIVITY} executed")
    after = wait_for(
        lambda: _added_rows(db_conn, "issue_activities", before) or None,
        what="issue_activities row",
    )
    assert len(after) == 1
    row = after[0]
    assert row["comment"] == "created the issue"
    assert row["verb"] == "created"
    assert row["issue_id"] == str(issue["id"])
    assert row["project_id"] == str(project["id"])
    assert row["actor_id"] == str(owner["id"])
    assert row["epoch"] == epoch


#: ``data:image/svg+xml;base64,`` + DEFAULT_FAVICON
#: (``work_item_link_task.py:24``), the favicon fallback.
DEFAULT_FAVICON_DATA_URI = (
    "data:image/svg+xml;base64,"
    "PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIyNCIgaGVpZ2h0PSIyNCIg"
    "dmlld0JveD0iMCAwIDI0IDI0IiBmaWxsPSJub25lIiBzdHJva2U9ImN1cnJlbnRDb2xvciIgc3Ryb2tlLXdp"
    "ZHRoPSIyIiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiIGNsYXNzPSJs"
    "dWNpZGUgbHVjaWRlLWxpbmstaWNvbiBsdWNpZGUtbGluayI+PHBhdGggZD0iTTEwIDEzYTUgNSAwIDAgMCA3"
    "LjU0LjU0bDMtM2E1IDUgMCAwIDAtNy4wNy03LjA3bC0xLjcyIDEuNzEiLz48cGF0aCBkPSJNMTQgMTFhNSA1"
    "IDAgMCAwLTcuNTQtLjU0bC0zIDNhNSA1IDAgMCAwIDcuMDcgNy4wN2wxLjcxLTEuNzEiLz48L3N2Zz4="
)


def test_rust_crawl_link_executes_locally(db_conn, broker_url):
    """crawl_work_item_link_title, Rust side: EXECUTE locally (PIDASHCONV-1107).

    Mirrors ``test_crawl_link_updates_metadata`` (same payload): the
    loopback URL trips the SSRF guard on both backends, so the metadata
    keeps the fallback shape. Settlement plus the metadata diff is the
    proof: with the Django worker stopped, a forward could never
    produce a local row change.
    """
    chain = seed_helpers.issue_chain(db_conn, "rustreplaycrawl")
    issue, project, workspace = chain["issue"], chain["project"], chain["workspace"]
    url = "http://127.0.0.1:9/rust-replay-probe"
    link = insert_row(
        db_conn,
        "issue_links",
        {
            "title": "Seed probe",
            "url": url,
            "issue_id": str(issue["id"]),
            "project_id": str(project["id"]),
            "workspace_id": str(workspace["id"]),
            "created_by_id": str(chain["owner"]["id"]),
            "updated_by_id": str(chain["owner"]["id"]),
            "metadata": {},
        },
    )
    celery_id = rust_queue.publish(
        CRAWL_LINK_TITLE,
        args=[str(link["id"]), url],
    )
    rust_queue.wait_for_settled(celery_id, what=f"{CRAWL_LINK_TITLE} executed")

    def _crawled():
        row = snapshot(db_conn, ["issue_links"])["issue_links"].get(str(link["id"]))
        return row if row and row["metadata"] != {} else None

    crawled = wait_for(_crawled, what="issue_links metadata")
    assert crawled["metadata"] == {
        "title": None,
        "favicon": DEFAULT_FAVICON_DATA_URI,
        "url": url,
        "favicon_url": None,
    }

