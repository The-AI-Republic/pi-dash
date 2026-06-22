# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

import hashlib
import hmac
import json
from uuid import uuid4

import pytest
from django.urls import reverse
from django.utils import timezone
from rest_framework import status

from pi_dash.db.models import (
    PlatformWebhookDelivery,
    ProjectMember,
    User,
    Workspace,
    WorkspaceMember,
)
from pi_dash.runner.models import MachineToken


pytestmark = [pytest.mark.contract, pytest.mark.django_db]


@pytest.fixture(autouse=True)
def platform_federation_settings(settings):
    settings.PLATFORM_FEDERATION_ENABLED = True
    settings.PLATFORM_IOS_WEBHOOK_SECRET = "platform-secret"
    settings.PLATFORM_IOS_ISSUER = "https://auth.example.test"
    settings.PLATFORM_IOS_AUDIENCE = "pi-dash"
    settings.PLATFORM_IOS_INTERNAL_API_BASE_URL = ""
    settings.REST_FRAMEWORK = {
        **settings.REST_FRAMEWORK,
        "DEFAULT_THROTTLE_CLASSES": (),
    }


def _event_payload(
    event_type: str,
    *,
    event_id=None,
    org_id=None,
    user_id=None,
    membership_id=None,
    membership_version=1,
    membership_status="active",
    role="member",
    role_rank=100,
    org_version=1,
    email="member@example.com",
    data=None,
):
    org_id = org_id or uuid4()
    user_id = user_id or uuid4()
    membership_id = membership_id or uuid4()
    occurred_at = timezone.now().isoformat().replace("+00:00", "Z")
    return {
        "version": "2026-06-21",
        "event_id": str(event_id or uuid4()),
        "event_type": event_type,
        "occurred_at": occurred_at,
        "actor": {"user_id": str(user_id), "email": email},
        "org": {
            "org_id": str(org_id),
            "slug": "acme-enterprise",
            "name": "Acme Enterprise",
            "version": org_version,
            "access_disabled_at": None,
        },
        "subject": {
            "membership_id": str(membership_id) if membership_id else None,
            "membership_version": membership_version,
            "membership_status": membership_status,
            "user_id": str(user_id),
            "email": email,
            "role": role,
            "role_rank": role_rank,
            "updated_at": occurred_at,
        },
        "data": data or {},
    }


def _post_signed(api_client, payload, *, secret="platform-secret", signature="valid"):
    raw = json.dumps(payload, separators=(",", ":"), sort_keys=True).encode("utf-8")
    timestamp = timezone.now().isoformat().replace("+00:00", "Z")
    digest = hmac.new(secret.encode("utf-8"), b"v1:" + timestamp.encode("utf-8") + b":" + raw, hashlib.sha256)
    header = f"sha256={digest.hexdigest()}" if signature == "valid" else "sha256=bad"
    return api_client.post(
        reverse("platform-ios-webhook"),
        data=raw,
        content_type="application/json",
        HTTP_X_IOS_DELIVERY=str(uuid4()),
        HTTP_X_IOS_EVENT=payload["event_type"],
        HTTP_X_IOS_TIMESTAMP=timestamp,
        HTTP_X_IOS_SIGNATURE_256=header,
    )


def test_platform_webhook_rejects_invalid_signature(api_client):
    payload = _event_payload("member.added")

    response = _post_signed(api_client, payload, signature="bad")

    assert response.status_code == status.HTTP_401_UNAUTHORIZED
    assert PlatformWebhookDelivery.objects.count() == 0


def test_org_created_bootstraps_workspace_and_owner_membership(api_client):
    org_id = uuid4()
    owner_id = uuid4()
    owner_membership_id = uuid4()
    payload = _event_payload(
        "org.created",
        org_id=org_id,
        user_id=owner_id,
        membership_id=None,
        role="owner",
        role_rank=1000,
        email="owner@example.com",
        data={
            "owner_membership_id": str(owner_membership_id),
            "owner_membership_version": 7,
            "owner_role": "owner",
        },
    )

    response = _post_signed(api_client, payload)

    assert response.status_code == status.HTTP_202_ACCEPTED
    assert response.data == {"status": PlatformWebhookDelivery.Status.PROCESSED}

    workspace = Workspace.objects.get(platform_org_id=org_id)
    owner = User.objects.get(platform_user_id=owner_id)
    membership = WorkspaceMember.objects.get(workspace=workspace, member=owner)
    assert workspace.slug == "acme-enterprise"
    assert workspace.platform_org_version == 1
    assert membership.role == 20
    assert membership.is_active is True
    assert membership.platform_member_id == owner_membership_id
    assert membership.platform_member_version == 7


