# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Instance-admin loop API — permission gate, CRUD, RRULE validation."""

from __future__ import annotations

import pytest
from django.utils import timezone
from rest_framework.test import APIClient

from pi_dash.db.models import LoopJob
from pi_dash.license.models import Instance, InstanceAdmin
from pi_dash.tests.contract.loop.conftest import make_job

pytestmark = pytest.mark.django_db

JOBS_URL = "/api/instances/loop/jobs/"


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


def test_non_admin_blocked(world):
    c = _client(world.member)
    assert c.get(JOBS_URL).status_code == 403


def test_admin_can_list(world, instance_admin):
    make_job(slug="j1")
    c = _client(instance_admin)
    res = c.get(JOBS_URL)
    assert res.status_code == 200
    assert any(j["slug"] == "j1" for j in res.data)
    # Admin surface exposes the full job, including the prompt.
    assert "prompt" in res.data[0]


def test_create_job(world, instance_admin):
    c = _client(instance_admin)
    res = c.post(
        JOBS_URL,
        {
            "slug": "new-job",
            "name": "New",
            "public_name": "New public",
            "public_description": "d",
            "prompt": "do it",
            "min_role": 15,
            "rrule": "FREQ=DAILY;BYHOUR=2",
            "tzid": "UTC",
        },
        format="json",
    )
    assert res.status_code == 201
    assert res.data["is_builtin"] is False
    assert LoopJob.objects.filter(slug="new-job").exists()


def test_create_rejects_subhourly_rrule(world, instance_admin):
    c = _client(instance_admin)
    res = c.post(
        JOBS_URL,
        {
            "slug": "fast",
            "name": "Fast",
            "public_name": "Fast",
            "prompt": "p",
            "rrule": "FREQ=MINUTELY",
        },
        format="json",
    )
    assert res.status_code == 400
    assert res.data["error"] == "rrule_too_frequent"


def test_create_rejects_bad_slug(world, instance_admin):
    c = _client(instance_admin)
    res = c.post(
        JOBS_URL,
        {"slug": "Bad Slug", "name": "n", "public_name": "n", "prompt": "p", "rrule": "FREQ=DAILY"},
        format="json",
    )
    assert res.status_code == 400
    assert res.data["error"] == "invalid_slug"


def test_patch_cannot_change_is_builtin(world, instance_admin):
    job = make_job(slug="b", is_builtin=True)
    c = _client(instance_admin)
    res = c.patch(f"{JOBS_URL}{job.id}/", {"is_builtin": False, "enabled": False}, format="json")
    assert res.status_code == 200
    job.refresh_from_db()
    assert job.is_builtin is True  # immutable
    assert job.enabled is False


def test_delete_soft_deletes(world, instance_admin):
    job = make_job(slug="del")
    c = _client(instance_admin)
    assert c.delete(f"{JOBS_URL}{job.id}/").status_code == 204
    assert not LoopJob.objects.filter(slug="del", deleted_at__isnull=True).exists()


def test_targets_listing(world, instance_admin, kms_crypto):
    from pi_dash.tests.contract.assistant.conftest import configure_llm
    from pi_dash.tests.contract.loop.conftest import make_target

    job = make_job(slug="t")
    configure_llm(world.member)
    make_target(job, world.ws, world.member)
    c = _client(instance_admin)
    res = c.get(f"{JOBS_URL}{job.id}/targets/")
    assert res.status_code == 200
    assert len(res.data["results"]) == 1
    assert res.data["results"][0]["workspace_slug"] == world.ws.slug
