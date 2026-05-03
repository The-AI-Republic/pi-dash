# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""DRF authentication for runner-daemon REST traffic.

Per ``.ai_design/move_to_https/design.md`` §5, the runner transport uses
a per-runner refresh-token + access-token pair (no Connection layer).
Two auth classes:

- :class:`RunnerAccessTokenAuthentication` — bearer JWT minted at refresh
  time. Used for every runner-scoped endpoint
  (``/runners/<rid>/sessions/...``, ``/runs/<run_id>/...``).
- :class:`RunnerRefreshTokenAuthentication` — bearer refresh token used
  only on ``POST /runners/<rid>/refresh/``. Verified inside the view
  (because the algorithm depends on row-locked DB state); this class
  parses the bearer header into ``request.auth_refresh_token`` and is
  otherwise a no-op.
- :class:`MachineTokenAuthentication` — separate machine-scoped CLI
  credential; used for ``/api/v1/`` user-action endpoints.
"""

from __future__ import annotations

import logging
from typing import Optional, Tuple
from uuid import UUID

from django.utils import timezone
from rest_framework import authentication, exceptions

from pi_dash.runner.models import MachineToken, Runner, RunnerForceRefresh
from pi_dash.runner.services.permissions import is_workspace_member
from pi_dash.runner.services.tokens import (
    AccessTokenError,
    decode_access_token,
    hash_token,
)

logger = logging.getLogger(__name__)


def _bearer(request) -> Optional[str]:
    header = authentication.get_authorization_header(request)
    if not header:
        return None
    parts = header.split()
    if len(parts) != 2 or parts[0].decode().lower() != "bearer":
        return None
    return parts[1].decode()


class RunnerAccessTokenAuthentication(authentication.BaseAuthentication):
    """Per-runner JWT bearer authentication.

    Verification order (``design.md`` §5.4):

    1. signature by ``kid`` + ``exp``
    2. load ``Runner`` by ``sub``; reject if revoked
    3. ``rtg >= runner.refresh_token_generation - 1``
    4. ``RunnerForceRefresh.min_rtg`` if a row exists
    5. ``token.sub == url_runner_id`` for runner-scoped URLs

    Membership is **not** re-checked here — that lives at refresh time
    (``design.md`` §5.3 step 4). Membership-staleness is bounded by the
    access-token TTL.
    """

    keyword = "Bearer"

    def authenticate(self, request) -> Optional[Tuple[object, None]]:
        raw = _bearer(request)
        if not raw:
            return None
        try:
            payload = decode_access_token(raw)
        except AccessTokenError as exc:
            raise exceptions.AuthenticationFailed(exc.code)

        runner_id = payload.get("sub")
        try:
            runner = Runner.objects.select_related("workspace", "pod").get(
                id=runner_id
            )
        except Runner.DoesNotExist:
            raise exceptions.AuthenticationFailed("runner_not_found")

        # Per-request live revocation check. design.md §5.4 mandates this:
        # Runner.revoke() does not bump rtg, so without this an access
        # token issued before revocation survives until expiry.
        if runner.revoked_at is not None:
            raise exceptions.AuthenticationFailed("runner_revoked")

        rtg = int(payload.get("rtg") or 0)
        if rtg < (runner.refresh_token_generation - 1):
            raise exceptions.AuthenticationFailed("access_token_stale_rtg")

        force_refresh = RunnerForceRefresh.objects.filter(runner=runner).first()
        if force_refresh is not None and rtg < force_refresh.min_rtg:
            raise exceptions.AuthenticationFailed("force_refresh_required")

        url_runner_id = self._url_runner_id(request)
        if url_runner_id is not None and str(url_runner_id) != str(runner.id):
            raise exceptions.AuthenticationFailed("runner_id_mismatch")

        request.auth_runner = runner
        request.auth_token_payload = payload
        return (runner.owner, None)

    def authenticate_header(self, request) -> str:
        return self.keyword

    @staticmethod
    def _url_runner_id(request) -> Optional[str]:
        match = getattr(request, "resolver_match", None)
        if match is None:
            return None
        return match.kwargs.get("runner_id")


class RunnerRefreshTokenAuthentication(authentication.BaseAuthentication):
    """Parse the bearer header into ``request.auth_refresh_token``.

    The actual refresh algorithm is row-locked and lives in the view.
    This class only extracts the token from the header.
    """

    keyword = "Bearer"

    def authenticate(self, request) -> Optional[Tuple[object, None]]:
        raw = _bearer(request)
        if not raw:
            return None
        request.auth_refresh_token = raw
        # No user is established at this point — the view will resolve
        # the runner row itself.
        return (None, None)

    def authenticate_header(self, request) -> str:
        return self.keyword


class MachineTokenAuthentication(authentication.BaseAuthentication):
    """Bearer machine-scoped token (``pidash`` CLI).

    Per-request workspace-membership check (no refresh chokepoint),
    best-effort ``last_used_at`` update.
    """

    keyword = "Bearer"

    def authenticate(self, request) -> Optional[Tuple[object, None]]:
        raw = _bearer(request)
        if not raw or not raw.startswith("mt_"):
            return None
        token_hash = hash_token(raw)
        try:
            token = MachineToken.objects.select_related("user", "workspace").get(
                token_hash=token_hash
            )
        except MachineToken.DoesNotExist:
            raise exceptions.AuthenticationFailed("machine_token_invalid")
        if token.revoked_at is not None:
            raise exceptions.AuthenticationFailed("machine_token_revoked")
        if not is_workspace_member(token.user, token.workspace_id):
            token.revoke()
            raise exceptions.AuthenticationFailed("membership_revoked")
        # Best-effort last_used_at; ignore failures so per-request DB
        # contention doesn't break the request.
        try:
            MachineToken.objects.filter(pk=token.pk).update(
                last_used_at=timezone.now()
            )
        except Exception:  # pragma: no cover - best-effort
            logger.debug("failed to bump last_used_at", exc_info=True)
        request.auth_machine_token = token
        return (token.user, None)

    def authenticate_header(self, request) -> str:
        return self.keyword


def resolve_runner_for_run(run, request) -> bool:
    """Per-run authorization helper (``design.md`` §7.5).

    Require ``run.runner_id == request.auth_runner.id``. Returns True
    when the run is owned by the authenticated runner, False otherwise.
    """
    runner = getattr(request, "auth_runner", None)
    if runner is None:
        return False
    if run.runner_id is None:
        return False
    return UUID(str(run.runner_id)) == UUID(str(runner.id))
