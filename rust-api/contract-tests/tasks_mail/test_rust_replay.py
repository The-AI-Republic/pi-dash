"""tasks_mail oracle vs the Rust worker (PIDASHCONV-820/834/865).

Every mail task name in this module is Rust-owned (PIDASHCONV-1103/1104
ported the last of them off the Python hand-off; PIDASHCONV-1108 removed
the hand-off entirely): the jobs settle in ``rust_job_queue``, no message
for the task lands on the AMQP broker, and the task-specific local effect
holds. This suite proves the Rust worker executes them locally:

- ``notifications`` (PIDASHCONV-834), ``stack_email_notification`` and
  ``send_email_notification`` (PIDASHCONV-865), and the four auth mail
  tasks (PIDASHCONV-1103): the stack marks ``processed_at`` and fans its
  sends out into the Rust queue, and the sends (and the auth mails)
  deliver into the SMTP sink (the sends mark ``sent_at``).
- The five membership mail tasks (PIDASHCONV-1104) execute locally too;
  their pins live in ``test_rust_replay_user_project.py``.

The broker pins are per-task (``headers["task"]``), never global depth:
the Django plane's own beat traffic can land on the shared broker, so a
global flat/+1 pin is inherently racy (PIDASHCONV-865 D3). Only messages
carrying our task name count; background beat traffic is republished back
untouched (the ticker-suite drains-and-filters precedent, plus restore so
later tests still observe it).

The broker/redis/database are shared across tests in this module, but every
payload carries unique ids (fresh projects/issues/receivers per test), so
tests cannot observe each other's rows. See ``_harness/rust_queue.py``.
"""

import json
import uuid
from datetime import datetime, timezone

import pytest
from psycopg.rows import dict_row

from _harness import redis_cache, rust_queue
from _harness import seed as seed_helpers
from _harness.db import diff as _diff
from _harness.db import insert_row, snapshot

STACK = "pi_dash.bgtasks.email_notification_task.stack_email_notification"
SEND = "pi_dash.bgtasks.email_notification_task.send_email_notification"
NOTIFICATIONS = "pi_dash.bgtasks.notification_task.notifications"


def _send_kwargs(receiver_id, issue_id):
    """The Django oracle's ``MAIL_TASKS[SEND]`` kwargs, with live ids."""
    return {
        "issue_id": str(issue_id),
        "notification_data": {},
        "receiver_id": str(receiver_id),
        "email_notification_ids": [],
    }


def _drain_matching(task_name):
    """Drain the broker once; return the messages for ``task_name``.

    The Rust worker never publishes, so an absence pin is the norm here;
    this drain is called *after* the job settles to prove the locally
    owned task sent nothing to the Python plane (PIDASHCONV-1108 removed
    the hand-off). Non-matching messages (the Django plane's own beat
    traffic — see the module docstring) are republished back with payload
    and headers intact, so later tests still observe them. Republishing,
    not ``message.requeue()``: a requeued message is redelivered
    immediately, which would loop the sweep on the first non-match
    (probed against RabbitMQ 3.13; the same trap ``collect_matching``
    lays — unused here for that reason and because it polls until a
    match, which an absence pin never finds).
    """
    from kombu import Connection

    from _harness import config

    matched = []
    others = []
    with Connection(config.required(config.CELERY_BROKER_URL)) as conn:
        queue_obj = conn.SimpleQueue("celery")
        try:
            while True:
                try:
                    message = queue_obj.get(block=False)
                except Exception:
                    break
                headers = message.headers or {}
                if headers.get("task") == task_name:
                    matched.append((message.headers, message.payload))
                else:
                    others.append((dict(headers), message.payload))
                message.ack()
            for headers, payload in others:
                queue_obj.put(payload, headers=headers)
        finally:
            queue_obj.close()
    return matched


def _publish_and_observe_local(task, kwargs, args=None):
    """Publish one Rust-queue job and prove it executes locally.

    Returns the job's celery id once the row settles; the caller asserts
    the task-specific local effects. No message for the task may land on
    the broker: nothing about a locally owned task may reach the Python
    plane (a per-task pin, not global flatness — see the module
    docstring).
    """
    celery_id = rust_queue.publish(task, list(args or []), kwargs)
    rust_queue.wait_for_settled(
        celery_id, what=f"locally executed mail task settled ({task})"
    )
    arrived = _drain_matching(task)
    assert arrived == [], (
        f"locally owned {task} published to the broker ({len(arrived)} message(s))"
    )
    return celery_id


