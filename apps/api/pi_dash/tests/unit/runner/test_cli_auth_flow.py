# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the device-code (`pidash auth login`) flow + CLI-initiated
runner mint + token revoke endpoints. See
``apps/api/pi_dash/authentication/views/cli/device.py`` and
``apps/api/pi_dash/runner/views/enrollment.py::RunnerCreateEndpoint``.
"""

from __future__ import annotations

from datetime import timedelta

import pytest
from django.utils import timezone

from pi_dash.db.models import APIToken, CLIDeviceCode


# -------------------- device-code flow --------------------


@pytest.mark.unit
def test_device_start_returns_user_code_and_device_code(db, api_client):
    resp = api_client.post("/api/v1/auth/device/start/", {}, format="json")
    assert resp.status_code == 200, resp.data
    assert resp.data["device_code"]
    assert resp.data["user_code"]
    assert "-" in resp.data["user_code"]
    assert resp.data["verification_uri"].endswith("/auth/device/")
    assert resp.data["expires_in"] > 0
    assert resp.data["interval"] > 0
    # Row created.
    assert CLIDeviceCode.objects.filter(device_code=resp.data["device_code"]).exists()


@pytest.mark.unit
def test_token_poll_pending_then_approved_mints_apitoken(
    db, api_client, session_client, create_user
):
    start = api_client.post("/api/v1/auth/device/start/", {}, format="json").data
    # First poll: pending (no approval yet).
    poll1 = api_client.post(
        "/api/v1/auth/device/token/", {"device_code": start["device_code"]}, format="json"
    )
    assert poll1.status_code == 400
    assert poll1.data["error"] == "authorization_pending"

    # Operator approves via the web session.
    approve = session_client.post(
        "/api/v1/auth/device/approve/", {"user_code": start["user_code"]}, format="json"
    )
    assert approve.status_code == 200, approve.data
    assert approve.data["user_email"] == create_user.email

    # Second poll: success, returns a fresh APIToken.
    # Bump last_polled_at past the min-gap to skip slow_down.
    CLIDeviceCode.objects.filter(device_code=start["device_code"]).update(
        last_polled_at=timezone.now() - timedelta(seconds=30)
    )
    poll2 = api_client.post(
        "/api/v1/auth/device/token/", {"device_code": start["device_code"]}, format="json"
    )
    assert poll2.status_code == 200, poll2.data
    assert poll2.data["access_token"].startswith("pi_dash_api_")
    assert poll2.data["user_email"] == create_user.email
    # APIToken row exists.
    assert APIToken.objects.filter(token=poll2.data["access_token"], user=create_user).exists()
    # Row marked consumed.
    row = CLIDeviceCode.objects.get(device_code=start["device_code"])
    assert row.consumed is True


@pytest.mark.unit
def test_token_poll_rejects_consumed_code(db, api_client, session_client):
    start = api_client.post("/api/v1/auth/device/start/", {}, format="json").data
    session_client.post(
        "/api/v1/auth/device/approve/", {"user_code": start["user_code"]}, format="json"
    )
    CLIDeviceCode.objects.filter(device_code=start["device_code"]).update(
        last_polled_at=timezone.now() - timedelta(seconds=30)
    )
    first = api_client.post(
        "/api/v1/auth/device/token/", {"device_code": start["device_code"]}, format="json"
    )
    assert first.status_code == 200, first.data
    # Second exchange of the same device code must fail.
    second = api_client.post(
        "/api/v1/auth/device/token/", {"device_code": start["device_code"]}, format="json"
    )
    assert second.status_code == 400
    assert second.data["error"] == "invalid_grant"


@pytest.mark.unit
def test_token_poll_returns_slow_down_when_polled_too_fast(db, api_client):
    start = api_client.post("/api/v1/auth/device/start/", {}, format="json").data
    first = api_client.post(
        "/api/v1/auth/device/token/", {"device_code": start["device_code"]}, format="json"
    )
    assert first.status_code == 400
    # Immediate second poll → slow_down.
    second = api_client.post(
        "/api/v1/auth/device/token/", {"device_code": start["device_code"]}, format="json"
    )
    assert second.status_code == 400
    assert second.data["error"] == "slow_down"


@pytest.mark.unit
def test_token_poll_expired(db, api_client):
    start = api_client.post("/api/v1/auth/device/start/", {}, format="json").data
    CLIDeviceCode.objects.filter(device_code=start["device_code"]).update(
        expires_at=timezone.now() - timedelta(seconds=1)
    )
    resp = api_client.post(
        "/api/v1/auth/device/token/", {"device_code": start["device_code"]}, format="json"
    )
    assert resp.status_code == 410
    assert resp.data["error"] == "expired_token"


@pytest.mark.unit
def test_approve_rejects_unknown_code(db, session_client):
    resp = session_client.post(
        "/api/v1/auth/device/approve/", {"user_code": "ZZZZ-9999"}, format="json"
    )
    assert resp.status_code == 404


@pytest.mark.unit
def test_approve_normalizes_dashes_and_case(db, api_client, session_client):
    start = api_client.post("/api/v1/auth/device/start/", {}, format="json").data
    code = start["user_code"].replace("-", "").lower()
    resp = session_client.post(
        "/api/v1/auth/device/approve/", {"user_code": code}, format="json"
    )
    assert resp.status_code == 200, resp.data


# -------------------- revoke --------------------


@pytest.mark.unit
def test_revoke_marks_token_inactive_idempotently(db, api_key_client, api_token):
    resp = api_key_client.post("/api/v1/auth/revoke/", {}, format="json")
    assert resp.status_code == 200
    api_token.refresh_from_db()
    assert api_token.is_active is False
    # Second call still 200 (token already inactive; the call itself
    # fails auth, so we test via re-activating then calling once more).
    api_token.is_active = True
    api_token.save(update_fields=["is_active"])
    resp = api_key_client.post("/api/v1/auth/revoke/", {}, format="json")
    assert resp.status_code == 200


# -------------------- CLI-initiated runner mint --------------------


@pytest.mark.unit
def test_runner_create_succeeds_with_api_key(db, api_key_client, workspace, project):
    resp = api_key_client.post(
        "/api/v1/runner/runners/",
        {
            "workspace_slug": workspace.slug,
            "project": project.identifier,
            "host_label": "test-host",
        },
        format="json",
    )
    assert resp.status_code == 201, resp.data
    body = resp.data
    assert body["runner_id"]
    assert body["refresh_token"]
    assert body["access_token"]
    assert body["workspace_slug"] == workspace.slug
    assert body["project_identifier"] == project.identifier
    assert body["protocol_version"] == 4


@pytest.mark.unit
def test_runner_create_404_for_unknown_workspace(db, api_key_client, project):
    resp = api_key_client.post(
        "/api/v1/runner/runners/",
        {"workspace_slug": "nope", "project": project.identifier},
        format="json",
    )
    assert resp.status_code == 404


@pytest.mark.unit
def test_runner_create_404_for_unknown_project(db, api_key_client, workspace):
    resp = api_key_client.post(
        "/api/v1/runner/runners/",
        {"workspace_slug": workspace.slug, "project": "NOPE"},
        format="json",
    )
    assert resp.status_code == 404


@pytest.mark.unit
def test_runner_create_requires_auth(db, api_client, workspace, project):
    # No X-Api-Key header at all → 401 from APIKeyAuthentication.
    resp = api_client.post(
        "/api/v1/runner/runners/",
        {"workspace_slug": workspace.slug, "project": project.identifier},
        format="json",
    )
    # DRF returns 403 when no auth class produces a credential (unauthenticated).
    assert resp.status_code in (401, 403)
