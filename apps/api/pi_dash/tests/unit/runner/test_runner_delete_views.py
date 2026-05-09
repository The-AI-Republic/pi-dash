# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the symmetric runner-delete surfaces.

Two endpoints share the same ``runner_delete.delete_runner`` service:

- ``DELETE /api/runners/<id>/`` — session auth, called by the web UI's
  delete modal.
- ``DELETE /api/v1/runners/<id>/`` — X-Api-Key auth, called by the
  local CLI (``pidash runner remove``).

Both honour a ``?purge_local=true|false`` query flag; ``true`` (default)
emits a ``remove_runner`` control frame so the daemon strips local
state, ``false`` emits a plain ``revoke`` so the local install is
untouched.
"""

from __future__ import annotations

from unittest.mock import patch

import pytest
from django.urls import reverse
from django.utils import timezone

from pi_dash.runner.models import Pod, Runner, RunnerStatus
from pi_dash.runner.services.runner_delete import parse_purge_local


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


def _make_runner(user, workspace, pod, name="r1"):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.fixture(autouse=True)
def _stub_outbox_send():
    """Patch every cloud → daemon emitter touched by the delete path.

    ``runner.revoke()`` already enqueues its own frames; the delete
    service then layers either ``remove_runner`` or ``revoke`` on
    top. We stub the lowest-level enqueue so tests don't need a real
    Redis instance and can assert on the per-type calls.

    ``send_runner_remove`` / ``send_runner_revoke`` import
    ``enqueue_for_runner`` into the ``pubsub`` namespace via a
    ``from … import`` (see pubsub.py), so the live binding is
    ``pi_dash.runner.services.pubsub.enqueue_for_runner`` — patching
    the source module would not intercept the local reference.
    """
    with patch(
        "pi_dash.runner.services.pubsub.send_to_runner"
    ) as mock_st, patch(
        "pi_dash.runner.services.pubsub.enqueue_for_runner"
    ) as mock_eq, patch(
        "pi_dash.runner.services.pubsub.close_runner_session"
    ) as mock_cl:
        yield {"send_to_runner": mock_st, "enqueue": mock_eq, "close": mock_cl}


@pytest.fixture(autouse=True)
def _run_on_commit_immediately():
    with patch(
        "django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()
    ):
        yield


# ---- parse_purge_local ----------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize(
    "raw,expected",
    [
        ("true", True),
        ("True", True),
        ("1", True),
        ("yes", True),
        ("false", False),
        ("FALSE", False),
        ("0", False),
        ("no", False),
    ],
)
def test_parse_purge_local_recognises_canonical_values(raw, expected):
    assert parse_purge_local({"purge_local": raw}) is expected


@pytest.mark.unit
def test_parse_purge_local_default_when_absent():
    assert parse_purge_local({}, default=True) is True
    assert parse_purge_local({}, default=False) is False


@pytest.mark.unit
def test_parse_purge_local_rejects_garbage():
    with pytest.raises(ValueError):
        parse_purge_local({"purge_local": "maybe"})


# ---- web UI surface (session auth) ---------------------------------------


@pytest.mark.unit
def test_web_delete_default_emits_remove_runner_and_drops_row(
    db, session_client, create_user, workspace, pod, _stub_outbox_send
):
    """No ``purge_local`` query param → defaults to True → cascade frame."""
    r = _make_runner(create_user, workspace, pod, "web-default")
    url = reverse("runner-detail", kwargs={"runner_id": r.id})
    resp = session_client.delete(url)
    assert resp.status_code == 204
    assert not Runner.objects.filter(pk=r.id).exists()
    types_enqueued = [
        c.kwargs.get("message", c.args[1] if len(c.args) > 1 else {}).get("type")
        for c in _stub_outbox_send["enqueue"].call_args_list
    ]
    assert "remove_runner" in types_enqueued
    assert "revoke" not in types_enqueued


@pytest.mark.unit
def test_web_delete_purge_false_emits_revoke_only(
    db, session_client, create_user, workspace, pod, _stub_outbox_send
):
    r = _make_runner(create_user, workspace, pod, "web-no-purge")
    url = (
        reverse("runner-detail", kwargs={"runner_id": r.id})
        + "?purge_local=false"
    )
    resp = session_client.delete(url)
    assert resp.status_code == 204
    assert not Runner.objects.filter(pk=r.id).exists()
    types_enqueued = [
        c.kwargs.get("message", c.args[1] if len(c.args) > 1 else {}).get("type")
        for c in _stub_outbox_send["enqueue"].call_args_list
    ]
    assert "revoke" in types_enqueued
    assert "remove_runner" not in types_enqueued


@pytest.mark.unit
def test_delete_propagates_canonical_reason_when_purging(
    db, create_user, workspace, pod, _stub_outbox_send
):
    """purge_local=True must put a canonical reason on the session row.

    The Rust synthesizer in ``runner/src/cloud/http.rs`` matches the
    session's ``revoked_reason`` (echoed in the 409 body) against a
    canonical set; ``runner_removed`` is in that set so the daemon
    will fall back to local cleanup if the wire frame above is lost.
    """
    from pi_dash.runner.models import RunnerSession
    from pi_dash.runner.services.runner_delete import delete_runner

    r = _make_runner(create_user, workspace, pod, "purge-true")
    session = RunnerSession.objects.create(runner=r)
    delete_runner(r, purge_local=True)
    session.refresh_from_db()
    assert session.revoked_at is not None
    assert session.revoked_reason == "runner_removed"


@pytest.mark.unit
def test_delete_uses_non_canonical_reason_when_not_purging(
    db, create_user, workspace, pod, _stub_outbox_send
):
    """purge_local=False must NOT trigger the daemon's local-wipe synthesizer.

    The synthesizer matches a small canonical set of reasons (see
    ``body_matches_canonical_reason`` in ``http.rs``). ``user_revoke``
    is deliberately outside that set: the daemon's RunnerLoop exits
    cleanly on 409 but does not strip ``config.toml`` or the data dir.
    Without this distinction every revoke-only delete was wiping local
    state too, breaking the "Also delete the local runner instance"
    checkbox-unchecked path in the web UI.
    """
    from pi_dash.runner.models import RunnerSession
    from pi_dash.runner.services.runner_delete import delete_runner

    r = _make_runner(create_user, workspace, pod, "purge-false")
    session = RunnerSession.objects.create(runner=r)
    delete_runner(r, purge_local=False)
    session.refresh_from_db()
    assert session.revoked_at is not None
    assert session.revoked_reason == "user_revoke"


@pytest.mark.unit
def test_web_delete_purge_invalid_returns_400(
    db, session_client, create_user, workspace, pod
):
    r = _make_runner(create_user, workspace, pod, "web-bad-flag")
    url = (
        reverse("runner-detail", kwargs={"runner_id": r.id})
        + "?purge_local=maybe"
    )
    resp = session_client.delete(url)
    assert resp.status_code == 400
    # Row must still exist — the validation runs before any destructive work.
    assert Runner.objects.filter(pk=r.id).exists()


@pytest.mark.unit
def test_web_delete_forbidden_when_not_owner_or_admin(
    db, api_client, workspace, pod, create_user
):
    """A workspace member who is neither owner nor admin can't delete."""
    from pi_dash.db.models import User, WorkspaceMember

    other = User.objects.create(email="other@example.com")
    other.set_password("x")
    other.save()
    WorkspaceMember.objects.create(workspace=workspace, member=other, role=15)
    r = _make_runner(create_user, workspace, pod, "web-forbidden")
    api_client.force_authenticate(user=other)
    url = reverse("runner-detail", kwargs={"runner_id": r.id})
    resp = api_client.delete(url)
    assert resp.status_code == 403
    assert Runner.objects.filter(pk=r.id).exists()


