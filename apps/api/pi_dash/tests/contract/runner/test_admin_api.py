# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Instance-admin runner list — permission gate, payload, provisioning filter.

Covers .ai_design/managed_runner/design.md §15.1 / §23 item 8: a support
operator can see per-runner ``provisioning``, ``runner_version`` and
``dev_metadata.codex_version``.
"""

from __future__ import annotations

from datetime import timedelta

import pytest
from django.utils import timezone
from rest_framework.test import APIClient

from pi_dash.license.models import Instance, InstanceAdmin
from pi_dash.runner.models import Pod, Runner, RunnerProvisioning, RunnerStatus

pytestmark = pytest.mark.django_db

RUNNERS_URL = "/api/instances/runners/"


@pytest.fixture
def instance_admin(world):
    instance = Instance.objects.create(
        instance_name="test",
        instance_id="i1",
        current_version="1.0.0",
        last_checked_at=timezone.now(),
    )
    InstanceAdmin.objects.create(instance=instance, user=world.admin, role=20, is_verified=True)
    return world.admin


def _client(user):
    c = APIClient()
    c.force_authenticate(user=user)
    return c


def make_runner(world, *, name, provisioning=RunnerProvisioning.MANUAL, dev_metadata=None, runner_version=""):
    return Runner.objects.create(
        owner=world.admin,
        workspace=world.ws,
        pod=Pod.default_for_project(world.proj_a),
        name=name,
        provisioning=provisioning,
        dev_metadata=dev_metadata or {},
        runner_version=runner_version,
    )


def test_non_admin_blocked(world):
    assert _client(world.member).get(RUNNERS_URL).status_code == 403


def test_anonymous_blocked(world):
    assert APIClient().get(RUNNERS_URL).status_code in (401, 403)


def test_admin_sees_versions(world, instance_admin):
    make_runner(
        world,
        name="bundled",
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
        dev_metadata={"codex_version": "rust-v0.153.4"},
        runner_version="1.2.3",
    )
    make_runner(world, name="manual")  # no codex_version reported

    res = _client(instance_admin).get(RUNNERS_URL)
    assert res.status_code == 200
    rows = {r["name"]: r for r in res.data["results"]}
    assert rows["bundled"]["provisioning"] == "desktop_bundled"
    assert rows["bundled"]["runner_version"] == "1.2.3"
    assert rows["bundled"]["codex_version"] == "rust-v0.153.4"
    assert rows["bundled"]["workspace_slug"] == world.ws.slug
    assert rows["bundled"]["owner_email"] == world.admin.email
    # Absent codex_version comes back as null (rendered as "—" in the UI).
    assert rows["manual"]["codex_version"] is None


def test_provisioning_filter(world, instance_admin):
    make_runner(world, name="bundled", provisioning=RunnerProvisioning.DESKTOP_BUNDLED)
    make_runner(world, name="manual")

    res = _client(instance_admin).get(RUNNERS_URL, {"provisioning": "desktop_bundled"})
    assert res.status_code == 200
    assert [r["name"] for r in res.data["results"]] == ["bundled"]
    assert res.data["total"] == 1


def test_invalid_provisioning_rejected(world, instance_admin):
    res = _client(instance_admin).get(RUNNERS_URL, {"provisioning": "bogus"})
    assert res.status_code == 400
    assert res.data["error"] == "invalid_provisioning"


def test_invalid_status_rejected(world, instance_admin):
    res = _client(instance_admin).get(RUNNERS_URL, {"status": "bogus"})
    assert res.status_code == 400
    assert res.data["error"] == "invalid_status"


def test_empty_filter_values_rejected(world, instance_admin):
    # ?provisioning= / ?status= (present but empty) is a 400, not "no filter".
    res = _client(instance_admin).get(RUNNERS_URL + "?provisioning=")
    assert res.status_code == 400
    assert res.data["error"] == "invalid_provisioning"
    res = _client(instance_admin).get(RUNNERS_URL + "?status=")
    assert res.status_code == 400
    assert res.data["error"] == "invalid_status"


def test_workspace_filter(world, instance_admin):
    make_runner(world, name="in-ws")
    res = _client(instance_admin).get(RUNNERS_URL, {"workspace": world.ws.slug})
    assert [r["name"] for r in res.data["results"]] == ["in-ws"]
    res = _client(instance_admin).get(RUNNERS_URL, {"workspace": "no-such-ws"})
    assert res.data["results"] == []


def test_status_filter(world, instance_admin):
    make_runner(world, name="offline")
    revoked = make_runner(world, name="revoked")
    Runner.objects.filter(pk=revoked.pk).update(status=RunnerStatus.REVOKED)
    res = _client(instance_admin).get(RUNNERS_URL, {"status": "revoked"})
    assert res.status_code == 200
    assert [r["name"] for r in res.data["results"]] == ["revoked"]
    assert res.data["total"] == 1


def test_newest_first(world, instance_admin):
    older = make_runner(world, name="older")
    make_runner(world, name="newer")
    Runner.objects.filter(pk=older.pk).update(created_at=timezone.now() - timedelta(days=1))
    res = _client(instance_admin).get(RUNNERS_URL)
    assert [r["name"] for r in res.data["results"]] == ["newer", "older"]


def test_pagination_50_per_page(world, instance_admin):
    for i in range(51):
        make_runner(world, name=f"r{i:02d}")
    res = _client(instance_admin).get(RUNNERS_URL)
    assert res.data["page"] == 1
    assert res.data["total"] == 51
    assert len(res.data["results"]) == 50
    res = _client(instance_admin).get(RUNNERS_URL, {"page": "2"})
    assert res.data["page"] == 2
    assert len(res.data["results"]) == 1


@pytest.mark.parametrize("page", ["abc", "", "0", "-3"])
def test_bad_page_falls_back_to_first(world, instance_admin, page):
    make_runner(world, name="only")
    res = _client(instance_admin).get(RUNNERS_URL + f"?page={page}")
    assert res.status_code == 200
    assert res.data["page"] == 1
    assert [r["name"] for r in res.data["results"]] == ["only"]
