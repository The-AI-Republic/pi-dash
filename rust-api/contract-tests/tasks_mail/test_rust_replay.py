# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-07 oracle, Rust-side replay (PIDASHCONV-262).

Executable form of the tasks_mail oracle against the Rust worker.
The Django oracle (``test_mail_tasks.py``) publishes jobs in Celery
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
    pytest -p no:django tasks_mail/test_rust_replay.py -v

What maps where (``celery_wire`` -> ``rust_queue``):

- ``celery_wire.publish(task, args, kwargs)`` ->
  ``rust_queue.publish(...)`` (same task name, same args/kwargs as
  ``MAIL_TASKS`` in ``test_mail_tasks.py``).
- ``broker_probe.wait_for_queue_drain`` -> ``rust_queue.wait_for_settled``
  per job (the Postgres row is acked/deleted once the worker forwards
  it) plus a ``broker_probe.queue_depth`` delta proving the message
  arrived on the broker ``celery`` queue for the Python plane.
- ``test_worker_registration`` (Celery inspect) ->
  ``test_rust_worker_liveness`` below: a no-arg stack job must be
  consumed from ``rust_job_queue``, proving a Rust worker (not Django)
  is serving this database.

What is NOT duplicated here: the static parity tests (wire payload,
task options, beat entries) are backend-independent — they assert on
Django source text and the in-process wire builder, so they already
pass with no worker at all. Only the live worker tests have a replay
form.

Ownership split (mirrors the worker registry on rust-dev): all 11 D-07
task names are Python-owned — the ``pidash-api`` binary registers no
``tasks_mail`` handler (``bin/pidash-api/src/main.rs`` wires only
cleanup + integrations), and ``crates/jobs/src/tasks_mail/mod.rs``
keeps every name in ``TASK_NAMES`` routing to ``PythonOwned``
(registering a local send handler would steal live traffic while the
SMTP/template/Redis clients live on Python). Every test below
therefore asserts observable AMQP forwarding — the ``rust_job_queue``
row is acked AND the broker ``celery`` queue grows while the Django
worker is stopped — instead of local DB effects, plus a no-local-row
pin proving the Rust worker never executes these jobs itself. No
skipped or weakened assertions.

``project_invitation`` stays excluded: dead code per
``fixtures/tasks_mail/dead/DEAD.project_invitation.json`` (nothing
imports the module, no beat entry names it), matching ``MAIL_TASKS``.
"""

import uuid

import pytest

from _harness import broker_probe, redis_cache, rust_queue
from _harness import seed as seed_helpers
from _harness.db import diff as _diff
from _harness.db import snapshot

M = "pi_dash.bgtasks"

STACK = f"{M}.email_notification_task.stack_email_notification"
SEND = f"{M}.email_notification_task.send_email_notification"
NOTIFICATIONS = f"{M}.notification_task.notifications"
MAGIC_LINK = f"{M}.magic_link_code_task.magic_link"
FORGOT_PASSWORD = f"{M}.forgot_password_task.forgot_password"
ACTIVATION = f"{M}.user_activation_email_task.user_activation_email"
DEACTIVATION = f"{M}.user_deactivation_email_task.user_deactivation_email"
UPDATE_MAGIC = f"{M}.user_email_update_task.send_email_update_magic_code"
UPDATE_CONFIRM = f"{M}.user_email_update_task.send_email_update_confirmation"
PROJECT_ADD_USER = f"{M}.project_add_user_email_task.project_add_user_email"
WORKSPACE_INVITATION = f"{M}.workspace_invitation_task.workspace_invitation"


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
    """Rust inspect-equivalent: a no-arg stack job is consumed.

    Replaces ``test_worker_registration`` (Celery broadcast inspect,
    which only a Django worker answers). Settlement of the
    ``rust_job_queue`` row proves the Rust worker owns the queue; a
    Django worker never reads that table.
    """
    _publish_and_observe_forward(STACK)


def _seed_stack(db_conn):
    """Seed what the stack fan-out needs, mirroring the Django oracle.

    Same seed as ``test_stack_fans_out_and_marks_processed`` (issue
    chain + receiver + two unprocessed logs + Redis base URL), so the
    forwarded payload stays executable when the Python plane runs it.
    Returns the seeded log ids.
    """
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"mailrust{tag}")
    receiver = seed_helpers.user(db_conn, f"contract-rust-replay-{tag}")
    issue_id = str(chain["issue"]["id"])
    log_ids = [
        seed_helpers.email_log(
            db_conn, receiver["id"], chain["owner"]["id"], issue_id
        )["id"]
        for _ in range(2)
    ]
    redis_cache.setex(issue_id, "http://localhost")
    return log_ids


def test_rust_stack_forwards_without_local_rows(db_conn, broker_url):
    """Stack path, Rust side: forward, never execute locally.

    Mirrors ``test_stack_fans_out_and_marks_processed`` (same seed):
    the DB diff and the SMTP send are Python-plane effects, so the
    replay pins the forward plus a no-local-row pin — the Rust worker
    must not mark ``processed_at`` itself.
    """
    _seed_stack(db_conn)
    before = snapshot(db_conn, ["email_notification_logs"])
    _publish_and_observe_forward(STACK)
    assert _diff(before, snapshot(db_conn, ["email_notification_logs"])) == {}


def test_rust_stack_redelivery_forwards_twice(db_conn, broker_url):
    """Redelivery: two publishes → two broker messages, nothing local.

    Mirrors ``test_stack_redelivery_is_idempotent``: Django intends the
    second run to send nothing twice, but idempotency is a Python-plane
    effect — the replay pins that both jobs still forward exactly once
    (both ``rust_job_queue`` rows settle, broker grows by at least two)
    and no local ``email_notification_logs`` row is touched.
    """
    _seed_stack(db_conn)
    before = snapshot(db_conn, ["email_notification_logs"])
    baseline = broker_probe.queue_depth()
    celery_ids = [rust_queue.publish(STACK) for _ in range(2)]
    for celery_id in celery_ids:
        rust_queue.wait_for_settled(celery_id, what="redelivered stack forwarded")
    assert broker_probe.queue_depth() >= baseline + 2
    assert _diff(before, snapshot(db_conn, ["email_notification_logs"])) == {}


def _send_kwargs(db_conn):
    """Executable send payload, mirroring the stack oracle's seed."""
    tag = uuid.uuid4().hex[:8]
    chain = seed_helpers.issue_chain(db_conn, f"mailsend{tag}")
    receiver = seed_helpers.user(db_conn, f"contract-rust-send-{tag}")
    issue_id = str(chain["issue"]["id"])
    redis_cache.setex(issue_id, "http://localhost")
    return {
        "issue_id": issue_id,
        "notification_data": {},
        "receiver_id": str(receiver["id"]),
        "email_notification_ids": [],
    }


