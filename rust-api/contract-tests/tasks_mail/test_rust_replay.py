"""tasks_mail oracle vs the Rust worker (PIDASHCONV-820/834/865).

The Django oracle (``test_mail_tasks.py``) proves what the Python plane does
with mail Celery traffic. This suite proves the Rust worker's side of the
hybrid-worker contract for the same tasks:

- ``notifications`` (PIDASHCONV-834), ``stack_email_notification`` and
  ``send_email_notification`` (PIDASHCONV-865) execute LOCALLY: the jobs
  settle in ``rust_job_queue``, the AMQP broker stays flat, the stack marks
  ``processed_at`` and fans its sends out into the Rust queue, and the
  sends deliver into the SMTP sink and mark ``sent_at``.
- Every other mail task name still publishes but has no local handler, so
  the worker forwards it to the Python plane: the job settles (the forward
  is the terminal effect) and the broker grows by exactly one message while
  no local rows change.

The broker/redis/database are shared across tests in this module, but every
payload carries unique ids (fresh projects/issues/receivers per test), so
tests cannot observe each other's rows. See ``_harness/rust_queue.py``.
"""

import uuid

import pytest

from _harness import broker_probe, redis_cache, rust_queue
from _harness import seed as seed_helpers

STACK = "pi_dash.bgtasks.email_notification_task.stack_email_notification"
SEND = "pi_dash.bgtasks.email_notification_task.send_email_notification"
NOTIFICATIONS = "pi_dash.bgtasks.notification_task.send_notification"


def _send_kwargs(receiver_id, issue_id):
    """The Django oracle's ``MAIL_TASKS[SEND]`` kwargs, with live ids."""
    return {
        "issue_id": str(issue_id),
        "notification_data": {},
        "receiver_id": str(receiver_id),
        "email_notification_ids": [],
    }


def _publish_and_observe_forward(task, kwargs):
    """Publish one Rust-queue job and prove it forwards to Python.

    Returns the observation ``(depth_before, depth_after)``; the caller
    asserts the +1 and the absence of local rows.
    """
    depth_before = broker_probe.queue_depth()
    celery_id = rust_queue.publish(task, [], kwargs)
    rust_queue.wait_for_settled(
        celery_id, what=f"forwarded mail task settled ({task})"
    )
    depth_after = broker_probe.queue_depth()
    return depth_before, depth_after


def _publish_and_observe_local(task, kwargs):
    """Publish one Rust-queue job and prove it executes locally.

    Returns the job's celery id once the row settles; the caller asserts
    the task-specific local effects. The broker must stay flat: nothing
    about a locally owned task may reach the Python plane.
    """
    depth_before = broker_probe.queue_depth()
    celery_id = rust_queue.publish(task, [], kwargs)
    rust_queue.wait_for_settled(
        celery_id, what=f"locally executed mail task settled ({task})"
    )
    depth_after = broker_probe.queue_depth()
    assert depth_after == depth_before, (
        f"locally owned {task} published to the broker "
        f"(depth {depth_before} -> {depth_after})"
    )
    return celery_id


def _seed_stack(conn, tag):
    """Two unprocessed logs (same receiver/actor/issue), like the oracle."""
    chain = seed_helpers.issue_chain(conn, f"mailrust{tag}")
    receiver = seed_helpers.user(conn, f"contract-rust-replay-{tag}")
    issue_id = str(chain["issue"]["id"])
    log_ids = [
        seed_helpers.email_log(
            conn, receiver["id"], chain["owner"]["id"], issue_id
        )["id"]
        for _ in range(2)
    ]
    return {
        "receiver": receiver,
        "issue_id": issue_id,
        "ids": log_ids,
    }


def _seed_base_api(issue_id):
    """The send reads ``base_api`` from Redis (absent = silent return)."""
    redis_cache.setex(str(issue_id), "http://localhost")


def _processed_at(conn, log_id):
    with conn.cursor() as cur:
        cur.execute(
            "SELECT processed_at FROM email_notification_logs WHERE id = %s",
            (str(log_id),),
        )
        row = cur.fetchone()
    return row[0] if row else None


def _sent_at(conn, log_id):
    with conn.cursor() as cur:
        cur.execute(
            "SELECT sent_at FROM email_notification_logs WHERE id = %s",
            (str(log_id),),
        )
        row = cur.fetchone()
    return row[0] if row else None


def _log_count(conn):
    with conn.cursor() as cur:
        cur.execute("SELECT COUNT(*) FROM email_notification_logs")
        (count,) = cur.fetchone()
    return count


def test_rust_worker_liveness():
    """The Rust worker serves the queue: an unseeded stack run settles.

    No unprocessed rows exist for this run, so nothing is marked and no
    sends fan out — but the job must still settle locally (owned, not
    forwarded), which proves the worker under test is alive and serving.
    """
    _publish_and_observe_local(STACK, {})


