"""D-07 oracle: mail + notification tasks.

Django sources: pi_dash/bgtasks/{email_notification_task, notification_task,
magic_link_code_task, forgot_password_task, user_activation_email_task,
user_deactivation_email_task, user_email_update_task,
project_add_user_email_task, workspace_invitation_task}.py
(project_invitation_task.py is dead code — see NOTE on MAIL_TASKS.)
"""

import uuid

import pytest

from _harness import broker_probe, celery_wire, taskspec
from _harness import seed as seed_helpers
from _harness.db import diff, snapshot, wait_for

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
    # NOTE: pi_dash/bgtasks/project_invitation_task.py::project_invitation is
    # deliberately absent. It is dead code: nothing imports the module, no
    # beat entry or send_task names it, and the live worker does not register
    # it (worker-plane CI 2026-09-24: 74 registered tasks, this one missing).
    # The apparent call site (app/views/project/invite.py:105) calls .delay()
    # on the list returned by bulk_create, not on the task. Per the Dead
    # Python Code page it stays unported and untested here.
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
    receiver = seed_helpers.user(db_conn, "contract-receiver")
    actor = seed_helpers.user(db_conn, "contract-actor")
    issue_id = str(uuid.uuid4())
    log_ids = [
        seed_helpers.email_log(db_conn, receiver["id"], actor["id"], issue_id)["id"]
        for _ in range(2)
    ]
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
    receiver = seed_helpers.user(db_conn, "contract-redeliver")
    actor = seed_helpers.user(db_conn, "contract-redeliver-actor")
    log_id = seed_helpers.email_log(
        db_conn, receiver["id"], actor["id"], str(uuid.uuid4())
    )["id"]
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


def _processed(conn, log_ids) -> bool:
    with conn.cursor() as cur:
        cur.execute(
            "SELECT count(*) AS n FROM email_notification_logs "
            "WHERE id = ANY(%s::uuid[]) AND processed_at IS NOT NULL",
            ([str(i) for i in log_ids],),
        )
        return cur.fetchone()["n"] == len(log_ids)
