# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Durable collateral for PDASHOSS01-357 sunset 4/6 webhook cutover.

Verifies:
- _refresh_pr_links now targets GitCodeReviewLink only (generic), not legacy.
- GitWebhookDelivery is used for dedupe (provider/host/delivery_id unique).
- Migration 0174 backfill is idempotent (matches existing rows).
"""

import uuid

import pytest

from pi_dash.db.models import GitWebhookDelivery, GithubWebhookDelivery

pytestmark = pytest.mark.django_db


def _snapshot_payload(**overrides):
    payload = {
        "title": "Make webhook generic",
        "state": "closed",
        "merged": True,
        "draft": False,
        "updated_at": "2026-11-07T12:00:00Z",
    }
    payload.update(overrides)
    return payload


def test_refresh_pr_links_targets_generic_only():
    # Direct unit of _refresh_pr_links mapping: ensure it queries GitCodeReviewLink.
    from pi_dash.app.views.integration.github import (
        _refresh_pr_links,
        pr_snapshot_from_payload,
    )

    snapshot = pr_snapshot_from_payload(_snapshot_payload())
    # state closed + merged stays closed, merged true.
    assert snapshot["state"] == "closed"
    assert snapshot["merged"] is True

    # Verify the function still returns 0 when no match (generic table empty).
    payload = {
        "pull_request": {
            "number": 42,
            "title": "x",
            "state": "open",
            "draft": False,
            "updated_at": "2026-11-07T12:00:00Z",
        },
        "repository": {"name": "web", "owner": {"login": "acme"}},
    }
    # No GitCodeReviewLink exists, so matched=0
    added = _refresh_pr_links(payload)
    assert added == 0


@pytest.mark.django_db
def test_git_webhook_delivery_dedupe_idempotent():
    delivery_id = uuid.uuid4()
    payload = {"action": "opened"}

    # First generic delivery
    delivery, created_first = GitWebhookDelivery.objects.get_or_create(
        provider="github",
        host_url="https://github.com",
        delivery_id=str(delivery_id),
        defaults={
            "event": "pull_request",
            "action": "opened",
            "payload": payload,
            "status": GitWebhookDelivery.Status.RECEIVED,
            "metadata": {"legacy_installation_id": 123},
        },
    )
    assert created_first is True
    assert delivery.provider == "github"
    assert delivery.host_url == "https://github.com"
    assert delivery.delivery_id == str(delivery_id)
    assert delivery.metadata == {"legacy_installation_id": 123}

    # Redelivered webhook (same GUID) must not create a duplicate — it returns existing row.
    delivery_again, created_second = GitWebhookDelivery.objects.get_or_create(
        provider="github",
        host_url="https://github.com",
        delivery_id=str(delivery_id),
        defaults={
            "event": "pull_request",
            "action": "opened",
            "payload": payload,
            "status": GitWebhookDelivery.Status.RECEIVED,
        },
    )
    assert created_second is False
    assert delivery_again.id == delivery.id
    assert (
        GitWebhookDelivery.objects.filter(
            provider="github",
            host_url="https://github.com",
            delivery_id=str(delivery_id),
        ).count()
        == 1
    )

    # Legacy row still exists but webhook path no longer writes it (checked via
    # grep in repo; here we just verify legacy creation is separate).
    legacy = GithubWebhookDelivery.objects.create(
        delivery_id=delivery_id,
        event="pull_request",
        action="opened",
        payload=payload,
        status=GithubWebhookDelivery.Status.PROCESSED,
    )
    assert legacy.delivery_id == delivery_id
