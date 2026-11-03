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


# PIDASHCONV-1285: description mention on an issue with NO other
# subscribers. Django (notification_task.py:522-570) still writes the
# single "You have been mentioned" Notification (identifiers read from the
# last activity — the :249 loop binds issue_activity whatever the
# subscriber count), creates the IssueMention row, and soft-deletes
# removed mentions. With preference.mention the email-log batch dies to
# the bool-receiver FK error and is swallowed (:672-674): no email log
# row either way. The replay mirror lives in test_rust_replay.py.
def _mention_1285_html(user_id) -> str:
    """Description HTML mentioning one user (extract_mentions shape)."""
    return (
        '<p>hi <mention-component entity_name="user_mention" '
        f'entity_identifier="{user_id}"></mention-component></p>'
    )


def _seed_1285_world(conn, slug, mention_pref):
    """Issue chain + mentioned B (unsubscribed) + removed-mention C.

    No IssueSubscriber rows at all: the actor and B are both excluded
    from the subscriber list, the normal state of a fresh issue.
    """
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
    # last_activity: the actor's description row, so the single-row
    # branch (:525-529) runs. Raw SQL: attachments is varchar[] and
    # insert_row wraps lists in Json().
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
    """Description-change activity with non-null identifiers.

    The DB last_activity row carries NULL identifiers, so asserting the
    wire UUIDs below pins the stale read to the last activity.
    """
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


def _assert_1285_write_set(conn, chain, activity, mentioned, removed, where):
    issue = chain["issue"]
    rows = _fetch_1285_notifications(conn, mentioned["id"], issue["id"])
    assert len(rows) == 1, f"{where}: B notifications {len(rows)}"
    row = rows[0]
    assert row["sender"] == "in_app:issue_activities:mentioned", where
    assert row["message"] == (
        f"You have been mentioned in the issue {issue['name']}"
    ), where
    block = row["data"]["issue_activity"]
    assert block["old_identifier"] == activity["old_identifier"], where
    assert block["new_identifier"] == activity["new_identifier"], where
    live = [
        r for r in _fetch_1285_mentions(conn, mentioned["id"], issue["id"])
        if r["deleted_at"] is None
    ]
    assert len(live) == 1, f"{where}: B live IssueMention rows {len(live)}"
    gone = _fetch_1285_mentions(conn, removed["id"], issue["id"])
    assert len(gone) == 1 and gone[0]["deleted_at"] is not None, (
        f"{where}: removed mention not soft-deleted: {gone!r}"
    )
    assert _count_1285_email_logs(conn, issue["id"]) == 0, (
        f"{where}: expected no email log rows"
    )


@pytest.mark.parametrize("mention_pref", [False, True])
def test_notifications_no_subscriber_description_mention(
    db_conn, broker_url, mention_pref
):
    """Django: B is notified + tracked even with zero subscribers.

    mention=True additionally pins the swallowed bool-receiver FK bug:
    the Notification and IssueMention writes commit, the email batch is
    lost, and nothing is retried.
    """
    slug = f"notif1285o{'t' if mention_pref else 'f'}"
    chain, mentioned, removed = _seed_1285_world(db_conn, slug, mention_pref)
    issue = chain["issue"]
    activity = _description_1285_activity(issue["id"], chain["owner"]["id"])
    celery_wire.publish(
        f"{M}.notification_task.notifications",
        args=_notifications_1285_args(
            chain, activity, mentioned["id"], removed["id"]
        ),
    )
    wait_for(
        lambda: _fetch_1285_notifications(
            db_conn, mentioned["id"], issue["id"]
        )
        or None,
        what=f"oracle mention notification for {slug}",
    )
    _assert_1285_write_set(
        db_conn, chain, activity, mentioned, removed, "oracle"
    )


# PIDASHCONV-1690: a webhook URL carrying HTML metacharacters must render
# as inert text in the deactivation mail (Django autoescape), in both the
# HTML part and the text part (strip_tags keeps `&...;` references
# verbatim). Mirrored case-for-case by
# test_rust_replay.py::test_rust_deactivation_mail_escapes_url.
DEACTIVATION_ESCAPE_URL = "https://x.example/?a=<img/src=https://t.example/p.png>&x=1"
DEACTIVATION_ESCAPE_SENTENCE = (
    "Webhook https://x.example/?a=&lt;img/src=https://t.example/p.png&gt;&amp;x=1"
    " has been deactivated due to failed requests."
)


