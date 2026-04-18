# Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""DRF authentication for runner daemons using a bearer credential.

Runner daemons call ``/api/v1/runner/...`` with ``Authorization: Bearer <runner_secret>``.
The secret is stored hashed in :class:`Runner.credential_hash`.
"""

from __future__ import annotations

from typing import Optional, Tuple

from rest_framework import authentication, exceptions

from apple_pi_dash.runner.models import Runner, RunnerStatus
from apple_pi_dash.runner.services.tokens import hash_token


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
        try:
            runner = Runner.objects.get(credential_hash=hashed)
        except Runner.DoesNotExist:
            raise exceptions.AuthenticationFailed("invalid runner credential")
        if runner.status == RunnerStatus.REVOKED:
            raise exceptions.AuthenticationFailed("runner is revoked")
        # Present the runner as the DRF-authenticated entity. Views that need
        # the owning user should use `request.auth_runner.owner`.
        request.auth_runner = runner
        return (runner, None)

    def authenticate_header(self, request) -> str:
        return self.keyword
