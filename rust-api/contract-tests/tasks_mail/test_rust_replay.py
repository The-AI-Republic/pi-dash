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
import os
import uuid
from datetime import datetime, timezone

import pytest
from psycopg.rows import dict_row

from _harness import config, redis_cache, rust_queue
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

    if not os.environ.get(config.CELERY_BROKER_URL):
        # No broker configured (standalone Rust boot, PIDASHCONV-1108):
        # there is nothing to observe, and the Rust worker has no broker
        # client, so the absence pin holds vacuously.
        return []

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


# PIDASHCONV-1287: case-for-case mirror of the Django oracle's
# test_mention_mail_html_normalized_like_bs4 (test_mail_tasks.py) — same
# mention payload, same bs4-normalized fragment in the HTML part, same
# no-literal-entities text/plain part — plus the local-execution pins
# (settle, no broker message). The two suites together are the
# Django-vs-Rust equivalence proof for mention normalization.
MENTION_HTML_TEMPLATE = (
    '<p>a&nbsp;&nbsp;b<br>c <img src="x.png"> it&#39;s &quot;q&quot; '
    '<mention-component entity_identifier="{user_id}" '
    'label="x"></mention-component></p>'
)
MENTION_HTML_EXPECTED = (
    '<p>a\xa0\xa0b<br/>c <img src="x.png"/> it\'s "q" @{display}</p>'
)


def _mail_parts(mail):
    """Decode the text/plain + text/html parts (transfer-encoding agnostic).

    Both planes send valid UTF-8 wire bytes (Django: 8bit), so the sink's
    `str` re-encodes exactly; parsing bytes (not `str`) keeps `get_content`
    from mangling 8bit parts. Bytes the sender emitted outside UTF-8 stay
    U+FFFD and fail the assertions below, as they should.
    """
    from email import message_from_bytes, policy

    html_part, text_part = None, None
    raw_bytes = mail["raw"].encode("utf-8")
    for part in message_from_bytes(raw_bytes, policy=policy.default).walk():
        ctype = part.get_content_type()
        if ctype == "text/html":
            html_part = part.get_content()
        elif ctype == "text/plain":
            text_part = part.get_content()
    assert html_part is not None, "mail has no text/html part"
    assert text_part is not None, "mail has no text/plain part"
    return html_part, text_part


def test_rust_send_mention_mail_normalized_like_bs4(smtp_sink, db_conn):
    """A stacked mention mail delivers bs4-normalized on the Rust worker."""
    from datetime import datetime, timezone

    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"mailmen{tag}")
    receiver = seed_helpers.user(db_conn, f"contract-rust-mention-{tag}")
    mentioned = seed_helpers.user(db_conn, f"Bob-{tag}")
    issue_id = str(chain["issue"]["id"])
    log_id = seed_helpers.email_log(
        db_conn,
        receiver["id"],
        chain["owner"]["id"],
        issue_id,
        data={
            "issue_activity": {
                "field": "mention",
                "old_value": "",
                "new_value": MENTION_HTML_TEMPLATE.format(user_id=mentioned["id"]),
                "activity_time": datetime.now(timezone.utc).isoformat(),
            }
        },
    )["id"]
    _seed_base_api(issue_id)
    smtp_sink.clear()
    _publish_and_observe_local(STACK, {})
    assert _processed_at(db_conn, log_id) is not None, (
        f"stack did not mark processed_at on {log_id}"
    )
    delivered = smtp_sink.wait_for_count(1, what="rust mention mail")
    matches = [m for m in delivered if receiver["email"] in m["rcpt_tos"]]
    assert matches, "rust stack sent no mail to the mention receiver"
    html_part, text_part = _mail_parts(matches[0])
    expected = MENTION_HTML_EXPECTED.format(display=mentioned["display_name"])
    assert expected in html_part, (
        f"HTML part misses the normalized fragment:\n{expected!r}\n"
        f"HTML head: {html_part[:600]!r}"
    )
    assert f"@{mentioned['display_name']}" in text_part
    assert 'it\'s "q"' in text_part
    assert "\xa0\xa0" in text_part
    for leaked in ("&nbsp;", "&#39;", "&quot;"):
        assert leaked not in text_part, f"text/plain leaks {leaked}"
    rust_queue.wait_for_queue_drain(what="fanned-out mention send settled")
    assert _sent_at(db_conn, log_id) is not None, (
        f"fanned-out send did not mark sent_at on {log_id}"
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


# PIDASHCONV-1285 replay: description mention on an issue with NO other
# subscribers, mirroring
# test_mail_tasks.py::test_notifications_no_subscriber_description_mention.
def _mention_1285_html(user_id) -> str:
    """Description HTML mentioning one user (extract_mentions shape)."""
    return (
        '<p>hi <mention-component entity_name="user_mention" '
        f'entity_identifier="{user_id}"></mention-component></p>'
    )


def _seed_1285_world(conn, slug, mention_pref):
    """Issue chain + mentioned B (unsubscribed) + removed-mention C."""
    chain = seed_helpers.issue_chain(conn, slug)
    project, ws, issue, owner = (
        chain["project"],
        chain["workspace"],
        chain["issue"],
        chain["owner"],
    )
    mentioned = seed_helpers.user(conn, f"contract-notif1285-{slug}-b")
    seed_helpers.project_member(conn, project["id"], ws["id"], mentioned["id"])
    seed_helpers.notification_preference(
        conn, mentioned["id"], mention=mention_pref
    )
    removed = seed_helpers.user(conn, f"contract-notif1285-{slug}-c")
    seed_helpers.project_member(conn, project["id"], ws["id"], removed["id"])
    insert_row(
        conn,
        "issue_mentions",
        {
            "workspace_id": str(ws["id"]),
            "project_id": str(project["id"]),
            "issue_id": str(issue["id"]),
            "mention_id": str(removed["id"]),
        },
    )
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO issue_activities (id, created_at, updated_at, "
            "deleted_at, workspace_id, project_id, issue_id, verb, field, "
            "old_value, new_value, comment, attachments, actor_id) VALUES "
            "(gen_random_uuid(), now(), now(), NULL, %s, %s, %s, "
            "'updated', 'description', '<p>plain</p>', %s, "
            "'updated the description', %s, %s)",
            (
                str(ws["id"]),
                str(project["id"]),
                str(issue["id"]),
                _mention_1285_html(mentioned["id"]),
                [],
                str(owner["id"]),
            ),
        )
    conn.commit()
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT count(*) AS n FROM issue_subscribers "
            "WHERE deleted_at IS NULL AND issue_id = %s",
            (str(issue["id"]),),
        )
        assert cur.fetchone()["n"] == 0, "1285 world needs zero subscribers"
    return chain, mentioned, removed


