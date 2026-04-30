# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""DRF authentication for runner daemons using a bearer credential.

Runner daemons call ``/api/v1/runner/...`` with ``Authorization: Bearer <secret>``
in one of two modes:

- Legacy / per-runner: Authorization-only. Looks up a Runner by
  credential_hash. ``request.auth_runner`` is set; ``request.auth_token``
  is None.
- Token / multi-runner: ``X-Token-Id`` header in addition to
  Authorization. Looks up a MachineToken by id, verifies the secret,
  and resolves the Runner from the request URL or body. ``request.
  auth_token`` is set; ``request.auth_runner`` is set when a runner_id
  is identifiable from the request.

The secret is hashed at rest in either case.
"""

from __future__ import annotations

from typing import Optional, Tuple
from uuid import UUID

from rest_framework import authentication, exceptions

from pi_dash.runner.models import MachineToken, Runner, RunnerStatus
from pi_dash.runner.services.tokens import hash_token


class RunnerBearerAuthentication(authentication.BaseAuthentication):
    keyword = "Bearer"

    def authenticate(self, request) -> Optional[Tuple[Runner, None]]:
        header = authentication.get_authorization_header(request)
        if not header:
            return None
        parts = header.split()
        if len(parts) != 2 or parts[0].decode().lower() != self.keyword.lower():
            return None
        raw = parts[1].decode()
        hashed = hash_token(raw)

        # Token mode: X-Token-Id header is present alongside the Bearer.
        token_id_raw = (request.META.get("HTTP_X_TOKEN_ID") or "").strip()
        if token_id_raw:
            try:
                token_id = UUID(token_id_raw)
            except (ValueError, AttributeError):
                raise exceptions.AuthenticationFailed("invalid X-Token-Id")
            try:
                token = MachineToken.objects.get(
                    id=token_id, secret_hash=hashed, revoked_at__isnull=True
                )
            except MachineToken.DoesNotExist:
                raise exceptions.AuthenticationFailed("invalid or revoked token")
            request.auth_token = token
            # Resolve the target runner from the URL kwarg if present, so
            # views like /<runner_id>/deregister/ pick it up automatically.
            runner_id = (
                getattr(request, "resolver_match", None)
                and request.resolver_match.kwargs.get("runner_id")
            )
            if runner_id:
                runner = Runner.objects.filter(
                    id=runner_id, machine_token=token
                ).first()
                if runner is None:
                    raise exceptions.AuthenticationFailed(
                        "runner not owned by this token"
                    )
                request.auth_runner = runner
                return (runner, None)
            # Token-only auth (no runner in URL). Present the token's
            # creator as the user; views can read request.auth_token.
            return (token.created_by, None)

        # Legacy per-runner auth.
        try:
            runner = Runner.objects.get(credential_hash=hashed)
        except Runner.DoesNotExist:
            raise exceptions.AuthenticationFailed("invalid runner credential")
        if runner.status == RunnerStatus.REVOKED:
            raise exceptions.AuthenticationFailed("runner is revoked")
        # Present the runner as the DRF-authenticated entity. Views that need
        # the owning user should use `request.auth_runner.owner`.
        request.auth_runner = runner
        request.auth_token = None
        return (runner, None)

    def authenticate_header(self, request) -> str:
        return self.keyword