def test_member_event_is_idempotent_and_ignores_stale_versions(api_client):
    org_id = uuid4()
    user_id = uuid4()
    membership_id = uuid4()
    payload = _event_payload(
        "member.added",
        org_id=org_id,
        user_id=user_id,
        membership_id=membership_id,
        membership_version=5,
    )

    first = _post_signed(api_client, payload)
    duplicate = _post_signed(api_client, payload)
    stale = _post_signed(
        api_client,
        _event_payload(
            "member.role_changed",
            org_id=org_id,
            user_id=user_id,
            membership_id=membership_id,
            membership_version=4,
            role="admin",
            role_rank=900,
        ),
    )

    assert first.status_code == status.HTTP_202_ACCEPTED
    assert duplicate.status_code == status.HTTP_202_ACCEPTED
    assert stale.status_code == status.HTTP_202_ACCEPTED
    assert stale.data == {"status": PlatformWebhookDelivery.Status.SKIPPED}
    membership = WorkspaceMember.objects.get(platform_member_id=membership_id)
    assert membership.platform_member_version == 5
    assert membership.role == 15
    assert PlatformWebhookDelivery.objects.count() == 2


def test_member_revoked_deactivates_local_access_and_machine_tokens(api_client, workspace, project, create_user):
    org_id = uuid4()
    membership_id = uuid4()
    create_user.platform_user_id = uuid4()
    create_user.platform_subject = f"ios:{create_user.platform_user_id}"
    create_user.platform_identity_linked_at = timezone.now()
    create_user.save(update_fields=["platform_user_id", "platform_subject", "platform_identity_linked_at", "updated_at"])
    workspace.platform_org_id = org_id
    workspace.platform_org_slug = "test-workspace"
    workspace.platform_linked_at = timezone.now()
    workspace.save(update_fields=["platform_org_id", "platform_org_slug", "platform_linked_at", "updated_at"])
    WorkspaceMember.objects.filter(workspace=workspace, member=create_user).update(
        platform_member_id=membership_id,
        platform_member_version=2,
        platform_member_status="active",
        is_active=True,
    )
    project_member = ProjectMember.objects.create(project=project, member=create_user, role=20)
    machine_token = MachineToken.objects.create(
        user=create_user,
        workspace=workspace,
        host_label="macbook",
        token_hash="hash",
    )
    payload = _event_payload(
        "member.revoked",
        org_id=org_id,
        user_id=create_user.platform_user_id,
        membership_id=membership_id,
        membership_version=3,
        membership_status="revoked",
        role="member",
        role_rank=100,
        email=create_user.email,
    )

    response = _post_signed(api_client, payload)

    assert response.status_code == status.HTTP_202_ACCEPTED
    membership = WorkspaceMember.objects.get(workspace=workspace, member=create_user)
    project_member.refresh_from_db()
    machine_token.refresh_from_db()
    assert membership.is_active is False
    assert membership.platform_member_status == "revoked"
    assert project_member.is_active is False
    assert machine_token.revoked_at is not None


def test_platform_session_consumes_launch_token(api_client, monkeypatch, workspace, create_user):
    create_user.platform_user_id = uuid4()
    create_user.platform_subject = f"ios:{create_user.platform_user_id}"
    create_user.platform_identity_linked_at = timezone.now()
    create_user.save(update_fields=["platform_user_id", "platform_subject", "platform_identity_linked_at", "updated_at"])
    org_id = uuid4()
    workspace.platform_org_id = org_id
    workspace.platform_org_slug = workspace.slug
    workspace.platform_linked_at = timezone.now()
    workspace.save(update_fields=["platform_org_id", "platform_org_slug", "platform_linked_at", "updated_at"])
    WorkspaceMember.objects.filter(workspace=workspace, member=create_user).update(
        platform_member_id=uuid4(),
        platform_member_version=1,
        platform_member_status="active",
        is_active=True,
    )

    monkeypatch.setattr(
        "pi_dash.core.platform_federation.verify_platform_token",
        lambda _token: {
            "sub": str(create_user.platform_user_id),
            "user_id": str(create_user.platform_user_id),
            "email": create_user.email,
            "active_org_id": str(org_id),
            "aud": "pi-dash",
        },
    )

    response = api_client.post(reverse("platform-session"), {"access_token": "launch-token"}, format="json")

    assert response.status_code == status.HTTP_200_OK
    assert response.data["workspace"]["slug"] == workspace.slug
    create_user.profile.refresh_from_db()
    assert create_user.profile.last_workspace_id == workspace.id
