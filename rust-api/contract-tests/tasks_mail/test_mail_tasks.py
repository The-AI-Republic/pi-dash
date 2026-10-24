"""D-07 oracle: mail + notification tasks.

Django sources: pi_dash/bgtasks/{email_notification_task, notification_task,
magic_link_code_task, forgot_password_task, user_activation_email_task,
user_deactivation_email_task, user_email_update_task,
project_add_user_email_task, workspace_invitation_task,
project_invitation_task}.py (project_invitation revived by Django d656243d;
see NOTE on MAIL_TASKS.)
"""

import json
import uuid
from datetime import datetime, timezone
from email import message_from_bytes, policy

import pytest
from psycopg.rows import dict_row

from _harness import broker_probe, celery_wire, redis_cache, taskspec
from _harness import seed as seed_helpers
from _harness.db import diff, insert_row, snapshot, wait_for_condition as wait_for

M = "pi_dash.bgtasks"

MAIL_TASKS = {
    # wire name: (module relpath, func, payload args, payload kwargs)
    f"{M}.email_notification_task.stack_email_notification": (
        "pi_dash/bgtasks/email_notification_task.py",
        "stack_email_notification",
        [],
        {},
    ),
    f"{M}.email_notification_task.send_email_notification": (
        "pi_dash/bgtasks/email_notification_task.py",
        "send_email_notification",
        [],
        {
            "issue_id": str(uuid.uuid4()),
            "notification_data": {},
            "receiver_id": str(uuid.uuid4()),
            "email_notification_ids": [],
        },
    ),
    f"{M}.notification_task.notifications": (
        "pi_dash/bgtasks/notification_task.py",
        "notifications",
        [
            "issue.activity.created",
            str(uuid.uuid4()),
            str(uuid.uuid4()),
            str(uuid.uuid4()),
            [],
            None,
            {},
            {},
        ],
        {},
    ),
    f"{M}.magic_link_code_task.magic_link": (
        "pi_dash/bgtasks/magic_link_code_task.py",
        "magic_link",
        ["contract@example.com", "key", "token"],
        {},
    ),
    f"{M}.forgot_password_task.forgot_password": (
        "pi_dash/bgtasks/forgot_password_task.py",
        "forgot_password",
        ["Contract", "contract@example.com", "uidb64", "token", "example.com"],
        {},
    ),
    f"{M}.user_activation_email_task.user_activation_email": (
        "pi_dash/bgtasks/user_activation_email_task.py",
        "user_activation_email",
        ["example.com", str(uuid.uuid4())],
        {},
    ),
    f"{M}.user_deactivation_email_task.user_deactivation_email": (
        "pi_dash/bgtasks/user_deactivation_email_task.py",
        "user_deactivation_email",
        ["example.com", str(uuid.uuid4())],
        {},
    ),
    f"{M}.user_email_update_task.send_email_update_magic_code": (
        "pi_dash/bgtasks/user_email_update_task.py",
        "send_email_update_magic_code",
        ["contract@example.com", "token"],
        {},
    ),
    f"{M}.user_email_update_task.send_email_update_confirmation": (
        "pi_dash/bgtasks/user_email_update_task.py",
        "send_email_update_confirmation",
        ["contract@example.com"],
        {},
    ),
    f"{M}.project_add_user_email_task.project_add_user_email": (
        "pi_dash/bgtasks/project_add_user_email_task.py",
        "project_add_user_email",
        ["example.com", str(uuid.uuid4()), str(uuid.uuid4())],
        {},
    ),
    # NOTE (PIDASHCONV-1104): project_invitation was deliberately absent
    # (dead code: invite.py called .delay() on the bulk_create list). Django
    # d656243d fixed the call site, so the task is live, ported, and covered
    # here; the oracle worker boots with it in -I (oracle_worker_includes).
    f"{M}.project_invitation_task.project_invitation": (
        "pi_dash/bgtasks/project_invitation_task.py",
        "project_invitation",
        ["contract@example.com", str(uuid.uuid4()), "token", "example.com", "inviter"],
        {},
    ),
    f"{M}.workspace_invitation_task.workspace_invitation": (
        "pi_dash/bgtasks/workspace_invitation_task.py",
        "workspace_invitation",
        ["contract@example.com", str(uuid.uuid4()), "token", "example.com", "inviter"],
        {},
    ),
}


