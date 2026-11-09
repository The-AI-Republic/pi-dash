# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Runner LLM model catalog — serializer, public read, admin CRUD (PDASHOSS01-364).

Smallest focused coverage for the behavior change: validation of the
known agent / non-empty model+label / duplicate-in-agent / allowed
efforts, the grouped enabled-only public read, the admin CRUD behind
``InstanceAdminPermission`` (403 for non-admins), and the same-
transaction preselect clearing. Contract parity with the Rust port is
pinned by the same expectations in ``rust-api/contract-tests/runner``.
"""

from __future__ import annotations

import pytest
from django.utils import timezone
from rest_framework.test import APIClient

from pi_dash.db.models import User
from pi_dash.license.models import Instance, InstanceAdmin
from pi_dash.runner.models import RUNNER_MODEL_AGENTS, RunnerModelCatalog
from pi_dash.runner.serializers import RunnerModelCatalogSerializer

pytestmark = pytest.mark.django_db

CATALOG_URL = "/api/runners/model-catalog/"
ADMIN_LIST_URL = "/api/instances/runner-models/"


def _admin_user():
    user = User.objects.create(email="godmode@example.com", username="godmode", first_name="Root")
    user.set_password("x")
    user.save()
    instance = Instance.objects.create(
        instance_name="test", instance_id="i1", current_version="1.0.0", last_checked_at=timezone.now()
    )
    InstanceAdmin.objects.create(instance=instance, user=user, role=20, is_verified=True)
    return user


def _plain_user():
    user = User.objects.create(email="member@example.com", username="member", first_name="Member")
    user.set_password("x")
    user.save()
    return user


def _client(user) -> APIClient:
    client = APIClient()
    client.force_authenticate(user=user)
    return client


def _seed():
    RunnerModelCatalog.objects.create(
        agent="codex", model="gpt-5.5", label="GPT-5.5", reasoning_efforts=["low", "medium"], sort_order=0
    )
    RunnerModelCatalog.objects.create(
        agent="codex",
        model="gpt-5.4",
        label="GPT-5.4",
        reasoning_efforts=["low"],
        sort_order=1,
    )
    RunnerModelCatalog.objects.create(
        agent="claude-code",
        model="claude-opus-4-8",
        label="Opus 4.8",
        sort_order=0,
        is_preselected=True,
    )


def test_serializer_rejects_unknown_agent():
    s = RunnerModelCatalogSerializer(data={"agent": "nope", "model": "m", "label": "L"})
    assert not s.is_valid()
    assert "agent" in s.errors


def test_serializer_rejects_empty_model_and_label():
    s = RunnerModelCatalogSerializer(data={"agent": "codex", "model": "  ", "label": "L"})
    assert not s.is_valid()
    assert "model" in s.errors
    s2 = RunnerModelCatalogSerializer(data={"agent": "codex", "model": "m", "label": ""})
    assert not s2.is_valid()
    assert "label" in s2.errors


def test_serializer_rejects_bad_efforts():
    s = RunnerModelCatalogSerializer(
        data={"agent": "codex", "model": "m", "label": "L", "reasoning_efforts": ["low", "bogus"]}
    )
    assert not s.is_valid()
    assert "reasoning_efforts" in s.errors
    s_dup = RunnerModelCatalogSerializer(
        data={"agent": "codex", "model": "m", "label": "L", "reasoning_efforts": ["low", "low"]}
    )
    assert not s_dup.is_valid()


def test_serializer_rejects_duplicate_model_in_agent(db):
    RunnerModelCatalog.objects.create(agent="codex", model="gpt-5.5", label="GPT-5.5")
    s = RunnerModelCatalogSerializer(data={"agent": "codex", "model": "gpt-5.5", "label": "Dup"})
    assert not s.is_valid()
    # DRF's UniqueTogetherValidator (from the DB constraint) reports
    # under non_field_errors; a custom field error is also acceptable.
    assert "model" in s.errors or "non_field_errors" in s.errors
    # Same slug under a different agent is fine.
    ok = RunnerModelCatalogSerializer(data={"agent": "grok", "model": "gpt-5.5", "label": "Grok slug"})
    assert ok.is_valid()


def test_public_catalog_groups_and_filters_disabled(db, create_user):
    _seed()
    RunnerModelCatalog.objects.create(
        agent="grok", model="grok-4.3", label="Grok 4.3", sort_order=0, is_enabled=False
    )
    client = _client(create_user)
    res = client.get(CATALOG_URL)
    assert res.status_code == 200, res.content
    agents = res.data["agents"]
    assert set(agents.keys()) == set(RUNNER_MODEL_AGENTS)
    assert [r["model"] for r in agents["codex"]] == ["gpt-5.5", "gpt-5.4"]
    assert agents["grok"] == []
    assert agents["open-claw"] == []
    # Preselected flag rides through on the claude row.
    claude = agents["claude-code"][0]
    assert claude["is_preselected"] is True
    assert claude["reasoning_efforts"] == []


def test_public_catalog_requires_auth(db):
    assert APIClient().get(CATALOG_URL).status_code == 401


def test_admin_crud_and_preselect_clearing(db):
    admin = _admin_user()
    client = _client(admin)
    # Create two codex rows; second preselected clears nothing yet.
    r1 = client.post(
        ADMIN_LIST_URL,
        {"agent": "codex", "model": "gpt-5.5", "label": "GPT-5.5", "reasoning_efforts": ["low"]},
        format="json",
    )
    assert r1.status_code == 201, r1.content
    r2 = client.post(
        ADMIN_LIST_URL,
        {
            "agent": "codex",
            "model": "gpt-5.6-sol",
            "label": "GPT-5.6 Sol",
            "reasoning_efforts": ["low", "high"],
            "is_preselected": True,
            "sort_order": 1,
        },
        format="json",
    )
    assert r2.status_code == 201, r2.content
    # Patch first row to preselected: second must clear in same tx.
    detail = f"{ADMIN_LIST_URL}{r1.data['id']}/"
    patch = client.patch(detail, {"is_preselected": True}, format="json")
    assert patch.status_code == 200, patch.content
    assert patch.data["is_preselected"] is True
    # Only one preselected per agent.
    preselected = RunnerModelCatalog.objects.filter(agent="codex", is_preselected=True)
    assert preselected.count() == 1
    assert str(preselected.first().id) == str(r1.data["id"])
    # List shows full rows including disabled (none disabled here, ordered).
    listed = client.get(ADMIN_LIST_URL)
    assert listed.status_code == 200
    assert {row["model"] for row in listed.data} == {"gpt-5.5", "gpt-5.6-sol"}
    # Delete second row.
    deleted = client.delete(f"{ADMIN_LIST_URL}{r2.data['id']}/")
    assert deleted.status_code == 204
    assert not RunnerModelCatalog.objects.filter(pk=r2.data["id"]).exists()


def test_admin_validation_errors(db):
    admin = _admin_user()
    client = _client(admin)
    bad_agent = client.post(
        ADMIN_LIST_URL, {"agent": "unknown", "model": "m", "label": "L"}, format="json"
    )
    assert bad_agent.status_code == 400
    bad_effort = client.post(
        ADMIN_LIST_URL,
        {"agent": "codex", "model": "m", "label": "L", "reasoning_efforts": ["ultra"]},
        format="json",
    )
    assert bad_effort.status_code == 400
    # Duplicate within agent is 400, not 500.
    client.post(ADMIN_LIST_URL, {"agent": "codex", "model": "dup", "label": "D"}, format="json")
    dup = client.post(ADMIN_LIST_URL, {"agent": "codex", "model": "dup", "label": "D2"}, format="json")
    assert dup.status_code == 400


def test_admin_rejects_non_admin_and_anonymous(db):
    plain = _plain_user()
    # Authenticated non-admin: 403 on both list and detail.
    assert _client(plain).get(ADMIN_LIST_URL).status_code == 403
    row = RunnerModelCatalog.objects.create(agent="codex", model="m", label="L")
    assert _client(plain).delete(f"{ADMIN_LIST_URL}{row.id}/").status_code == 403
    assert _client(plain).patch(f"{ADMIN_LIST_URL}{row.id}/", {"label": "X"}, format="json").status_code == 403
    # Anonymous: 401.
    assert APIClient().get(ADMIN_LIST_URL).status_code == 401
    assert APIClient().post(ADMIN_LIST_URL, {"agent": "codex", "model": "m", "label": "L"}).status_code == 401


def test_detail_404_for_missing_row(db):
    admin = _admin_user()
    client = _client(admin)
    import uuid

    assert client.patch(f"{ADMIN_LIST_URL}{uuid.uuid4()}/", {"label": "X"}, format="json").status_code == 404
    assert client.delete(f"{ADMIN_LIST_URL}{uuid.uuid4()}/").status_code == 404
