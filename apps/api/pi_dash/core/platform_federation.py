# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

import hashlib
import hmac
import json
import re
import uuid
from datetime import timedelta, timezone as datetime_timezone
from typing import Any

import jwt
import requests
from django.conf import settings
from django.db import transaction
from django.db.models import Q
from django.utils import timezone
from django.utils.dateparse import parse_datetime

from pi_dash.db.models import (
    PlatformFederationState,
    ProjectMember,
    User,
    Workspace,
    WorkspaceMember,
)

ROLE_ADMIN = 20
ROLE_MEMBER = 15
ROLE_GUEST = 5

MEMBER_ACTIVE_STATUSES = {"active"}
MEMBER_DISABLED_STATUSES = {"revoked", "suspended", "disabled", "inactive"}
ACCESS_CUTTING_EVENTS = {"member.revoked", "member.suspended", "org.access_disabled"}

_JWKS_CACHE: dict[str, Any] = {"keys": [], "expires_at": None}


class PlatformFederationError(Exception):
    pass


class PlatformConfigurationError(PlatformFederationError):
    pass


class PlatformAuthError(PlatformFederationError):
    pass


class PlatformForbiddenError(PlatformFederationError):
    pass


def platform_federation_enabled() -> bool:
    return bool(getattr(settings, "PLATFORM_FEDERATION_ENABLED", False))


def _require_enabled() -> None:
    if not platform_federation_enabled():
        raise PlatformConfigurationError("platform_federation_disabled")


def _parse_uuid(value: Any, *, field: str) -> uuid.UUID:
    try:
        return uuid.UUID(str(value))
    except (TypeError, ValueError) as exc:
        raise PlatformFederationError(f"invalid_{field}") from exc


def _parse_datetime(value: Any):
    if value in (None, ""):
        return None
    if hasattr(value, "tzinfo"):
        dt = value
    else:
        dt = parse_datetime(str(value))
    if dt is None:
        raise PlatformFederationError("invalid_datetime")
    if timezone.is_naive(dt):
        return timezone.make_aware(dt, datetime_timezone.utc)
    return dt


def map_platform_role(role: str | None, role_rank: int | None = None) -> int:
    normalized = (role or "").lower()
    rank = int(role_rank or 0)
    if normalized in {"owner", "admin"} or rank >= 900:
        return ROLE_ADMIN
    if normalized == "member" or rank >= 100:
        return ROLE_MEMBER
    return ROLE_GUEST


def _unique_username(email: str, platform_user_id: uuid.UUID | None = None) -> str:
    base = re.sub(r"[^a-zA-Z0-9_.-]", "-", (email.split("@", 1)[0] or "platform-user")).strip("-")
    base = (base or "platform-user")[:96]
    candidate = base
    suffix = 1
    while User.objects.filter(username=candidate).exists():
        if platform_user_id and suffix == 1:
            candidate = f"{base}-{str(platform_user_id)[:8]}"[:128]
        else:
            candidate = f"{base}-{suffix}"[:128]
        suffix += 1
    return candidate


def _unique_workspace_slug(slug: str, platform_org_id: uuid.UUID) -> str:
    cleaned = re.sub(r"[^a-zA-Z0-9_-]", "-", (slug or "workspace").lower()).strip("-")
    base = (cleaned or "workspace")[:48]
    candidate = base
    suffix = str(platform_org_id)[:8]
    if not Workspace.objects.filter(slug=candidate).exists():
        return candidate
    candidate = f"{base[:39]}-{suffix}"[:48]
    counter = 1
    while Workspace.objects.filter(slug=candidate).exists():
        candidate = f"{base[:35]}-{suffix}-{counter}"[:48]
        counter += 1
    return candidate


def _schedule_workspace_seed(workspace_id) -> None:
    try:
        from pi_dash.bgtasks.workspace_seed_task import workspace_seed

        workspace_seed.delay(str(workspace_id))
    except Exception:
        pass


def _identity_from_subject(subject: dict[str, Any]) -> dict[str, Any]:
    user_id = subject.get("user_id")
    return {
        "platform_user_id": _parse_uuid(user_id, field="user_id") if user_id else None,
        "platform_subject": f"ios:{user_id}" if user_id else "",
        "email": (subject.get("email") or "").strip().lower(),
        "display_name": subject.get("display_name") or "",
    }