@pytest.mark.parametrize("task_name", sorted(MAIL_TASKS))
def test_wire_payload_parity(task_name):
    """Payload parity without infra: the exact wire message for each task."""
    _, _, args, kwargs = MAIL_TASKS[task_name]
    message = celery_wire.capture_wire_message(task_name, args=args, kwargs=kwargs)
    celery_wire.assert_wire_message(message, task_name, args=args, kwargs=kwargs)


@pytest.mark.parametrize("task_name", sorted(MAIL_TASKS))
def test_task_options_parity(task_name):
    """Ack parity: plain @shared_task — default ack-on-success, no overrides."""
    module, func, _, _ = MAIL_TASKS[task_name]
    taskspec.assert_task_options(module, func, {})
    for keyword in ("acks_late", "acks_on_failure_or_timeout", "max_retries",
                    "autoretry_for", "retry_backoff"):
        taskspec.assert_no_option(module, func, keyword)


def test_beat_entry_parity():
    taskspec.assert_beat_entry(
        "check-every-five-minutes-to-send-email-notifications",
        f"{M}.email_notification_task.stack_email_notification",
        'crontab(minute="*/5")',
    )


def test_worker_registration(broker_url):
    broker_probe.wait_for_registration(set(MAIL_TASKS))


def test_stack_fans_out_and_marks_processed(db_conn, broker_url, smtp_sink):
    """stack_email_notification: DB before/after diff + SMTP side effect."""
    # The send task needs a real issue (Issue.objects.get) and the base
    # URL primed in Redis under the issue id, else it silently returns.
    chain = seed_helpers.issue_chain(db_conn, "mailstack")
    receiver = seed_helpers.user(db_conn, "contract-receiver")
    issue_id = str(chain["issue"]["id"])
    log_ids = [
        seed_helpers.email_log(db_conn, receiver["id"], chain["owner"]["id"], issue_id)["id"]
        for _ in range(2)
    ]
    redis_cache.setex(issue_id, "http://localhost")
    smtp_sink.clear()
    before = snapshot(db_conn, ["email_notification_logs"])

    celery_wire.publish(f"{M}.email_notification_task.stack_email_notification")
    wait_for(
        lambda: _processed(db_conn, log_ids),
        what="stack_email_notification marking processed_at",
    )
    after = snapshot(db_conn, ["email_notification_logs"])
    changes = diff(before, after)["email_notification_logs"]["changed"]
    assert {str(k) for k in changes} == {str(k) for k in log_ids}
    assert all(v["after"]["processed_at"] for v in changes.values())

    # The fan-out enqueues real send tasks: the worker must deliver the mail.
    delivered = smtp_sink.wait_for_count(1, what="stacked notification mail")
    assert any(receiver["email"] in m["rcpt_tos"] for m in delivered)


def test_stack_redelivery_is_idempotent(db_conn, broker_url, smtp_sink):
    """Redelivery: re-running the stack sends nothing twice."""
    chain = seed_helpers.issue_chain(db_conn, "mailredel")
    receiver = seed_helpers.user(db_conn, "contract-redeliver")
    issue_id = str(chain["issue"]["id"])
    log_id = seed_helpers.email_log(
        db_conn, receiver["id"], chain["owner"]["id"], issue_id
    )["id"]
    redis_cache.setex(issue_id, "http://localhost")
    smtp_sink.clear()

    celery_wire.publish(f"{M}.email_notification_task.stack_email_notification")
    wait_for(lambda: _processed(db_conn, [log_id]), what="first stack run")
    smtp_sink.wait_for_count(1, what="first stacked mail")
    first_run_total = len(smtp_sink.snapshot())

    celery_wire.publish(f"{M}.email_notification_task.stack_email_notification")
    broker_probe.wait_for_queue_drain(what="redelivered stack consumed")
    assert len(smtp_sink.snapshot()) == first_run_total, (
        "redelivered stack_email_notification produced duplicate mail"
    )


@pytest.mark.parametrize(
    "task_name,args",
    [
        (f"{M}.magic_link_code_task.magic_link", ["contract@example.com", "key", "token"]),
        (
            f"{M}.forgot_password_task.forgot_password",
            ["Contract", "contract@example.com", "uid", "token", "example.com"],
        ),
        (
            f"{M}.user_email_update_task.send_email_update_magic_code",
            ["contract@example.com", "token"],
        ),
        (
            f"{M}.user_email_update_task.send_email_update_confirmation",
            ["contract@example.com"],
        ),
    ],
)
def test_pure_mail_tasks_deliver(db_conn, broker_url, smtp_sink, task_name, args):
    """String-arg mail tasks: enqueue → worker executes → SMTP sink receipt."""
    smtp_sink.clear()
    celery_wire.publish(task_name, args=args)
    delivered = smtp_sink.wait_for_count(1, what=f"{task_name} mail")
    assert any("contract@example.com" in m["rcpt_tos"] for m in delivered)


