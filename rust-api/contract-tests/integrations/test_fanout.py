"""Beat fan-out: sync_all_bindings / sync_all_repos.

Oracle pins, per fan-out task:
  * payload parity — the worker fans out exactly the enabled set (each
    enabled binding shows a sync attempt; nothing else does);
  * ack parity — the published parent is consumed (queue drains below the
    pre-publish high-water mark) and produces no retry storm;
  * kill/empty behavior — with nothing enabled the task is an acked no-op.

Provider-neutral path uses a binding whose token is invalid, so the attempt
deterministically ends on the 4xx error-recording branch (no retry).
"""
from __future__ import annotations

from _harness import broker, db


FANOUT = "pi_dash.bgtasks.git_sync_task.sync_all_bindings"
LEGACY_FANOUT = "pi_dash.bgtasks.github_sync_task.sync_all_repos"


def _publish_and_drain(broker_url: str, task: str, **kwargs) -> str:
    # Delivery note: publish_task uses publisher confirms (a nack raises),
    # so a clean return proves the message reached the broker; the drain
    # proves the worker consumed it. For no-op tasks the drain alone would
    # be vacuous if delivery silently failed — the enabled-binding test in
    # this file is the positive control proving the same publish path lands
    # on the worker and executes.
    before = broker.queue_depth(broker_url)
    task_id = broker.publish_task(broker_url, task, **kwargs)
    assert broker.wait_for_drain(broker_url, before, timeout=120), (
        f"worker never consumed {task} (queue never drained back to {before})"
    )
    return task_id


def test_git_fanout_attempts_enabled_binding(database_url, broker_url, github_binding):
    binding_id = github_binding["binding_id"]
    account_id = github_binding["account_id"]

    _publish_and_drain(broker_url, FANOUT)

    # Payload parity: exactly this binding was attempted — the 401 from the
    # invalid token lands on the mapped 4xx branch and is recorded, with the
    # account degraded. last_synced_at stays null on the error path.
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


def test_git_fanout_skips_disabled_binding(database_url, broker_url, github_binding):
    binding_id = github_binding["binding_id"]
    db.execute(
        database_url,
        "UPDATE git_repository_bindings SET is_sync_enabled = false,"
        " last_sync_error = 'sentinel-untouched' WHERE id = %s",
        (binding_id,),
    )

    _publish_and_drain(broker_url, FANOUT)

    row = db.fetchone(
        database_url,
        "SELECT last_sync_error, last_synced_at FROM git_repository_bindings WHERE id = %s",
        (binding_id,),
    )
    assert row["last_sync_error"] == "sentinel-untouched", row
    assert row["last_synced_at"] is None


def test_git_fanout_without_bindings_is_noop(database_url, broker_url, anchor):
    before_issues = db.fetchone(
        database_url, "SELECT count(*) AS n FROM git_issue_syncs"
    )["n"]
    _publish_and_drain(broker_url, FANOUT)
    after_issues = db.fetchone(
        database_url, "SELECT count(*) AS n FROM git_issue_syncs"
    )["n"]
    assert after_issues == before_issues


def test_legacy_fanout_without_syncs_is_noop(database_url, broker_url):
    before = db.fetchone(
        database_url, "SELECT count(*) AS n FROM github_issue_syncs"
    )["n"]
    _publish_and_drain(broker_url, LEGACY_FANOUT)
    after = db.fetchone(
        database_url, "SELECT count(*) AS n FROM github_issue_syncs"
    )["n"]
    assert after == before
