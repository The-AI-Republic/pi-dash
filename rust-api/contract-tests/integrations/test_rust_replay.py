# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-05 oracle, Rust-side replay (PIDASHCONV-239).

Executable form of the integrations oracle against the Rust worker.
The Django oracle (``test_single.py`` / ``test_fanout.py``) publishes
jobs in Celery protocol v2 to ``CELERY_BROKER_URL`` and pins the Django
``celery.py`` source text (``test_beat.py``) — only a Celery (Django)
worker can execute or answer those. The Rust worker consumes the
Postgres queue ``rust_job_queue`` instead, so this module replays the
same oracle by seeding ``rust_job_queue`` rows carrying the exact Celery
v2 payloads (see ``_harness/rust_queue.py``) and running the identical
DB before/after assertions.

Run with the Django worker (and beat) STOPPED and the Rust worker
running against the same DATABASE_URL::

    export DATABASE_URL=postgresql://...  # your own scratch database
    export CELERY_BROKER_URL=amqp://...   # same broker the Rust worker uses
    pidash-api worker &  # Rust worker; Django worker stopped
    pytest -p no:django integrations/test_rust_replay.py -v

What maps where (broker publish -> ``rust_queue``):

- ``broker.publish_task(task, args, kwargs, task_id)`` ->
  ``rust_queue.publish(...)`` (same task name, same args/kwargs, same
  message id; ``countdown`` becomes ``visible_at``).
- ``broker.wait_for_drain(broker_url, before)`` ->
  ``rust_queue.wait_for_queue_drain(baseline=before)``
  (Postgres depth instead of broker depth).
- Celery broadcast inspect (no D-05 equivalent in the Django oracle) ->
  ``test_rust_worker_liveness`` below: an unknown-id no-op job must be
  consumed from ``rust_job_queue`` with zero DB effects, proving a Rust
  worker (not Django) is serving this database.

What is NOT duplicated here: the static parity pins in ``test_beat.py``
(beat entry identity, retry schedule, signal-hook wiring) are
backend-independent — they assert on Django source text, so they
already pass with no worker at all. Only the live worker tests have a
replay form. Signal-hook firing stays pinned on the source (needs an
authenticated HTTP state transition); the delayed
``post_completion_comment`` tasks are proven executable by this replay.

Ownership split (mirrors the worker registry on rust-dev):

- Locally-owned git_sync tasks assert the SAME row-level DB diffs as
  the Django oracle, seed for seed: fan-out attempt set, unknown-id
  no-ops, completion idempotency, auth error-record, setup-ordering
  pin, redelivery guards. No skipped or weakened assertions. These go
  green once PIDASHCONV-238 registers the D-05 handlers in the
  ``pidash-api`` binary; until then they forward (see below).
- The 3 legacy ``github_sync_task`` names are still Python-owned (the
  PIDASHCONV-148 port is unmerged, so no Rust handler exists): the Rust
  worker forwards them to AMQP. Their replay form asserts observable
  forwarding — the ``rust_job_queue`` row is acked AND the broker
  ``celery`` queue grows while the Django worker is stopped — instead
  of local DB effects. If a later port registers them, these tests
  become DB-diff tests in a follow-up.