def _mail_cases(db_conn):
    """(task, args, kwargs) for the ten non-stack D-07 names.

    Payload shapes match ``MAIL_TASKS`` in ``test_mail_tasks.py``
    exactly; the send entry is seeded so the forwarded payload stays
    executable on the Python plane.
    """
    return [
        (SEND, [], _send_kwargs(db_conn)),
        (
            NOTIFICATIONS,
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
        (MAGIC_LINK, ["contract@example.com", "key", "token"], {}),
        (
            FORGOT_PASSWORD,
            ["Contract", "contract@example.com", "uid", "token", "example.com"],
            {},
        ),
        (ACTIVATION, ["example.com", str(uuid.uuid4())], {}),
        (DEACTIVATION, ["example.com", str(uuid.uuid4())], {}),
        (UPDATE_MAGIC, ["contract@example.com", "token"], {}),
        (UPDATE_CONFIRM, ["contract@example.com"], {}),
        (
            PROJECT_ADD_USER,
            ["example.com", str(uuid.uuid4()), str(uuid.uuid4())],
            {},
        ),
        (
            WORKSPACE_INVITATION,
            [
                "contract@example.com",
                str(uuid.uuid4()),
                "token",
                "example.com",
                "inviter",
            ],
            {},
        ),
    ]


@pytest.mark.parametrize(
    "task_name", sorted([SEND, NOTIFICATIONS, MAGIC_LINK, FORGOT_PASSWORD,
                         ACTIVATION, DEACTIVATION, UPDATE_MAGIC, UPDATE_CONFIRM,
                         PROJECT_ADD_USER, WORKSPACE_INVITATION])
)
def test_rust_mail_task_forwards_to_python(db_conn, broker_url, task_name):
    """Forward parity, Rust side: each mail job forwards for Python.

    Mirrors the live Django tests (``test_pure_mail_tasks_deliver``
    for the four string-arg senders; wire + registration pins only for
    the rest): the SMTP receipt is a Python-plane effect while the
    Django worker is stopped, so the replay pins the forward — the
    ``rust_job_queue`` row settles and the broker ``celery`` queue
    grows — proving the Rust worker never executes these jobs itself.
    """
    cases = {task: (args, kwargs) for task, args, kwargs in _mail_cases(db_conn)}
    args, kwargs = cases[task_name]
    _publish_and_observe_forward(task_name, args=args, kwargs=kwargs)
