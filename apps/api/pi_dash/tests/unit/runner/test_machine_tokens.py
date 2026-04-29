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
    """A second user with no membership in the primary `workspace` fixture.

    Sets ``username`` explicitly so this fixture composes with the shared
    ``create_user`` fixture (which leaves username at the model default).
    Without a distinct username the two users collide on the
    ``users_username_key`` unique constraint when both fixtures run in
    the same test (cross-workspace tests do exactly that).
    """
    user = User.objects.create(
        username="other",
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
def pod(project):
    return Pod.default_for_project(project)


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
    ) as mock_close, patch(
        "pi_dash.runner.services.pubsub.send_token_revoke"
    ) as mock_send_revoke:
        token.revoke()

    closed_ids = {call.args[0] for call in mock_close.call_args_list}
    assert closed_ids == {r1.id, r2.id}, closed_ids
    # Defence in depth: a wire-level Revoke frame is also pushed so the
    # daemon's supervisor calls state.shutdown() rather than
    # reconnect-with-401-loop forever. Broadcast to every owned runner
    # group so the live consumer sees it even if some owned runners are
    # stale/offline on this daemon.
    revoke_targets = {call.args[0] for call in mock_send_revoke.call_args_list}
    assert revoke_targets == {r1.id, r2.id}


@pytest.mark.unit
def test_revoke_token_emits_revoke_frame_when_runners_present(
    db, create_user, workspace, pod
):
    """The connection-scoped Revoke frame is the runner-side signal that
    triggers a clean daemon shutdown. Without it, the daemon would just
    see a TCP close and bounce into a reconnect loop where every retry
    fails the (now-revoked) auth check.
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
    with patch(
        "pi_dash.runner.services.pubsub.close_runner_session"
    ), patch(
        "pi_dash.runner.services.pubsub.send_token_revoke"
    ) as mock_send_revoke:
        token.revoke()

    mock_send_revoke.assert_called_once_with(r1.id, reason="token revoked")


@pytest.mark.unit
def test_revoke_token_with_no_runners_skips_revoke_frame(
    db, create_user, workspace
):
    """If no runners are owned by the token, there's no consumer group
    to push the Revoke frame to — skip it cleanly without raising.
    """
    minted = tokens.mint_machine_token_secret()
    token = MachineToken.objects.create(
        workspace=workspace,
        created_by=create_user,
        title="t",
        secret_hash=minted.hashed,
        secret_fingerprint=minted.fingerprint,
    )
    with patch(
        "pi_dash.runner.services.pubsub.send_token_revoke"
    ) as mock_send_revoke:
        token.revoke()
    mock_send_revoke.assert_not_called()
    token.refresh_from_db()
    assert token.revoked_at is not None


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

    with patch("pi_dash.runner.services.pubsub.close_runner_session"), patch(
        "pi_dash.runner.services.pubsub.send_token_revoke"
    ):
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
    ) as mock_close, patch(
        "pi_dash.runner.services.pubsub.send_token_revoke"
    ) as mock_send_revoke:
        token.revoke()
    # Second revoke is a no-op — neither pubsub side effect fires.
    mock_close.assert_not_called()
    mock_send_revoke.assert_not_called()


# ---------------------------------------------------------------------------
# POST /api/v1/runner/<id>/deregister/  — token-mode emits RemoveRunner
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_deregister_in_token_mode_emits_remove_runner(
    db, api_client, create_user, workspace, pod
):
    """Per-runner deregistration in token mode must NOT close the WS —
    that would knock every sibling runner under the same token offline.
    Instead we push a per-runner ``remove_runner`` ServerMsg, leaving
    the connection up for the rest.
    """
    minted = tokens.mint_machine_token_secret()
    token = MachineToken.objects.create(
        workspace=workspace,
        created_by=create_user,
        title="t",
        secret_hash=minted.hashed,
        secret_fingerprint=minted.fingerprint,
    )
    runner_minted = tokens.mint_runner_secret()
    runner = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="r1",
        credential_hash=runner_minted.hashed,
        credential_fingerprint=runner_minted.fingerprint,
        machine_token=token,
        status=RunnerStatus.ONLINE,
    )

    api_client.credentials(
        HTTP_AUTHORIZATION=f"Bearer {minted.raw}",
        HTTP_X_TOKEN_ID=str(token.id),
    )
    with patch(
        "pi_dash.runner.views.register.send_to_runner"
    ) as mock_send, patch(
        "pi_dash.runner.views.register.close_runner_session"
    ) as mock_close:
        url = reverse("runner:deregister", args=[runner.id])
        resp = api_client.post(url)

    assert resp.status_code == 200, resp.content
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.REVOKED
    # Token mode → push remove_runner via pubsub; NEVER force-close.
    mock_send.assert_called_once()
    assert mock_send.call_args.args[0] == runner.id
    payload = mock_send.call_args.args[1]
    assert payload["type"] == "remove_runner"
    assert payload["runner_id"] == str(runner.id)
    mock_close.assert_not_called()


@pytest.mark.unit
def test_deregister_in_legacy_mode_force_closes_ws(
    db, api_client, create_user, workspace, pod
):
    """Legacy single-runner deregister still uses the connection-close
    path — there's no other runner on this WS to spare.
    """
    runner_minted = tokens.mint_runner_secret()
    runner = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="r1",
        credential_hash=runner_minted.hashed,
        credential_fingerprint=runner_minted.fingerprint,
        status=RunnerStatus.ONLINE,
    )
    assert runner.machine_token_id is None

    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {runner_minted.raw}")
    with patch(
        "pi_dash.runner.views.register.send_to_runner"
    ) as mock_send, patch(
        "pi_dash.runner.views.register.close_runner_session"
    ) as mock_close:
        url = reverse("runner:deregister", args=[runner.id])
        resp = api_client.post(url)

    assert resp.status_code == 200, resp.content
    runner.refresh_from_db()
    assert runner.status == RunnerStatus.REVOKED
    mock_close.assert_called_once_with(runner.id)
    mock_send.assert_not_called()


# ---------------------------------------------------------------------------
# POST /api/v1/runner/<id>/link-to-token/  — migrate legacy runner to token
# ---------------------------------------------------------------------------


def _legacy_runner(user, workspace, pod, name="primary"):
    """A runner registered via the legacy /register/ flow — no machine_token."""
    minted = tokens.mint_runner_secret()
    runner = Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        credential_hash=minted.hashed,
        credential_fingerprint=minted.fingerprint,
        status=RunnerStatus.OFFLINE,
    )
    return runner, minted.raw


def _make_token(user, workspace, title="laptop"):
    minted = tokens.mint_machine_token_secret()
    token = MachineToken.objects.create(
        workspace=workspace,
        created_by=user,
        title=title,
        secret_hash=minted.hashed,
        secret_fingerprint=minted.fingerprint,
    )
    return token, minted.raw


@pytest.mark.unit
def test_link_to_token_links_legacy_runner(
    db, api_client, create_user, workspace, pod
):
    """Happy path: a runner registered via the legacy flow can migrate
    onto a freshly-minted MachineToken using its existing runner_secret.
    """
    runner, runner_secret = _legacy_runner(create_user, workspace, pod)
    assert runner.machine_token_id is None  # Sanity.
    token, token_secret = _make_token(create_user, workspace)

    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {runner_secret}")
    url = reverse("runner:link-to-token", args=[runner.id])
    resp = api_client.post(
        url,
        {"token_id": str(token.id), "token_secret": token_secret},
        format="json",
    )

    assert resp.status_code == 200, resp.content
    runner.refresh_from_db()
    assert runner.machine_token_id == token.id


@pytest.mark.unit
def test_link_to_token_rejects_wrong_token_secret(
    db, api_client, create_user, workspace, pod
):
    runner, runner_secret = _legacy_runner(create_user, workspace, pod)
    token, _good_secret = _make_token(create_user, workspace)

    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {runner_secret}")
    url = reverse("runner:link-to-token", args=[runner.id])
    resp = api_client.post(
        url,
        {"token_id": str(token.id), "token_secret": "apd_mt_wrongwrong"},
        format="json",
    )
    assert resp.status_code == 401, resp.content
    runner.refresh_from_db()
    assert runner.machine_token_id is None  # Unchanged.


@pytest.mark.unit
def test_link_to_token_rejects_revoked_token(
    db, api_client, create_user, workspace, pod
):
    runner, runner_secret = _legacy_runner(create_user, workspace, pod)
    token, token_secret = _make_token(create_user, workspace)
    token.revoked_at = timezone.now()
    token.save(update_fields=["revoked_at"])

    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {runner_secret}")
    url = reverse("runner:link-to-token", args=[runner.id])
    resp = api_client.post(
        url,
        {"token_id": str(token.id), "token_secret": token_secret},
        format="json",
    )
    assert resp.status_code == 401


@pytest.mark.unit
def test_link_to_token_rejects_cross_workspace(
    db, api_client, create_user, workspace, pod, other_user, other_workspace
):
    """Runner and token must share a workspace. Cross-workspace linking
    would let a token mint runners outside its scope.
    """
    runner, runner_secret = _legacy_runner(create_user, workspace, pod)
    other_pod = Pod.default_for_project(other_project)
    # Token in the OTHER workspace — runner shouldn't be linkable to it.
    token, token_secret = _make_token(other_user, other_workspace)
    # Sanity: the token belongs to a different workspace from the runner.
    assert token.workspace_id != runner.workspace_id
    _ = other_pod  # Quiet unused-fixture lint.

    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {runner_secret}")
    url = reverse("runner:link-to-token", args=[runner.id])
    resp = api_client.post(
        url,
        {"token_id": str(token.id), "token_secret": token_secret},
        format="json",
    )
    assert resp.status_code == 400, resp.content
    runner.refresh_from_db()
    assert runner.machine_token_id is None


@pytest.mark.unit
def test_link_to_token_refuses_relink_to_different_active_token(
    db, api_client, create_user, workspace, pod
):
    """Once a runner is linked to an active token, linking it to a
    DIFFERENT active token must fail. Otherwise a leaked token's
    holder could steal a runner that's already bound to someone else.
    """
    runner, runner_secret = _legacy_runner(create_user, workspace, pod)
    first_token, _ = _make_token(create_user, workspace, title="first")
    Runner.objects.filter(pk=runner.pk).update(machine_token=first_token)

    second_token, second_secret = _make_token(create_user, workspace, title="second")
    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {runner_secret}")
    url = reverse("runner:link-to-token", args=[runner.id])
    resp = api_client.post(
        url,
        {"token_id": str(second_token.id), "token_secret": second_secret},
        format="json",
    )
    assert resp.status_code == 409, resp.content
    runner.refresh_from_db()
    assert runner.machine_token_id == first_token.id


@pytest.mark.unit
def test_link_to_token_is_idempotent_to_same_token(
    db, api_client, create_user, workspace, pod
):
    """Calling link-to-token twice with the same token must succeed
    both times — `pidash token install` should be safe to retry."""
    runner, runner_secret = _legacy_runner(create_user, workspace, pod)
    token, token_secret = _make_token(create_user, workspace)

    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {runner_secret}")
    url = reverse("runner:link-to-token", args=[runner.id])
    body = {"token_id": str(token.id), "token_secret": token_secret}

    r1 = api_client.post(url, body, format="json")
    r2 = api_client.post(url, body, format="json")
    assert r1.status_code == 200
    assert r2.status_code == 200


@pytest.mark.unit
def test_link_to_token_without_runner_secret_unauthorized(
    db, api_client, create_user, workspace, pod
):
    runner, _ = _legacy_runner(create_user, workspace, pod)
    token, token_secret = _make_token(create_user, workspace)
    # No Authorization header at all.
    url = reverse("runner:link-to-token", args=[runner.id])
    resp = api_client.post(
        url,
        {"token_id": str(token.id), "token_secret": token_secret},
        format="json",
    )
    # DRF returns 401 when authentication fails for an endpoint that
    # requires it. (RunnerBearerAuthentication returns None on missing
    # header, so the auth_runner attr is unset and the view's own
    # forbidden-check fires with 403.)
    assert resp.status_code in (401, 403)