Residue: every ``rust_job_queue`` row is acked (deleted) by the end of
each test, so the queue drains to zero. Forwarded messages accumulate
on the broker ``celery`` queue until the Python plane consumes them;
that accumulation is the forwarding evidence, asserted via per-test
baselines.
"""

from __future__ import annotations

import uuid

import pytest

from _harness import broker_probe, db, rust_queue
from _harness import seed as seed_helpers

from . import seed


SYNC_ALL = "pi_dash.bgtasks.git_sync_task.sync_all_bindings"
SYNC_ONE = "pi_dash.bgtasks.git_sync_task.sync_one_binding"
POST_COMMENT = "pi_dash.bgtasks.git_sync_task.post_completion_comment"
LEGACY_SYNC_ALL = "pi_dash.bgtasks.github_sync_task.sync_all_repos"
LEGACY_SYNC_ONE = "pi_dash.bgtasks.github_sync_task.sync_one_repo"
LEGACY_POST_COMMENT = "pi_dash.bgtasks.github_sync_task.post_completion_comment"


def _publish_and_drain(task: str, args=None, kwargs=None, celery_id=None) -> str:
    # Delivery note: rust_queue.publish raises on a failed INSERT, so a
    # clean return proves the row reached the queue; the drain proves the
    # worker consumed it. For no-op tasks the drain alone would be
    # vacuous if delivery silently failed — the enabled-binding fan-out
    # test is the positive control proving the same publish path lands
    # on the worker and executes.
    baseline = rust_queue.queue_depth()
    celery_id = rust_queue.publish(task, args=args, kwargs=kwargs, celery_id=celery_id)
    rust_queue.wait_for_queue_drain(
        baseline=baseline, what=f"rust worker consumed {task}"
    )
    return celery_id


def _counts(database_url) -> dict:
    return {
        "bindings": db.fetchone(
            database_url, "SELECT count(*) AS n FROM git_repository_bindings"
        )["n"],
        "issues": db.fetchone(database_url, "SELECT count(*) AS n FROM git_issue_syncs")[
            "n"
        ],
        "legacy": db.fetchone(
            database_url, "SELECT count(*) AS n FROM github_issue_syncs"
        )["n"],
    }


@pytest.fixture()
def rscope(database_url):
    """Teardown tracker that needs no borrowed anchor rows.

    The oracle conftest's ``sync_scope`` pulls the session ``anchor``
    fixture (borrowed workspace/project/actor), which does not exist on
    a freshly migrated database. The replay seeds its own anchor
    (``chain`` below), so it tracks in a local scope instead.
    """
    scope = seed.Scope(database_url)
    yield scope
    scope.cleanup()


@pytest.fixture()
def chain(db_conn, rscope):
    """Self-seeded anchor: user -> workspace -> project -> state -> issue.

    The Django oracle borrows anchor rows read-only from the target DB;
    the replay seeds its own so the module runs hermetic on a migrated
    database with no fixture data. Rows are tracked in ``rscope``
    (creation order) so teardown deletes them in reverse-FK order.
    """
    built = seed_helpers.issue_chain(db_conn, f"rustreplay{uuid.uuid4().hex[:8]}")
    for table, row in (
        ("users", built["owner"]),
        ("workspaces", built["workspace"]),
        ("projects", built["project"]),
        ("states", built["state"]),
        ("issues", built["issue"]),
    ):
        rscope.track(table, str(row["id"]))
    return {
        "workspace": built["workspace"],
        "projects": [built["project"]],
        "actor": built["owner"],
        "state": built["state"],
        "issue": built["issue"],
    }


@pytest.fixture()
def github_binding(database_url, chain, rscope) -> dict:
    """One enabled provider-neutral binding with an invalid token.

    Mirrors the oracle conftest fixture: the worker's attempt hits
    api.github.com -> 401 -> GitProviderAuthError -> deterministic
    error-recording path (no retry, account degraded).
    """
    return seed.github_binding(
        database_url, chain, rscope, token="contract-test-invalid-token"
    )


def test_rust_worker_liveness(database_url):
    """Rust serving proof: an unknown-id job is consumed with zero effects.

    An empty eligible set means the DB is untouched; consumption itself
    proves the Rust worker (not Django) owns the queue for this
    database.
    """
    before = _counts(database_url)
    _publish_and_drain(SYNC_ONE, args=[str(uuid.uuid4())])
    assert _counts(database_url) == before


def test_rust_git_fanout_attempts_enabled_binding(database_url, github_binding):
    binding_id = github_binding["binding_id"]
    account_id = github_binding["account_id"]

    _publish_and_drain(SYNC_ALL)

    # Payload parity: exactly this binding was attempted — the 401 from
    # the invalid token lands on the mapped 4xx branch and is recorded,
    # with the account degraded. last_synced_at stays null on the error
    # path. (Identical to test_git_fanout_attempts_enabled_binding.)
    row = db.wait_for(
        database_url,
        "SELECT last_sync_error, last_synced_at FROM git_repository_bindings WHERE id = %s"
        " AND last_sync_error <> ''",
        (binding_id,),
    )
    assert row is not None, "enabled binding shows no sync attempt after fan-out"
    assert "GitProviderAuthError" in row["last_sync_error"], row["last_sync_error"]
    assert row["last_synced_at"] is None

    account = db.fetchone(
        database_url,
        "SELECT status FROM git_provider_accounts WHERE id = %s",
        (account_id,),
    )
    assert account["status"] == "degraded"

    # No mirror rows: auth failed before any listing, and no retry storm
    # followed (4xx never calls self.retry).
    assert (
        db.fetchone(
            database_url,
            "SELECT count(*) AS n FROM git_issue_syncs WHERE binding_id = %s",
            (binding_id,),
        )["n"]
        == 0
    )


def test_rust_git_fanout_skips_disabled_binding(database_url, github_binding):
    binding_id = github_binding["binding_id"]
    db.execute(
        database_url,
        "UPDATE git_repository_bindings SET is_sync_enabled = false,"
        " last_sync_error = 'sentinel-untouched' WHERE id = %s",
        (binding_id,),
    )

    _publish_and_drain(SYNC_ALL)

    row = db.fetchone(
        database_url,
        "SELECT last_sync_error, last_synced_at FROM git_repository_bindings WHERE id = %s",
        (binding_id,),
    )
    assert row["last_sync_error"] == "sentinel-untouched", row
    assert row["last_synced_at"] is None


def test_rust_git_fanout_without_bindings_is_noop(database_url, chain):
    before_issues = db.fetchone(
        database_url, "SELECT count(*) AS n FROM git_issue_syncs"
    )["n"]
    _publish_and_drain(SYNC_ALL)
    after_issues = db.fetchone(
        database_url, "SELECT count(*) AS n FROM git_issue_syncs"
    )["n"]
    assert after_issues == before_issues


def test_rust_sync_one_unknown_id_is_noop(database_url):
    before = _counts(database_url)
    _publish_and_drain(SYNC_ONE, args=[str(uuid.uuid4())])
    assert _counts(database_url) == before


def test_rust_post_completion_unknown_id_is_noop(database_url):
    before = _counts(database_url)
    _publish_and_drain(POST_COMMENT, args=[str(uuid.uuid4())])
    _publish_and_drain(LEGACY_POST_COMMENT, args=[str(uuid.uuid4())])
    assert _counts(database_url) == before


def test_rust_completion_comment_short_circuits_when_already_posted(
    database_url, chain, rscope, github_binding
):
    sync_id = seed.git_issue_sync(
        database_url,
        chain,
        rscope,
        binding_id=github_binding["binding_id"],
        issue_id=str(chain["issue"]["id"]),
        external_iid="ct-idem-1",
        metadata={"completion_comment_id": "ct-already-123"},
    )
    _publish_and_drain(POST_COMMENT, args=[sync_id])

    row = db.fetchone(
        database_url, "SELECT metadata FROM git_issue_syncs WHERE id = %s", (sync_id,)
    )
    assert row["metadata"].get("completion_comment_id") == "ct-already-123"
    assert "completion_comment_error" not in row["metadata"]
    # No mirror rows: short-circuit returns before any provider HTTP.
    assert (
        db.fetchone(
            database_url,
            "SELECT count(*) AS n FROM git_issue_syncs WHERE binding_id = %s"
            " AND id <> %s",
            (github_binding["binding_id"], sync_id),
        )["n"]
        == 0
    )


def test_rust_completion_comment_records_error_on_auth_failure(
    database_url, chain, rscope, github_binding
):
    sync_id = seed.git_issue_sync(
        database_url,
        chain,
        rscope,
        binding_id=github_binding["binding_id"],
        issue_id=str(chain["issue"]["id"]),
        external_iid="ct-err-1",
        metadata={},
    )
    _publish_and_drain(POST_COMMENT, args=[sync_id])

    row = db.wait_for(
        database_url,
        "SELECT metadata FROM git_issue_syncs WHERE id = %s"
        " AND metadata ? 'completion_comment_error'",
        (sync_id,),
    )
    assert row is not None, "expected completion_comment_error after 401 from provider"
    assert "GitProviderAuthError" in row["metadata"]["completion_comment_error"]
    assert "completion_comment_id" not in row["metadata"]


def test_rust_sync_one_unknown_provider_fails_without_record_or_retry(
    database_url, chain, rscope
):
    # Setup ordering pin: get_adapter runs BEFORE the guarded try in
    # sync_one_binding, so an unsupported provider escapes as a plain task
    # failure — nothing is recorded on the binding and self.retry never
    # runs. The Rust port must reproduce this ordering (translate, don't
    # redesign): setup failures are silent, in-try failures record + retry.
    seeded = seed.unknown_provider_binding(database_url, chain, rscope)
    binding_id = seeded["binding_id"]

    _publish_and_drain(SYNC_ONE, args=[binding_id])

    row = db.fetchone(
        database_url,
        "SELECT last_sync_error, last_synced_at FROM git_repository_bindings"
        " WHERE id = %s",
        (binding_id,),
    )
    assert row["last_sync_error"] == "", row
    assert row["last_synced_at"] is None
    assert (
        db.fetchone(
            database_url,
            "SELECT count(*) AS n FROM git_issue_syncs WHERE binding_id = %s",
            (binding_id,),
        )["n"]
        == 0
    )


def test_rust_redelivered_completion_comment_has_single_effect(
    database_url, chain, rscope, github_binding
):
    sync_id = seed.git_issue_sync(
        database_url,
        chain,
        rscope,
        binding_id=github_binding["binding_id"],
        issue_id=str(chain["issue"]["id"]),
        external_iid="ct-redeliver-1",
        metadata={"completion_comment_id": "ct-already-999"},
    )
    before = _counts(database_url)
    task_id = str(uuid.uuid4())
    _publish_and_drain(POST_COMMENT, args=[sync_id], celery_id=task_id)
    # Same message id redelivered (broker redelivery reuses the id).
    _publish_and_drain(POST_COMMENT, args=[sync_id], celery_id=task_id)

    row = db.fetchone(
        database_url, "SELECT metadata FROM git_issue_syncs WHERE id = %s", (sync_id,)
    )
    assert row["metadata"].get("completion_comment_id") == "ct-already-999"
    assert "completion_comment_error" not in row["metadata"]
    assert _counts(database_url) == before


def test_rust_redelivered_unknown_sync_one_stays_noop(database_url):
    before = _counts(database_url)
    unknown = str(uuid.uuid4())
    task_id = str(uuid.uuid4())
    _publish_and_drain(SYNC_ONE, args=[unknown], celery_id=task_id)
    _publish_and_drain(SYNC_ONE, args=[unknown], celery_id=task_id)
    assert _counts(database_url) == before


def _forward_and_prove(task: str, args, what: str) -> None:
    """Forwarding form for Python-owned names: acked + broker grows.

    The Rust worker has no local handler for these names, so it
    publishes the Celery v2 message to the broker and acks the row.
    With the Django worker stopped the broker ``celery`` queue grows —
    that growth is the observable proof the job executed through the
    Rust worker instead of being dropped.
    """
    baseline = broker_probe.queue_depth()
    celery_id = rust_queue.publish(task, args=args)
    rust_queue.wait_for_settled(celery_id, what=f"{what} forwarded")

    def _arrived():
        depth = broker_probe.queue_depth()
        return depth if depth >= baseline + 1 else None

    arrived = db.wait_for_condition(_arrived, what=f"{what} arrived on broker")
    assert arrived >= baseline + 1


def test_rust_legacy_fanout_forwards_to_python(database_url):
    before = db.fetchone(
        database_url, "SELECT count(*) AS n FROM github_issue_syncs"
    )["n"]
    _forward_and_prove(LEGACY_SYNC_ALL, [], "legacy fan-out")
    after = db.fetchone(database_url, "SELECT count(*) AS n FROM github_issue_syncs")[
        "n"
    ]
    assert after == before


def test_rust_legacy_sync_one_unknown_id_forwards_to_python(database_url):
    before = db.fetchone(
        database_url, "SELECT count(*) AS n FROM github_issue_syncs"
    )["n"]
    _forward_and_prove(LEGACY_SYNC_ONE, [str(uuid.uuid4())], "legacy sync_one")
    after = db.fetchone(database_url, "SELECT count(*) AS n FROM github_issue_syncs")[
        "n"
    ]
    assert after == before


def test_rust_legacy_post_completion_unknown_id_forwards_to_python(database_url):
    before = _counts(database_url)
    _forward_and_prove(
        LEGACY_POST_COMMENT, [str(uuid.uuid4())], "legacy post_completion_comment"
    )
    assert _counts(database_url) == before