# The four auth mail tasks (PIDASHCONV-1103), pinned to the full captured
# mail: recipient, exact subject, template markers in the raw MIME, and an
# empty DB diff (the bodies write no rows — config is read, mail is sent).
# The Rust replay suite mirrors these pins case-for-case
# (test_rust_replay.py::test_rust_auth_mail_executes_locally); the two
# suites together are the Django-vs-Rust equivalence proof. No Redis pin:
# neither plane touches Redis on these paths (no cache/redis use in the
# three task modules, utils/email.py, or the instance-value resolver;
# the Rust send_mail path is pool + SMTP only), so "same Redis keys"
# holds vacuously.
AUTH_MAIL_CASES = [
    (
        f"{M}.magic_link_code_task.magic_link",
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
        f"{M}.forgot_password_task.forgot_password",
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
        f"{M}.user_email_update_task.send_email_update_magic_code",
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
        f"{M}.user_email_update_task.send_email_update_confirmation",
        ["contract@example.com"],
        "Pi Dash email address successfully updated",
        [
            "contract@example.com",
            "successfully updated",
        ],
    ),
]


@pytest.mark.parametrize("task_name,args,subject,markers", AUTH_MAIL_CASES)
def test_auth_mail_full_capture(db_conn, broker_url, smtp_sink, task_name, args, subject, markers):
    """Auth mail tasks: full captured mail (to/subject/body/template) + no row writes."""
    smtp_sink.clear()
    before = snapshot(db_conn, ["users", "email_notification_logs"])
    celery_wire.publish(task_name, args=args)
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
    assert diff(before, snapshot(db_conn, ["users", "email_notification_logs"])) == {}


# PIDASHCONV-1287: mention content carrying editor-emitted entities and
# voids must arrive normalized exactly like Django's BeautifulSoup
# round-trip — in the HTML part, and with no literal entities in the
# text/plain part. Mirrored case-for-case by
# test_rust_replay.py::test_rust_send_mention_mail_normalized_like_bs4.
MENTION_HTML_TEMPLATE = (
    '<p>a&nbsp;&nbsp;b<br>c <img src="x.png"> it&#39;s &quot;q&quot; '
    '<mention-component entity_identifier="{user_id}" '
    'label="x"></mention-component></p>'
)
# bs4 output for the template above: entities decoded to characters,
# void tags self-closed, attributes double-quoted, mention → @name.
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


def assert_mention_mail_normalized(mail, display_name):
    """Pin the bs4-normalized mention fragment in both MIME parts."""
    html_part, text_part = _mail_parts(mail)
    expected = MENTION_HTML_EXPECTED.format(display=display_name)
    assert expected in html_part, (
        f"HTML part misses the normalized fragment:\n{expected!r}\n"
        f"HTML head: {html_part[:600]!r}"
    )
    assert f"@{display_name}" in text_part
    assert 'it\'s "q"' in text_part
    assert "\xa0\xa0" in text_part
    for leaked in ("&nbsp;", "&#39;", "&quot;"):
        assert leaked not in text_part, f"text/plain leaks {leaked}"


def test_mention_mail_html_normalized_like_bs4(db_conn, broker_url, smtp_sink):
    """Mention mail: entities/voids normalized like BeautifulSoup (Django)."""
    chain = seed_helpers.issue_chain(db_conn, "mailmention")
    receiver = seed_helpers.user(db_conn, "contract-mention-receiver")
    mentioned = seed_helpers.user(db_conn, "Bob")
    issue_id = str(chain["issue"]["id"])
    seed_helpers.email_log(
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
    )
    redis_cache.setex(issue_id, "http://localhost")
    smtp_sink.clear()

    celery_wire.publish(f"{M}.email_notification_task.stack_email_notification")
    delivered = smtp_sink.wait_for_count(1, what="mention notification mail")
    matches = [m for m in delivered if receiver["email"] in m["rcpt_tos"]]
    assert matches, "stack sent no mail to the mention receiver"
    assert_mention_mail_normalized(matches[0], mentioned["display_name"])


def _processed(conn, log_ids) -> bool:
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT count(*) AS n FROM email_notification_logs "
            "WHERE id = ANY(%s::uuid[]) AND processed_at IS NOT NULL",
            ([str(i) for i in log_ids],),
        )
        return cur.fetchone()["n"] == len(log_ids)