def test_webhook_deactivation_mail_escapes_url(db_conn, broker_url, smtp_sink):
    """Django: hostile webhook URL arrives escaped in both MIME parts."""
    owner = seed_helpers.user(db_conn, "contract-deact-esc-owner")
    receiver = seed_helpers.user(db_conn, "contract-deact-esc-receiver")
    workspace = seed_helpers.workspace(db_conn, "contractdeactesc", owner["id"])
    hook = seed_helpers.webhook(
        db_conn,
        workspace["id"],
        DEACTIVATION_ESCAPE_URL,
        created_by_id=str(owner["id"]),
    )
    smtp_sink.clear()
    celery_wire.publish(
        f"{M}.webhook_task.send_webhook_deactivation_email",
        args=[str(hook["id"]), str(receiver["id"]), "example.com", "contract reason"],
    )
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


# PIDASHCONV-1692: one save renames the issue to a value with a stray `<`
# and adds a comment mentioning B. Django (notification_task.py
# comment_mentions loop) copies EVERY activity's new_value into the
# mention EmailNotificationLog rows, so the plain-text rename value
# reaches process_mention, which must normalize it — not truncate it
# (3.12.10 html.parser keeps unterminated-`<` text; the 3.12.3 oracle
# dropped it, PIDASHCONV-1693). The mail renders each mention new_value
# through {{ actor_comment|safe }}. The replay mirror lives in
# test_rust_replay.py. The DB issue row is deliberately NOT renamed: the
# mail's issue-name block would otherwise also carry the value and the
# pin below could pass on a truncated mention value.
STRAY_LT_RENAME = "Fix x<y handling"
STRAY_LT_RENDERED = "Fix x&lt;y handling"


def _comment_1692_html(mentioned_id) -> str:
    """Comment HTML mentioning one user (extract_comment_mentions shape)."""
    return (
        '<p>please review <mention-component entity_name="user_mention" '
        f'entity_identifier="{mentioned_id}"></mention-component></p>'
    )


def _seed_1692_world(conn, slug):
    """Issue chain + mentioned B (active member, mention pref on).

    Zero IssueSubscriber rows: the subscriber loop is skipped and only
    the comment_mentions branch writes.
    """
    chain = seed_helpers.issue_chain(conn, slug)
    project, ws, issue = chain["project"], chain["workspace"], chain["issue"]
    mentioned = seed_helpers.user(conn, "contract-notif1692-b")
    seed_helpers.project_member(conn, project["id"], ws["id"], mentioned["id"])
    seed_helpers.notification_preference(conn, mentioned["id"], mention=True)
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
    """Created-comment activity (create_comment_activity shape).

    Real created comments carry no old_value (None) and the comment id
    as new_identifier/issue_comment.
    """
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
            "SELECT data FROM email_notification_logs "
            "WHERE receiver_id = %s AND entity_identifier = %s "
            "ORDER BY created_at",
            (str(receiver_id), str(issue_id)),
        )
        return [row["data"] for row in cur.fetchall()]


def test_notifications_mention_mail_renders_stray_lt(
    db_conn, broker_url, smtp_sink
):
    """Django: rename value with a stray `<` survives into the mention mail."""
    chain, mentioned = _seed_1692_world(db_conn, "notif1692o")
    issue = chain["issue"]
    activities = [
        _rename_1692_activity(issue["id"], chain["owner"]["id"]),
        _comment_1692_activity(issue["id"], chain["owner"]["id"], mentioned["id"]),
    ]
    celery_wire.publish(
        f"{M}.notification_task.notifications",
        args=_notifications_1692_args(chain, activities),
    )
    def _both_rows():
        rows = _fetch_1692_mention_logs(db_conn, mentioned["id"], issue["id"])
        return rows if len(rows) == 2 else None

    rows = wait_for(_both_rows, what="oracle mention email logs for notif1692o")
    # The :503 copy: one mention row per activity in the save, each
    # carrying that activity's own new_value under field="mention".
    # Order-agnostic: the two bulk-created rows can tie on created_at.
    assert [r["issue_activity"]["field"] for r in rows] == ["mention"] * 2
    assert sorted(r["issue_activity"]["new_value"] for r in rows) == sorted(
        [STRAY_LT_RENAME, activities[1]["new_value"]]
    )

    redis_cache.setex(str(issue["id"]), "http://localhost")
    smtp_sink.clear()
    celery_wire.publish(f"{M}.email_notification_task.stack_email_notification")
    delivered = smtp_sink.wait_for_count(1, what="stray-lt mention mail")
    matches = [m for m in delivered if mentioned["email"] in m["rcpt_tos"]]
    assert matches, "stack sent no mail to the mentioned user"
    html_part, text_part = _mail_parts(matches[0])
    assert STRAY_LT_RENDERED in html_part, (
        f"HTML part misses the escaped rename value; head: {html_part[:600]!r}"
    )
    assert f"@{mentioned['display_name']}" in html_part, (
        "HTML part misses the rewritten mention; "
        f"head: {html_part[:600]!r}"
    )
    assert f"@{mentioned['display_name']}" in text_part
