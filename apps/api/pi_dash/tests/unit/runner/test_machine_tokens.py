# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the machine-token endpoints and MachineToken.revoke().

Covers:
- Authorization on POST /api/runners/machine-tokens/ — only members of
  the target workspace may mint a token in it.
- UUID validation on the workspace field.
- Revocation cascade: MachineToken.revoke() flips revoked_at, revokes
  every owned runner, and force-closes any active WS sessions joined
  to those runners' pubsub groups.
"""

from __future__ import annotations

import uuid
from unittest.mock import patch

import pytest
from django.urls import reverse
from django.utils import timezone

from pi_dash.db.models import User, Workspace, WorkspaceMember
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    MachineToken,
    Pod,
    Runner,
    RunnerStatus,
)
from pi_dash.runner.services import tokens


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def other_user(db):
    """A second user with no membership in the primary `workspace` fixture."""
    user = User.objects.create(
        email="other@example.com",
        first_name="Other",
        last_name="User",
    )
    user.set_password("x")
    user.save()
    return user


@pytest.fixture
def other_workspace(other_user):
    ws = Workspace.objects.create(
        name="Other Workspace",
        owner=other_user,
        slug="other-workspace",
    )
    WorkspaceMember.objects.create(workspace=ws, member=other_user, role=20)
    return ws


@pytest.fixture
def pod(workspace):
    return Pod.default_for_workspace(workspace)


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    with patch("pi_dash.runner.services.pubsub.send_to_runner"):
        yield


@pytest.fixture(autouse=True)
def _on_commit_immediate():
    with patch(
        "django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()
    ):
        yield


# ---------------------------------------------------------------------------
# POST /api/runners/machine-tokens/ — workspace authorization
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_create_token_rejects_non_member_workspace(
    db, session_client, other_workspace
):
    """A user who is not a member of the target workspace must not be
    able to mint a token in it. Without this check, any authenticated
    user could create a token in any workspace they could guess the
    UUID of and use it to register runners — full takeover.
    """
    url = reverse("machine-token-list")
    resp = session_client.post(
        url,
        {"title": "laptop", "workspace": str(other_workspace.id)},
        format="json",
    )
    assert resp.status_code == 404, resp.content
    assert MachineToken.objects.count() == 0


@pytest.mark.unit
def test_create_token_succeeds_for_member(db, session_client, workspace):
    url = reverse("machine-token-list")
    resp = session_client.post(
        url,
        {"title": "laptop", "workspace": str(workspace.id)},
        format="json",
    )
    assert resp.status_code == 201, resp.content
    body = resp.json()
    assert body["title"] == "laptop"
    assert "secret" in body  # Plaintext returned exactly once at creation.
    token = MachineToken.objects.get(id=body["token_id"])
    assert token.workspace_id == workspace.id


@pytest.mark.unit
def test_create_token_rejects_malformed_workspace_uuid(db, session_client):
    url = reverse("machine-token-list")
    resp = session_client.post(
        url,
        {"title": "laptop", "workspace": "not-a-uuid"},
        format="json",
    )
    assert resp.status_code == 400, resp.content
    assert MachineToken.objects.count() == 0


@pytest.mark.unit
def test_create_token_requires_title(db, session_client, workspace):
    url = reverse("machine-token-list")
    resp = session_client.post(
        url, {"title": "", "workspace": str(workspace.id)}, format="json"
    )
    assert resp.status_code == 400


# ---------------------------------------------------------------------------
# MachineToken.revoke() — closes WS sessions for owned runners
# ---------------------------------------------------------------------------


def _make_runner(user, workspace, pod, machine_token, name="r1"):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        machine_token=machine_token,
        name=name,
        credential_hash=f"h-{name}",
        credential_fingerprint=name[:16].ljust(16, "x")[:16],
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.mark.unit
def test_revoke_token_closes_ws_for_each_owned_runner(
    db, create_user, workspace, pod
):
    """Token revoke must force-close active WS sessions for every owned
    runner. Without this, a daemon authenticated under the revoked token
    keeps full connectivity until the next reconnect — defeating the
    point of revocation.
    """
    minted = tokens.mint_machine_token_secret()
    token = MachineToken.objects.create(
        workspace=workspace,
        created_by=create_user,
        title="t",
        secret_hash=minted.hashed,
        secret_fingerprint=minted.fingerprint,
    )
    r1 = _make_runner(create_user, workspace, pod, token, name="a")
    r2 = _make_runner(create_user, workspace, pod, token, name="b")

    with patch(
        "pi_dash.runner.services.pubsub.close_runner_session"
    ) as mock_close:
        token.revoke()

    closed_ids = {call.args[0] for call in mock_close.call_args_list}
    assert closed_ids == {r1.id, r2.id}, closed_ids


@pytest.mark.unit
def test_revoke_token_marks_revoked_and_cascades(
    db, create_user, workspace, pod
):
    minted = tokens.mint_machine_token_secret()
    token = MachineToken.objects.create(
        workspace=workspace,
        created_by=create_user,
        title="t",
        secret_hash=minted.hashed,
        secret_fingerprint=minted.fingerprint,
    )
    r1 = _make_runner(create_user, workspace, pod, token, name="a")
    in_flight = AgentRun.objects.create(
        workspace=workspace,
        owner=create_user,
        created_by=create_user,
        pod=pod,
        runner=r1,
        status=AgentRunStatus.RUNNING,
        prompt="x",
    )

    with patch("pi_dash.runner.services.pubsub.close_runner_session"):
        token.revoke()

    token.refresh_from_db()
    r1.refresh_from_db()
    in_flight.refresh_from_db()
    assert token.revoked_at is not None
    assert r1.status == RunnerStatus.REVOKED
    assert in_flight.status == AgentRunStatus.CANCELLED


@pytest.mark.unit
def test_revoke_token_is_idempotent(db, create_user, workspace, pod):
    minted = tokens.mint_machine_token_secret()
    token = MachineToken.objects.create(
        workspace=workspace,
        created_by=create_user,
        title="t",
        secret_hash=minted.hashed,
        secret_fingerprint=minted.fingerprint,
        revoked_at=timezone.now(),
    )
    with patch(
        "pi_dash.runner.services.pubsub.close_runner_session"
    ) as mock_close:
        token.revoke()
    # Second revoke is a no-op — no session closes fired.
    mock_close.assert_not_called()
