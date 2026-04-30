# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from datetime import timedelta

import pytest
from django.urls import reverse
from django.utils import timezone

from pi_dash.db.models import Workspace, WorkspaceMember
from pi_dash.db.models.api import APIToken
from pi_dash.runner.models import (
    AgentRunStatus,
    Runner,
    RunnerRegistrationToken,
    RunnerStatus,
)
from pi_dash.runner.services import tokens


@pytest.fixture
def registration(db, create_user, workspace):
    minted = tokens.mint_registration_token()
    record = RunnerRegistrationToken.objects.create(
        workspace=workspace,
        created_by=create_user,
        token_hash=minted.hashed,
        label="test",
        expires_at=minted.expires_at,
    )
    return minted, record


@pytest.mark.contract
def test_register_exchanges_token_for_secret(api_client, registration):
    minted, record = registration
    url = reverse("runner:register")
    payload = {
        "token": minted.raw,
        "runner_name": "laptop",
        "os": "linux",
        "arch": "x86_64",
        "version": "0.1.0",
        "protocol_version": 2,
    }
    resp = api_client.post(url, payload, format="json")
    assert resp.status_code == 201
    body = resp.json()
    assert body["runner_secret"].startswith("apd_rs_")
    assert body["api_token"].startswith("pi_dash_api_")
    assert body["protocol_version"] == 2
    # Runner enrollment is workspace-scoped; the CLI relies on this slug to
    # build `/api/v1/workspaces/<slug>/...` URLs without asking the user.
    assert body["workspace_slug"] == record.workspace.slug
    runner = Runner.objects.get(id=body["runner_id"])
    assert runner.owner_id == record.created_by_id
    record.refresh_from_db()
    assert record.consumed_at is not None
    assert record.consumed_by_runner_id == runner.id

    # The minted APIToken is owned by the same user as the runner, scoped
    # to the same workspace, and classified Human (user_type=0) since
    # create_user is not a bot. It is also flagged as a service token so
    # CLI traffic from the runner lands on the 300/min throttle.
    api_token_row = APIToken.objects.get(token=body["api_token"])
    assert api_token_row.user_id == record.created_by_id
    assert api_token_row.workspace_id == record.workspace_id
    assert api_token_row.user_type == 0
    assert api_token_row.is_service is True


@pytest.mark.contract
def test_register_classifies_bot_owned_token_correctly(
    api_client, db, create_bot_user, workspace
):
    # Registration tokens can be minted by bot users (the endpoint only
    # requires IsAuthenticated), so the auto-issued APIToken must inherit
    # user_type=1 rather than falling back to the Human default.
    minted = tokens.mint_registration_token()
    RunnerRegistrationToken.objects.create(
        workspace=workspace,
        created_by=create_bot_user,
        token_hash=minted.hashed,
        label="bot-test",
        expires_at=minted.expires_at,
    )
    url = reverse("runner:register")
    resp = api_client.post(
        url,
        {
            "token": minted.raw,
            "runner_name": "bot-laptop",
            "os": "linux",
            "arch": "x86_64",
            "version": "0.1.0",
            "protocol_version": 1,
        },
        format="json",
    )
    assert resp.status_code == 201
    body = resp.json()
    api_token_row = APIToken.objects.get(token=body["api_token"])
    assert api_token_row.user_id == create_bot_user.id
    assert api_token_row.user_type == 1


@pytest.mark.contract
def test_register_rejects_expired_token(api_client, registration):
    minted, record = registration
    record.expires_at = timezone.now() - timedelta(minutes=1)
    record.save(update_fields=["expires_at"])
    url = reverse("runner:register")
    resp = api_client.post(
        url,
        {
            "token": minted.raw,
            "runner_name": "laptop",
            "os": "linux",
            "arch": "x86_64",
            "version": "0.1.0",
            "protocol_version": 1,
        },
        format="json",
    )
    assert resp.status_code == 401


@pytest.mark.contract
def test_register_rejects_reused_token(api_client, registration):
    minted, _ = registration
    url = reverse("runner:register")
    payload = {
        "token": minted.raw,
        "runner_name": "laptop",
        "os": "linux",
        "arch": "x86_64",
        "version": "0.1.0",
        "protocol_version": 1,
    }
    first = api_client.post(url, payload, format="json")
    second = api_client.post(url, payload, format="json")
    assert first.status_code == 201
    assert second.status_code == 401


