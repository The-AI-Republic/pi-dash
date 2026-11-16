"""Tombstoned forward-FK reads (PIDASHCONV-1210).

Django resolves a forward FK (`runner.pod`, `pod.project`) through the
model's `_base_manager` — a plain `models.Manager`. `Pod` and `Project` set
no `base_manager_name`, so their `PodManager` / `SoftDeletionManager` scopes
do NOT apply on an FK dereference: a soft-deleted pod or project still
renders. Rust's FK-follow reads used the scoped builders and 500'd on the
miss. Covers (a) re-revoke of a revoked runner whose pod is tombstoned, and
(b) pod list / detail / PATCH with a tombstoned project.
"""

from __future__ import annotations

import pytest

from contextlib import contextmanager

from _harness.auth import login_session
from _harness.http import api_client
from _harness.web import web_delete, web_patch, web_post

WEB = "/api/runners"

pytestmark = pytest.mark.contract


@contextmanager
def _admin_client(settings, seeder, daemon_world):
    """Logged-in session client for a workspace admin (pod manage needs admin)."""
    admin = seeder.create_user()
    seeder.create_member(daemon_world["workspace"]["id"], admin["id"], role=20)
    with api_client(settings.base_url) as client:
        login_session(client, email=admin["email"], password=admin["password"])
        yield client


def test_revoke_rerun_with_tombstoned_pod(settings, seeder, daemon_world, user_client):
    """(a) Re-revoke a revoked runner whose pod is soft-deleted → 200.

    `pod_detail` is a forward-FK read (`PodMiniSerializer(source="pod")`), so
    the tombstoned pod still renders; Rust used the `PodManager`-scoped read
    and 500'd.
    """
    pod = seeder.create_pod(
        daemon_world["workspace"]["id"],
        daemon_world["project"]["id"],
        name="Tombstoned pod",
    )
    seeded = seeder.enroll_runner(
        daemon_world["owner"]["id"],
        daemon_world["workspace"]["id"],
        pod["id"],
        f"apd_en_{seeder.tag}tomb",
    )
    runner_id = seeded["id"]

    first = web_post(user_client, f"{WEB}/{runner_id}/revoke/", {})
    assert first.status_code == 200, first.text
    assert first.json()["pod_detail"]["id"] == pod["id"]

    # The only runner is revoked, so the pod deletes cleanly; an admin runs it.
    with _admin_client(settings, seeder, daemon_world) as client:
        deleted = web_delete(client, f"{WEB}/pods/{pod['id']}/")
    assert deleted.status_code == 204, deleted.text
    assert (
        seeder.db.fetchval("SELECT deleted_at FROM pod WHERE id = %s", (pod["id"],))
        is not None
    )

    second = web_post(user_client, f"{WEB}/{runner_id}/revoke/", {})
    assert second.status_code == 200, second.text
    body = second.json()
    assert body["pod_detail"]["id"] == pod["id"]
    assert body["pod_detail"]["is_default"] is False


def test_pod_reads_with_tombstoned_project(settings, seeder, daemon_world, user_client):
    """(b) Pod list / detail / PATCH with a tombstoned project → 200.

    `project_identifier` is a forward-FK read (`source="project.identifier"`),
    so a project row with `deleted_at` set (but a live pod) still renders.
    """
    project_id = daemon_world["project"]["id"]
    project_identifier = daemon_world["project"]["identifier"]
    pod_id = daemon_world["pod"]["id"]
    workspace_id = daemon_world["workspace"]["id"]

    # Soft-delete the project without tombstoning its pods (the async Celery
    # task that would do that has not run — set the column directly).
    seeder.db.execute(
        "UPDATE projects SET deleted_at = now() WHERE id = %s", (project_id,)
    )
    live = seeder.db.fetchval(
        "SELECT deleted_at FROM pod WHERE id = %s", (pod_id,)
    )
    assert live is None, "pod must remain live for this scenario"

    listing = user_client.get(f"{WEB}/pods/", params={"workspace": workspace_id})
    assert listing.status_code == 200, listing.text
    rows = {row["id"]: row for row in listing.json()}
    assert pod_id in rows, listing.text
    assert rows[pod_id]["project_identifier"] == project_identifier

    detail = user_client.get(f"{WEB}/pods/{pod_id}/")
    assert detail.status_code == 200, detail.text
    assert detail.json()["project_identifier"] == project_identifier

    with _admin_client(settings, seeder, daemon_world) as client:
        # description-only PATCH hits the `Pod.save()` denorm read (the pod's
        # `project` was never cached on this path).
        desc = web_patch(client, f"{WEB}/pods/{pod_id}/", {"description": "denorm"})
        assert desc.status_code == 200, desc.text
        assert desc.json()["project_identifier"] == project_identifier

        # rename PATCH hits the rename branch's direct `pod.project.identifier`.
        named = web_patch(client, f"{WEB}/pods/{pod_id}/", {"name": "renamed"})
        assert named.status_code == 200, named.text
        assert named.json()["project_identifier"] == project_identifier
        assert named.json()["name"] == f"{project_identifier}_renamed"
