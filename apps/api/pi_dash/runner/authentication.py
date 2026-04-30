# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""DRF authentication for runner-daemon REST traffic.

Daemons call ``/api/v1/runner/...`` with two headers:

    Authorization: Bearer <connection_secret>
    X-Connection-Id: <uuid>

The header pair is verified against an active (``revoked_at IS NULL``,
``enrolled_at IS NOT NULL``) Connection row. ``request.auth_connection``
is set on success; if the URL kwargs name a runner_id, that runner is
verified to belong to the connection and exposed on
``request.auth_runner``.
"""

from __future__ import annotations

from typing import Optional, Tuple
from uuid import UUID

from rest_framework import authentication, exceptions

from pi_dash.runner.models import Connection, Runner
from pi_dash.runner.services.tokens import hash_token


class ConnectionBearerAuthentication(authentication.BaseAuthentication):
    keyword = "Bearer"

    def authenticate(self, request) -> Optional[Tuple[object, None]]:
        header = authentication.get_authorization_header(request)
        if not header:
            return None
        parts = header.split()
        if len(parts) != 2 or parts[0].decode().lower() != self.keyword.lower():
            return None
        raw = parts[1].decode()
        connection_id_raw = (request.META.get("HTTP_X_CONNECTION_ID") or "").strip()
        if not connection_id_raw:
            raise exceptions.AuthenticationFailed("missing X-Connection-Id")
        try:
            connection_id = UUID(connection_id_raw)
        except (ValueError, AttributeError):
            raise exceptions.AuthenticationFailed("invalid X-Connection-Id")
        secret_hash = hash_token(raw)
        try:
            connection = Connection.objects.get(
                id=connection_id,
                secret_hash=secret_hash,
                revoked_at__isnull=True,
                enrolled_at__isnull=False,
            )
        except Connection.DoesNotExist:
            raise exceptions.AuthenticationFailed("invalid or revoked connection")

        request.auth_connection = connection
        request.auth_runner = None
        runner_id = (
            getattr(request, "resolver_match", None)
            and request.resolver_match.kwargs.get("runner_id")
        )
        if runner_id:
            runner = Runner.objects.filter(
                id=runner_id, connection=connection
            ).first()
            if runner is None:
                raise exceptions.AuthenticationFailed(
                    "runner not owned by this connection"
                )
            request.auth_runner = runner
        return (connection.created_by, None)

    def authenticate_header(self, request) -> str:
        return self.keyword