def _seed_stack(conn, tag):
    """Two unprocessed logs (same receiver/actor/issue), like the oracle."""
    chain = seed_helpers.issue_chain(conn, f"mailrust{tag}")
    receiver = seed_helpers.user(conn, f"contract-rust-replay-{tag}")
    issue_id = str(chain["issue"]["id"])
    log_ids = [
        seed_helpers.email_log(conn, receiver["id"], chain["owner"]["id"], issue_id)[
            "id"
        ]
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
    preserves). No message for the tasks lands on the broker throughout.
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
    assert len(smtp_sink.snapshot()) == 1, "redelivered stack sent a duplicate mail"
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


def test_rust_send_skips_soft_deleted_issue(smtp_sink, db_conn):
    """A direct send to a soft-deleted issue stays silent (no mail).

    Django reads the issue through the soft-deleting default manager, so
    ``Issue.objects.get`` raises ``DoesNotExist`` and the task returns
    silently after releasing the lock. The Rust read must scope the same
    way: the job settles, no message for the task lands on the broker,
    and the sink stays empty.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"maildead{tag}")
    receiver = seed_helpers.user(db_conn, f"contract-rust-dead-{tag}")
    issue_id = str(chain["issue"]["id"])
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET deleted_at = now() WHERE id = %s",
            (issue_id,),
        )
    _seed_base_api(issue_id)
    smtp_sink.clear()
    _publish_and_observe_local(
        SEND,
        _send_kwargs(receiver["id"], issue_id),
    )
    rust_queue.wait_for_queue_drain(what="deleted-issue send settled")
    assert smtp_sink.snapshot() == [], "send mailed a soft-deleted issue"


def test_rust_stack_ignores_soft_deleted_logs(smtp_sink, db_conn):
    """The stack never fans out (or marks) soft-deleted logs.

    Django's ``:49`` select runs on the soft-deleting default manager, so
    deleted logs are invisible to the fan-out and stay unmarked by the
    ``:84`` update. The Rust statements must scope the same way: the run
    settles, no message for the task lands on the broker, both logs stay
    unprocessed, and no mail is sent for them.
    """
    seeded = _seed_stack(db_conn, uuid.uuid4().hex[:8])
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE email_notification_logs SET deleted_at = now() WHERE id = ANY(%s)",
            ([str(log_id) for log_id in seeded["ids"]],),
        )
    _seed_base_api(seeded["issue_id"])
    smtp_sink.clear()
    _publish_and_observe_local(STACK, {})
    rust_queue.wait_for_queue_drain(what="deleted-log stack settled")
    assert smtp_sink.snapshot() == [], "stack fanned out soft-deleted logs"
    for log_id in seeded["ids"]:
        assert _processed_at(db_conn, log_id) is None, (
            f"stack marked processed_at on soft-deleted {log_id}"
        )


# The four auth mail tasks (PIDASHCONV-1103): case-for-case mirror of the
# Django oracle's AUTH_MAIL_CASES (test_mail_tasks.py) — same args, same
# subjects, same body markers — plus the local-execution pins (settle,
# no broker message, no row writes). The two suites together are the
# Django-vs-Rust equivalence proof. No Redis pin: neither plane touches
# Redis on these paths (see the oracle-side note), so "same Redis keys"
# holds vacuously.
AUTH_MAIL_CASES = [
    (
        "pi_dash.bgtasks.magic_link_code_task.magic_link",
        ["contract@example.com", "key-1103", "842103"],
        "Your unique Pi Dash login code is 842103",
        [
            "842103",
            "contract@example.com",
            "Your login code for Pi Dash",
            'src="/static/logos/Logo.png"',
        ],
    ),
    (
        "pi_dash.bgtasks.forgot_password_task.forgot_password",
        ["Contract", "contract@example.com", "uidb64-1103", "tok-1103", "example.com"],
        "A new password to your Pi Dash account has been requested",
        [
            "Reset your Pi Dash password",
            # Django autoescape: the raw-interpolated `&` renders as `&amp;`
            # (same bytes the Rust pre-escape produces).
            "example.com/accounts/reset-password/?uidb64=uidb64-1103&amp;token=tok-1103&amp;email=contract@example.com",
            "contract@example.com",
        ],
    ),
    (
        "pi_dash.bgtasks.user_email_update_task.send_email_update_magic_code",
        ["contract@example.com", "654321"],
        "Verify your new email address",
        [
            "654321",
            "contract@example.com",
            "Your login code for Pi Dash",
            'src="/static/logos/Logo.png"',
        ],
    ),
    (
        "pi_dash.bgtasks.user_email_update_task.send_email_update_confirmation",
        ["contract@example.com"],
        "Pi Dash email address successfully updated",
        [
            "contract@example.com",
            "successfully updated",
        ],
    ),
]


@pytest.mark.parametrize("task_name,args,subject,markers", AUTH_MAIL_CASES)
def test_rust_auth_mail_executes_locally(smtp_sink, db_conn, task_name, args, subject, markers):
    """Auth mail tasks execute locally and deliver the oracle-identical mail.

    Settles in `rust_job_queue` with no message for the task on the
    broker, delivers into the SMTP sink with the oracle's subject and
    body markers, and writes no rows (mirrors the Django oracle's
    `test_auth_mail_full_capture` case-for-case).
    """
    smtp_sink.clear()
    before = snapshot(db_conn, ["users", "email_notification_logs"])
    _publish_and_observe_local(task_name, {}, args=args)
    delivered = smtp_sink.wait_for_count(1, what=f"{task_name} mail")
    matches = [m for m in delivered if "contract@example.com" in m["rcpt_tos"]]
    assert matches, f"{task_name} delivered nothing to contract@example.com"
    mail = matches[0]
    assert mail["subject"] == subject, f"{task_name} subject: {mail['subject']!r}"
    raw = mail["raw"]
    assert "multipart/alternative" in raw, f"{task_name} is not multipart/alternative"
    assert "text/plain" in raw and "text/html" in raw, f"{task_name} misses a MIME part"
    for marker in markers:
        assert marker in raw, f"{task_name} body misses {marker!r}"
    assert _diff(before, snapshot(db_conn, ["users", "email_notification_logs"])) == {}


def test_rust_stack_sends_non_ascii_mention_mail(smtp_sink, db_conn):
    """PIDASHCONV-1284 replay: non-ASCII mention HTML delivers, no panic.

    Mirrors the Django oracle's ``test_stack_sends_non_ascii_mention_mail``
    pin-for-pin: the é straddling byte 17 after the ``<p>`` tag start
    panicked the Rust handler (``match_open_tag`` sliced ``rest[..17]``
    at a non-char boundary); after the fix the send executes locally,
    rewrites the component to ``@display_name``, and marks ``sent_at``.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"mailrustna{tag}")
    receiver = seed_helpers.user(db_conn, f"contract-rust-na-rcv-{tag}")
    mentioned = seed_helpers.user(db_conn, f"contract-rust-mentioned-{tag}")
    issue_id = str(chain["issue"]["id"])
    html = (
        "<p>Merci beaucoupé pour la revue</p>"
        f'<mention-component entity_identifier="{mentioned["id"]}">X</mention-component>'
    )
    log_id = seed_helpers.email_log(
        db_conn,
        receiver["id"],
        chain["owner"]["id"],
        issue_id,
        data={
            "issue_activity": {
                "field": "mention",
                "old_value": html,
                "new_value": html,
                "activity_time": datetime.now(timezone.utc).isoformat(),
            }
        },
    )["id"]
    _seed_base_api(issue_id)
    smtp_sink.clear()
    _publish_and_observe_local(STACK, {})
    delivered = smtp_sink.wait_for_count(1, what="non-ascii mention mail")
    matches = [m for m in delivered if receiver["email"] in m["rcpt_tos"]]
    assert matches, "stacked mention mail never arrived"
    raw = matches[0]["raw"]
    # ASCII-only markers: the é itself may QP-encode in the raw MIME.
    assert f"@{mentioned['display_name']}" in raw, "mention not rewritten to @name"
    assert "mention-component" not in raw, "raw mention-component leaked into the mail"
    # The DATA receipt lands before the send marks `sent_at`; wait out the
    # fanned-out sends, then pin the marks.
    rust_queue.wait_for_queue_drain(what="fanned-out sends settled")
    assert _processed_at(db_conn, log_id) is not None, "stack never marked processed_at"
    assert _sent_at(db_conn, log_id) is not None, "fanned-out send never marked sent_at"