def _identity_from_actor(actor: dict[str, Any]) -> dict[str, Any]:
    user_id = actor.get("user_id")
    return {
        "platform_user_id": _parse_uuid(user_id, field="actor_user_id") if user_id else None,
        "platform_subject": f"ios:{user_id}" if user_id else "",
        "email": (actor.get("email") or "").strip().lower(),
        "display_name": actor.get("display_name") or "",
    }


def _identity_from_claims(claims: dict[str, Any]) -> dict[str, Any]:
    user_id = claims.get("user_id") or claims.get("sub")
    platform_user_id = _parse_uuid(user_id, field="user_id") if user_id else None
    return {
        "platform_user_id": platform_user_id,
        "platform_subject": claims.get("sub") or (f"ios:{user_id}" if user_id else ""),
        "email": (claims.get("email") or "").strip().lower(),
        "display_name": claims.get("name") or claims.get("display_name") or "",
    }


def upsert_platform_user(identity: dict[str, Any]) -> User:
    platform_user_id = identity.get("platform_user_id")
    platform_subject = (identity.get("platform_subject") or "").strip() or None
    email = (identity.get("email") or "").strip().lower()
    display_name = (identity.get("display_name") or "").strip()
    if not platform_user_id and not platform_subject:
        raise PlatformFederationError("platform_user_identity_required")
    if not email:
        raise PlatformFederationError("platform_user_email_required")

    user = None
    if platform_user_id:
        user = User.objects.select_for_update().filter(platform_user_id=platform_user_id).first()
    if user is None and platform_subject:
        user = User.objects.select_for_update().filter(platform_subject=platform_subject).first()
    if user is None:
        email_match = User.objects.select_for_update().filter(email=email).first()
        if email_match is not None:
            if email_match.platform_user_id and email_match.platform_user_id != platform_user_id:
                raise PlatformFederationError("email_already_linked_to_different_platform_user")
            user = email_match

    now = timezone.now()
    if user is None:
        user = User(
            email=email,
            username=_unique_username(email, platform_user_id),
            display_name=display_name or email.split("@", 1)[0],
            is_managed=True,
            is_email_verified=True,
            platform_user_id=platform_user_id,
            platform_subject=platform_subject,
            platform_identity_linked_at=now,
        )
        user.set_unusable_password()
        user.save()
        return user

    update_fields = []
    if platform_user_id and user.platform_user_id != platform_user_id:
        user.platform_user_id = platform_user_id
        update_fields.append("platform_user_id")
    if platform_subject and user.platform_subject != platform_subject:
        user.platform_subject = platform_subject
        update_fields.append("platform_subject")
    if user.platform_identity_linked_at is None:
        user.platform_identity_linked_at = now
        update_fields.append("platform_identity_linked_at")
    if user.email != email:
        user.email = email
        update_fields.append("email")
    if display_name and not user.display_name:
        user.display_name = display_name
        update_fields.append("display_name")
    if not user.username:
        user.username = _unique_username(email, platform_user_id)
        update_fields.append("username")
    if update_fields:
        update_fields.append("updated_at")
        user.save(update_fields=sorted(set(update_fields)))
    return user


def _actor_or_subject_user(payload: dict[str, Any]) -> User:
    subject = payload.get("subject") or {}
    actor = payload.get("actor") or {}
    identity = _identity_from_subject(subject)
    if not identity["platform_user_id"] and actor.get("user_id"):
        identity = _identity_from_actor(actor)
    return upsert_platform_user(identity)


