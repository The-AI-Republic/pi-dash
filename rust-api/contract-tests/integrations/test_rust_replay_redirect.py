# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Rust replay of the rename-redirect oracle (PIDASHCONV-1472).

The new-path cases from ``test_github_redirect.py`` (binding sync and
comment-back), driven through ``rust_job_queue`` against the Rust worker
instead of the Celery broker. The worker boots with
``PIDASH_GITHUB_API_BASE`` pointing at the shared route table served over
plain HTTP (fixed ``GITHUB_REDIRECT_STUB_PORT``); the Django leg reaches
the same routes through its MITM proxy instead. Seeds, payloads and DB
assertions are imported from the oracle module; only the transport (this
module's publish + stub-hit gate) differs.

The legacy cases below (PIDASHCONV-1512) replay the legacy oracle
tests the same way: the legacy handler used to panic on ANY live HTTP
sync ("Cannot drop a runtime in a context where blocking is not
allowed" — ``GithubRestClient`` was built in async context and dropped
there), so this module covered the new path only until that fix moved
client construction inside ``spawn_blocking``. The Django-leg legacy
tests still pin the oracle behavior.

Run with the Django worker (and beat) STOPPED and the Rust worker
running against the same DATABASE_URL (dedicated CI job; no shared
worker). A Rust worker without the override env hits real GitHub and
records 401s, so the loop-membership gate skips instead of failing when
the stub saw no fresh hit.
"""

from __future__ import annotations

import uuid

import pytest

from _harness import db, rust_queue
from _harness import seed as seed_helpers
from _harness import sinks

from . import seed
from . import test_github_redirect as oracle


@pytest.fixture(scope="module")
def redirect_stub():
    """Plain-HTTP redirect stub on the fixed port the worker dials."""
    with sinks.RedirectStub(oracle.redirect_routes(), port=oracle.STUB_PORT) as stub:
        yield stub


@pytest.fixture()
def redirect_case(database_url, db_conn):
    """Per-test self-seeded chain + scope; deletes mirrors on teardown."""
    tag = f"rredir{uuid.uuid4().hex[:8]}"
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
            "Rust worker is not pointed at the redirect stub — boot it with"
            f" PIDASH_GITHUB_API_BASE=http://127.0.0.1:{oracle.STUB_PORT}"
        )
    return row


def test_rust_sync_one_binding_follows_rename_redirects(
    database_url, redirect_stub, redirect_case
):
    binding_id = oracle.seed_old_binding(database_url, redirect_case)
    _publish_and_settle(
        database_url,
        redirect_stub,
        oracle.SYNC_ONE,
        [binding_id],
        "SELECT last_sync_error, last_synced_at FROM git_repository_bindings"
        " WHERE id = %s AND (last_synced_at IS NOT NULL OR last_sync_error <> '')",
        (binding_id,),
        "sync_one_binding",
    )
    oracle.assert_sync_binding_ok(database_url, binding_id)
    entries = redirect_stub.snapshot()
    assert oracle.hits_matching(
        entries, "GET", f"/repos/{oracle.OLD_OWNER}/{oracle.OLD_NAME}/issues"
    ), "stub never saw the old-path issues listing"
    assert oracle.hits_matching(entries, "GET", "/repositories/1/issues"), (
        "stub never saw the followed listing"
    )


def test_rust_post_completion_comment_follows_307(
    database_url, redirect_stub, redirect_case
):
    binding_id = oracle.seed_old_binding(database_url, redirect_case)
    sync_id = seed.git_issue_sync(
        database_url,
        redirect_case["anchor"],
        redirect_case["scope"],
        binding_id=binding_id,
        issue_id=str(redirect_case["issue"]["id"]),
        external_iid=str(oracle.ISSUE_NUMBER),
    )
    _publish_and_settle(
        database_url,
        redirect_stub,
        oracle.POST_COMMENT,
        [sync_id],
        "SELECT metadata FROM git_issue_syncs WHERE id = %s"
        " AND (metadata ? 'completion_comment_id'"
        " OR metadata ? 'completion_comment_error')",
        (sync_id,),
        "post_completion_comment",
    )
    oracle.assert_comment_posted(
        database_url, sync_id, table="git_issue_syncs", expect_id=str(oracle.COMMENT_ID)
    )
    oracle.assert_post_reached_stub(
        redirect_stub.snapshot(), body_marker="completed in Pi Dash"
    )


def test_rust_legacy_sync_one_repo_follows_rename_redirects(
    database_url, redirect_stub, redirect_case
):
    sync_id = oracle.seed_old_legacy_sync(database_url, redirect_case)
    project_id = str(redirect_case["anchor"]["projects"][0]["id"])
    _publish_and_settle(
        database_url,
        redirect_stub,
        oracle.LEGACY_SYNC_ONE,
        [sync_id],
        "SELECT last_sync_error, last_synced_at FROM github_repository_syncs"
        " WHERE id = %s AND (last_synced_at IS NOT NULL OR last_sync_error <> '')",
        (sync_id,),
        "legacy sync_one_repo",
    )
    oracle.assert_sync_repo_ok(database_url, sync_id, project_id)
    entries = redirect_stub.snapshot()
    assert oracle.hits_matching(
        entries, "GET", f"/repos/{oracle.OLD_OWNER}/{oracle.OLD_NAME}/issues"
    ), "stub never saw the old-path issues listing"
    assert oracle.hits_matching(entries, "GET", "/repositories/1/issues"), (
        "stub never saw the followed listing"
    )


def test_rust_legacy_post_completion_comment_follows_307(
    database_url, redirect_stub, redirect_case
):
    sync_id = oracle.seed_old_legacy_sync(database_url, redirect_case)
    issue_sync_id = seed.legacy_issue_sync(
        database_url,
        redirect_case["scope"],
        sync_id=sync_id,
        issue_id=str(redirect_case["issue"]["id"]),
        project_id=str(redirect_case["anchor"]["projects"][0]["id"]),
        workspace_id=redirect_case["workspace_id"],
        repo_issue_id=oracle.ISSUE_NUMBER,
    )
    _publish_and_settle(
        database_url,
        redirect_stub,
        oracle.LEGACY_POST_COMMENT,
        [issue_sync_id],
        "SELECT metadata FROM github_issue_syncs WHERE id = %s"
        " AND (metadata ? 'completion_comment_id'"
        " OR metadata ? 'completion_comment_error')",
        (issue_sync_id,),
        "legacy post_completion_comment",
    )
    # Legacy stores the payload id verbatim (JSON number), unlike the new
    # path's str() (github_sync_task.py:387 vs git_sync_task.py:321).
    oracle.assert_comment_posted(
        database_url, issue_sync_id, table="github_issue_syncs", expect_id=oracle.COMMENT_ID
    )
    oracle.assert_post_reached_stub(
        redirect_stub.snapshot(), body_marker="completed in Pi Dash"
    )
