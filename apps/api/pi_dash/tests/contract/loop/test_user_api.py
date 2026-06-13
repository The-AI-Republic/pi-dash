# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""User-facing Auto PM settings API — whitelist shape, toggles, precedence."""

from __future__ import annotations

import pytest
from rest_framework.test import APIClient

from pi_dash.db.models import LoopUserPreference
from pi_dash.tests.contract.loop.conftest import make_job

pytestmark = pytest.mark.django_db

SETTINGS_URL = "/api/users/me/auto-pm/"


def _client(user):
    c = APIClient()
    c.force_authenticate(user=user)
    return c


def test_get_shape_whitelists_keys(world):
    make_job(slug="j1", public_name="Job One", prompt="SECRET PROMPT", min_role=20)
    c = _client(world.member)
    res = c.get(SETTINGS_URL)
    assert res.status_code == 200
    assert res.data["enabled"] is True
    job = res.data["jobs"][0]
    assert set(job.keys()) == {"slug", "name", "description", "interval_label", "enabled"}
    # The prompt / admin name / min_role must never leak.
    assert "SECRET" not in str(res.data)
    assert "min_role" not in job
    assert job["name"] == "Job One"  # public_name, not admin name


def test_disabled_jobs_excluded(world):
    make_job(slug="on", enabled=True)
    make_job(slug="off", enabled=False)
    c = _client(world.member)
    res = c.get(SETTINGS_URL)
    slugs = {j["slug"] for j in res.data["jobs"]}
    assert slugs == {"on"}


def test_master_pause_toggle(world):
    make_job(slug="j1")
    c = _client(world.member)
    res = c.patch(SETTINGS_URL, {"enabled": False}, format="json")
    assert res.status_code == 200
    assert res.data["enabled"] is False
    assert LoopUserPreference.objects.filter(user=world.member, job__isnull=True, enabled=False).exists()


def test_per_job_toggle(world):
    make_job(slug="j1")
    c = _client(world.member)
    res = c.patch(f"{SETTINGS_URL}jobs/j1/", {"enabled": False}, format="json")
    assert res.status_code == 200
    job = next(j for j in res.data["jobs"] if j["slug"] == "j1")
    assert job["enabled"] is False
    # Toggling again flips it back (upsert, not duplicate rows).
    res = c.patch(f"{SETTINGS_URL}jobs/j1/", {"enabled": True}, format="json")
    job = next(j for j in res.data["jobs"] if j["slug"] == "j1")
    assert job["enabled"] is True
    assert LoopUserPreference.objects.filter(user=world.member, job__slug="j1").count() == 1


def test_invalid_payload_rejected(world):
    make_job(slug="j1")
    c = _client(world.member)
    assert c.patch(SETTINGS_URL, {"enabled": "yes"}, format="json").status_code == 400
    assert c.patch(SETTINGS_URL, {"foo": True}, format="json").status_code == 400


def test_unknown_job_404(world):
    c = _client(world.member)
    assert c.patch(f"{SETTINGS_URL}jobs/nope/", {"enabled": False}, format="json").status_code == 404


def test_guest_can_still_toggle(world):
    # No workspace-role gate — preferences are the user's own.
    make_job(slug="j1")
    c = _client(world.guest)
    assert c.get(SETTINGS_URL).status_code == 200
    assert c.patch(f"{SETTINGS_URL}jobs/j1/", {"enabled": False}, format="json").status_code == 200
