# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Completion comment-back on app_issues/v1 state transitions (PIDASHCONV-1460).

Django fires ``post_completion_comment`` from the ``github_signals``
post_save receiver (``bgtasks/github_signals.py:44-75``) on every
non-create Issue save whose state changed into the ``completed`` group,
when a ``GitIssueSync``/``GithubIssueSync`` mirror exists with no
``completion_comment_id``. The Rust app_issues/v1 write paths never
called the jobs-layer dispatch (only the assistant ``update_issue``
path wired by PIDASHCONV-1252 did), so closing a synced issue through
the app/v1 UIs or API skipped the upstream completion comment.

Live oracle per affected write path: seed an issue with a
``GitIssueSync`` row whose metadata carries no
``completion_comment_id``, move it into a completed-group state through
the path, and prove the provider's ``post_completion_comment`` job was
enqueued. No worker runs in HTTP suites, so the enqueue is observed
directly: Django ``.delay()``s onto the Celery broker, Rust serve
INSERTs into ``rust_job_queue`` (each helper reads both; only the
active backend has rows — the ``app_cycles/test_activity_origin.py``
pattern). The negative case proves the idempotency guard: with
``completion_comment_id`` already set, the move lands but nothing is
enqueued.
"""

from __future__ import annotations

import json
import os
import uuid

import httpx
import pytest

from _harness.auth import api_client, login_session_cookie
from _harness.config import get_settings
from _harness.db import LazyDatabase
from _harness.seed import PASSWORD, Seeder, SeedTracker, ensure_setup_done
from integrations import seed as int_seed

pytestmark = pytest.mark.contract

GIT_TASK = "pi_dash.bgtasks.git_sync_task.post_completion_comment"


@pytest.fixture(scope="session")
def settings():
    return get_settings()


@pytest.fixture()
def db(settings):
    database = LazyDatabase(settings.database_url)
    yield database
    database.close()


@pytest.fixture()
def seeder(db):
    tracker = SeedTracker(db)
    seeder = Seeder(db, tracker, tag=uuid.uuid4().hex[:10])
    yield seeder
    tracker.cleanup()


def _world(seeder):
    """Owner + workspace + project + ADMIN memberships."""
    ensure_setup_done(seeder.db.connect())
    owner = seeder.create_user()
    workspace = seeder.create_workspace(owner["id"])
    seeder.create_workspace_member(workspace["id"], owner["id"], role=20)
    project = seeder.create_project(workspace["id"])
    seeder.create_project_member(
        workspace["id"], project["id"], owner["id"], role=20
    )
    return {"owner": owner, "workspace": workspace, "project": project}


def _states(db, seeder, world, *, done_default=False):
    started = seeder.create_state(
        world["workspace"]["id"], world["project"]["id"],
        name="Doing", group="started",
    )
    done = seeder.create_state(
        world["workspace"]["id"], world["project"]["id"],
        name="Done", group="completed",
    )
    if done_default:
        db.execute('UPDATE states SET "default" = true WHERE id = %s', (done["id"],))
    return started, done


def _sync(database_url, world, issue_id, metadata):
    """Provider account + repo + binding + one GitIssueSync row.

    Returns (scope, sync_id); the caller must ``scope.cleanup()`` in a
    finally (before the seeder teardown deletes the issue).
    """
    anchor = {
        "workspace": {"id": str(world["workspace"]["id"])},
        "projects": [{
            "id": str(world["project"]["id"]),
            "workspace_id": str(world["workspace"]["id"]),
        }],
        "actor": {"id": str(world["owner"]["id"])},
    }
    scope = int_seed.Scope(database_url)
    unique = uuid.uuid4().hex[:8]
    account_id = int_seed.provider_account(database_url, anchor, scope)
    repo_id = int_seed.repository(database_url, scope, suffix=f"ct1460-{unique}")
    binding_id = int_seed.binding(
        database_url, anchor, scope, account_id=account_id, repository_id=repo_id
    )
    sync_id = int_seed.git_issue_sync(
        database_url,
        anchor,
        scope,
        binding_id=binding_id,
        issue_id=issue_id,
        external_iid=f"ct-1460-{unique}",
        metadata=dict(metadata),
    )
    return scope, sync_id


def _session_client(base_url, owner):
    cookie = login_session_cookie(base_url, owner["email"], PASSWORD)
    return api_client(base_url, cookie)


def _v1_client(base_url, seeder, world):
    token = seeder.create_api_token(world["owner"]["id"], world["workspace"]["id"])
    return httpx.Client(
        base_url=base_url, timeout=30, headers={"X-Api-Key": token["token"]}
    )


def _drain_broker():
    """Pending Celery-protocol messages (Django's transport; [] without one)."""
    if not os.environ.get("CELERY_BROKER_URL"):
        return []
    from _harness import broker_probe

    return broker_probe.drain_queue()


def _rust_rows(db, task, after_id):
    """Rows Rust serve left in ``rust_job_queue`` ([], table may be absent)."""
    row = db.fetchone("SELECT to_regclass('public.rust_job_queue') AS oid")
    if not row or row["oid"] is None:
        return []
    rows = db.fetchall(
        "SELECT task, args, kwargs FROM rust_job_queue WHERE id > %s AND task = %s",
        (after_id, task),
    )
    out = []
    for found in rows:
        args = found["args"]
        kwargs = found["kwargs"]
        out.append({
            "task": found["task"],
            "args": args if isinstance(args, list) else json.loads(args),
            "kwargs": kwargs if isinstance(kwargs, dict) else json.loads(kwargs),
        })
    return out


def _queue_baseline(db):
    row = db.fetchone("SELECT to_regclass('public.rust_job_queue') AS oid")
    if not row or row["oid"] is None:
        return 0
    row = db.fetchone("SELECT COALESCE(MAX(id), 0) AS max_id FROM rust_job_queue")
    return row["max_id"]


def _completion_jobs(db, baseline):
    """New completion-comment jobs on either transport since the baseline."""
    jobs = []
    for headers, payload in _drain_broker():
        if (headers or {}).get("task") == GIT_TASK:
            args = payload[0] if isinstance(payload, list) else []
            kwargs = payload[1] if isinstance(payload, list) and len(payload) > 1 else {}
            jobs.append({"task": GIT_TASK, "args": args, "kwargs": kwargs})
    jobs.extend(_rust_rows(db, GIT_TASK, baseline))
    return jobs


def _assert_completion_enqueued(db, baseline, sync_id):
    jobs = _completion_jobs(db, baseline)
    assert jobs, "expected a post_completion_comment job on the broker or rust_job_queue"
    assert [sync_id] in [job["args"] for job in jobs], (
        f"expected args [{sync_id}], got {[job['args'] for job in jobs]}"
    )
    assert {} in [job["kwargs"] for job in jobs], (
        f"expected empty kwargs, got {[job['kwargs'] for job in jobs]}"
    )


def _assert_completion_silent(db, baseline):
    jobs = _completion_jobs(db, baseline)
    assert jobs == [], f"expected no post_completion_comment job, got {jobs!r}"


def test_app_patch_to_completed_enqueues_post_completion_comment(db, seeder, settings):
    world = _world(seeder)
    started, done = _states(db, seeder, world)
    issue = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], started["id"]
    )
    scope, sync_id = _sync(settings.database_url, world, issue["id"], {})
    try:
        client = _session_client(settings.base_url, world["owner"])
        try:
            _drain_broker()
            baseline = _queue_baseline(db)
            resp = client.patch(
                f"/api/workspaces/{world['workspace']['slug']}"
                f"/projects/{world['project']['id']}/issues/{issue['id']}/",
                json={"state": done["id"]},
            )
            assert resp.status_code == 204, resp.text
            _assert_completion_enqueued(db, baseline, sync_id)
        finally:
            client.close()
    finally:
        scope.cleanup()