def _get_or_create_platform_workspace(org: dict[str, Any], owner: User) -> tuple[Workspace, bool]:
    platform_org_id = _parse_uuid(org.get("org_id"), field="org_id")
    slug = org.get("slug") or str(platform_org_id)
    name = (org.get("name") or slug)[:80]
    now = timezone.now()

    workspace = Workspace.objects.select_for_update().filter(platform_org_id=platform_org_id).first()
    created = False
    if workspace is None:
        workspace = (
            Workspace.objects.select_for_update()
            .filter(slug=slug, platform_org_id__isnull=True, deleted_at__isnull=True)
            .first()
        )
    if workspace is None:
        workspace = Workspace.objects.create(
            name=name,
            owner=owner,
            slug=_unique_workspace_slug(slug, platform_org_id),
            platform_org_id=platform_org_id,
            platform_org_slug=slug[:128],
            platform_linked_at=now,
        )
        created = True
        transaction.on_commit(lambda workspace_id=workspace.id: _schedule_workspace_seed(workspace_id))

    update_fields = []
    if workspace.platform_org_id != platform_org_id:
        workspace.platform_org_id = platform_org_id
        update_fields.append("platform_org_id")
    if workspace.platform_org_slug != slug[:128]:
        workspace.platform_org_slug = slug[:128]
        update_fields.append("platform_org_slug")
    if workspace.platform_linked_at is None:
        workspace.platform_linked_at = now
        update_fields.append("platform_linked_at")
    if workspace.name != name:
        workspace.name = name
        update_fields.append("name")
    incoming_version = int(org.get("version") or 0)
    if incoming_version >= int(workspace.platform_org_version or 0):
        disabled_at = _parse_datetime(org.get("access_disabled_at"))
        if workspace.platform_org_version != incoming_version:
            workspace.platform_org_version = incoming_version
            update_fields.append("platform_org_version")
        if workspace.platform_access_disabled_at != disabled_at:
            workspace.platform_access_disabled_at = disabled_at
            update_fields.append("platform_access_disabled_at")
    if update_fields:
        update_fields.append("updated_at")
        workspace.save(update_fields=sorted(set(update_fields)))
    return workspace, created


def _upsert_federation_state(
    workspace: Workspace,
    *,
    status: str,
    event_id: uuid.UUID | None = None,
    reconciled: bool = False,
    error: str = "",
) -> None:
    defaults = {"status": status, "last_error": error}
    if event_id:
        defaults["last_event_id"] = event_id
    if reconciled:
        defaults["last_reconciled_at"] = timezone.now()
    PlatformFederationState.objects.update_or_create(workspace=workspace, defaults=defaults)


def _ensure_owner_membership_from_org_event(workspace: Workspace, owner: User, payload: dict[str, Any]) -> None:
    data = payload.get("data") or {}
    member_id = data.get("owner_membership_id")
    member_version = int(data.get("owner_membership_version") or 0)
    if not member_id:
        return
    role = map_platform_role(data.get("owner_role") or "owner", 1000)
    membership_uuid = _parse_uuid(member_id, field="membership_id")
    member, _ = WorkspaceMember.objects.select_for_update().get_or_create(
        workspace=workspace,
        member=owner,
        defaults={
            "role": role,
            "is_active": True,
            "platform_member_id": membership_uuid,
            "platform_member_version": member_version,
            "platform_member_status": "active",
            "platform_last_event_id": _parse_uuid(payload.get("event_id"), field="event_id"),
            "platform_last_event_at": _parse_datetime(payload.get("occurred_at")) or timezone.now(),
        },
    )
    update_fields = []
    if member.role != role:
        member.role = role
        update_fields.append("role")
    if not member.is_active:
        member.is_active = True
        update_fields.append("is_active")
    if member.platform_member_id != membership_uuid:
        member.platform_member_id = membership_uuid
        update_fields.append("platform_member_id")
    if member_version >= int(member.platform_member_version or 0):
        member.platform_member_version = member_version
        member.platform_member_status = "active"
        update_fields.extend(["platform_member_version", "platform_member_status"])
    if update_fields:
        update_fields.append("updated_at")
        member.save(update_fields=sorted(set(update_fields)))


