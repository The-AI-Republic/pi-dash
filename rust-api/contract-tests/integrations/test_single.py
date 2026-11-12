"""Single-task probes: unknown ids, completion comments, retry/ETA, redelivery.

Black-box oracle per task (publish in Celery wire format → live Django
worker executes → diff Postgres):

  * github_app config sourcing — sync_one_binding on an installation
    credential reads GITHUB_APP_ID/SLUG from instance_configurations
    (PIDASHCONV-1471): empty DB fails with the exact config-missing
    message; seeded DB mints (real GitHub answers 404 for the unknown
    numeric app id, proving the db id reached the wire as the JWT iss);
  * unknown-id no-op — sync_one_binding / sync_one_repo /
    post_completion_comment (both providers) with a random UUID ack without
    touching any row;
  * completion idempotency — a GitIssueSync whose metadata already carries
    completion_comment_id short-circuits (no error recorded, no new rows);
  * completion error-record — with an invalid token the worker attempts the
    real provider POST, gets 401, and records completion_comment_error
    (one-shot: no retry, no comment id stored);
  * setup-ordering — adapter lookup runs before the guarded try, so an
    unsupported provider fails silently (no record, no retry); in-try
    faults record + retry instead. Live retry needs a real transient
    (e.g. provider 5xx), which has no hermetic seam, so the retry
    schedule itself (max_retries=3, countdown 60 * 2^retries) is pinned
    statically in test_beat.py;
  * redelivery — republishing the same logical job twice creates no
    duplicate effects (mirror counts stable, metadata written once).
"""
from __future__ import annotations

import uuid

from _harness import broker, db

from . import seed


SYNC_ONE = "pi_dash.bgtasks.git_sync_task.sync_one_binding"
POST_COMMENT = "pi_dash.bgtasks.git_sync_task.post_completion_comment"
LEGACY_SYNC_ONE = "pi_dash.bgtasks.github_sync_task.sync_one_repo"
LEGACY_POST_COMMENT = "pi_dash.bgtasks.github_sync_task.post_completion_comment"


def _publish_and_drain(broker_url: str, task: str, **kwargs) -> str:
    before = broker.queue_depth(broker_url)
    task_id = broker.publish_task(broker_url, task, **kwargs)
    assert broker.wait_for_drain(broker_url, before, timeout=120), (
        f"worker never consumed {task} (queue never drained back to {before})"
    )
    return task_id


def _publish_and_wait_error(
    database_url: str, broker_url: str, task: str, binding_id: str
) -> str:
    """Publish one job, poll the binding until `last_sync_error` is set.

    Plain `_publish_and_drain` cannot work here: unexpected faults requeue
    with a retry countdown, so the broker never drains back to baseline.
    A fresh binding (error starts `""`) plus the error transition proves
    this task executed.
    """
    broker.publish_task(broker_url, task, args=[binding_id])
    row = db.wait_for(
        database_url,
        "SELECT last_sync_error FROM git_repository_bindings WHERE id = %s"
        " AND last_sync_error <> ''",
        (binding_id,),
    )
    assert row is not None, f"{task} never recorded last_sync_error"
    return row["last_sync_error"]


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


def test_sync_one_unknown_id_is_noop(database_url, broker_url):
    before = _counts(database_url)
    _publish_and_drain(broker_url, SYNC_ONE, args=[str(uuid.uuid4())])
    assert _counts(database_url) == before


def test_legacy_sync_one_unknown_id_is_noop(database_url, broker_url):
    before = db.fetchone(database_url, "SELECT count(*) AS n FROM github_issue_syncs")[
        "n"
    ]
    _publish_and_drain(broker_url, LEGACY_SYNC_ONE, args=[str(uuid.uuid4())])
    after = db.fetchone(database_url, "SELECT count(*) AS n FROM github_issue_syncs")[
        "n"
    ]
    assert after == before


def test_post_completion_unknown_id_is_noop(database_url, broker_url):
    before = _counts(database_url)
    _publish_and_drain(broker_url, POST_COMMENT, args=[str(uuid.uuid4())])
    _publish_and_drain(broker_url, LEGACY_POST_COMMENT, args=[str(uuid.uuid4())])
    assert _counts(database_url) == before


def test_completion_comment_short_circuits_when_already_posted(
    database_url, broker_url, anchor, sync_scope, github_binding
):
    borrowed = seed.find_borrowed_issue(database_url)
    sync_id = seed.git_issue_sync(
        database_url,
        anchor,
        sync_scope,
        binding_id=github_binding["binding_id"],
        issue_id=str(borrowed["id"]),
        external_iid="ct-idem-1",
        metadata={"completion_comment_id": "ct-already-123"},
    )
    _publish_and_drain(broker_url, POST_COMMENT, args=[sync_id])

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


def test_completion_comment_records_error_on_auth_failure(
    database_url, broker_url, anchor, sync_scope, github_binding
):
    borrowed = seed.find_borrowed_issue(database_url)
    sync_id = seed.git_issue_sync(
        database_url,
        anchor,
        sync_scope,
        binding_id=github_binding["binding_id"],
        issue_id=str(borrowed["id"]),
        external_iid="ct-err-1",
        metadata={},
    )
    _publish_and_drain(broker_url, POST_COMMENT, args=[sync_id])

    row = db.wait_for(
        database_url,
        "SELECT metadata FROM git_issue_syncs WHERE id = %s"
        " AND metadata ? 'completion_comment_error'",
        (sync_id,),
    )
    assert row is not None, "expected completion_comment_error after 401 from provider"
    assert "GitProviderAuthError" in row["metadata"]["completion_comment_error"]
    assert "completion_comment_id" not in row["metadata"]