def _description_1285_activity(issue_id, actor_id):
    """Description-change activity with non-null identifiers."""
    return {
        "id": str(uuid.uuid4()),
        "verb": "updated",
        "field": "description",
        "actor_id": str(actor_id),
        "new_value": "desc new",
        "old_value": "desc old",
        "comment": "updated the description",
        "created_at": seed_helpers.now_iso(),
        "old_identifier": str(uuid.uuid4()),
        "new_identifier": str(uuid.uuid4()),
        "issue_detail": {"id": str(issue_id)},
        "issue_comment": None,
    }


def _notifications_1285_args(chain, activity, mentioned_id, removed_id):
    issue, project, owner = chain["issue"], chain["project"], chain["owner"]
    requested = json.dumps(
        {"description_html": _mention_1285_html(mentioned_id)}
    )
    current = json.dumps({"description_html": _mention_1285_html(removed_id)})
    return [
        "issue.activity.created",
        str(issue["id"]),
        str(project["id"]),
        str(owner["id"]),
        True,
        json.dumps([activity]),
        requested,
        current,
    ]


def _fetch_1285_notifications(conn, receiver_id, issue_id):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT sender, message, data FROM notifications "
            "WHERE deleted_at IS NULL AND receiver_id = %s "
            "AND entity_identifier = %s ORDER BY created_at",
            (str(receiver_id), str(issue_id)),
        )
        return cur.fetchall()


def _fetch_1285_mentions(conn, mention_id, issue_id):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT id, deleted_at FROM issue_mentions "
            "WHERE mention_id = %s AND issue_id = %s",
            (str(mention_id), str(issue_id)),
        )
        return cur.fetchall()


def _count_1285_email_logs(conn, issue_id) -> int:
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT count(*) AS n FROM email_notification_logs "
            "WHERE deleted_at IS NULL AND entity_identifier = %s",
            (str(issue_id),),
        )
        return cur.fetchone()["n"]