def _revoke_runtime_access(workspace: Workspace, user: User | None = None) -> None:
    now = timezone.now()
    try:
        from pi_dash.runner.models import MachineToken, Runner, RunnerSession

        token_qs = MachineToken.objects.filter(workspace=workspace, revoked_at__isnull=True)
        runner_qs = Runner.objects.filter(workspace=workspace, revoked_at__isnull=True)
        if user is not None:
            token_qs = token_qs.filter(user=user)
            runner_qs = runner_qs.filter(owner=user)
        token_qs.update(revoked_at=now)
        for runner in runner_qs.select_for_update():
            runner.revoke(reason="membership_revoked")
        session_qs = RunnerSession.objects.filter(runner__workspace=workspace, revoked_at__isnull=True)
        if user is not None:
            session_qs = session_qs.filter(runner__owner=user)
        session_qs.update(revoked_at=now, revoked_reason="membership_revoked")
    except Exception:
        pass


def apply_platform_event(payload: dict[str, Any]) -> str:
    _require_enabled()
    event_type = payload.get("event_type") or ""
    event_id = _parse_uuid(payload.get("event_id"), field="event_id")
    if not event_type:
        raise PlatformFederationError("event_type_required")

    with transaction.atomic():
        if event_type.startswith("org."):
            return _apply_org_event(payload, event_id)
        if event_type.startswith("member."):
            return _apply_member_event(payload, event_id)
    return PlatformWebhookDeliveryStatus.SKIPPED


def _apply_org_event(payload: dict[str, Any], event_id: uuid.UUID) -> str:
    org = payload.get("org") or {}
    owner = _actor_or_subject_user(payload)
    workspace, _ = _get_or_create_platform_workspace(org, owner)
    incoming_version = int(org.get("version") or 0)
    if incoming_version < int(workspace.platform_org_version or 0):
        _upsert_federation_state(workspace, status=PlatformFederationState.Status.ACTIVE, event_id=event_id)
        return "skipped"

    if payload.get("event_type") == "org.access_disabled":
        disabled_at = _parse_datetime(org.get("access_disabled_at")) or timezone.now()
        Workspace.objects.filter(pk=workspace.pk).update(
            platform_access_disabled_at=disabled_at,
            platform_org_version=incoming_version,
            updated_at=timezone.now(),
        )
        _revoke_runtime_access(workspace)
        _upsert_federation_state(workspace, status=PlatformFederationState.Status.DISABLED, event_id=event_id)
        return "processed"

    _ensure_owner_membership_from_org_event(workspace, owner, payload)
    state = PlatformFederationState.Status.DISABLED if workspace.platform_access_disabled_at else PlatformFederationState.Status.ACTIVE
    _upsert_federation_state(workspace, status=state, event_id=event_id)
    return "processed"


def _apply_member_event(payload: dict[str, Any], event_id: uuid.UUID) -> str:
    org = payload.get("org") or {}
    subject = payload.get("subject") or {}
    if not subject.get("user_id") or not subject.get("membership_id"):
        raise PlatformFederationError("member_event_subject_required")

    user = upsert_platform_user(_identity_from_subject(subject))
    workspace, _ = _get_or_create_platform_workspace(org, user)
    membership_uuid = _parse_uuid(subject.get("membership_id"), field="membership_id")
    membership_version = int(subject.get("membership_version") or 0)
    occurred_at = _parse_datetime(subject.get("updated_at") or payload.get("occurred_at")) or timezone.now()

    member = (
        WorkspaceMember.objects.select_for_update()
        .filter(Q(platform_member_id=membership_uuid) | Q(workspace=workspace, member=user), workspace=workspace)
        .first()
    )
    if member is not None and membership_version < int(member.platform_member_version or 0):
        _upsert_federation_state(workspace, status=PlatformFederationState.Status.ACTIVE, event_id=event_id)
        return "skipped"

    event_type = payload.get("event_type") or ""
    member_status = (subject.get("membership_status") or "").lower()
    active = event_type not in ACCESS_CUTTING_EVENTS and member_status not in MEMBER_DISABLED_STATUSES
    role = map_platform_role(subject.get("role"), subject.get("role_rank"))

    if member is None:
        member = WorkspaceMember.objects.create(
            workspace=workspace,
            member=user,
            role=role,
            is_active=active,
            platform_member_id=membership_uuid,
            platform_member_version=membership_version,
            platform_member_status=member_status or ("active" if active else "revoked"),
            platform_last_event_id=event_id,
            platform_last_event_at=occurred_at,
        )
    else:
        member.role = role
        member.is_active = active
        member.platform_member_id = membership_uuid
        member.platform_member_version = membership_version
        member.platform_member_status = member_status or ("active" if active else "revoked")
        member.platform_last_event_id = event_id
        member.platform_last_event_at = occurred_at
        member.save(
            update_fields=[
                "role",
                "is_active",
                "platform_member_id",
                "platform_member_version",
                "platform_member_status",
                "platform_last_event_id",
                "platform_last_event_at",
                "updated_at",
            ]
        )

    if active:
        ProjectMember.objects.filter(workspace=workspace, member=user, is_active=True).update(role=role)
    else:
        ProjectMember.objects.filter(workspace=workspace, member=user, is_active=True).update(is_active=False)
        _revoke_runtime_access(workspace, user)

    state = PlatformFederationState.Status.DISABLED if workspace.platform_access_disabled_at else PlatformFederationState.Status.ACTIVE
    _upsert_federation_state(workspace, status=state, event_id=event_id)
    return "processed"


