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
  per job (the Postgres row is deleted once the worker finishes it).
- ``test_worker_registration`` (Celery inspect) ->
  ``test_rust_worker_liveness`` below: a webhook-send job must settle
  from ``rust_job_queue`` and POST to the harness sink, proving a Rust
  worker (not Django) is serving this database.

What is NOT duplicated here: the static parity tests (wire payload,
retry options, beat entries, fan-out call sites) are
backend-independent — they assert on Django source text and the
in-process wire builder, so they already pass with no worker at all.
Only the live worker tests have a replay form.

Ownership split (mirrors the worker registry on rust-dev): all 11
D-08 task names are Rust-owned — ``process_logs`` (PIDASHCONV-260),
``archive_and_close_old_issues`` (PIDASHCONV-837),
``issue_activity`` (PIDASHCONV-836), ``track_event``,
``page_transaction``, ``recent_visited_task`` and
``crawl_work_item_link_title`` (PIDASHCONV-1107), plus the four
``webhook_task`` names (PIDASHCONV-1105 wires
``register_webhook_send_task``, ``register_deactivation_email_task``,
``register_webhook_activity_task`` and ``register_model_activity_task``
in ``register_oss_handlers``). Every test below asserts local
execution — the same DB effects and sink captures the Django oracle
pins — with the Django worker stopped, so a forward could never
produce them. No skipped or weakened assertions.

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
PostHog sink assertions need the replay job's ``POSTHOG_*`` env.

standalone note 2 (PIDASHCONV-1105 registers the four ``webhook_task``
names): the last forward pins are gone too. The liveness probe stays
on ``WEBHOOK_SEND`` but now asserts local execution (settle + sink
POST); the ``test_rust_webhook_*`` / ``test_rust_activity_*`` /
``test_rust_deactivation_*`` tests mirror the oracle's send, failure,
redelivery, chain, midpoint, signature, exhaustion and mail cases.
The ``webhook_sink`` / ``smtp_sink`` fixtures serve both suites from
their session defaults — no replay-job env needed.

soft-delete note: these webhook reads go through ``Model.objects`` (the
inherited ``SoftDeletionManager``, ``pi_dash/db/mixins.py:65``), so a
soft-deleted webhook / issue is invisible to the Django tasks. The
oracle suite does not seed deleted rows, so the ``*_soft_deleted_*``
tests below are the regression pins: a deleted webhook gets no send job
and no POST, and a deleted event model swallows the fan-out.