def test_rust_notifications_executes_locally(db_conn):
    """Local parity, Rust side: the owned notifications job never forwards.

    Restored from PIDASHCONV-834 (the 865 rewrite queried a
    ``notifications_notification`` table that does not exist and published
    to a ``send_notification`` task name nothing registers): the
    ``rust_job_queue`` row must settle (the local handler always
    acknowledges, mirroring the swallow-everything body) while no message
    for the task lands on the broker — nothing is forwarded for Python —
    and none of the four ``notifications`` write tables changes. No local
    rows is the faithful pin here, not an empty assertion: the oracle
    payload carries ``issue_activities_created=None``, which dies on
    ``for … in None`` before any write in both planes
    (``notification_task.py`` broad ``except``; ``run_notifications``
    ``None`` arm ahead of the first read).
    """
    args = [
        "issue.activity.created",
        str(uuid.uuid4()),
        str(uuid.uuid4()),
        str(uuid.uuid4()),
        [],
        None,
        {},
        {},
    ]
    tables = [
        "notifications",
        "email_notification_logs",
        "issue_mentions",
        "issue_subscribers",
    ]
    before = snapshot(db_conn, tables)
    celery_id = rust_queue.publish(NOTIFICATIONS, args=args, kwargs={})
    rust_queue.wait_for_settled(celery_id, what=f"{NOTIFICATIONS} executed")
    arrived = _drain_matching(NOTIFICATIONS)
    assert arrived == [], (
        f"{NOTIFICATIONS} is Rust-owned but {len(arrived)} message(s) "
        "for it arrived on the broker"
    )
    assert _diff(before, snapshot(db_conn, tables)) == {}