def _fetch_jwks() -> list[dict[str, Any]]:
    _require_enabled()
    jwks_url = getattr(settings, "PLATFORM_IOS_JWKS_URL", "")
    if not jwks_url:
        raise PlatformConfigurationError("platform_jwks_url_not_configured")
    now = timezone.now()
    expires_at = _JWKS_CACHE.get("expires_at")
    if _JWKS_CACHE["keys"] and expires_at and expires_at > now:
        return _JWKS_CACHE["keys"]
    headers = {}
    internal_token = getattr(settings, "PLATFORM_IOS_INTERNAL_API_TOKEN", "")
    if internal_token:
        headers["Authorization"] = f"Bearer {internal_token}"
    response = requests.get(
        jwks_url,
        headers=headers,
        timeout=getattr(settings, "PLATFORM_IOS_HTTP_TIMEOUT_SECONDS", 5),
    )
    response.raise_for_status()
    keys = response.json().get("keys") or []
    _JWKS_CACHE["keys"] = keys
    _JWKS_CACHE["expires_at"] = now + timedelta(seconds=getattr(settings, "PLATFORM_IOS_JWKS_CACHE_SECONDS", 300))
    return keys


def verify_platform_token(token: str) -> dict[str, Any]:
    _require_enabled()
    issuer = getattr(settings, "PLATFORM_IOS_ISSUER", "")
    audience = getattr(settings, "PLATFORM_IOS_AUDIENCE", "pi-dash")
    if not issuer:
        raise PlatformConfigurationError("platform_issuer_not_configured")
    try:
        header = jwt.get_unverified_header(token)
    except jwt.PyJWTError as exc:
        raise PlatformAuthError("invalid_token_header") from exc
    kid = header.get("kid")
    jwk = next((key for key in _fetch_jwks() if key.get("kid") == kid), None)
    if jwk is None:
        _JWKS_CACHE["expires_at"] = None
        jwk = next((key for key in _fetch_jwks() if key.get("kid") == kid), None)
    if jwk is None:
        raise PlatformAuthError("token_key_not_found")
    try:
        public_key = jwt.algorithms.RSAAlgorithm.from_jwk(json.dumps(jwk))
        return jwt.decode(
            token,
            public_key,
            algorithms=["RS256"],
            audience=audience,
            issuer=issuer,
            options={"require": ["exp", "iat", "iss", "aud", "sub"]},
        )
    except jwt.PyJWTError as exc:
        raise PlatformAuthError("invalid_platform_token") from exc


def consume_platform_session_token(token: str) -> tuple[User, Workspace]:
    claims = verify_platform_token(token)
    platform_org_id = _parse_uuid(claims.get("active_org_id") or claims.get("org"), field="org_id")
    identity = _identity_from_claims(claims)

    def _lookup() -> tuple[User, Workspace | None, WorkspaceMember | None]:
        with transaction.atomic():
            local_user = upsert_platform_user(identity)
            local_workspace = Workspace.objects.select_for_update().filter(platform_org_id=platform_org_id).first()
            local_membership = None
            if local_workspace is not None:
                local_membership = (
                    WorkspaceMember.objects.select_for_update()
                    .filter(workspace=local_workspace, member=local_user, is_active=True)
                    .first()
                )
            return local_user, local_workspace, local_membership

    user, workspace, membership = _lookup()
    if workspace is None or membership is None:
        reconcile_platform_org(platform_org_id)
        user, workspace, membership = _lookup()
    if workspace is None:
        raise PlatformForbiddenError("workspace_not_linked")
    if workspace.platform_access_disabled_at is not None:
        raise PlatformForbiddenError("workspace_access_disabled")
    if membership is None:
        raise PlatformForbiddenError("active_membership_required")
    return user, workspace