@pytest.mark.parametrize("mention_pref", [False, True])
def test_rust_notifications_no_subscriber_description_mention(
    db_conn, mention_pref
):
    """Rust: same write set as the oracle — B notified + tracked."""
    slug = f"notif1285r{'t' if mention_pref else 'f'}"
    chain, mentioned, removed = _seed_1285_world(db_conn, slug, mention_pref)
    issue = chain["issue"]
    activity = _description_1285_activity(issue["id"], chain["owner"]["id"])
    celery_id = rust_queue.publish(
        NOTIFICATIONS,
        args=_notifications_1285_args(
            chain, activity, mentioned["id"], removed["id"]
        ),
        kwargs={},
    )
    rust_queue.wait_for_settled(celery_id, what="1285 notifications executed")
    rows = _fetch_1285_notifications(db_conn, mentioned["id"], issue["id"])
    assert len(rows) == 1, f"rust: B notifications {len(rows)}"
    row = rows[0]
    assert row["sender"] == "in_app:issue_activities:mentioned", "rust"
    assert row["message"] == (
        f"You have been mentioned in the issue {issue['name']}"
    ), "rust"
    block = row["data"]["issue_activity"]
    assert block["old_identifier"] == activity["old_identifier"], "rust"
    assert block["new_identifier"] == activity["new_identifier"], "rust"
    live = [
        r
        for r in _fetch_1285_mentions(db_conn, mentioned["id"], issue["id"])
        if r["deleted_at"] is None
    ]
    assert len(live) == 1, f"rust: B live IssueMention rows {len(live)}"
    gone = _fetch_1285_mentions(db_conn, removed["id"], issue["id"])
    assert len(gone) == 1 and gone[0]["deleted_at"] is not None, (
        f"rust: removed mention not soft-deleted: {gone!r}"
    )
    assert _count_1285_email_logs(db_conn, issue["id"]) == 0, (
        "rust: expected no email log rows"
    )


# PIDASHCONV-1690 replay: mirrors the oracle's
# test_mail_tasks.py::test_webhook_deactivation_mail_escapes_url
# case-for-case (same URL, same escaped sentence, same MIME pins).
DEACTIVATION_EMAIL = "pi_dash.bgtasks.webhook_task.send_webhook_deactivation_email"
DEACTIVATION_ESCAPE_URL = "https://x.example/?a=<img/src=https://t.example/p.png>&x=1"
DEACTIVATION_ESCAPE_SENTENCE = (
    "Webhook https://x.example/?a=&lt;img/src=https://t.example/p.png&gt;&amp;x=1"
    " has been deactivated due to failed requests."
)


def test_rust_deactivation_mail_escapes_url(smtp_sink, db_conn):
    """Rust: hostile webhook URL arrives escaped in both MIME parts."""
    owner = seed_helpers.user(db_conn, "rust-replay-deact-esc-owner")
    receiver = seed_helpers.user(db_conn, "rust-replay-deact-esc-receiver")
    workspace = seed_helpers.workspace(db_conn, "rustreplaydeactesc", owner["id"])
    hook = seed_helpers.webhook(
        db_conn,
        workspace["id"],
        DEACTIVATION_ESCAPE_URL,
        created_by_id=str(owner["id"]),
    )
    smtp_sink.clear()
    celery_id = rust_queue.publish(
        DEACTIVATION_EMAIL,
        args=[str(hook["id"]), str(receiver["id"]), "example.com", "contract reason"],
    )
    rust_queue.wait_for_settled(celery_id, what=f"{DEACTIVATION_EMAIL} executed")
    delivered = smtp_sink.wait_for_count(1, what="deactivation mail")
    matches = [m for m in delivered if receiver["email"] in m["rcpt_tos"]]
    assert matches, "deactivation mail delivered nothing to the receiver"
    assert matches[0]["subject"] == "Webhook Deactivated"
    html_part, text_part = _mail_parts(matches[0])
    assert DEACTIVATION_ESCAPE_SENTENCE in html_part, (
        f"HTML part misses the escaped sentence; head: {html_part[:400]!r}"
    )
    assert "<img/src" not in html_part, "HTML part carries the raw injection"
    assert DEACTIVATION_ESCAPE_SENTENCE in text_part, (
        f"text part misses the escaped sentence; head: {text_part[:400]!r}"
    )


# PIDASHCONV-1692 replay: case-for-case mirror of the Django oracle's
# test_notifications_mention_mail_renders_stray_lt (test_mail_tasks.py)
# — same rename+comment-mention save, same :503 new_value copy into the
# mention log rows, same escaped rename value in the delivered mail —
# plus the local-execution pins (settle, no broker message,
# processed_at/sent_at). The two suites together are the Django-vs-Rust
# equivalence proof for the stray-`<` mention-mail fix.
STRAY_LT_RENAME = "Fix x<y handling"
STRAY_LT_RENDERED = "Fix x&lt;y handling"


def _comment_1692_html(mentioned_id) -> str:
    """Comment HTML mentioning one user (extract_comment_mentions shape)."""
    return (
        '<p>please review <mention-component entity_name="user_mention" '
        f'entity_identifier="{mentioned_id}"></mention-component></p>'
    )


