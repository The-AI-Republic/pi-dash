# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from datetime import timedelta

import pytest
from django.urls import reverse
from django.utils import timezone

from apple_pi_dash.runner.models import (
    AgentRunStatus,
    Runner,
    RunnerRegistrationToken,
    RunnerStatus,
)
from apple_pi_dash.runner.services import tokens


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
        "protocol_version": 1,
    }
    resp = api_client.post(url, payload, format="json")
    assert resp.status_code == 201
    body = resp.json()
    assert body["runner_secret"].startswith("apd_rs_")
    assert body["protocol_version"] == 1
    runner = Runner.objects.get(id=body["runner_id"])
    assert runner.owner_id == record.created_by_id
    record.refresh_from_db()
    assert record.consumed_at is not None
    assert record.consumed_by_runner_id == runner.id


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