# ---- v1 surface (X-Api-Key) ----------------------------------------------


@pytest.mark.unit
def test_v1_delete_default_emits_remove_runner_and_drops_row(
    db, api_key_client, create_user, workspace, pod, _stub_outbox_send
):
    r = _make_runner(create_user, workspace, pod, "v1-default")
    url = reverse("api-runner-delete", kwargs={"runner_id": r.id})
    resp = api_key_client.delete(url)
    assert resp.status_code == 204
    assert not Runner.objects.filter(pk=r.id).exists()
    types_enqueued = [
        c.kwargs.get("message", c.args[1] if len(c.args) > 1 else {}).get("type")
        for c in _stub_outbox_send["enqueue"].call_args_list
    ]
    assert "remove_runner" in types_enqueued


@pytest.mark.unit
def test_v1_delete_purge_false_emits_revoke_only(
    db, api_key_client, create_user, workspace, pod, _stub_outbox_send
):
    r = _make_runner(create_user, workspace, pod, "v1-no-purge")
    url = (
        reverse("api-runner-delete", kwargs={"runner_id": r.id})
        + "?purge_local=false"
    )
    resp = api_key_client.delete(url)
    assert resp.status_code == 204
    types_enqueued = [
        c.kwargs.get("message", c.args[1] if len(c.args) > 1 else {}).get("type")
        for c in _stub_outbox_send["enqueue"].call_args_list
    ]
    assert "revoke" in types_enqueued
    assert "remove_runner" not in types_enqueued


@pytest.mark.unit
def test_v1_delete_unauthenticated_returns_401_or_403(
    db, api_client, create_user, workspace, pod
):
    r = _make_runner(create_user, workspace, pod, "v1-unauth")
    url = reverse("api-runner-delete", kwargs={"runner_id": r.id})
    resp = api_client.delete(url)
    assert resp.status_code in (401, 403)
    assert Runner.objects.filter(pk=r.id).exists()


@pytest.mark.unit
def test_v1_delete_forbidden_when_token_user_cannot_manage(
    db, api_client, workspace, pod, create_user
):
    """An API key whose owner is not the runner owner / a workspace
    admin must not be able to delete via the v1 surface either."""
    from pi_dash.db.models import APIToken, User, WorkspaceMember

    other = User.objects.create(email="other-v1@example.com")
    other.set_password("x")
    other.save()
    WorkspaceMember.objects.create(workspace=workspace, member=other, role=15)
    other_token = APIToken.objects.create(
        user=other,
        label="other token",
        token="other-tok-xyz",
    )
    r = _make_runner(create_user, workspace, pod, "v1-forbidden")
    api_client.credentials(HTTP_X_API_KEY=other_token.token)
    url = reverse("api-runner-delete", kwargs={"runner_id": r.id})
    resp = api_client.delete(url)
    assert resp.status_code == 403
    assert Runner.objects.filter(pk=r.id).exists()


@pytest.mark.unit
def test_v1_delete_404_when_runner_missing(
    db, api_key_client
):
    """Unknown UUIDs return 404 even with valid auth."""
    import uuid

    url = reverse(
        "api-runner-delete", kwargs={"runner_id": uuid.uuid4()}
    )
    resp = api_key_client.delete(url)
    assert resp.status_code == 404
