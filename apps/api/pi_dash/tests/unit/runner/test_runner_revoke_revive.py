# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for runner revoke and disabled legacy revive endpoints."""

from __future__ import annotations

import pytest

from pi_dash.runner.models import Pod, Runner, RunnerStatus
from pi_dash.runner.services import tokens


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def pending_runner(db, create_user, workspace, pod):
    enrollment = tokens.mint_enrollment_token()
    return (
        Runner.objects.create(
            owner=create_user,
            workspace=workspace,
            pod=pod,
            name="agent-x",
            enrollment_token_hash=enrollment.hashed,
            enrollment_token_fingerprint=enrollment.fingerprint,
        ),
        enrollment,
    )


@pytest.fixture
def enrolled_runner(db, api_client, pending_runner):
    runner, enrollment = pending_runner
    resp = api_client.post(
        "/api/v1/runner/runners/enroll/",
        {"enrollment_token": enrollment.raw, "host_label": "host"},
        format="json",
    )
    assert resp.status_code == 201, resp.data
    runner.refresh_from_db()
    return runner


@pytest.mark.unit
def test_revoke_keeps_row_and_marks_revoked(
    db, session_client, enrolled_runner
):
    resp = session_client.post(f"/api/runners/{enrolled_runner.id}/revoke/")
    assert resp.status_code == 200, resp.data
    assert resp.data["status"] == RunnerStatus.REVOKED
    assert resp.data["revoked_at"] is not None
    # Row not deleted — historic context preserved.
    assert Runner.objects.filter(pk=enrolled_runner.id).exists()


@pytest.mark.unit
def test_revoke_is_idempotent(db, session_client, enrolled_runner):
    first = session_client.post(f"/api/runners/{enrolled_runner.id}/revoke/")
    assert first.status_code == 200
    second = session_client.post(f"/api/runners/{enrolled_runner.id}/revoke/")
    assert second.status_code == 200
    assert second.data["status"] == RunnerStatus.REVOKED


@pytest.mark.unit
def test_revoke_404_for_unknown_runner(db, session_client):
    import uuid

    resp = session_client.post(f"/api/runners/{uuid.uuid4()}/revoke/")
    assert resp.status_code == 404


@pytest.mark.unit
def test_invite_endpoint_is_disabled(db, session_client, workspace, project):
    resp = session_client.post(
        "/api/runners/invites/",
        {"workspace": str(workspace.id), "project": project.identifier},
        format="json",
    )
    assert resp.status_code == 410, resp.data
    assert resp.data["error"] == "legacy_enrollment_disabled"


@pytest.mark.unit
def test_revive_endpoint_is_disabled_for_pending_runner(
    db, session_client, pending_runner
):
    runner, original = pending_runner
    resp = session_client.post(f"/api/runners/{runner.id}/revive/")
    assert resp.status_code == 410, resp.data
    assert resp.data["error"] == "legacy_enrollment_disabled"
    runner.refresh_from_db()
    assert runner.enrollment_token_hash == original.hashed


@pytest.mark.unit
def test_revive_endpoint_is_disabled_for_revoked_runner(
    db, session_client, enrolled_runner
):
    revoke = session_client.post(f"/api/runners/{enrolled_runner.id}/revoke/")
    assert revoke.status_code == 200
    enrolled_runner.refresh_from_db()
    assert enrolled_runner.revoked_at is not None

    resp = session_client.post(f"/api/runners/{enrolled_runner.id}/revive/")
    assert resp.status_code == 410, resp.data
    assert resp.data["error"] == "legacy_enrollment_disabled"
    enrolled_runner.refresh_from_db()
    assert enrolled_runner.revoked_at is not None


@pytest.mark.unit
def test_revoke_forbidden_for_non_owner_non_admin(
    db, api_client, create_user, enrolled_runner
):
    from pi_dash.db.models import User

    # ``username`` is unique; ``create_user`` already produced a user
    # with the default empty username, so this second one must pick
    # its own to avoid a unique-constraint collision.
    other = User.objects.create_user(
        email="other-revoke-forbidden@example.com",
        username="other-revoke-forbidden",
    )
    api_client.force_authenticate(user=other)
    resp = api_client.post(f"/api/runners/{enrolled_runner.id}/revoke/")
    assert resp.status_code == 403


@pytest.mark.unit
def test_revive_endpoint_is_disabled_for_unknown_runner(db, session_client):
    import uuid

    resp = session_client.post(f"/api/runners/{uuid.uuid4()}/revive/")
    assert resp.status_code == 410, resp.data
    assert resp.data["error"] == "legacy_enrollment_disabled"
