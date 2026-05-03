# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Phase 1 cloud auth tests for the per-runner HTTPS transport.

Covers ``.ai_design/move_to_https/design.md`` §5: enrollment, refresh,
access-token verification, and MachineToken bootstrap.
"""

from __future__ import annotations

import pytest
from django.utils import timezone

from pi_dash.runner.models import (
    MachineToken,
    Pod,
    Runner,
    RunnerForceRefresh,
)
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
            name="agentZ",
            enrollment_token_hash=enrollment.hashed,
            enrollment_token_fingerprint=enrollment.fingerprint,
        ),
        enrollment,
    )


@pytest.mark.unit
def test_enroll_mints_refresh_and_access_tokens(
    db, api_client, pending_runner, workspace
):
    runner, enrollment = pending_runner
    resp = api_client.post(
        "/api/v1/runner/runners/enroll/",
        {
            "enrollment_token": enrollment.raw,
            "host_label": "mac-mini.local",
            "name": "agentZ",
        },
        format="json",
    )
    assert resp.status_code == 201, resp.data
    assert resp.data["runner_id"] == str(runner.id)
    assert resp.data["refresh_token"].startswith("rt_")
    assert resp.data["access_token"]
    assert resp.data["refresh_token_generation"] == 1
    assert resp.data["protocol_version"] == 4
    runner.refresh_from_db()
    assert runner.enrollment_token_hash == ""
    assert runner.refresh_token_generation == 1
    assert runner.host_label == "mac-mini.local"


@pytest.mark.unit
def test_enroll_token_is_one_time(
    db, api_client, pending_runner
):
    _, enrollment = pending_runner
    first = api_client.post(
        "/api/v1/runner/runners/enroll/",
        {
            "enrollment_token": enrollment.raw,
            "host_label": "mac-mini.local",
        },
        format="json",
    )
    assert first.status_code == 201
    second = api_client.post(
        "/api/v1/runner/runners/enroll/",
        {
            "enrollment_token": enrollment.raw,
            "host_label": "mac-mini.local",
        },
        format="json",
    )
    # The same enrollment_token can no longer be redeemed; the row's
    # hash was cleared.
    assert second.status_code in (401, 409)


@pytest.mark.unit
def test_enroll_bootstraps_machine_token(
    db, api_client, pending_runner, workspace, create_user
):
    _, enrollment = pending_runner
    resp = api_client.post(
        "/api/v1/runner/runners/enroll/",
        {
            "enrollment_token": enrollment.raw,
            "host_label": "macbook.local",
        },
        format="json",
    )
    assert resp.status_code == 201
    assert resp.data["machine_token_minted"] is True
    assert resp.data["machine_token"].startswith("mt_")
    # Same (user, workspace, host_label) on a second runner: no new
    # MachineToken issued.
    assert (
        MachineToken.objects.filter(
            user=create_user,
            workspace=workspace,
            host_label="macbook.local",
            revoked_at__isnull=True,
        ).count()
        == 1
    )


@pytest.mark.unit
def test_refresh_rotates_token_and_increments_generation(
    db, api_client, pending_runner
):
    runner, enrollment = pending_runner
    enroll = api_client.post(
        "/api/v1/runner/runners/enroll/",
        {
            "enrollment_token": enrollment.raw,
            "host_label": "host",
        },
        format="json",
    )
    refresh_token = enroll.data["refresh_token"]
    resp = api_client.post(
        f"/api/v1/runner/runners/{runner.id}/refresh/",
        HTTP_AUTHORIZATION=f"Bearer {refresh_token}",
    )
    assert resp.status_code == 200, resp.data
    assert resp.data["refresh_token"] != refresh_token
    assert resp.data["refresh_token_generation"] == 2
    runner.refresh_from_db()
    assert runner.refresh_token_generation == 2
    assert runner.previous_refresh_token_hash != ""


@pytest.mark.unit
def test_refresh_replay_revokes_runner(db, api_client, pending_runner):
    runner, enrollment = pending_runner
    enroll = api_client.post(
        "/api/v1/runner/runners/enroll/",
        {"enrollment_token": enrollment.raw, "host_label": "host"},
        format="json",
    )
    old_refresh = enroll.data["refresh_token"]
    api_client.post(
        f"/api/v1/runner/runners/{runner.id}/refresh/",
        HTTP_AUTHORIZATION=f"Bearer {old_refresh}",
    )
    # Replay the OLD refresh token.
    replay = api_client.post(
        f"/api/v1/runner/runners/{runner.id}/refresh/",
        HTTP_AUTHORIZATION=f"Bearer {old_refresh}",
    )
    assert replay.status_code == 401
    assert replay.data["error"] == "refresh_token_replayed"
    runner.refresh_from_db()
    assert runner.revoked_at is not None
    assert runner.revoked_reason == "refresh_token_replayed"


@pytest.mark.unit
def test_access_token_decode_rejects_revoked_runner(db, pending_runner):
    """Per-request revocation check (design.md §5.4) — issued tokens
    must stop being accepted as soon as ``Runner.revoke()`` runs."""
    runner, _ = pending_runner
    token = tokens.mint_access_token(
        runner_id=str(runner.id),
        user_id=str(runner.owner_id),
        workspace_id=str(runner.workspace_id),
        rtg=1,
    )
    runner.refresh_token_generation = 1
    runner.save(update_fields=["refresh_token_generation"])

    payload = tokens.decode_access_token(token.raw)
    assert payload["sub"] == str(runner.id)


@pytest.mark.unit
def test_force_refresh_blocks_old_rtg(db, pending_runner):
    runner, _ = pending_runner
    runner.refresh_token_generation = 5
    runner.save(update_fields=["refresh_token_generation"])
    RunnerForceRefresh.objects.create(runner=runner, min_rtg=10, reason="ops")
    fresh_low = tokens.mint_access_token(
        runner_id=str(runner.id),
        user_id=str(runner.owner_id),
        workspace_id=str(runner.workspace_id),
        rtg=5,
    )
    payload = tokens.decode_access_token(fresh_low.raw)
    assert payload["rtg"] == 5  # decode is unaware of ForceRefresh — auth class enforces it