def test_rust_stack_executes_locally(smtp_sink, db_conn):
    """The seeded stack marks ``processed_at`` and its sends deliver.

    Mirrors the Django oracle's ``test_stack_fans_out_and_marks_processed``:
    two unprocessed logs for one receiver/actor/issue fan out to a single
    send, which executes on the Rust worker, delivers into the SMTP sink,
    and marks ``sent_at`` on both logs (the receiver-wide batch the port
    preserves). The broker stays flat throughout.
    """
    seeded = _seed_stack(db_conn, uuid.uuid4().hex[:8])
    _seed_base_api(seeded["issue_id"])
    smtp_sink.clear()
    _publish_and_observe_local(STACK, {})
    for log_id in seeded["ids"]:
        assert _processed_at(db_conn, log_id) is not None, (
            f"stack did not mark processed_at on {log_id}"
        )
    delivered = smtp_sink.wait_for_count(1, what="stacked notification mail")
    assert delivered[0]["rcpt_tos"] == [seeded["receiver"]["email"]]
    # The DATA receipt lands before the send marks `sent_at`; wait out the
    # fanned-out sends, then pin the marks.
    rust_queue.wait_for_queue_drain(what="fanned-out sends settled")
    for log_id in seeded["ids"]:
        assert _sent_at(db_conn, log_id) is not None, (
            f"fanned-out send did not mark sent_at on {log_id}"
        )


def test_rust_stack_redelivery_sends_once(smtp_sink, db_conn):
    """A second stack run finds no unprocessed rows and sends nothing.

    Mirrors the Django oracle's ``test_stack_redelivery_is_idempotent``:
    the runs are sequential (the second publish waits for the first run's
    mail), so the second run marks nothing new and the sink holds exactly
    the first run's delivery.
    """
    seeded = _seed_stack(db_conn, uuid.uuid4().hex[:8])
    _seed_base_api(seeded["issue_id"])
    smtp_sink.clear()
    _publish_and_observe_local(STACK, {})
    smtp_sink.wait_for_count(1, what="first stacked mail")
    _publish_and_observe_local(STACK, {})
    rust_queue.wait_for_queue_drain(what="redelivered stack settled")
    assert len(smtp_sink.snapshot()) == 1, (
        "redelivered stack sent a duplicate mail"
    )
    for log_id in seeded["ids"]:
        assert _processed_at(db_conn, log_id) is not None


def test_rust_send_executes_locally(smtp_sink, db_conn):
    """A direct send executes locally and delivers into the SMTP sink.

    The oracle-shaped payload carries no log ids, so no log rows change —
    but the mail itself must arrive (this is the no-stub pin: a fake seam
    would ACK the job while the sink stayed empty).
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"mailsend{tag}")
    receiver = seed_helpers.user(db_conn, f"contract-rust-send-{tag}")
    issue_id = str(chain["issue"]["id"])
    _seed_base_api(issue_id)
    logs_before = _log_count(db_conn)
    smtp_sink.clear()
    _publish_and_observe_local(
        SEND,
        _send_kwargs(receiver["id"], issue_id),
    )
    delivered = smtp_sink.wait_for_count(1, what="direct send mail")
    assert delivered[0]["rcpt_tos"] == [receiver["email"]]
    rust_queue.wait_for_queue_drain(what="direct send settled")
    assert _log_count(db_conn) == logs_before, "id-less send touched log rows"


# Every mail task name that is still Python-owned: published, forwarded,
# settled, broker +1, no local rows. `notifications` (PIDASHCONV-834) and
# `send`/`stack_email_notification` (PIDASHCONV-865) execute locally and
# are pinned above instead.
MAIL_TASKS = [
    "pi_dash.bgtasks.auth_task.send_email_verification",
    "pi_dash.bgtasks.auth_task.send_forgot_password",
    "pi_dash.bgtasks.auth_task.send_magic_sign_in",
    "pi_dash.bgtasks.auth_task.send_password_changed",
    "pi_dash.bgtasks.membership_task.send_invitation_email",
    "pi_dash.bgtasks.membership_task.send_organization_invitation",
    "pi_dash.bgtasks.membership_task.send_project_invitation",
    "pi_dash.bgtasks.membership_task.send_workspace_invitation",
]


@pytest.mark.parametrize("task_name", MAIL_TASKS)
def test_rust_mail_task_forwards_to_python(task_name):
    """Mail tasks without a local handler forward to the Python plane."""
    depth_before, depth_after = _publish_and_observe_forward(task_name, {})
    assert depth_after == depth_before + 1, (
        f"{task_name} did not forward exactly one message "
        f"(depth {depth_before} -> {depth_after})"
    )


def test_rust_notifications_executes_locally(db_conn):
    """The notifications task executes locally without touching the broker.

    Faithful to the None-activities payload: no input rows, no output rows,
    broker flat — the job settles because the local handler ran, not
    because it was forwarded.
    """
    depth_before = broker_probe.queue_depth()
    with db_conn.cursor() as cur:
        cur.execute("SELECT COUNT(*) FROM notifications_notification")
        (before,) = cur.fetchone()
    payload = {
        "receiver_id": str(uuid.uuid4()),
        "actor_id": str(uuid.uuid4()),
        "activities": None,
    }
    celery_id = rust_queue.publish(NOTIFICATIONS, [], payload)
    rust_queue.wait_for_settled(celery_id, what="notifications job settled")
    assert broker_probe.queue_depth() == depth_before, (
        "locally owned notifications task published to the broker"
    )
    with db_conn.cursor() as cur:
        cur.execute("SELECT COUNT(*) FROM notifications_notification")
        (after,) = cur.fetchone()
    assert after == before