Beat note: D-08 owns no beat entries (``test_no_beat_entries_owned`` —
cleanup owns the webhook-log delete; the daily
``check-every-day-to-archive-and-close`` entry names
``issue_automation_task.archive_and_close_old_issues``, which that
static test does not count as owned). Beat pins are source-text, so
there is no replay form.
"""

import ast
import hashlib
import hmac
import json
import socket
import time
import uuid
from datetime import datetime, timedelta, timezone

import pytest

from _harness import config, rust_queue
from _harness import seed as seed_helpers
from _harness.db import insert_row
from _harness.db import snapshot, wait_for_condition as wait_for
from _harness.sinks import HostileLengthSink, WildcardHttpProbe
from psycopg.types.json import Json

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


def test_rust_worker_liveness(db_conn, broker_url, webhook_sink):
    """Rust inspect-equivalent: an owned send job settles and POSTs.

    Replaces ``test_worker_registration`` (Celery broadcast inspect,
    which only a Django worker answers). Settlement of the
    ``rust_job_queue`` row proves a Rust worker owns the queue (a
    Django worker never reads that table); the sink POST proves it
    executed the job locally instead of forwarding it.
    """
    hook = _seed_sink_hook(db_conn)
    webhook_sink.clear()
    celery_id = rust_queue.publish(
        WEBHOOK_SEND, kwargs=_send_kwargs(hook, "rust-replay-liveness")
    )
    rust_queue.wait_for_settled(celery_id, what="liveness send settled")
    webhook_sink.wait_for_count(1, what="liveness webhook POST")


def _seed_sink_hook(db_conn, **flags):
    """Workspace + active hook pointing at the harness sink (PIDASHCONV-1105)."""
    owner = seed_helpers.user(db_conn, "rust-replay-hook-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplayhook", owner["id"])
    url = f"{config.WEBHOOK_SINK_BASE}/hook/{uuid.uuid4().hex}"
    hook = seed_helpers.webhook(db_conn, workspace["id"], url, issue=True, **flags)
    hook = dict(hook)
    hook["workspace_slug"] = workspace["slug"]
    hook["owner_id"] = owner["id"]
    return hook


def _seed_hook_at(db_conn, base_url, **flags):
    owner = seed_helpers.user(db_conn, "rust-replay-chunked-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplaychunked", owner["id"])
    url = f"{base_url}/hook/{uuid.uuid4().hex}"
    hook = seed_helpers.webhook(db_conn, workspace["id"], url, issue=True, **flags)
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


def test_rust_webhook_send_executes_locally(db_conn, broker_url, webhook_sink):
    """Send path, Rust side: EXECUTE locally (PIDASHCONV-1105).

    Mirrors ``test_webhook_send_success`` (same payload incl. the
    header-spaces bug surface): the Rust worker POSTs through the raw
    requests-faithful client and writes the ``webhook_logs`` row
    itself. Settlement plus the sink receipt plus the row diff is the
    proof: with the Django worker stopped, a forward could never
    produce a local POST or a local row.
    """
    hook = _seed_sink_hook(db_conn)
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])
    celery_id = rust_queue.publish(
        WEBHOOK_SEND, kwargs=_send_kwargs(hook, "rust-replay-probe")
    )
    rust_queue.wait_for_settled(celery_id, what=f"{WEBHOOK_SEND} executed")
    (delivery,) = webhook_sink.wait_for_count(1, what="webhook POST")
    assert delivery["headers"].get("Content-Type") == "application/json"
    assert delivery["headers"].get("User-Agent") == "Autopilot"
    assert "X-Pi Dash-Event" not in delivery["headers"]
    assert "X-Pi Dash-Delivery" not in delivery["headers"]
    assert "X-Pi Dash-Signature" not in delivery["headers"]
    assert delivery["body"] == ""

    (row,) = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="webhook_logs row",
    )
    assert str(row["response_status"]) == "200"
    assert row["retry_count"] == 0
    assert row["request_method"] == "create"
    assert "X-Pi Dash-Event" in row["request_headers"]
    assert "rust-replay-probe" in row["request_body"]
    assert "create" in row["request_body"]


def test_rust_webhook_send_chunked_keepalive_returns_at_terminal_chunk(
    db_conn, broker_url, chunked_keepalive_sink
):
    """Rust side: chunked + keep-alive must not time out, log 500, or retry."""
    hook = _seed_hook_at(db_conn, chunked_keepalive_sink.url)
    chunked_keepalive_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])
    baseline = rust_queue.queue_depth()
    celery_id = rust_queue.publish(
        WEBHOOK_SEND, kwargs=_send_kwargs(hook, "rust-replay-chunked")
    )
    deliveries = chunked_keepalive_sink.wait_for_count(1, what="chunked webhook POST")
    assert len(deliveries) == 1
    (row,) = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="chunked webhook_logs row",
    )
    assert str(row["response_status"]) == "200"
    assert row["response_body"] == "ok"
    assert row["retry_count"] == 0
    rust_queue.wait_for_settled(celery_id, what="chunked send acked")
    rust_queue.wait_for_queue_drain(baseline=baseline, what="chunked queue drained")
    time.sleep(1)
    assert len(chunked_keepalive_sink.snapshot()) == 1, "event was redelivered"
    assert not rust_queue.row_present(celery_id), "chunked send was requeued"
    assert len(_log_diff_rows(db_conn, before)) == 1


def test_rust_webhook_failure_executes_locally(db_conn, broker_url, webhook_sink):
    """First-attempt failure, Rust side: same 500 row, no retry (PIDASHCONV-1105).

    Mirrors ``test_webhook_send_failure_logs_retry_count``: a 500
    *response* is the success path (only transport errors retry), so
    the Rust worker logs the failure row with ``retry_count`` 0 and
    acks. Retry timing stays pinned statically
    (``test_retry_options_parity``).
    """
    hook = _seed_sink_hook(db_conn)
    webhook_sink.clear()
    webhook_sink.fail_next(100)
    before = snapshot(db_conn, ["webhook_logs"])
    celery_id = rust_queue.publish(
        WEBHOOK_SEND, kwargs=_send_kwargs(hook, "rust-replay-fail")
    )
    rust_queue.wait_for_settled(celery_id, what=f"{WEBHOOK_SEND} executed")
    webhook_sink.wait_for_count(1, what="failed webhook POST")
    (row,) = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="failure webhook_logs row",
    )
    assert row["retry_count"] == 0
    assert str(row["response_status"]) == "500"


def test_rust_webhook_send_hostile_content_length_retries_locally(
    db_conn, broker_url, webhook_sink
):
    """Hostile Content-Length, Rust side: failure row + requeue, worker alive (PIDASHCONV-1273).

    Mirrors ``test_webhook_send_hostile_content_length_logs_failure_and_retries``:
    the sink answers ``Content-Length: 1000000000000`` and closes. The
    client must report a transport error (EOF before the promised
    bytes) — not abort the worker on a 1 TB preallocation — so the
    handler logs the failure row (500, ``retry_count`` 0) and the
    worker requeues with the Celery backoff (the row returns to
    ``queued`` with ``attempts`` bumped). A follow-up send to the
    healthy sink proves the worker survived. The requeued row is
    deleted at teardown so its jittered refire (up to 600s out) cannot
    pollute later tests. A fast jitter draw may refire before the
    delete lands, so the requeue poll accepts ``attempts >= 1`` and
    the row assertions pin the first attempt.
    """
    with HostileLengthSink() as hostile:
        owner = seed_helpers.user(db_conn, "rust-replay-hostile-owner")
        workspace = seed_helpers.workspace(db_conn, "rustreplayhostile", owner["id"])
        hook = seed_helpers.webhook(
            db_conn,
            workspace["id"],
            f"{hostile.url}/hook/{uuid.uuid4().hex}",
            issue=True,
        )
        hook = dict(hook)
        hook["workspace_slug"] = workspace["slug"]
        before = snapshot(db_conn, ["webhook_logs"])
        celery_id = rust_queue.publish(
            WEBHOOK_SEND, kwargs=_send_kwargs(hook, "rust-replay-hostile")
        )
        hostile.wait_for_count(1, what="hostile webhook POST")
        rows = wait_for(
            lambda: [
                row
                for row in _log_diff_rows(db_conn, before)
                if "rust-replay-hostile" in row["request_body"]
            ]
            or None,
            what="hostile-length webhook_logs row",
        )
        first = min(rows, key=lambda row: row["retry_count"])
        assert first["retry_count"] == 0
        assert str(first["response_status"]) == "500"
        assert first["response_headers"] == ""
        assert first["response_body"]

        def _requeued():
            with db_conn.cursor() as cur:
                cur.execute(
                    "SELECT status, attempts FROM rust_job_queue WHERE celery_id = %s",
                    (celery_id,),
                )
                row = cur.fetchone()
            if row is None:
                return None
            status, attempts = row
            if status == "queued" and attempts >= 1:
                return (status, attempts)
            return None

        assert wait_for(_requeued, what="requeued retry row") is not None
        with db_conn.cursor() as cur:
            cur.execute(
                "DELETE FROM rust_job_queue WHERE celery_id = %s", (celery_id,)
            )

    # The worker survived the hostile delivery: a follow-up send to the
    # healthy sink executes end to end.
    hook = _seed_sink_hook(db_conn)
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])
    celery_id = rust_queue.publish(
        WEBHOOK_SEND, kwargs=_send_kwargs(hook, "rust-replay-hostile-liveness")
    )
    rust_queue.wait_for_settled(celery_id, what="post-hostile send settled")
    webhook_sink.wait_for_count(1, what="post-hostile webhook POST")
    alive = wait_for(
        lambda: [
            row
            for row in _log_diff_rows(db_conn, before)
            if "rust-replay-hostile-liveness" in row["request_body"]
        ]
        or None,
        what="post-hostile webhook_logs row",
    )
    assert str(alive[0]["response_status"]) == "200"


def test_rust_webhook_redelivery_executes_locally(db_conn, broker_url, webhook_sink):
    """Redelivery: two publishes → two POSTs, two rows (PIDASHCONV-1105).

    Mirrors ``test_webhook_redelivery_posts_twice_with_identical_bodies``:
    Django intends distinct per-execution delivery ids, but the
    ``X-Pi Dash-Delivery`` header never reaches the wire, so parity is
    two POSTs with identical (empty) bodies plus two ``webhook_logs``
    rows. Both ``rust_job_queue`` rows settle.
    """
    hook = _seed_sink_hook(db_conn)
    webhook_sink.clear()
    kwargs = _send_kwargs(hook, "rust-replay-redelivery")
    before = snapshot(db_conn, ["webhook_logs"])
    celery_ids = [
        rust_queue.publish(WEBHOOK_SEND, kwargs=dict(kwargs)) for _ in range(2)
    ]
    for celery_id in celery_ids:
        rust_queue.wait_for_settled(celery_id, what="redelivered webhook executed")
    deliveries = webhook_sink.wait_for_count(2, what="redelivered webhook POSTs")
    assert all("X-Pi Dash-Delivery" not in d["headers"] for d in deliveries)
    assert [d["body"] for d in deliveries] == [""] * 2
    logged = wait_for(
        lambda: _logs_with_count(db_conn, before, 2),
        what="two redelivered webhook_logs rows",
    )
    assert all(str(r["response_status"]) == "200" for r in logged)


def test_rust_activity_chain_executes_locally(db_conn, broker_url, webhook_sink):
    """Chain entry, Rust side: the whole fan-out runs locally (PIDASHCONV-1105).

    Mirrors ``test_activity_chain_fans_out_to_sink`` (same seed, same
    positional payload): ``model_activity`` enqueues
    ``webhook_activity`` into ``rust_job_queue``, which renders
    ``get_model_data`` live and enqueues the sends — the sink POST and
    the ``webhook_logs`` row prove every link ran with the Django
    worker stopped.
    """
    # model_id must be a real issue: webhook_activity serializes it via
    # get_model_data, and a miss swallows the fan-out (no POST).
    chain = seed_helpers.issue_chain(db_conn, "rustreplaychain")
    owner, workspace, issue = chain["owner"], chain["workspace"], chain["issue"]
    url = f"{config.WEBHOOK_SINK_BASE}/hook/{uuid.uuid4().hex}"
    seed_helpers.webhook(db_conn, workspace["id"], url, issue=True)
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])
    celery_id = rust_queue.publish(
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
    rust_queue.wait_for_settled(celery_id, what=f"{MODEL_ACTIVITY} executed")
    (delivery,) = webhook_sink.wait_for_count(1, what="chained webhook POST")
    assert delivery["body"] == ""
    (row,) = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="chained webhook_logs row",
    )
    assert row["event_type"] == "issue"


def test_rust_webhook_activity_executes_locally(db_conn, broker_url, webhook_sink):
    """Fan-out midpoint, Rust side: EXECUTE locally (PIDASHCONV-1105).

    Publishes ``webhook_activity`` directly in the oracle arg shape (the
    kwargs ``model_activity`` passes on the created path,
    ``webhook_task.py:467-480``): the Rust worker renders the issue via
    ``get_model_data`` and the send POSTs to the sink.
    """
    chain = seed_helpers.issue_chain(db_conn, "rustreplayfanout")
    owner, workspace, issue = chain["owner"], chain["workspace"], chain["issue"]
    url = f"{config.WEBHOOK_SINK_BASE}/hook/{uuid.uuid4().hex}"
    seed_helpers.webhook(db_conn, workspace["id"], url, issue=True)
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])
    celery_id = rust_queue.publish(
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
            "event_id": str(issue["id"]),
            "old_identifier": None,
            "new_identifier": None,
        },
    )
    rust_queue.wait_for_settled(celery_id, what=f"{WEBHOOK_ACTIVITY} executed")
    webhook_sink.wait_for_count(1, what="fan-out webhook POST")
    (row,) = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="fan-out webhook_logs row",
    )
    assert row["event_type"] == "issue"
    assert str(row["response_status"]) == "200"


def _soft_delete(conn, table, row_id):
    """Mark one seeded row soft-deleted (``deleted_at = now()``)."""
    with conn.cursor() as cur:
        cur.execute(
            f"UPDATE {table} SET deleted_at = now() WHERE id = %s",
            (str(row_id),),
        )


def test_rust_webhook_send_skips_soft_deleted_webhook(
    db_conn, broker_url, webhook_sink
):
    """Send task, Rust side: a soft-deleted webhook is swallowed.

    Django reads ``Webhook.objects.get(id=..., workspace__slug=...)``
    through the inherited ``SoftDeletionManager`` (``pi_dash/db/mixins.py:65``),
    so a soft-deleted row is invisible: the task returns before POSTing
    or logging. The Rust fetch must scope ``deleted_at IS NULL`` the same
    way — the job settles, no POST reaches the sink, and no
    ``webhook_logs`` row appears. (The oracle suite does not seed deleted
    rows, so this replay case is the regression pin.)
    """
    hook = _seed_sink_hook(db_conn)
    _soft_delete(db_conn, "webhooks", hook["id"])
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])
    celery_id = rust_queue.publish(
        WEBHOOK_SEND, kwargs=_send_kwargs(hook, "rust-replay-dead-hook")
    )
    rust_queue.wait_for_settled(
        celery_id, what=f"{WEBHOOK_SEND} settled for a soft-deleted webhook"
    )
    # Settlement deletes the row only after the handler returns, so any
    # (incorrect) POST would already be recorded in the sink.
    assert webhook_sink.snapshot() == [], "send POSTed to a soft-deleted webhook"
    assert _log_diff_rows(db_conn, before) == [], "send logged a soft-deleted webhook"


def test_rust_fanout_skips_soft_deleted_webhook(db_conn, broker_url, webhook_sink):
    """Fan-out, Rust side: soft-deleted webhooks get no send job.

    ``webhook_activity`` selects
    ``Webhook.objects.filter(workspace__slug=..., is_active=True)`` — the
    soft-deleting manager hides deleted hooks, so they never receive a
    send. Seed one live issue plus a single soft-deleted hook: the Rust
    worker renders the issue, finds no active hook, and enqueues nothing.
    """
    chain = seed_helpers.issue_chain(db_conn, "rustreplaydeadhook")
    owner, workspace, issue = chain["owner"], chain["workspace"], chain["issue"]
    url = f"{config.WEBHOOK_SINK_BASE}/hook/{uuid.uuid4().hex}"
    hook = seed_helpers.webhook(db_conn, workspace["id"], url, issue=True)
    _soft_delete(db_conn, "webhooks", hook["id"])
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])
    baseline = rust_queue.queue_depth()
    celery_id = rust_queue.publish(
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
            "event_id": str(issue["id"]),
            "old_identifier": None,
            "new_identifier": None,
        },
    )
    rust_queue.wait_for_settled(
        celery_id, what=f"{WEBHOOK_ACTIVITY} settled with only a deleted hook"
    )
    # Any send the fan-out enqueued is claimed before the activity row
    # settles, so draining back to the pre-publish depth proves none (and
    # no POST/log) followed.
    rust_queue.wait_for_queue_drain(
        baseline=baseline, what="soft-deleted hook fan-out drained"
    )
    assert webhook_sink.snapshot() == [], "fan-out notified a soft-deleted webhook"
    assert _log_diff_rows(db_conn, before) == [], "fan-out logged a deleted hook"


def test_rust_fanout_swallows_soft_deleted_issue(db_conn, broker_url, webhook_sink):
    """Fan-out, Rust side: a soft-deleted event model is swallowed.

    ``get_model_data`` reads via ``Model.objects.get(pk=...)``, so a
    soft-deleted issue raises ``ObjectDoesNotExist``; the task's broad
    ``except`` returns silently and no webhook is notified. Seed a live
    hook plus a soft-deleted issue: the Rust worker must also find no
    issue (``deleted_at IS NULL``) and enqueue nothing.
    """
    chain = seed_helpers.issue_chain(db_conn, "rustreplaydeadissue")
    owner, workspace, issue = chain["owner"], chain["workspace"], chain["issue"]
    url = f"{config.WEBHOOK_SINK_BASE}/hook/{uuid.uuid4().hex}"
    seed_helpers.webhook(db_conn, workspace["id"], url, issue=True)
    _soft_delete(db_conn, "issues", issue["id"])
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])
    baseline = rust_queue.queue_depth()
    celery_id = rust_queue.publish(
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
            "event_id": str(issue["id"]),
            "old_identifier": None,
            "new_identifier": None,
        },
    )
    rust_queue.wait_for_settled(
        celery_id, what=f"{WEBHOOK_ACTIVITY} settled for a deleted issue"
    )
    rust_queue.wait_for_queue_drain(
        baseline=baseline, what="soft-deleted issue fan-out drained"
    )
    assert webhook_sink.snapshot() == [], "fan-out notified a soft-deleted issue"
    assert _log_diff_rows(db_conn, before) == [], "fan-out logged a deleted issue"


def test_rust_deactivation_email_executes_locally(db_conn, broker_url, smtp_sink):
    """Deactivation mail, Rust side: EXECUTE locally (PIDASHCONV-1105).

    Mirrors ``test_deactivation_email_delivers`` (same positional
    payload): the Rust worker renders the shared template and SMTPs it
    through the live mail send seam.
    """
    owner = seed_helpers.user(db_conn, "rust-replay-deact-owner")
    receiver = seed_helpers.user(db_conn, "rust-replay-deact-receiver")
    workspace = seed_helpers.workspace(db_conn, "rustreplaydeact", owner["id"])
    hook = seed_helpers.webhook(
        db_conn, workspace["id"], "https://example.com/hook",
        created_by_id=str(owner["id"]),
    )
    smtp_sink.clear()
    celery_id = rust_queue.publish(
        DEACTIVATION_EMAIL,
        args=[str(hook["id"]), str(receiver["id"]), "example.com", "contract reason"],
    )
    rust_queue.wait_for_settled(celery_id, what=f"{DEACTIVATION_EMAIL} executed")
    delivered = smtp_sink.wait_for_count(1, what="deactivation mail")
    assert any(receiver["email"] in m["rcpt_tos"] for m in delivered)
    assert any(m["subject"] == "Webhook Deactivated" for m in delivered)


def _expected_payload(hook, probe_id):
    """The delivery envelope the send task signs and POSTs (PIDASHCONV-1105)."""
    return {
        "event": "issue",
        "action": "create",
        "webhook_id": str(hook["id"]),
        "workspace_id": str(hook["workspace_id"]),
        "data": {"id": probe_id},
        "activity": None,
    }


def _expected_signature(secret, payload):
    return hmac.new(
        secret.encode("utf-8"),
        json.dumps(payload).encode("utf-8"),
        hashlib.sha256,
    ).hexdigest()


def test_rust_webhook_send_signature_logged(db_conn, broker_url, webhook_sink):
    """Fixed secret, Rust side: identical HMAC over the envelope (PIDASHCONV-1105).

    Mirrors ``test_webhook_send_signature_logged``: the logged
    ``X-Pi Dash-Signature`` must equal the same recomputation, and the
    logged headers/body must parse (``ast.literal_eval``) to the same
    ordered dicts — pinning the signature input bytes, the HMAC, and
    the ``str()`` log rendering on both backends.
    """
    hook = _seed_sink_hook(db_conn, secret_key="contract-secret-1105")
    probe_id = "rust-replay-signature"
    payload = _expected_payload(hook, probe_id)
    webhook_sink.clear()
    before = snapshot(db_conn, ["webhook_logs"])
    celery_id = rust_queue.publish(
        WEBHOOK_SEND, kwargs=_send_kwargs(hook, probe_id)
    )
    rust_queue.wait_for_settled(celery_id, what=f"{WEBHOOK_SEND} executed")
    webhook_sink.wait_for_count(1, what="signed webhook POST")
    (row,) = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="signed webhook_logs row",
    )
    logged_headers = ast.literal_eval(row["request_headers"])
    assert list(logged_headers) == [
        "Content-Type",
        "User-Agent",
        "X-Pi Dash-Delivery",
        "X-Pi Dash-Event",
        "X-Pi Dash-Signature",
    ]
    assert logged_headers["X-Pi Dash-Signature"] == _expected_signature(
        "contract-secret-1105", payload
    )
    logged_body = ast.literal_eval(row["request_body"])
    assert list(logged_body) == [
        "event",
        "action",
        "webhook_id",
        "workspace_id",
        "data",
        "activity",
    ]
    assert logged_body == payload


def test_rust_webhook_send_exhaustion_deactivates(db_conn, broker_url, smtp_sink):
    """Retries exhausted, Rust side: same row, disable, mail (PIDASHCONV-1105).

    Mirrors ``test_webhook_send_exhaustion_deactivates``: the row lands
    with ``attempts=5`` preset (a redelivered sixth attempt) against a
    refused URL, so the transport error takes the
    ``attempts >= max_retries`` arm directly — failure row, webhook
    flipped ``is_active`` with ``updated_at`` untouched, deactivation
    mail to the creator. No retry is scheduled.
    """
    owner = seed_helpers.user(db_conn, "rust-replay-exhaust-owner")
    workspace = seed_helpers.workspace(db_conn, "rustreplayexhaust", owner["id"])
    hook = seed_helpers.webhook(
        db_conn,
        workspace["id"],
        f"http://127.0.0.1:9/{uuid.uuid4().hex}",
        issue=True,
        created_by_id=str(owner["id"]),
    )
    hook = dict(hook)
    hook["workspace_slug"] = workspace["slug"]
    before_hooks = snapshot(db_conn, ["webhooks"])["webhooks"][str(hook["id"])]
    before = snapshot(db_conn, ["webhook_logs"])
    smtp_sink.clear()
    celery_id = str(uuid.uuid4())
    kwargs = _send_kwargs(hook, "rust-replay-exhaustion")
    with db_conn.cursor() as cur:
        cur.execute(
            "INSERT INTO rust_job_queue "
            "(celery_id, task, args, kwargs, queue, visible_at, attempts, max_retries) "
            "VALUES (%s, %s, %s, %s, 'celery', now(), 5, 5)",
            (celery_id, WEBHOOK_SEND, Json([]), Json(kwargs)),
        )
    db_conn.commit()
    rust_queue.wait_for_settled(celery_id, what="exhausted send settled")
    (row,) = wait_for(
        lambda: _log_diff_rows(db_conn, before) or None,
        what="exhaustion webhook_logs row",
    )
    assert row["retry_count"] == 5
    assert str(row["response_status"]) == "500"
    assert row["response_headers"] == ""
    assert "refused" in row["response_body"].lower()

    def _deactivated():
        current = snapshot(db_conn, ["webhooks"])["webhooks"][str(hook["id"])]
        return current if current["is_active"] is False else None

    deactivated = wait_for(_deactivated, what="webhook deactivation")
    assert deactivated["updated_at"] == before_hooks["updated_at"]

    delivered = smtp_sink.wait_for_count(1, what="exhaustion deactivation mail")
    assert any(owner["email"] in m["rcpt_tos"] for m in delivered)
    assert any(m["subject"] == "Webhook Deactivated" for m in delivered)


def _log_diff_rows(conn, before):
    after = snapshot(conn, ["webhook_logs"])["webhook_logs"]
    new_keys = set(after) - set(before["webhook_logs"])
    return [after[k] for k in new_keys]


def _logs_with_count(conn, before, n):
    rows = _log_diff_rows(conn, before)
    return rows if len(rows) == n else None


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


def _seed_closed_to_world(db_conn, slug):
    """Issue + cancel state + one notified subscriber (PIDASHCONV-1276).

    Mirrors the oracle seed in ``test_webhook_tasks.py``: the
    subscriber is an active project member with a preferences row, so
    ``notifications`` writes exactly one ``notifications`` row for
    them.
    """
    chain = seed_helpers.issue_chain(db_conn, slug)
    owner, workspace, project, issue = (
        chain["owner"],
        chain["workspace"],
        chain["project"],
        chain["issue"],
    )
    close_state = seed_helpers.state(
        db_conn, project["id"], workspace["id"], name="Cancelled", group="cancelled"
    )
    sub = seed_helpers.user(db_conn, f"{slug}-subscriber")
    seed_helpers.project_member(db_conn, project["id"], workspace["id"], sub["id"])
    seed_helpers.issue_subscriber(
        db_conn, project["id"], workspace["id"], issue["id"], sub["id"]
    )
    seed_helpers.notification_preference(db_conn, sub["id"])
    return {
        "owner": owner,
        "workspace": workspace,
        "project": project,
        "issue": issue,
        "close_state": close_state,
        "subscriber": sub,
    }


def test_rust_issue_activity_closed_to_writes_state_row_and_notifies(db_conn, broker_url):
    """closed_to, Rust side: EXECUTE locally (PIDASHCONV-1276).

    Mirrors ``test_issue_activity_closed_to_writes_state_row_and_notifies``
    (same auto-close payload): the Rust worker resolves ``closed_to``
    through the project-scoped states snapshot
    (``activity_dispatch.rs``), writes the one
    ``"Pi Dash updated the state to "`` row, and enqueues + executes
    ``notifications`` for the subscriber. Settlement plus the row
    diffs is the proof: with the Django worker stopped, a forward
    could never produce local rows.
    """
    world = _seed_closed_to_world(db_conn, "rustclosedto")
    issue, owner, project = world["issue"], world["owner"], world["project"]
    close_state, sub = world["close_state"], world["subscriber"]
    epoch = 1728500000
    before = snapshot(db_conn, ["issue_activities", "notifications", "email_notification_logs"])

    celery_id = rust_queue.publish(
        ISSUE_ACTIVITY,
        kwargs={
            "type": "issue.activity.updated",
            "requested_data": json.dumps({"closed_to": str(close_state["id"])}),
            "current_instance": None,
            "issue_id": str(issue["id"]),
            "actor_id": str(owner["id"]),
            "project_id": str(project["id"]),
            "epoch": epoch,
            "subscriber": False,
            "notification": True,
        },
    )
    rust_queue.wait_for_settled(celery_id, what=f"{ISSUE_ACTIVITY} executed")
    (row,) = wait_for(
        lambda: _added_rows(db_conn, "issue_activities", before) or None,
        what="closed_to issue_activities row",
    )
    assert row["field"] == "state"
    assert row["verb"] == "updated"
    assert row["comment"] == "Pi Dash updated the state to "
    assert row["new_value"] == close_state["name"]
    assert row["new_identifier"] == str(close_state["id"])
    assert row["old_value"] is None
    assert row["old_identifier"] is None
    assert row["issue_id"] == str(issue["id"])
    assert row["project_id"] == str(project["id"])
    assert row["actor_id"] == str(owner["id"])
    assert row["epoch"] == epoch

    (note,) = wait_for(
        lambda: _added_rows(db_conn, "notifications", before) or None,
        what="closed_to notifications row",
    )
    assert note["receiver_id"] == str(sub["id"])
    assert note["entity_identifier"] == str(issue["id"])
    assert note["sender"] == "in_app:issue_activities:subscribed"
    assert note["title"] == "Pi Dash updated the state to "
    assert note["triggered_by_id"] == str(owner["id"])
    assert len(_added_rows(db_conn, "email_notification_logs", before)) == 1


@pytest.mark.parametrize(
    "family",
    [socket.AF_INET, socket.AF_INET6],
    ids=["wildcard-ipv4", "wildcard-ipv6"],
)
def test_rust_crawl_link_denies_wildcard_hosts(db_conn, broker_url, family):
    """Rust-side guard refusal for 0.0.0.0 and :: (Django parity).

    The listener is bound to the wildcard address and serves a distinctive
    title/icon, so a bypass is visible as populated metadata rather than
    being indistinguishable from a connection failure.
    """
    with WildcardHttpProbe(family) as probe:
        chain = seed_helpers.issue_chain(
            db_conn,
            "rustwildcard" + ("v6" if family == socket.AF_INET6 else "v4"),
        )
        issue, project, workspace = chain["issue"], chain["project"], chain["workspace"]
        url = probe.url
        link = insert_row(
            db_conn,
            "issue_links",
            {
                "title": "Wildcard probe",
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


def test_rust_crawl_link_nulls_audit_columns(db_conn, broker_url):
    """Crawl, Rust side: the save NULLs both audit columns (PIDASHCONV-1281).

    Mirrors ``test_crawl_link_nulls_audit_columns``: Django's
    worker-context full ``save()`` blanks ``created_by``/``updated_by``
    (``BaseModel.save``, no crum user — the PB-2008/PB-2009 semantics),
    so the Rust ``UPDATE`` must carry ``created_by_id = NULL,
    updated_by_id = NULL`` alongside the metadata write.
    """
    chain = seed_helpers.issue_chain(db_conn, "rustreplaycrawlaudit")
    issue, project, workspace = chain["issue"], chain["project"], chain["workspace"]
    url = "http://127.0.0.1:9/rust-replay-audit-probe"
    link = insert_row(
        db_conn,
        "issue_links",
        {
            "title": "Audit probe",
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
    assert crawled["created_by_id"] is None
    assert crawled["updated_by_id"] is None


def test_rust_crawl_link_skips_soft_deleted_link(db_conn, broker_url):
    """Crawl, Rust side: a soft-deleted link is not touched (PIDASHCONV-1281).

    Mirrors ``test_crawl_link_skips_soft_deleted_link``: the Rust lookup
    must scope ``deleted_at IS NULL`` like the ``SoftDeletionManager``,
    so the job warns and settles without writing. Settlement deletes the
    queue row only after the handler returns, so any (incorrect) write
    would already be visible in the re-read row.
    """
    chain = seed_helpers.issue_chain(db_conn, "rustreplaycrawldead")
    issue, project, workspace = chain["issue"], chain["project"], chain["workspace"]
    url = "http://127.0.0.1:9/rust-replay-dead-probe"
    link = insert_row(
        db_conn,
        "issue_links",
        {
            "title": "Dead probe",
            "url": url,
            "issue_id": str(issue["id"]),
            "project_id": str(project["id"]),
            "workspace_id": str(workspace["id"]),
            "created_by_id": str(chain["owner"]["id"]),
            "updated_by_id": str(chain["owner"]["id"]),
            "metadata": {},
        },
    )
    _soft_delete(db_conn, "issue_links", link["id"])
    before = snapshot(db_conn, ["issue_links"])["issue_links"][str(link["id"])]
    celery_id = rust_queue.publish(
        CRAWL_LINK_TITLE,
        args=[str(link["id"]), url],
    )
    rust_queue.wait_for_settled(
        celery_id, what=f"{CRAWL_LINK_TITLE} settled for a soft-deleted link"
    )
    after = snapshot(db_conn, ["issue_links"])["issue_links"][str(link["id"])]
    assert after == before