def _seed_1692_world(conn, slug, tag):
    """Issue chain + mentioned B (active member, mention pref on).

    Zero IssueSubscriber rows: the subscriber loop is skipped and only
    the comment_mentions branch writes.
    """
    chain = seed_helpers.issue_chain(conn, slug)
    project, ws = chain["project"], chain["workspace"]
    mentioned = seed_helpers.user(conn, f"contract-notif1692-{tag}-b")
    seed_helpers.project_member(conn, project["id"], ws["id"], mentioned["id"])
    insert_row(
        conn,
        "user_notification_preferences",
        {
            "user_id": str(mentioned["id"]),
            "property_change": True,
            "state_change": True,
            "comment": True,
            "mention": True,
            "issue_completed": True,
        },
    )
    return chain, mentioned


def _rename_1692_activity(issue_id, actor_id):
    """Rename activity (track_name shape: verb/field/comment/values)."""
    return {
        "id": str(uuid.uuid4()),
        "verb": "updated",
        "field": "name",
        "actor_id": str(actor_id),
        "new_value": STRAY_LT_RENAME,
        "old_value": "Old name",
        "comment": "updated the name to",
        "created_at": seed_helpers.now_iso(),
        "old_identifier": None,
        "new_identifier": None,
        "issue_detail": {"id": str(issue_id)},
    }


def _comment_1692_activity(issue_id, actor_id, mentioned_id):
    """Created-comment activity (create_comment_activity shape)."""
    comment_id = str(uuid.uuid4())
    return {
        "id": str(uuid.uuid4()),
        "verb": "created",
        "field": "comment",
        "actor_id": str(actor_id),
        "new_value": _comment_1692_html(mentioned_id),
        "old_value": None,
        "comment": "created a comment",
        "created_at": seed_helpers.now_iso(),
        "old_identifier": None,
        "new_identifier": comment_id,
        "issue_detail": {"id": str(issue_id)},
        "issue_comment": comment_id,
    }


def _notifications_1692_args(chain, activities):
    issue, project, owner = chain["issue"], chain["project"], chain["owner"]
    plain = json.dumps({"description_html": "<p>plain</p>"})
    return [
        "issue.activity.created",
        str(issue["id"]),
        str(project["id"]),
        str(owner["id"]),
        [],
        json.dumps(activities),
        plain,
        plain,
    ]


def _fetch_1692_mention_logs(conn, receiver_id, issue_id):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT id, data FROM email_notification_logs "
            "WHERE receiver_id = %s AND entity_identifier = %s "
            "ORDER BY created_at",
            (str(receiver_id), str(issue_id)),
        )
        return cur.fetchall()


def test_rust_notifications_mention_mail_renders_stray_lt(smtp_sink, db_conn):
    """Rust: same save, same escaped rename value in the mention mail."""
    tag = uuid.uuid4().hex[:8]
    chain, mentioned = _seed_1692_world(db_conn, f"mail1692r{tag}", tag)
    issue = chain["issue"]
    activities = [
        _rename_1692_activity(issue["id"], chain["owner"]["id"]),
        _comment_1692_activity(
            issue["id"], chain["owner"]["id"], mentioned["id"]
        ),
    ]
    celery_id = rust_queue.publish(
        NOTIFICATIONS,
        args=_notifications_1692_args(chain, activities),
        kwargs={},
    )
    rust_queue.wait_for_settled(
        celery_id, what="1692 notifications executed"
    )
    rows = _fetch_1692_mention_logs(db_conn, mentioned["id"], issue["id"])
    assert len(rows) == 2, f"expected 2 mention log rows, got {len(rows)}"
    assert [r["data"]["issue_activity"]["field"] for r in rows] == [
        "mention"
    ] * 2
    assert sorted(
        r["data"]["issue_activity"]["new_value"] for r in rows
    ) == sorted([STRAY_LT_RENAME, activities[1]["new_value"]])

    _seed_base_api(str(issue["id"]))
    smtp_sink.clear()
    _publish_and_observe_local(STACK, {})
    for row in rows:
        assert _processed_at(db_conn, row["id"]) is not None, (
            f"stack did not mark processed_at on {row['id']}"
        )
    delivered = smtp_sink.wait_for_count(1, what="rust stray-lt mention mail")
    matches = [m for m in delivered if mentioned["email"] in m["rcpt_tos"]]
    assert matches, "rust stack sent no mail to the mentioned user"
    html_part, text_part = _mail_parts(matches[0])
    assert STRAY_LT_RENDERED in html_part, (
        f"HTML part misses the escaped rename value: {html_part[:600]!r}"
    )
    assert f"@{mentioned['display_name']}" in html_part, (
        f"HTML part misses the rewritten mention: {html_part[:600]!r}"
    )
    assert f"@{mentioned['display_name']}" in text_part
    rust_queue.wait_for_queue_drain(what="fanned-out mention send settled")
    for row in rows:
        assert _sent_at(db_conn, row["id"]) is not None, (
            f"fanned-out send did not mark sent_at on {row['id']}"
        )