def test_sync_one_unknown_provider_fails_without_record_or_retry(
    database_url, broker_url, anchor, sync_scope
):
    # Setup ordering pin: get_adapter runs BEFORE the guarded try in
    # sync_one_binding, so an unsupported provider escapes as a plain task
    # failure — nothing is recorded on the binding and self.retry never
    # runs. The Rust port must reproduce this ordering (translate, don't
    # redesign): setup failures are silent, in-try failures record + retry.
    seeded = seed.unknown_provider_binding(database_url, anchor, sync_scope)
    binding_id = seeded["binding_id"]

    _publish_and_drain(broker_url, SYNC_ONE, args=[binding_id])

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


def test_redelivered_completion_comment_has_single_effect(
    database_url, broker_url, anchor, sync_scope, github_binding
):
    borrowed = seed.find_borrowed_issue(database_url)
    sync_id = seed.git_issue_sync(
        database_url,
        anchor,
        sync_scope,
        binding_id=github_binding["binding_id"],
        issue_id=str(borrowed["id"]),
        external_iid="ct-redeliver-1",
        metadata={"completion_comment_id": "ct-already-999"},
    )
    before = _counts(database_url)
    task_id = str(uuid.uuid4())
    _publish_and_drain(broker_url, POST_COMMENT, args=[sync_id], task_id=task_id)
    # Same task id redelivered (broker redelivery reuses the id).
    _publish_and_drain(broker_url, POST_COMMENT, args=[sync_id], task_id=task_id)

    row = db.fetchone(
        database_url, "SELECT metadata FROM git_issue_syncs WHERE id = %s", (sync_id,)
    )
    assert row["metadata"].get("completion_comment_id") == "ct-already-999"
    assert "completion_comment_error" not in row["metadata"]
    assert _counts(database_url) == before


def test_redelivered_unknown_sync_one_stays_noop(database_url, broker_url):
    before = _counts(database_url)
    unknown = str(uuid.uuid4())
    _publish_and_drain(broker_url, SYNC_ONE, args=[unknown])
    _publish_and_drain(broker_url, SYNC_ONE, args=[unknown])
    assert _counts(database_url) == before


def test_sync_one_github_app_config_db_sourced(
    database_url, broker_url, anchor, sync_scope
):
    """The github_app mint reads ID/slug from instance_configurations.

    Workers must run with GITHUB_APP_ID/SLUG unset and
    GITHUB_APP_PRIVATE_KEY set to a valid test RSA key. Phase (a) proves
    that env precondition at runtime: with no DB rows the sync fails with
    the exact config-missing message (any env values would mint instead).
    Phase (b) seeds a numeric app id only in the DB: the sync mints, and
    real GitHub answers 404 for the unknown app — the 404 (not a
    JWT-decode 401) proves the db id reached the wire as the JWT issuer.
    last_synced_at stays null on both error paths; each phase deletes its
    triple right after asserting (one active binding per project, and the
    pending retry redeliveries then ack on the missing row).
    """
    keys = ["GITHUB_APP_ID", "GITHUB_APP_SLUG"]
    saved = seed.save_instance_config(database_url, keys)
    try:
        # Phase (a): nothing configured anywhere.
        seed.clear_instance_config(database_url, keys)
        seeded_a = seed.github_app_binding(
            database_url, anchor, sync_scope, suffix=f"cfg-a-{uuid.uuid4().hex[:8]}"
        )
        err_a = _publish_and_wait_error(
            database_url, broker_url, SYNC_ONE, seeded_a["binding_id"]
        )
        assert err_a == "GitHub App config missing: app_id, app_slug", err_a
        row_a = db.fetchone(
            database_url,
            "SELECT last_synced_at FROM git_repository_bindings WHERE id = %s",
            (seeded_a["binding_id"],),
        )
        assert row_a["last_synced_at"] is None
        seed.delete_binding_triple(database_url, seeded_a)

        # Phase (b): numeric app id lives only in the DB.
        seed.set_instance_config(
            database_url,
            {"GITHUB_APP_ID": "14719999", "GITHUB_APP_SLUG": "ct-1471-app-slug"},
        )
        seeded_b = seed.github_app_binding(
            database_url, anchor, sync_scope, suffix=f"cfg-b-{uuid.uuid4().hex[:8]}"
        )
        err_b = _publish_and_wait_error(
            database_url, broker_url, SYNC_ONE, seeded_b["binding_id"]
        )
        assert "GitHub App config missing" not in err_b, err_b
        assert "404" in err_b and "access_tokens" in err_b, err_b
        row_b = db.fetchone(
            database_url,
            "SELECT last_synced_at FROM git_repository_bindings WHERE id = %s",
            (seeded_b["binding_id"],),
        )
        assert row_b["last_synced_at"] is None
        seed.delete_binding_triple(database_url, seeded_b)
    finally:
        seed.restore_instance_config(database_url, saved)