# PIDASHCONV-1288 replay: the subscriber email log keeps
# ``data.issue_activity.issue_comment`` (=="") on non-comment activities,
# mirroring test_mail_tasks.py::test_notifications_email_log_keeps_issue_comment.
def _seed_1288_world(conn, slug):
    """Issue chain + email-enabled subscriber S (not the actor)."""
    chain = seed_helpers.issue_chain(conn, slug)
    project, ws, issue = chain["project"], chain["workspace"], chain["issue"]
    subscriber = seed_helpers.user(conn, f"contract-notif1288-{slug}")
    seed_helpers.project_member(conn, project["id"], ws["id"], subscriber["id"])
    insert_row(
        conn,
        "issue_subscribers",
        {
            "workspace_id": str(ws["id"]),
            "project_id": str(project["id"]),
            "issue_id": str(issue["id"]),
            "subscriber_id": str(subscriber["id"]),
        },
    )
    insert_row(
        conn,
        "user_notification_preferences",
        {
            "user_id": str(subscriber["id"]),
            "property_change": True,
            "state_change": False,
            "comment": False,
            "mention": False,
            "issue_completed": False,
        },
    )
    return chain, subscriber


def _priority_1288_activity(issue_id, actor_id):
    """A non-comment property-change activity (no ``issue_comment`` key)."""
    return {
        "id": str(uuid.uuid4()),
        "verb": "updated",
        "field": "priority",
        "actor_id": str(actor_id),
        "new_value": "high",
        "old_value": "low",
        "comment": "changed the priority",
        "created_at": seed_helpers.now_iso(),
        "old_identifier": None,
        "new_identifier": None,
        "issue_detail": {"id": str(issue_id)},
    }


def _notifications_1288_args(chain, activity):
    issue, project, owner = chain["issue"], chain["project"], chain["owner"]
    plain = json.dumps({"description_html": "<p>plain</p>"})
    return [
        "issue.activity.created",
        str(issue["id"]),
        str(project["id"]),
        str(owner["id"]),
        [],
        json.dumps([activity]),
        plain,
        plain,
    ]


def _fetch_1288_row(conn, table, receiver_id, issue_id):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            f"SELECT data FROM {table} "
            "WHERE receiver_id = %s AND entity_identifier = %s "
            "ORDER BY created_at DESC LIMIT 1",
            (str(receiver_id), str(issue_id)),
        )
        row = cur.fetchone()
        return row["data"] if row else None


def test_rust_notifications_email_log_keeps_issue_comment(db_conn):
    """Rust: same payload, same ``issue_comment == ""`` pin as the oracle."""
    chain, subscriber = _seed_1288_world(db_conn, "notif1288r")
    issue = chain["issue"]
    celery_id = rust_queue.publish(
        NOTIFICATIONS,
        args=_notifications_1288_args(
            chain, _priority_1288_activity(issue["id"], chain["owner"]["id"])
        ),
        kwargs={},
    )
    rust_queue.wait_for_settled(celery_id, what="1288 notifications executed")
    email_data = _fetch_1288_row(
        db_conn, "email_notification_logs", subscriber["id"], issue["id"]
    )
    assert email_data is not None, "rust email log: no row written"
    block = email_data["issue_activity"]
    assert "issue_comment" in block, f"rust email log: keys {sorted(block)}"
    assert block["issue_comment"] == "", f"rust email log: {block['issue_comment']!r}"
    app_data = _fetch_1288_row(
        db_conn, "notifications", subscriber["id"], issue["id"]
    )
    assert app_data is not None, "rust in-app notification: no row written"
    assert app_data["issue_activity"]["issue_comment"] == ""