def test_app_put_on_null_state_issue_with_completed_default_enqueues_comment(
    db, seeder, settings
):
    world = _world(seeder)
    _, done = _states(db, seeder, world, done_default=True)
    issue = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], None
    )
    scope, sync_id = _sync(settings.database_url, world, issue["id"], {})
    try:
        client = _session_client(settings.base_url, world["owner"])
        try:
            _drain_broker()
            baseline = _queue_baseline(db)
            resp = client.put(
                f"/api/workspaces/{world['workspace']['slug']}"
                f"/projects/{world['project']['id']}/issues/{issue['id']}/",
                json={"name": "Null-state PUT", "priority": "low"},
            )
            assert resp.status_code == 200, resp.text
            _assert_completion_enqueued(db, baseline, sync_id)
        finally:
            client.close()
    finally:
        scope.cleanup()


def test_app_move_to_completed_default_project_enqueues_comment(db, seeder, settings):
    world = _world(seeder)
    started, _ = _states(db, seeder, world)
    issue = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], started["id"]
    )
    target = seeder.create_project(world["workspace"]["id"], name="Move target")
    seeder.create_project_member(
        world["workspace"]["id"], target["id"], world["owner"]["id"], role=20
    )
    target_done = seeder.create_state(
        world["workspace"]["id"], target["id"], name="Target Done", group="completed"
    )
    db.execute(
        'UPDATE states SET "default" = true WHERE id = %s', (target_done["id"],)
    )
    scope, sync_id = _sync(settings.database_url, world, issue["id"], {})
    try:
        client = _session_client(settings.base_url, world["owner"])
        try:
            _drain_broker()
            baseline = _queue_baseline(db)
            resp = client.post(
                f"/api/workspaces/{world['workspace']['slug']}"
                f"/projects/{world['project']['id']}/work-items/{issue['id']}/move/",
                json={"project": target["id"]},
            )
            assert resp.status_code == 200, resp.text
            moved = db.fetchone(
                "SELECT project_id, state_id FROM issues WHERE id = %s",
                (issue["id"],),
            )
            assert str(moved["project_id"]) == target["id"]
            assert str(moved["state_id"]) == target_done["id"]
            _assert_completion_enqueued(db, baseline, sync_id)
        finally:
            client.close()
    finally:
        db.execute("DELETE FROM agent_run WHERE work_item_id = %s", (issue["id"],))
        db.execute("DELETE FROM issue_sequences WHERE issue_id = %s", (issue["id"],))
        scope.cleanup()


