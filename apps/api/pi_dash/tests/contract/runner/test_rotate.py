# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from django.urls import reverse

from pi_dash.runner.models import Runner, RunnerStatus
from pi_dash.runner.services import tokens


@pytest.fixture
def provisioned_runner(db, create_user, workspace):
    minted = tokens.mint_runner_secret()
    runner = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        name="rot",
        credential_hash=minted.hashed,
        credential_fingerprint=minted.fingerprint,
        status=RunnerStatus.ONLINE,
    )
    return runner, minted.raw


@pytest.mark.contract
def test_rotate_returns_new_secret_and_invalidates_old(api_client, provisioned_runner):
    runner, old_secret = provisioned_runner
    url = reverse("runner:rotate", kwargs={"runner_id": runner.id})
    resp = api_client.post(
        url,
        {},
        HTTP_AUTHORIZATION=f"Bearer {old_secret}",
        format="json",
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["runner_secret"] != old_secret
    # Old secret no longer authenticates.
    resp2 = api_client.post(
        url,
        {},
        HTTP_AUTHORIZATION=f"Bearer {old_secret}",
        format="json",
    )
    assert resp2.status_code in (401, 403)
    # New secret does.
    resp3 = api_client.post(
        url,
        {},
        HTTP_AUTHORIZATION=f"Bearer {body['runner_secret']}",
        format="json",
    )
    assert resp3.status_code == 200


@pytest.mark.contract
def test_rotate_rejects_cross_runner_credential(api_client, db, create_user, workspace):
    minted_a = tokens.mint_runner_secret()
    runner_a = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        name="a",
        credential_hash=minted_a.hashed,
        credential_fingerprint=minted_a.fingerprint,
        status=RunnerStatus.ONLINE,
    )
    minted_b = tokens.mint_runner_secret()
    runner_b = Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        name="b",
        credential_hash=minted_b.hashed,
        credential_fingerprint=minted_b.fingerprint,
        status=RunnerStatus.ONLINE,
    )
    url = reverse("runner:rotate", kwargs={"runner_id": runner_a.id})
    resp = api_client.post(
        url,
        {},
        HTTP_AUTHORIZATION=f"Bearer {minted_b.raw}",
        format="json",
    )
    assert resp.status_code == 403