def _mention_html(mentioned_id) -> str:
    """Comment HTML with a multi-byte char straddling byte 17 after a tag
    start (the é sits at bytes 16..18 of the rest after ``<``), plus one
    real ``<mention-component>`` to rewrite."""
    return (
        "<p>Merci beaucoupé pour la revue</p>"
        f'<mention-component entity_identifier="{mentioned_id}">X</mention-component>'
    )


def test_stack_sends_non_ascii_mention_mail(db_conn, broker_url, smtp_sink):
    """PIDASHCONV-1284 oracle: non-ASCII mention HTML still delivers.

    The mention slot's ``new_value``/``old_value`` is comment HTML
    (``notification_task.py``): BeautifulSoup rewrites the component to
    ``@display_name`` and the mail is sent with ``sent_at`` marked. The
    Rust replay (``test_rust_replay.py``) mirrors these pins exactly.
    """
    chain = seed_helpers.issue_chain(db_conn, "mailnonascii")
    receiver = seed_helpers.user(db_conn, "contract-nonascii-rcv")
    mentioned = seed_helpers.user(db_conn, "contract-mentioned")
    issue_id = str(chain["issue"]["id"])
    html = _mention_html(mentioned["id"])
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
    redis_cache.setex(issue_id, "http://localhost")
    smtp_sink.clear()

    celery_wire.publish(f"{M}.email_notification_task.stack_email_notification")
    wait_for(
        lambda: _processed(db_conn, [log_id]),
        what="stack_email_notification marking processed_at",
    )
    delivered = smtp_sink.wait_for_count(1, what="non-ascii mention mail")
    matches = [m for m in delivered if receiver["email"] in m["rcpt_tos"]]
    assert matches, "stacked mention mail never arrived"
    raw = matches[0]["raw"]
    # ASCII-only markers: the é itself may QP-encode in the raw MIME.
    assert f"@{mentioned['display_name']}" in raw, "mention not rewritten to @name"
    assert "mention-component" not in raw, "raw mention-component leaked into the mail"
    wait_for(
        lambda: _sent(db_conn, [log_id]),
        what="send_email_notification marking sent_at",
    )


def _sent(conn, log_ids) -> bool:
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT count(*) AS n FROM email_notification_logs "
            "WHERE id = ANY(%s::uuid[]) AND sent_at IS NOT NULL",
            ([str(i) for i in log_ids],),
        )
        return cur.fetchone()["n"] == len(log_ids)


# PIDASHCONV-1288: the subscriber email log keeps
# ``data.issue_activity.issue_comment`` on non-comment activities — Django
# (notification_task.py:426-447) always writes it, as "" when the activity
# names no comment. The replay mirror lives in test_rust_replay.py.
def _seed_1288_world(conn, slug):
    """Issue chain + email-enabled subscriber S (not the actor).

    S is an active project member subscribed to the issue, with
    ``property_change`` email allowed (every other email gate off, so the
    log row below can only come from the property-change gate).
    """
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


def _assert_1288_issue_comment_empty(data, where):
    assert data is not None, f"{where}: no row written"
    block = data["issue_activity"]
    assert "issue_comment" in block, f"{where}: keys {sorted(block)}"
    assert block["issue_comment"] == "", f"{where}: {block['issue_comment']!r}"


def test_notifications_email_log_keeps_issue_comment(db_conn, broker_url):
    """Django: email log carries ``issue_comment == ""`` on priority change.

    The in-app Notification row is asserted too — the control, already
    correct on both planes.
    """
    chain, subscriber = _seed_1288_world(db_conn, "notif1288o")
    issue = chain["issue"]
    celery_wire.publish(
        f"{M}.notification_task.notifications",
        args=_notifications_1288_args(
            chain, _priority_1288_activity(issue["id"], chain["owner"]["id"])
        ),
    )
    email_data = wait_for(
        lambda: _fetch_1288_row(
            db_conn, "email_notification_logs", subscriber["id"], issue["id"]
        ),
        what="oracle email log for notif1288o",
    )
    app_data = wait_for(
        lambda: _fetch_1288_row(
            db_conn, "notifications", subscriber["id"], issue["id"]
        ),
        what="oracle in-app notification for notif1288o",
    )
    _assert_1288_issue_comment_empty(email_data, "oracle email log")
    _assert_1288_issue_comment_empty(app_data, "oracle in-app notification")
