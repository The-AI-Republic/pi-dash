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

Ownership split (mirrors the worker registry on rust-dev): all 11 D-08
task names are Python-owned — the ``pidash-api`` binary registers no
``tasks_webhooks`` handler (``bin/pidash-api/src/main.rs`` wires only
cleanup + integrations), and ``crates/jobs/src/tasks_webhooks/mod.rs``
keeps the webhook send/fan-out path Python-owned deliberately
(registering a local send handler would steal live traffic while the
webhook-row fetch, HTTP send and SMTP send still live on Python).
Every test below therefore asserts observable AMQP forwarding — the
``rust_job_queue`` row is acked AND the broker ``celery`` queue grows
while the Django worker is stopped — instead of local DB effects, plus
a no-local-row pin proving the Rust worker never executes these jobs
itself. No skipped or weakened assertions.

process_logs note (coordinate with PIDASHCONV-242): Django never
executes ``process_logs`` either (unregistered at worker boot), so
forwarding is the faithful replay of stock behavior on both backends.
If 242 decides the Rust worker must execute it locally, its port
registers the handler and this module's process_logs test is revisited
to assert the row diff.

Beat note: D-08 owns no beat entries (``test_no_beat_entries_owned`` —
cleanup owns the webhook-log delete; the daily
``check-every-day-to-archive-and-close`` entry names
``issue_automation_task.archive_and_close_old_issues``, which that
static test does not count as owned). Beat pins are source-text, so
there is no replay form.
"""

import uuid

from _harness import broker_probe, rust_queue
from _harness import seed as seed_helpers
from _harness.db import diff as _diff
from _harness.db import snapshot

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
    """Rust inspect-equivalent: a no-op track_event job is consumed.

    Replaces ``test_worker_registration`` (Celery broadcast inspect,
    which only a Django worker answers). Settlement of the
    ``rust_job_queue`` row proves the Rust worker owns the queue; a
    Django worker never reads that table.
    """
    _publish_and_observe_forward(
        TRACK_EVENT,
        args=[str(uuid.uuid4()), "contract.probe", "contract-ws", {}],
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
    owner, workspace = chain["owner"], chain["workspace"]
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


def test_rust_process_logs_forwards_to_python(db_conn, broker_url):
    """process_logs, Rust side: forward, never execute locally.

    Mirrors ``test_process_logs_writes_postgres_row`` (same payload):
    the ``api_activity_logs`` row is a Python-plane effect, so the
    replay pins forwarding plus a no-local-row pin. See the module
    docstring re PIDASHCONV-242: if the parity decision registers a
    local handler, this test is revisited to assert the row diff.
    """
    before = snapshot(db_conn, ["api_activity_logs"])
    _publish_and_observe_forward(
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
    assert _diff(before, snapshot(db_conn, ["api_activity_logs"])) == {}


def test_rust_track_event_forwarded_without_crash(db_conn, broker_url):
    """No PostHog configured → early return on Python; job still forwards.

    Mirrors ``test_track_event_consumed_without_crash``: consumption is
    the assertion, observed as settlement + one broker message.
    """
    _publish_and_observe_forward(
        TRACK_EVENT,
        args=[str(uuid.uuid4()), "rust.replay.probe", "rust-replay-ws", {}],
    )


def test_rust_light_tasks_forwarded_without_crash(db_conn, broker_url):
    """Execution parity: remaining D-08 tasks forward for the Python plane.

    Mirrors ``test_light_tasks_consumed_without_crash`` (same probe
    payloads, same set): archive, page_transaction, recent_visited.
    """
    cases = [
        (ARCHIVE_AND_CLOSE, [], {}),
        (
            PAGE_TRANSACTION,
            ["<p>new</p>", "<p>old</p>", str(uuid.uuid4())],
            {},
        ),
        (
            RECENT_VISITED,
            ["issue", str(uuid.uuid4()), str(uuid.uuid4()), str(uuid.uuid4()), "ws"],
            {},
        ),
    ]
    for task_name, args, kwargs in cases:
        _publish_and_observe_forward(task_name, args=args, kwargs=kwargs)


def test_rust_issue_activity_forwards_to_python(db_conn, broker_url):
    """issue_activity has no behavioral oracle test (wire + registration
    pins only), so the replay pins the forward with the Python
    positional-arg shape (``issue_activities_task.py:1504``)."""
    chain = seed_helpers.issue_chain(db_conn, "rustreplayactivity")
    issue, owner = chain["issue"], chain["owner"]
    project = chain["project"]
    _publish_and_observe_forward(
        ISSUE_ACTIVITY,
        args=[
            "created",
            {"id": str(issue["id"])},
            None,
            str(issue["id"]),
            str(owner["id"]),
            str(project["id"]),
            0.0,
        ],
    )


def test_rust_crawl_link_forwards_to_python(db_conn, broker_url):
    """crawl_work_item_link_title has no behavioral oracle test (wire +
    registration pins only), so the replay pins the forward with the
    Python ``(id, url)`` shape (``work_item_link_task.py:263``)."""
    _publish_and_observe_forward(
        CRAWL_LINK_TITLE,
        args=[str(uuid.uuid4()), "https://example.com/rust-replay-probe"],
    )