def test_v1_patch_to_completed_enqueues_post_completion_comment(db, seeder, settings):
    world = _world(seeder)
    started, done = _states(db, seeder, world)
    issue = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], started["id"]
    )
    scope, sync_id = _sync(settings.database_url, world, issue["id"], {})
    try:
        with _v1_client(settings.base_url, seeder, world) as client:
            _drain_broker()
            baseline = _queue_baseline(db)
            resp = client.patch(
                f"/api/v1/workspaces/{world['workspace']['slug']}"
                f"/projects/{world['project']['id']}/work-items/{issue['id']}/",
                json={"state": done["id"]},
            )
            assert resp.status_code == 200, resp.text
            _assert_completion_enqueued(db, baseline, sync_id)
    finally:
        scope.cleanup()


def test_v1_move_to_completed_default_project_enqueues_comment(db, seeder, settings):
    world = _world(seeder)
    started, _ = _states(db, seeder, world)
    issue = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], started["id"]
    )
    target = seeder.create_project(world["workspace"]["id"], name="V1 move target")
    seeder.create_project_member(
        world["workspace"]["id"], target["id"], world["owner"]["id"], role=20
    )
    target_done = seeder.create_state(
        world["workspace"]["id"], target["id"], name="V1 target Done", group="completed"
    )
    db.execute(
        'UPDATE states SET "default" = true WHERE id = %s', (target_done["id"],)
    )
    scope, sync_id = _sync(settings.database_url, world, issue["id"], {})
    try:
        with _v1_client(settings.base_url, seeder, world) as client:
            _drain_broker()
            baseline = _queue_baseline(db)
            resp = client.post(
                f"/api/v1/workspaces/{world['workspace']['slug']}"
                f"/projects/{world['project']['id']}/work-items/{issue['id']}/move/",
                json={"project": target["id"]},
            )
            assert resp.status_code == 200, resp.text
            moved = db.fetchone(
                "SELECT project_id, state_id FROM issues WHERE id = %s",
                (issue["id"],),
            )
            assert str(moved["project_id"]) == target["id"]
            assert str(moved["state_id"]) == target_done["id"]
            _assert_completion_enqueued(db, baseline, sync_id)
    finally:
        db.execute("DELETE FROM agent_run WHERE work_item_id = %s", (issue["id"],))
        db.execute("DELETE FROM issue_sequences WHERE issue_id = %s", (issue["id"],))
        scope.cleanup()


def test_app_unarchive_null_state_issue_with_completed_default_enqueues_comment(
    db, seeder, settings
):
    world = _world(seeder)
    _, done = _states(db, seeder, world, done_default=True)
    issue = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], None
    )
    db.execute(
        "UPDATE issues SET archived_at = now() WHERE id = %s", (issue["id"],)
    )
    scope, sync_id = _sync(settings.database_url, world, issue["id"], {})
    try:
        client = _session_client(settings.base_url, world["owner"])
        try:
            _drain_broker()
            baseline = _queue_baseline(db)
            resp = client.request(
                "DELETE",
                f"/api/workspaces/{world['workspace']['slug']}"
                f"/projects/{world['project']['id']}/issues/{issue['id']}/archive/",
            )
            assert resp.status_code == 204, resp.text
            _assert_completion_enqueued(db, baseline, sync_id)
        finally:
            client.close()
    finally:
        scope.cleanup()


def test_v1_delete_null_state_issue_with_completed_default_enqueues_comment(
    db, seeder, settings
):
    world = _world(seeder)
    _, done = _states(db, seeder, world, done_default=True)
    issue = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], None
    )
    scope, sync_id = _sync(settings.database_url, world, issue["id"], {})
    try:
        with _v1_client(settings.base_url, seeder, world) as client:
            _drain_broker()
            baseline = _queue_baseline(db)
            resp = client.delete(
                f"/api/v1/workspaces/{world['workspace']['slug']}"
                f"/projects/{world['project']['id']}/work-items/{issue['id']}/",
            )
            assert resp.status_code == 204, resp.text
            _assert_completion_enqueued(db, baseline, sync_id)
    finally:
        scope.cleanup()


def test_app_patch_with_existing_completion_comment_enqueues_nothing(
    db, seeder, settings
):
    world = _world(seeder)
    started, done = _states(db, seeder, world)
    issue = seeder.create_issue(
        world["workspace"]["id"], world["project"]["id"], started["id"]
    )
    seeded = {"completion_comment_id": "ct-seeded-1460"}
    scope, _ = _sync(settings.database_url, world, issue["id"], seeded)
    try:
        client = _session_client(settings.base_url, world["owner"])
        try:
            _drain_broker()
            baseline = _queue_baseline(db)
            resp = client.patch(
                f"/api/workspaces/{world['workspace']['slug']}"
                f"/projects/{world['project']['id']}/issues/{issue['id']}/",
                json={"state": done["id"]},
            )
            assert resp.status_code == 204, resp.text
            _assert_completion_silent(db, baseline)
        finally:
            client.close()
    finally:
        scope.cleanup()