def _internal_api_headers() -> dict[str, str]:
    token = getattr(settings, "PLATFORM_IOS_INTERNAL_API_TOKEN", "")
    return {"Authorization": f"Bearer {token}"} if token else {}


def reconcile_platform_org(platform_org_id: uuid.UUID | str) -> dict[str, int]:
    _require_enabled()
    base_url = getattr(settings, "PLATFORM_IOS_INTERNAL_API_BASE_URL", "").rstrip("/")
    if not base_url:
        raise PlatformConfigurationError("platform_internal_api_not_configured")
    org_uuid = _parse_uuid(platform_org_id, field="org_id")
    response = requests.get(
        f"{base_url}/orgs/{org_uuid}/members",
        params={"include_inactive": "true"},
        headers=_internal_api_headers(),
        timeout=getattr(settings, "PLATFORM_IOS_HTTP_TIMEOUT_SECONDS", 5),
    )
    response.raise_for_status()
    snapshot = response.json()
    org = snapshot.get("org") or {}
    members = snapshot.get("members") or []
    applied = 0
    skipped = 0
    with transaction.atomic():
        actor_user = None
        if members:
            actor_user = upsert_platform_user(_identity_from_subject({"user_id": members[0].get("user_id"), "email": members[0].get("email")}))
        if actor_user is None:
            raise PlatformFederationError("reconcile_requires_at_least_one_member")
        workspace, _ = _get_or_create_platform_workspace(org, actor_user)
        for item in members:
            event_payload = {
                "event_id": str(uuid.uuid4()),
                "event_type": "member.reconciled",
                "occurred_at": item.get("updated_at") or timezone.now().isoformat(),
                "org": org,
                "subject": {
                    "membership_id": item.get("membership_id"),
                    "membership_version": item.get("membership_version") or 0,
                    "membership_status": item.get("status") or "active",
                    "user_id": item.get("user_id"),
                    "email": item.get("email"),
                    "display_name": item.get("display_name"),
                    "role": item.get("role"),
                    "role_rank": item.get("role_rank"),
                    "updated_at": item.get("updated_at"),
                },
                "data": {"source": "reconcile"},
            }
            result = _apply_member_event(event_payload, _parse_uuid(event_payload["event_id"], field="event_id"))
            if result == "processed":
                applied += 1
            else:
                skipped += 1
        status = PlatformFederationState.Status.DISABLED if workspace.platform_access_disabled_at else PlatformFederationState.Status.ACTIVE
        _upsert_federation_state(workspace, status=status, reconciled=True)
    return {"applied": applied, "skipped": skipped}


def verify_ios_webhook_signature(raw_body: bytes, timestamp: str | None, signature: str | None) -> bool:
    _require_enabled()
    secret = getattr(settings, "PLATFORM_IOS_WEBHOOK_SECRET", "")
    if not secret:
        raise PlatformConfigurationError("platform_webhook_secret_not_configured")
    if not timestamp or not signature:
        return False
    signed_at = _parse_datetime(timestamp)
    if signed_at is None or abs(timezone.now() - signed_at) > timedelta(minutes=5):
        return False
    signed = b"v1:" + timestamp.encode("utf-8") + b":" + raw_body
    digest = hmac.new(secret.encode("utf-8"), signed, hashlib.sha256).hexdigest()
    expected = f"sha256={digest}"
    return hmac.compare_digest(expected, signature)


class PlatformWebhookDeliveryStatus:
    PROCESSED = "processed"
    FAILED = "failed"
    SKIPPED = "skipped"
    DEAD_LETTERED = "dead_lettered"
