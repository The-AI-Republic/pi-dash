# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the runner revoke + revive web endpoints.

These two endpoints close the gap left by ``RunnerInviteEndpoint``,
which only ever creates a brand-new Runner row. ``revoke`` keeps the
row visible while killing its credentials; ``revive`` mints a fresh
enrollment token on the same row so the operator can re-enroll the
daemon without losing pod / name / id continuity.
"""

from __future__ import annotations

import pytest

from pi_dash.runner.models import Pod, Runner, RunnerForceRefresh, RunnerStatus
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
def test_revive_404_for_unknown_runner(db, session_client):
    import uuid

    resp = session_client.post(f"/api/runners/{uuid.uuid4()}/revive/")
    assert resp.status_code == 404


@pytest.mark.unit
def test_revive_pending_mints_new_enrollment_token(
    db, session_client, pending_runner
):
    runner, original = pending_runner
    original_hash = runner.enrollment_token_hash
    resp = session_client.post(f"/api/runners/{runner.id}/revive/")
    assert resp.status_code == 201, resp.data
    new_token = resp.data["enrollment_token"]
    assert new_token and new_token != original.raw
    runner.refresh_from_db()
    assert runner.enrollment_token_hash != original_hash
    assert runner.enrollment_token_hash == tokens.hash_token(new_token)
    # Same row, same name — that's the whole point.
    assert resp.data["runner_id"] == str(runner.id)
    assert resp.data["name"] == runner.name


@pytest.mark.unit
def test_revive_revoked_runner_resets_state(
    db, api_client, session_client, enrolled_runner
):
    # Revoke first, then revive.
    revoke = session_client.post(f"/api/runners/{enrolled_runner.id}/revoke/")
    assert revoke.status_code == 200
    enrolled_runner.refresh_from_db()
    assert enrolled_runner.refresh_token_hash != ""
    assert enrolled_runner.revoked_at is not None

    resp = session_client.post(f"/api/runners/{enrolled_runner.id}/revive/")
    assert resp.status_code == 201, resp.data
    enrolled_runner.refresh_from_db()
    assert enrolled_runner.revoked_at is None
    assert enrolled_runner.revoked_reason == ""
    assert enrolled_runner.enrolled_at is None
    assert enrolled_runner.refresh_token_hash == ""
    assert enrolled_runner.refresh_token_generation == 0
    assert enrolled_runner.status == RunnerStatus.OFFLINE

    # And the freshly minted enrollment token actually works against the
    # public enroll endpoint — meaning the same row gets re-enrolled
    # rather than a new Runner being created.
    new_token = resp.data["enrollment_token"]
    enroll = api_client.post(
        "/api/v1/runner/runners/enroll/",
        {"enrollment_token": new_token, "host_label": "host-2"},
        format="json",
    )
    assert enroll.status_code == 201, enroll.data
    assert enroll.data["runner_id"] == str(enrolled_runner.id)


@pytest.mark.unit
def test_revive_rejects_active_runner(db, session_client, enrolled_runner):
    resp = session_client.post(f"/api/runners/{enrolled_runner.id}/revive/")
    assert resp.status_code == 409, resp.data


@pytest.mark.unit
def test_revive_clears_force_refresh_directive(
    db, session_client, enrolled_runner
):
    RunnerForceRefresh.objects.create(runner=enrolled_runner, min_rtg=99)
    session_client.post(f"/api/runners/{enrolled_runner.id}/revoke/")
    resp = session_client.post(f"/api/runners/{enrolled_runner.id}/revive/")
    assert resp.status_code == 201
    assert not RunnerForceRefresh.objects.filter(runner=enrolled_runner).exists()


@pytest.mark.unit
def test_revoke_forbidden_for_non_owner_non_admin(
    db, api_client, create_user, enrolled_runner
):
    from pi_dash.db.models import User

    other = User.objects.create(email="other@example.com")
    other.set_password("x")
    other.save()
    api_client.force_authenticate(user=other)
    resp = api_client.post(f"/api/runners/{enrolled_runner.id}/revoke/")
    assert resp.status_code == 403


@pytest.mark.unit
def test_revive_forbidden_for_non_owner_non_admin(
    db, api_client, pending_runner
):
    from pi_dash.db.models import User

    runner, _ = pending_runner
    other = User.objects.create(email="other2@example.com")
    other.set_password("x")
    other.save()
    api_client.force_authenticate(user=other)
    resp = api_client.post(f"/api/runners/{runner.id}/revive/")
    assert resp.status_code == 403
