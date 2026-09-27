# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""BYO speech-to-text (dictation) config endpoints.

Mirrors ``llm_config.py`` for the STT provider: a per-user OpenAI-compatible
``/v1/audio/transcriptions`` endpoint (base URL + API key + model). GET exposes
only ``has_api_key`` — there is no read path for the stored key, same as BYOK.
The SSRF guard runs both at save time (a friendly rejection) and at test time
(the actual guard before any outbound request).
"""

from __future__ import annotations

from django.utils import timezone
from rest_framework import status
from rest_framework.response import Response
from rest_framework.throttling import UserRateThrottle

from pi_dash.app.views.base import BaseAPIView
from pi_dash.assistant import crypto, ssrf
from pi_dash.assistant.errors import AssistantError
from pi_dash.assistant.models import UserSTTConfig
from pi_dash.assistant.serializers import UserSTTConfigSerializer


def _serialize(cfg: UserSTTConfig | None) -> dict:
    if cfg is None:
        return {
            "base_url": "",
            "model_name": "",
            "has_api_key": False,
            "last_verified_at": None,
        }
    return {
        "base_url": cfg.base_url,
        "model_name": cfg.model_name,
        "has_api_key": cfg.has_api_key,
        "last_verified_at": cfg.last_verified_at.isoformat() if cfg.last_verified_at else None,
    }


class UserSTTConfigEndpoint(BaseAPIView):
    def get(self, request):
        cfg = UserSTTConfig.objects.filter(user=request.user).first()
        return Response(_serialize(cfg))

    def put(self, request):
        cfg = UserSTTConfig.objects.filter(user=request.user).first()
        serializer = UserSTTConfigSerializer(instance=cfg, data=request.data, partial=cfg is not None)
        serializer.is_valid(raise_exception=True)
        data = serializer.validated_data

        base_url = data.get("base_url", cfg.base_url if cfg else "")
        if base_url and ssrf.is_blocked(base_url):
            return Response(
                {"error": "base_url_blocked", "detail": "That endpoint host is not allowed."},
                status=status.HTTP_400_BAD_REQUEST,
            )

        api_key = data.pop("api_key", None)
        if cfg is None:
            cfg = UserSTTConfig(user=request.user)
        for field in ("base_url", "model_name"):
            if field in data:
                setattr(cfg, field, data[field])
        if api_key:
            try:
                cfg.api_key_encrypted = crypto.encrypt(api_key)
            except AssistantError as exc:
                return Response({"error": exc.code, "detail": exc.detail}, status=exc.http_status)
        try:
            cfg.save()
        except Exception:
            return Response({"error": "invalid"}, status=status.HTTP_400_BAD_REQUEST)
        return Response(_serialize(cfg))

    def delete(self, request):
        UserSTTConfig.objects.filter(user=request.user).delete()
        return Response(status=status.HTTP_204_NO_CONTENT)


class UserSTTConfigTestThrottle(UserRateThrottle):
    scope = "assistant_stt_test"


class UserSTTConfigTestEndpoint(BaseAPIView):
    throttle_classes = [UserSTTConfigTestThrottle]

    def post(self, request):
        cfg = UserSTTConfig.objects.filter(user=request.user).first()
        if cfg is None or not cfg.has_api_key:
            return Response({"ok": False, "error_code": "stt_config_missing"})
        if cfg.base_url and ssrf.is_blocked(cfg.base_url):
            return Response({"ok": False, "error_code": "base_url_blocked"})
        try:
            api_key = crypto.decrypt(cfg.api_key_encrypted)
        except AssistantError as exc:
            return Response({"ok": False, "error_code": exc.code})

        try:
            ok, code, detail = _run_test(cfg, api_key)
        except Exception:  # noqa: BLE001 — never echo raw errors (may reveal internal hosts)
            return Response({"ok": False, "error_code": "provider_unreachable"})

        if ok:
            cfg.last_verified_at = timezone.now()
            cfg.save(update_fields=["last_verified_at"])
            return Response({"ok": True})
        return Response({"ok": False, "error_code": code, "detail": detail})


def _run_test(cfg: UserSTTConfig, api_key: str) -> tuple[bool, str, str]:
    """Probe the configured transcription endpoint to prove it answers.

    We deliberately do not send a real recording: transcription is a paid,
    latency-heavy call, and all the connection test needs to confirm is that
    the endpoint is reachable and the key is accepted. A minimal, intentionally
    invalid multipart body gets us that — a well-behaved server answers with a
    4xx (bad audio) once it has accepted the credential, while auth failures and
    unreachable hosts are distinguishable by status/exception. Only the response
    *class* is inspected; response bodies are never surfaced (they may reveal
    internal detail).
    """
    import httpx

    url = cfg.base_url.rstrip("/") + "/audio/transcriptions"
    headers = {"Authorization": f"Bearer {api_key}"}
    files = {"file": ("probe.wav", b"\x00", "audio/wav")}
    data = {"model": cfg.model_name}

    try:
        resp = httpx.post(url, headers=headers, files=files, data=data, timeout=15.0)
    except httpx.HTTPError:
        return False, "provider_unreachable", "Could not reach the endpoint."

    code = resp.status_code
    if code in (401, 403):
        return False, "provider_auth_failed", "API key rejected."
    if code == 404:
        return False, "model_invalid", "Transcription endpoint or model not found."
    if code >= 500:
        return False, "provider_unreachable", "The endpoint returned a server error."
    # 2xx (transcribed) or a non-auth 4xx (our probe payload rejected, but the
    # endpoint answered and the key was accepted) both prove reachability.
    return True, "", ""
