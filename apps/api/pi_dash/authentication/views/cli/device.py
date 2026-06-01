# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Device-authorization flow for `pidash auth login` (RFC 8628-shaped).

Three endpoints make up the dance:

* ``POST /api/v1/auth/device/start/`` — AllowAny. CLI requests a
  ``device_code``/``user_code`` pair. ``device_code`` stays on the CLI;
  ``user_code`` is shown to the human so they can type it into the web UI.
* ``POST /api/v1/auth/device/approve/`` — session-authenticated. The
  logged-in human submits the ``user_code`` they read off their terminal,
  stamping the row with ``user`` + ``approved``.
* ``POST /api/v1/auth/device/token/`` — AllowAny. CLI polls with the
  ``device_code`` it kept; once the row is approved we mint an
  :class:`APIToken` for the user and consume the row.

Also hosts ``POST /api/v1/auth/revoke/`` — used by ``pidash auth logout``
to invalidate the caller's CLI token server-side.
"""

# Python imports
import logging
import uuid as _uuid
from datetime import timedelta

# Django imports
from django.db import IntegrityError, transaction
from django.utils import timezone

# Third party imports
from rest_framework import status
from rest_framework.permissions import AllowAny, IsAuthenticated
from rest_framework.response import Response
from rest_framework.throttling import AnonRateThrottle
from rest_framework.views import APIView

# Module imports
from pi_dash.api.middleware.api_authentication import APIKeyAuthentication
from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.db.models import APIToken, CLIDeviceCode, WorkspaceMember
from pi_dash.authentication.utils.host import base_host
from pi_dash.runner.models import DevMachine, MachineToken
from pi_dash.runner.services import tokens as runner_tokens
from pi_dash.runner.services.permissions import is_workspace_member

logger = logging.getLogger("pi_dash.auth.cli")


# RFC 8628 §3.2 — we choose 10 minutes for the grant window and 5 seconds
# as the recommended polling interval. The CLI must honor `slow_down` if
# we return it; we double the floor on each violation up to 30s.
DEVICE_CODE_TTL = timedelta(minutes=10)
DEVICE_CODE_POLL_INTERVAL_SECONDS = 5
DEVICE_CODE_MIN_POLL_GAP = timedelta(seconds=3)

# Generated user_codes have ~3×10^11 entropy across the 10-min window,
# so collisions are vanishingly rare in practice — but a single
# collision still 500s the request, which we'd rather convert into a
# retry. Bounded so a pathological alphabet exhaustion can't loop.
DEVICE_CODE_START_MAX_RETRIES = 5


class DevMachineOwnershipError(Exception):
    pass


def _touch_dev_machine(machine: DevMachine, *, host_label: str) -> DevMachine:
    host_label = (host_label or "").strip()[:255]
    now = timezone.now()
    update_fields = ["last_seen_at", "updated_at"]
    machine.last_seen_at = now
    if host_label and machine.host_label != host_label:
        machine.host_label = host_label
        update_fields.append("host_label")
    if host_label and not machine.label:
        machine.label = host_label[:128]
        update_fields.append("label")
    machine.save(update_fields=update_fields)
    return machine


def _get_or_create_dev_machine(*, user, dev_machine_id: _uuid.UUID, host_label: str) -> DevMachine:
    host_label = (host_label or "").strip()[:255]
    now = timezone.now()
    locked = DevMachine.objects.select_for_update().filter(pk=dev_machine_id).first()
    if locked is not None:
        if locked.owner_id != user.id:
            raise DevMachineOwnershipError
        return _touch_dev_machine(locked, host_label=host_label)
    try:
        with transaction.atomic():
            return DevMachine.objects.create(
                id=dev_machine_id,
                owner=user,
                host_label=host_label,
                label=host_label[:128],
                last_seen_at=now,
            )
    except IntegrityError:
        locked = DevMachine.objects.select_for_update().filter(pk=dev_machine_id).first()
        if locked is None or locked.owner_id != user.id:
            raise DevMachineOwnershipError
        return _touch_dev_machine(locked, host_label=host_label)


def _rotate_machine_token(*, user, workspace, dev_machine: DevMachine, host_label: str) -> runner_tokens.MintedToken:
    MachineToken.objects.select_for_update().filter(
        workspace=workspace,
        dev_machine=dev_machine,
        revoked_at__isnull=True,
    ).update(revoked_at=timezone.now())
    minted = runner_tokens.mint_machine_token()
    MachineToken.objects.create(
        user=user,
        dev_machine=dev_machine,
        workspace=workspace,
        host_label=host_label[:255],
        token_hash=minted.hashed,
        token_fingerprint=minted.fingerprint,
        label=f"machine: {host_label[:96]}",
        is_service=True,
    )
    return minted


class DeviceCodeStartThrottle(AnonRateThrottle):
    """Per-IP cap on `device/start/` to keep the table from being
    filled with junk by an unauthenticated caller. The legit flow
    needs one call per `pidash auth login`, so a modest cap is fine.
    """

    scope = "auth_device_start"


def _verification_uri(request) -> str:
    return f"{base_host(request=request).rstrip('/')}/auth/device/"


class DeviceCodeStartEndpoint(APIView):
    """RFC 8628 §3.1 — issue a device/user code pair.

    Anonymous endpoint. Anyone with network access can request a code;
    this is harmless because the code only unlocks anything once a
    logged-in human explicitly approves it via the web UI. Throttled
    per IP so a malicious caller can't fill the table.
    """

    permission_classes = [AllowAny]
    authentication_classes: list = []
    throttle_classes = [DeviceCodeStartThrottle]

    def post(self, request):
        expires_at = timezone.now() + DEVICE_CODE_TTL
        # Retry on the (vanishingly improbable) unique-constraint
        # collision so a 1-in-10^11 unlucky generator output doesn't
        # surface a 500 to the user.
        for _attempt in range(DEVICE_CODE_START_MAX_RETRIES):
            try:
                row = CLIDeviceCode.objects.create(
                    expires_at=expires_at,
                    # No user/workspace yet — the row is "pending" until
                    # the human approves it in their browser session.
                )
                break
            except IntegrityError as exc:
                logger.warning("CLIDeviceCode create collided, retrying: %s", exc)
        else:
            return Response(
                {"error": "internal_error", "error_description": "Could not allocate a device code; try again."},
                status=status.HTTP_503_SERVICE_UNAVAILABLE,
            )
        return Response(
            {
                "device_code": row.device_code,
                "user_code": row.user_code,
                "verification_uri": _verification_uri(request),
                "expires_in": int(DEVICE_CODE_TTL.total_seconds()),
                "interval": DEVICE_CODE_POLL_INTERVAL_SECONDS,
            },
            status=status.HTTP_200_OK,
        )


class DeviceCodeApproveEndpoint(APIView):
    """Session-auth: logged-in human approves a pending CLI login.

    The web UI at ``/auth/device/`` calls this with the ``user_code``
    typed by the human. We stamp the row with ``request.user`` so the
    subsequent CLI poll can mint a token for that user.
    """

    permission_classes = [IsAuthenticated]
    authentication_classes = [BaseSessionAuthentication]

    def post(self, request):
        raw = (request.data.get("user_code") or "").strip().upper()
        if not raw:
            return Response(
                {"error": "user_code is required."},
                status=status.HTTP_400_BAD_REQUEST,
            )

        # Be lenient about hyphens / case so users can type the code as
        # rendered (e.g. "WXYZ-1234") or paste with whitespace. The DB
        # stores the canonical "XXXX-XXXX" form.
        normalized = raw.replace(" ", "").replace("-", "")
        if len(normalized) != 8:
            return Response(
                {"error": "user_code must be 8 characters."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        canonical = f"{normalized[:4]}-{normalized[4:]}"

        with transaction.atomic():
            try:
                row = CLIDeviceCode.objects.select_for_update().get(user_code=canonical)
            except CLIDeviceCode.DoesNotExist:
                return Response(
                    {"error": "Code not recognized. Check the code on your terminal and try again."},
                    status=status.HTTP_404_NOT_FOUND,
                )

            if row.consumed:
                return Response(
                    {"error": "This code has already been used."},
                    status=status.HTTP_410_GONE,
                )
            if row.denied:
                return Response(
                    {"error": "This code has been denied."},
                    status=status.HTTP_410_GONE,
                )
            if row.expires_at <= timezone.now():
                return Response(
                    {"error": "This code has expired. Run `pidash auth login` again."},
                    status=status.HTTP_410_GONE,
                )
            # Reject second-user takeover: once a code is approved, only
            # the same user can re-approve it (idempotent). Otherwise an
            # attacker who shoulder-surfs the user_code in the ~10-min
            # window could overwrite `row.user` before the CLI polls and
            # end up with a token impersonating the second-approver's
            # account.
            if row.approved and row.user_id is not None and row.user_id != request.user.id:
                return Response(
                    {"error": "This code has already been approved by another user."},
                    status=status.HTTP_409_CONFLICT,
                )

            # Pick a workspace the approving user is a member of. v1 is
            # single-workspace-per-host, so we just pick the most recent
            # one; future work can let the user choose during approval.
            membership = (
                WorkspaceMember.objects.filter(member=request.user, is_active=True)
                .select_related("workspace")
                .order_by("-created_at")
                .first()
            )

            row.user = request.user
            row.workspace = membership.workspace if membership else None
            row.approved = True
            row.save(update_fields=["user", "workspace", "approved", "updated_at"])

        return Response(
            {
                "ok": True,
                "user_email": request.user.email,
                "workspace_slug": row.workspace.slug if row.workspace else None,
            },
            status=status.HTTP_200_OK,
        )


class DeviceCodeTokenEndpoint(APIView):
    """RFC 8628 §3.4 — CLI polls here trading device_code for an APIToken.

    Returns one of:
      * 200 ``{access_token, workspace_slug, user_email}`` on approval.
      * 400 ``{error: "authorization_pending"}`` while waiting.
      * 400 ``{error: "slow_down"}`` if polling too fast.
      * 410 ``{error: "expired_token"}`` after TTL.
      * 410 ``{error: "access_denied"}`` if the human denied it.
    """

    permission_classes = [AllowAny]
    authentication_classes: list = []

    def post(self, request):
        device_code = (request.data.get("device_code") or "").strip()
        if not device_code:
            return Response(
                {"error": "invalid_request", "error_description": "device_code is required."},
                status=status.HTTP_400_BAD_REQUEST,
            )

        with transaction.atomic():
            try:
                row = CLIDeviceCode.objects.select_for_update().get(device_code=device_code)
            except CLIDeviceCode.DoesNotExist:
                return Response(
                    {"error": "invalid_grant", "error_description": "Unknown device code."},
                    status=status.HTTP_400_BAD_REQUEST,
                )

            now = timezone.now()

            if row.consumed:
                return Response(
                    {"error": "invalid_grant", "error_description": "Device code already consumed."},
                    status=status.HTTP_400_BAD_REQUEST,
                )
            if row.denied:
                return Response(
                    {"error": "access_denied"},
                    status=status.HTTP_410_GONE,
                )
            if row.expires_at <= now:
                return Response(
                    {"error": "expired_token"},
                    status=status.HTTP_410_GONE,
                )

            # Enforce a minimum gap between polls. RFC 8628 says a client
            # MUST NOT poll faster than `interval`; if they do we should
            # tell them to `slow_down`. We accept the first poll
            # unconditionally and clamp subsequent ones to a 3s floor.
            #
            # Critically, do NOT bump `last_polled_at` on a slow_down
            # rejection — otherwise a malicious caller holding the
            # device_code could spam fast polls and starve the legit
            # CLI, which would always see `(now - last_polled_at) < gap`
            # because the attacker just touched it.
            if row.last_polled_at is not None and (now - row.last_polled_at) < DEVICE_CODE_MIN_POLL_GAP:
                return Response(
                    {"error": "slow_down"},
                    status=status.HTTP_400_BAD_REQUEST,
                )

            row.last_polled_at = now

            if not row.approved or row.user_id is None:
                row.save(update_fields=["last_polled_at", "updated_at"])
                return Response(
                    {"error": "authorization_pending"},
                    status=status.HTTP_400_BAD_REQUEST,
                )

            # Override the default `generate_label_token` (opaque hex)
            # so this row is distinguishable from user-created PATs in
            # the settings UI token list.
            api_token = APIToken.objects.create(
                user=row.user,
                workspace=row.workspace,
                user_type=0,  # Human
                label=f"pidash CLI · {now.strftime('%Y-%m-%d %H:%M')} UTC",
                description="Issued by pidash auth login (device-code flow).",
            )
            row.consumed = True
            row.save(update_fields=["consumed", "last_polled_at", "updated_at"])

        return Response(
            {
                "access_token": api_token.token,
                "token_type": "X-Api-Key",
                "user_email": row.user.email if row.user else None,
                "workspace_slug": row.workspace.slug if row.workspace else None,
            },
            status=status.HTTP_200_OK,
        )


class WorkspaceListEndpoint(APIView):
    """``GET /api/v1/auth/workspaces/`` — workspaces the caller belongs to.

    Used by ``pidash auth login`` to drive a "which workspace should this
    host be bound to?" picker. The Pi Dash CLI on a dev host is
    single-workspace-per-install in v1: after login the CLI persists one
    ``workspace_slug`` and forwards it on subsequent runner-create calls.

    Authenticated with the CLI's ``X-Api-Key`` token. Returns
    ``{"workspaces": [{"slug", "name"}, ...]}`` in member-since order so
    the picker stays stable across calls.
    """

    permission_classes = [IsAuthenticated]
    authentication_classes = [APIKeyAuthentication]

    def get(self, request):
        members = (
            WorkspaceMember.objects.filter(member=request.user, is_active=True)
            .select_related("workspace")
            .order_by("created_at")
        )
        workspaces = [{"slug": m.workspace.slug, "name": m.workspace.name} for m in members if m.workspace is not None]
        return Response({"workspaces": workspaces}, status=status.HTTP_200_OK)


class DeviceMachineTokenEndpoint(APIView):
    """Exchange the just-minted device-flow APIToken for a dev-machine token.

    The APIToken is an in-memory bridge used only to finish workspace
    selection. The CLI persists the returned ``mt_...`` token as
    ``[cli].token``; both CLI commands and all local runners then share that
    one dev-machine credential.
    """

    permission_classes = [IsAuthenticated]
    authentication_classes = [APIKeyAuthentication]

    def post(self, request):
        from pi_dash.db.models.workspace import Workspace

        workspace_slug = (request.data.get("workspace_slug") or "").strip()
        dev_machine_id_raw = (request.data.get("dev_machine_id") or "").strip()
        host_label = (request.data.get("host_label") or "").strip()[:255]
        if not workspace_slug:
            return Response(
                {"error": "workspace_slug is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not dev_machine_id_raw:
            return Response(
                {"error": "dev_machine_id is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if not host_label:
            return Response(
                {"error": "host_label is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        try:
            dev_machine_id = _uuid.UUID(dev_machine_id_raw)
        except (TypeError, ValueError, AttributeError):
            return Response(
                {"error": "invalid_dev_machine_id"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        workspace = Workspace.objects.filter(slug=workspace_slug).first()
        if workspace is None or not is_workspace_member(request.user, workspace.id):
            return Response(
                {"error": "workspace_not_found"},
                status=status.HTTP_404_NOT_FOUND,
            )

        try:
            with transaction.atomic():
                dev_machine = _get_or_create_dev_machine(
                    user=request.user,
                    dev_machine_id=dev_machine_id,
                    host_label=host_label,
                )
                minted = _rotate_machine_token(
                    user=request.user,
                    workspace=workspace,
                    dev_machine=dev_machine,
                    host_label=host_label,
                )
        except DevMachineOwnershipError:
            return Response(
                {"error": "dev_machine_not_found"},
                status=status.HTTP_404_NOT_FOUND,
            )

        return Response(
            {
                "machine_token": minted.raw,
                "workspace_slug": workspace.slug,
                "dev_machine_id": str(dev_machine.id),
                "host_label": dev_machine.host_label,
            },
            status=status.HTTP_201_CREATED,
        )


class DeviceCodeRevokeEndpoint(APIView):
    """Invalidate the caller's CLI token. Idempotent.

    Used by ``pidash auth logout``. The caller authenticates with their
    current token (which we then mark inactive). Subsequent requests
    with that token will 401.
    """

    permission_classes = [IsAuthenticated]
    authentication_classes = [APIKeyAuthentication]

    def post(self, request):
        machine_token = getattr(request, "auth_machine_token", None)
        if machine_token is not None:
            machine_token.revoke()
            return Response({"ok": True}, status=status.HTTP_200_OK)
        # request.auth is the raw token string (see APIKeyAuthentication).
        raw_token = request.auth
        try:
            tok = APIToken.objects.get(token=raw_token)
        except APIToken.DoesNotExist:
            # Already gone — idempotent OK.
            return Response({"ok": True}, status=status.HTTP_200_OK)
        if tok.is_active:
            tok.is_active = False
            tok.save(update_fields=["is_active", "updated_at"])
        return Response({"ok": True}, status=status.HTTP_200_OK)