@pytest.mark.contract
def test_register_enforces_runner_cap(api_client, db, create_user, workspace):
    # Pre-create Runner.MAX_PER_USER runners to reach the cap.
    for i in range(Runner.MAX_PER_USER):
        Runner.objects.create(
            owner=create_user,
            workspace=workspace,
            name=f"existing-{i}",
            credential_hash=f"h{i}",
            credential_fingerprint="f" * 12,
            status=RunnerStatus.OFFLINE,
        )
    minted = tokens.mint_registration_token()
    RunnerRegistrationToken.objects.create(
        workspace=workspace,
        created_by=create_user,
        token_hash=minted.hashed,
        expires_at=minted.expires_at,
    )
    url = reverse("runner:register")
    resp = api_client.post(
        url,
        {
            "token": minted.raw,
            "runner_name": "another",
            "os": "linux",
            "arch": "x86_64",
            "version": "0.1.0",
            "protocol_version": 1,
        },
        format="json",
    )
    assert resp.status_code == 409


@pytest.mark.contract
def test_register_rejects_duplicate_name_in_same_workspace(
    api_client, db, create_user, workspace
):
    # Pre-existing runner with the name we're about to try to register.
    Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        name="dup-name",
        credential_hash="h-existing",
        credential_fingerprint="f" * 12,
        status=RunnerStatus.OFFLINE,
    )
    minted = tokens.mint_registration_token()
    reg = RunnerRegistrationToken.objects.create(
        workspace=workspace,
        created_by=create_user,
        token_hash=minted.hashed,
        expires_at=minted.expires_at,
    )
    url = reverse("runner:register")
    resp = api_client.post(
        url,
        {
            "token": minted.raw,
            "runner_name": "dup-name",
            "os": "linux",
            "arch": "x86_64",
            "version": "0.1.0",
            "protocol_version": 1,
        },
        format="json",
    )
    assert resp.status_code == 409
    assert resp.json() == {"error": "runner_name_taken"}
    # @transaction.atomic rolls back the consume, so the runner can retry
    # with a different name on the same token.
    reg.refresh_from_db()
    assert reg.consumed_at is None
    assert reg.consumed_by_runner_id is None


@pytest.mark.contract
def test_register_allows_duplicate_name_in_different_workspace(
    api_client, db, create_user, workspace
):
    # Same name in a second workspace is fine — uniqueness is per-workspace.
    Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        name="shared-name",
        credential_hash="h-other-ws",
        credential_fingerprint="f" * 12,
        status=RunnerStatus.OFFLINE,
    )
    other_ws = Workspace.objects.create(
        name="Other Workspace", owner=create_user, slug="other-workspace"
    )
    WorkspaceMember.objects.create(
        workspace=other_ws, member=create_user, role=20
    )
    minted = tokens.mint_registration_token()
    RunnerRegistrationToken.objects.create(
        workspace=other_ws,
        created_by=create_user,
        token_hash=minted.hashed,
        expires_at=minted.expires_at,
    )
    url = reverse("runner:register")
    resp = api_client.post(
        url,
        {
            "token": minted.raw,
            "runner_name": "shared-name",
            "os": "linux",
            "arch": "x86_64",
            "version": "0.1.0",
            "protocol_version": 1,
        },
        format="json",
    )
    assert resp.status_code == 201


@pytest.mark.contract
@pytest.mark.parametrize(
    "bad_name",
    [
        "has space",
        "dot.separated",
        "slash/separator",
        "emoji-💥",
        "semicolon;",
    ],
)
def test_register_rejects_invalid_runner_name_charset(
    api_client, registration, bad_name
):
    minted, _ = registration
    url = reverse("runner:register")
    resp = api_client.post(
        url,
        {
            "token": minted.raw,
            "runner_name": bad_name,
            "os": "linux",
            "arch": "x86_64",
            "version": "0.1.0",
            "protocol_version": 1,
        },
        format="json",
    )
    assert resp.status_code == 400
    body = resp.json()
    # DRF serializer validation puts the field-level error under the field name.
    assert "runner_name" in body, f"expected runner_name error, got {body}"


@pytest.mark.contract
def test_runner_deregister_revokes(api_client, db, registration):
    minted, _record = registration
    url = reverse("runner:register")
    resp = api_client.post(
        url,
        {
            "token": minted.raw,
            "runner_name": "laptop",
            "os": "linux",
            "arch": "x86_64",
            "version": "0.1.0",
            "protocol_version": 1,
        },
        format="json",
    )
    body = resp.json()
    deregister_url = reverse(
        "runner:deregister", kwargs={"runner_id": body["runner_id"]}
    )
    resp = api_client.post(
        deregister_url,
        {},
        HTTP_AUTHORIZATION=f"Bearer {body['runner_secret']}",
        format="json",
    )
    assert resp.status_code == 200
    runner = Runner.objects.get(id=body["runner_id"])
    assert runner.status == RunnerStatus.REVOKED
    assert runner.revoked_at is not None
