# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Rust replay of the sync signal-legs oracle (PIDASHCONV-1576).

The one git case from ``test_sync_signal_legs.py`` (binding
sync-create into a ticking default arms the ticker), driven through
``rust_job_queue`` against the Rust worker instead of the Celery broker.
The worker boots with ``PIDASH_GITHUB_API_BASE`` pointing at the shared
route table served over plain HTTP (fixed ``SIGNAL_LEGS_STUB_PORT``); the
Django leg reaches the same routes through its MITM proxy instead. Seeds,
payloads and DB assertions are imported from the oracle module; only the
transport (this module's publish + stub-hit gate) differs.

Run with the Django worker (and beat) STOPPED and the Rust worker
running against the same DATABASE_URL (dedicated CI job; no shared
worker). A Rust worker without the override env hits real GitHub and
records 401s, so the loop-membership gate skips instead of failing when
the stub saw no fresh hit. The legacy sync cases are deleted with the
legacy poller (PDASHOSS01-358) and live with the Django oracle only
(``test_sync_signal_legs.py``).
"""

from __future__ import annotations

import uuid

import pytest

from _harness import db, rust_queue
from _harness import seed as seed_helpers
from _harness import sinks

from . import seed
from . import test_sync_signal_legs as oracle


@pytest.fixture(scope="module")
def signal_stub():
    """Plain-HTTP signal-legs stub on the fixed port the worker dials."""
    with sinks.RedirectStub(oracle.signal_routes(), port=oracle.STUB_PORT) as stub:
        yield stub


@pytest.fixture()
def signal_case(database_url, db_conn):
    """Per-test self-seeded chain + scope; deletes mirrors on teardown."""
    tag = f"rsig{uuid.uuid4().hex[:8]}"
    scope = seed.Scope(database_url)
    built = seed_helpers.issue_chain(db_conn, tag)
    for table, row in (
        ("users", built["owner"]),
        ("workspaces", built["workspace"]),
        ("projects", built["project"]),
        ("states", built["state"]),
        ("issues", built["issue"]),
    ):
        scope.track(table, str(row["id"]))
    workspace_id = str(built["workspace"]["id"])
    yield {
        "scope": scope,
        "workspace_id": workspace_id,
        "anchor": {
            "workspace": built["workspace"],
            "projects": [built["project"]],
            "actor": built["owner"],
        },
        "issue": built["issue"],
        "conn": db_conn,
    }
    seed.cleanup_workspace_mirrors(database_url, workspace_id)
    scope.cleanup()


def _publish_and_settle(
    database_url: str,
    stub,
    task: str,
    args: list,
    sql: str,
    params: tuple,
    what: str,
) -> dict:
    """Publish one job via rust_job_queue, wait for a terminal row state.

    Same settle-then-gate shape as the oracle: the loop-membership gate
    skips when the consuming worker never hit this module's stub (a Rust
    worker without PIDASH_GITHUB_API_BASE), instead of failing on its
    direct-to-GitHub 401.
    """
    before = len(stub.snapshot())
    rust_queue.publish(task, args=args)
    row = db.wait_for(database_url, sql, params)
    assert row is not None, f"{task} never settled ({what})"
    if len(stub.snapshot()) <= before:
        raise pytest.skip(
            "Rust worker is not pointed at the signal-legs stub — boot it with"
            f" PIDASH_GITHUB_API_BASE=http://127.0.0.1:{oracle.STUB_PORT}"
        )
    return row


def test_rust_binding_sync_create_into_ticking_default_arms_ticker(
    database_url, signal_stub, signal_case
):
    ticking_id = oracle.project_state(
        signal_case, name="In Progress", group="started"
    )
    binding_id = oracle.seed_binding(database_url, signal_case)
    project_id = str(signal_case["anchor"]["projects"][0]["id"])
    _publish_and_settle(
        database_url,
        signal_stub,
        oracle.SYNC_ONE,
        [binding_id],
        "SELECT last_sync_error, last_synced_at FROM git_repository_bindings"
        " WHERE id = %s AND (last_synced_at IS NOT NULL OR last_sync_error <> '')",
        (binding_id,),
        "sync_one_binding",
    )
    row = db.fetchone(
        database_url,
        "SELECT last_sync_error, last_synced_at FROM git_repository_bindings"
        " WHERE id = %s",
        (binding_id,),
    )
    assert row["last_sync_error"] == "", row
    assert row["last_synced_at"] is not None, row
    issue_id = oracle.mirrored_issue_id(database_url, project_id)
    assert issue_id is not None, "no mirrored Issue row"
    issue = db.fetchone(
        database_url, "SELECT state_id FROM issues WHERE id = %s", (issue_id,)
    )
    assert str(issue["state_id"]) == ticking_id, issue
    oracle.assert_ticker_armed(database_url, issue_id)
    sync_row = db.fetchone(
        database_url,
        "SELECT metadata FROM git_issue_syncs WHERE binding_id = %s"
        " AND external_iid = %s",
        (binding_id, str(oracle.ISSUE_NUMBER)),
    )
    assert sync_row is not None
    oracle.assert_no_completion_stamp(sync_row)
