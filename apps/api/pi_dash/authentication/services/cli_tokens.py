# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.utils import timezone

from pi_dash.db.models import APIToken

CLI_DEVICE_API_TOKEN_DESCRIPTION = "Issued by pidash auth login (device-code flow)."


def deactivate_api_token(raw_token: str | None, *, only_cli_device_tokens: bool = False) -> int:
    """Mark an APIToken inactive after it has been exchanged for an mt_ token."""
    if not raw_token:
        return 0
    qs = APIToken.objects.filter(token=raw_token, is_active=True)
    if only_cli_device_tokens:
        qs = qs.filter(description=CLI_DEVICE_API_TOKEN_DESCRIPTION)
    return qs.update(is_active=False, updated_at=timezone.now())
