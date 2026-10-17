# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``runner_gone`` auth answer (PDASHOSS01-306).

A verified caller asking for a runner id that no longer exists gets the
distinct ``runner_gone`` code, which the daemon counts toward purging the
runner locally. Every failure before the runner lookup keeps its own code,
and a revoked runner still answers ``runner_revoked``.
"""

from __future__ import annotations

import uuid

import pytest
from django.utils import timezone

from pi_dash.runner.models import DevMachine, MachineToken, Pod, Runner
from pi_dash.runner.services import tokens


@pytest.fixture
def dev_machine(create_user):
    return DevMachine.objects.create(owner=create_user, host_label="host-a")


@pytest.fixture
def minted_token(create_user, workspace, dev_machine):
    minted = tokens.mint_machine_token()
    row = MachineToken.objects.create(
        user=create_user,
        workspace=workspace,
        dev_machine=dev_machine,
        host_label="host-a",
        token_hash=minted.hashed,
        token_fingerprint=minted.fingerprint,
        label="machine: host-a",
        is_service=True,
    )
    return minted.raw, row


@pytest.fixture
def bound_runner(create_user, workspace, project, dev_machine):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=Pod.default_for_project(project),
        dev_machine=dev_machine,
        host_label="host-a",
        name="bound",
    )


def _open_session(api_client, raw_token, runner_id):
    api_client.credentials(HTTP_AUTHORIZATION=f"Bearer {raw_token}")
    return api_client.post(
        f"/api/v1/runner/runners/{runner_id}/sessions/",
        {"version": "test", "status": "idle"},
        format="json",
    )


@pytest.mark.unit
def test_verified_machine_token_with_missing_runner_gets_runner_gone(db, api_client, minted_token):
    raw, _ = minted_token
    resp = _open_session(api_client, raw, uuid.uuid4())
    assert resp.status_code == 401
    assert resp.json() == {"detail": "runner_gone"}


@pytest.mark.unit
def test_deleted_runner_gets_runner_gone(db, api_client, minted_token, bound_runner):
    raw, _ = minted_token
    runner_id = bound_runner.id
    Runner.objects.filter(pk=runner_id).delete()
    resp = _open_session(api_client, raw, runner_id)
    assert resp.status_code == 401
    assert resp.json() == {"detail": "runner_gone"}


@pytest.mark.unit
def test_unknown_machine_token_keeps_machine_token_invalid(db, api_client):
    # An empty / wrong database fails here and must never say runner_gone.
    resp = _open_session(api_client, tokens.mint_machine_token().raw, uuid.uuid4())
    assert resp.status_code == 401
    assert resp.json() == {"detail": "machine_token_invalid"}


@pytest.mark.unit
def test_revoked_machine_token_keeps_its_code(db, api_client, minted_token):
    raw, row = minted_token
    MachineToken.objects.filter(pk=row.pk).update(revoked_at=timezone.now())
    resp = _open_session(api_client, raw, uuid.uuid4())
    assert resp.status_code == 401
    assert resp.json() == {"detail": "machine_token_revoked"}


@pytest.mark.unit
def test_revoked_dev_machine_keeps_its_code(db, api_client, minted_token, dev_machine):
    raw, _ = minted_token
    DevMachine.objects.filter(pk=dev_machine.pk).update(revoked_at=timezone.now())
    resp = _open_session(api_client, raw, uuid.uuid4())
    assert resp.status_code == 401
    assert resp.json() == {"detail": "dev_machine_revoked"}


@pytest.mark.unit
def test_revoked_membership_keeps_its_code(db, api_client, minted_token, monkeypatch):
    raw, _ = minted_token
    monkeypatch.setattr(
        "pi_dash.runner.authentication.is_workspace_member",
        lambda user, workspace_id: False,
    )
    resp = _open_session(api_client, raw, uuid.uuid4())
    assert resp.status_code == 401
    assert resp.json() == {"detail": "membership_revoked"}


@pytest.mark.unit
def test_revoked_runner_still_gets_runner_revoked(db, api_client, minted_token, bound_runner):
    raw, _ = minted_token
    Runner.objects.filter(pk=bound_runner.pk).update(revoked_at=timezone.now())
    resp = _open_session(api_client, raw, bound_runner.id)
    assert resp.status_code == 401
    assert resp.json() == {"detail": "runner_revoked"}


@pytest.mark.unit
def test_access_token_for_deleted_runner_gets_runner_gone(db, api_client, create_user, workspace):
    missing = uuid.uuid4()
    token = tokens.mint_access_token(
        runner_id=str(missing),
        user_id=str(create_user.id),
        workspace_id=str(workspace.id),
        rtg=1,
    )
    resp = _open_session(api_client, token.raw, missing)
    assert resp.status_code == 401
    assert resp.json() == {"detail": "runner_gone"}


@pytest.mark.unit
def test_undecodable_access_token_keeps_its_code(db, api_client):
    resp = _open_session(api_client, "not-a-jwt", uuid.uuid4())
    assert resp.status_code == 401
    assert resp.json()["detail"] != "runner_gone"
