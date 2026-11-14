# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Voice dictation kill switch (``VOICE_DICTATION_ENABLED``).

Shared by the STT config and transcribe endpoints so every dictation surface
answers the same "is this on here" question.
"""

from __future__ import annotations

from django.conf import settings
from rest_framework.response import Response

from pi_dash.assistant.errors import DictationDisabled


def dictation_is_enabled() -> bool:
    """Operator kill switch for voice dictation."""
    return bool(getattr(settings, "VOICE_DICTATION_ENABLED", False))


def dictation_disabled_response() -> Response:
    exc = DictationDisabled("Voice dictation is not available.")
    return Response({"error": exc.code, "detail": exc.detail}, status=exc.http_status)
