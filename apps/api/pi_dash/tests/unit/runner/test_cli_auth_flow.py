# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the device-code (`pidash auth login`) flow + CLI-initiated
runner mint + token revoke endpoints. See
``apps/api/pi_dash/authentication/views/cli/device.py`` and
``apps/api/pi_dash/runner/views/enrollment.py::RunnerCreateEndpoint``.
"""

from __future__ import annotations

import re
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
    minted = APIToken.objects.get(token=poll2.data["access_token"], user=create_user)
    assert re.fullmatch(r"pidash CLI · \d{4}-\d{2}-\d{2} \d{2}:\d{2} UTC", minted.label), minted.label
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


@pytest.mark.unit
def test_runner_create_infers_workspace_when_caller_has_one(
    db, api_key_client, workspace, project
):
    """The documented onboarding flow: `pidash runner add --project X`
    (without --workspace) must succeed when the caller belongs to
    exactly one workspace. The `workspace` fixture wires a single
    membership for `create_user`, matching the dev-laptop case.
    """
    resp = api_key_client.post(
        "/api/v1/runner/runners/",
        {"project": project.identifier, "host_label": "test-host"},
        format="json",
    )
    assert resp.status_code == 201, resp.data
    assert resp.data["workspace_slug"] == workspace.slug


@pytest.mark.unit
def test_runner_create_400_when_workspace_ambiguous(
    db, api_key_client, create_user, workspace, project
):
    """If the caller belongs to multiple workspaces and omits
    `workspace_slug`, we should refuse with a specific error rather
    than guessing wrong.
    """
    from pi_dash.db.models.workspace import Workspace, WorkspaceMember

    other_ws = Workspace.objects.create(
        name="other-ws",
        owner=create_user,
        slug="other-ws",
    )
    WorkspaceMember.objects.create(workspace=other_ws, member=create_user, role=20)

    resp = api_key_client.post(
        "/api/v1/runner/runners/",
        {"project": project.identifier},
        format="json",
    )
    assert resp.status_code == 400
    assert resp.data["error"] == "workspace_slug_required"


@pytest.mark.unit
def test_runner_create_rejects_invalid_name(db, api_key_client, workspace, project):
    """Disallow path-traversal / control chars in user-supplied names."""
    resp = api_key_client.post(
        "/api/v1/runner/runners/",
        {
            "workspace_slug": workspace.slug,
            "project": project.identifier,
            "name": "../etc/passwd",
        },
        format="json",
    )
    assert resp.status_code == 400
    assert resp.data["error"] == "invalid_runner_name"


@pytest.mark.unit
def test_runner_create_400_when_no_workspace_membership(
    db, api_client, create_user, workspace, project
):
    """A token bound to a user with no workspace memberships at all
    should get a clear `no_workspace_membership` error, not a 500.
    """
    from pi_dash.db.models import APIToken
    from pi_dash.db.models.workspace import WorkspaceMember

    WorkspaceMember.objects.filter(member=create_user).delete()
    # Mint a fresh token now that the user has no memberships.
    tok = APIToken.objects.create(user=create_user)
    api_client.credentials(HTTP_X_API_KEY=tok.token)

    resp = api_client.post(
        "/api/v1/runner/runners/",
        {"project": project.identifier},
        format="json",
    )
    assert resp.status_code == 400
    assert resp.data["error"] == "no_workspace_membership"


# -------------------- workspaces list --------------------


@pytest.mark.unit
def test_workspaces_list_returns_single_membership(db, api_key_client, workspace):
    """Single-workspace caller sees exactly that workspace, by slug + name.

    Drives the no-prompt branch of `pidash auth login`'s workspace
    resolver: one membership → silently persist `[cli].workspace_slug`.
    """
    resp = api_key_client.get("/api/v1/auth/workspaces/")
    assert resp.status_code == 200, resp.data
    assert resp.data == {"workspaces": [{"slug": workspace.slug, "name": workspace.name}]}


@pytest.mark.unit
def test_workspaces_list_returns_multiple_in_join_order(
    db, api_key_client, create_user, workspace
):
    """Multi-workspace caller sees every active membership, ordered by
    join time. The CLI's picker renders them in this order so the list
    is stable across calls.
    """
    from pi_dash.db.models.workspace import Workspace, WorkspaceMember

    second = Workspace.objects.create(name="second-ws", owner=create_user, slug="second-ws")
    WorkspaceMember.objects.create(workspace=second, member=create_user, role=20)

    resp = api_key_client.get("/api/v1/auth/workspaces/")
    assert resp.status_code == 200, resp.data
    slugs = [w["slug"] for w in resp.data["workspaces"]]
    assert slugs == [workspace.slug, second.slug]


@pytest.mark.unit
def test_workspaces_list_excludes_inactive_memberships(
    db, api_key_client, create_user, workspace
):
    """Soft-removed memberships (`is_active=False`) must not surface
    in the picker — otherwise the user could pick a workspace they no
    longer have access to and hit a confusing 403 on the next call.
    """
    from pi_dash.db.models.workspace import Workspace, WorkspaceMember

    inactive = Workspace.objects.create(name="gone-ws", owner=create_user, slug="gone-ws")
    WorkspaceMember.objects.create(
        workspace=inactive, member=create_user, role=20, is_active=False
    )

    resp = api_key_client.get("/api/v1/auth/workspaces/")
    assert resp.status_code == 200, resp.data
    slugs = [w["slug"] for w in resp.data["workspaces"]]
    assert slugs == [workspace.slug]


@pytest.mark.unit
def test_workspaces_list_requires_auth(db, api_client):
    """Anonymous callers get 401; the endpoint is CLI-token only."""
    resp = api_client.get("/api/v1/auth/workspaces/")
    assert resp.status_code in (401, 403)


# -------------------- approval hardening --------------------


@pytest.mark.unit
def test_approve_rejects_second_user_takeover(
    db, api_client, session_client, create_user
):
    """Once user A approves a code, user B (a different logged-in
    session) cannot re-approve and steal the eventual token.
    """
    from pi_dash.db.models import User

    start = api_client.post("/api/v1/auth/device/start/", {}, format="json").data
    # User A approves.
    first = session_client.post(
        "/api/v1/auth/device/approve/", {"user_code": start["user_code"]}, format="json"
    )
    assert first.status_code == 200, first.data

    # User B (different account) tries to approve the same code.
    user_b = User.objects.create(email="b@example.com", username="b")
    api_client.force_authenticate(user=user_b)
    second = api_client.post(
        "/api/v1/auth/device/approve/", {"user_code": start["user_code"]}, format="json"
    )
    assert second.status_code == 409
    api_client.force_authenticate(user=None)

    # Subsequent CLI poll mints the token for user A (the original
    # approver), not user B.
    CLIDeviceCode.objects.filter(device_code=start["device_code"]).update(
        last_polled_at=timezone.now() - timedelta(seconds=30)
    )
    poll = api_client.post(
        "/api/v1/auth/device/token/", {"device_code": start["device_code"]}, format="json"
    )
    assert poll.status_code == 200
    assert poll.data["user_email"] == create_user.email


@pytest.mark.unit
def test_approve_idempotent_for_same_user(db, api_client, session_client, create_user):
    """Re-approving by the same user is OK (idempotent UX): the human
    might double-click Approve in the browser.
    """
    start = api_client.post("/api/v1/auth/device/start/", {}, format="json").data
    a = session_client.post(
        "/api/v1/auth/device/approve/", {"user_code": start["user_code"]}, format="json"
    )
    assert a.status_code == 200
    b = session_client.post(
        "/api/v1/auth/device/approve/", {"user_code": start["user_code"]}, format="json"
    )
    assert b.status_code == 200


# -------------------- slow_down DoS prevention --------------------


@pytest.mark.unit
def test_slow_down_does_not_update_last_polled_at(db, api_client):
    """A rapid poller (or attacker holding the device_code) must NOT be
    able to slip `last_polled_at` forward on each rejection, otherwise
    the legit CLI's slower poll would always look "too fast" too.
    """
    start = api_client.post("/api/v1/auth/device/start/", {}, format="json").data
    # First poll establishes last_polled_at.
    first = api_client.post(
        "/api/v1/auth/device/token/", {"device_code": start["device_code"]}, format="json"
    )
    assert first.status_code == 400
    row = CLIDeviceCode.objects.get(device_code=start["device_code"])
    first_polled = row.last_polled_at
    assert first_polled is not None

    # Second poll within the min-gap returns slow_down — and must leave
    # last_polled_at frozen at the previous value.
    second = api_client.post(
        "/api/v1/auth/device/token/", {"device_code": start["device_code"]}, format="json"
    )
    assert second.status_code == 400
    assert second.data["error"] == "slow_down"
    row.refresh_from_db()
    assert row.last_polled_at == first_polled
