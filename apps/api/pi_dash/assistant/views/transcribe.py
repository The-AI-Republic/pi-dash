# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Voice-dictation transcribe endpoint.

Accepts a recorded audio upload, resolves the user's BYO speech-to-text
provider through the CE seam (:mod:`pi_dash.ee.assistant.stt_provider`), and
forwards it as multipart to ``{base_url}/audio/transcriptions`` per the
OpenAI-compatible transcription contract, returning ``{text}``.

Design constraints (see ``PDASHOSS01-151`` / the epic ``PDASHOSS01-148``):

* **Gated** — reject before dispatch when the user has no usable STT config,
  with a dictation-specific code the composer turns into a "configure dictation
  in Settings" state (mirrors the chat ``llm_config_missing`` gate).
* **Size-capped** — OpenHub's gateway caps transcription uploads at 25 MB, so
  anything larger is guaranteed to fail on the cloud path; reject it cleanly
  rather than stream a doomed upload. Note the effective cap is usually lower:
  ``RequestBodySizeLimitMiddleware`` rejects any body over
  ``DATA_UPLOAD_MAX_MEMORY_SIZE`` (``FILE_SIZE_LIMIT``, 5 MB by default) with
  413 ``REQUEST_BODY_TOO_LARGE`` before this view runs. That is ample for the
  composer's 60 s opus recordings; raise ``FILE_SIZE_LIMIT`` to accept more.
* **Throttled** — every call spends the user's money (or cloud wallet credit).
* **Not persisted** — the audio is forwarded for the one request and dropped;
  it never touches the DB or backups (same as the OpenHub gateway).
* **Stable error taxonomy** — provider failures map onto a small fixed set of
  dictation codes so a dictation failure never renders as a chat failure.
"""

from __future__ import annotations

import httpx
from rest_framework import status
from rest_framework.response import Response
from rest_framework.throttling import UserRateThrottle

from pi_dash.app.views.base import BaseAPIView
from pi_dash.assistant.errors import AssistantError
from pi_dash.ee.assistant.stt_provider import has_usable_stt_config, resolve_stt_provider

# OpenHub's gateway caps transcription uploads at 25 MB; anything larger is
# guaranteed to fail on the cloud path. Kept at (not below) that ceiling so a
# self-hoster with a more generous provider is not needlessly restricted.
MAX_AUDIO_BYTES = 25 * 1024 * 1024

# Multipart framing + the small optional text fields add a little on top of the
# raw audio, so the cheap Content-Length pre-filter allows this much slack; the
# authoritative check is against the parsed file's own size.
_CONTENT_LENGTH_SLACK = 1 * 1024 * 1024

# Response formats that return plain text rather than a JSON object with a
# "text" field (the OpenAI-compatible transcription contract).
_TEXT_FORMATS = {"text", "srt", "vtt"}

# Transcription is latency-heavy — a minute of audio can take many seconds — so
# the read/write timeout is generous while the connect timeout stays short.
_TIMEOUT = httpx.Timeout(connect=10.0, read=120.0, write=120.0, pool=10.0)


class AssistantTranscribeThrottle(UserRateThrottle):
    scope = "assistant_transcribe"


class AssistantTranscribeEndpoint(BaseAPIView):
    """POST an audio file, get back ``{text}``. Per-user, BYO STT, not persisted."""

    throttle_classes = [AssistantTranscribeThrottle]

    def post(self, request):
        # Gate before touching the upload: no usable config -> a code the
        # composer turns into a "configure dictation in Settings" state.
        if not has_usable_stt_config(request.user):
            return Response(
                {"error": "stt_config_missing", "detail": "Configure dictation in Settings."},
                status=status.HTTP_422_UNPROCESSABLE_ENTITY,
            )

        # Cheap pre-filter on Content-Length: reject an oversize body before
        # Django spools the multipart to a temp file at all.
        too_large = _content_length_over_cap(request)
        if too_large:
            return _audio_too_large()

        upload = request.FILES.get("file")
        if upload is None:
            return Response(
                {"error": "no_audio", "detail": "No audio file was uploaded."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        if upload.size is not None and upload.size > MAX_AUDIO_BYTES:
            return _audio_too_large()

        try:
            provider = resolve_stt_provider(request.user)
        except AssistantError as exc:
            return Response({"error": exc.code, "detail": exc.detail}, status=exc.http_status)

        # OpenAI-compatible multipart body: model is server-resolved; language
        # and response_format are optional passthroughs.
        data = {"model": provider.model}
        language = (request.data.get("language") or "").strip()
        if language:
            data["language"] = language
        response_format = (request.data.get("response_format") or "").strip()
        if response_format:
            data["response_format"] = response_format

        url = provider.base_url.rstrip("/") + "/audio/transcriptions"
        headers = {"Authorization": f"Bearer {provider.api_key}"}
        upload.seek(0)
        files = {
            "file": (
                upload.name or "audio",
                upload.file,
                upload.content_type or "application/octet-stream",
            )
        }

        try:
            resp = httpx.post(url, headers=headers, files=files, data=data, timeout=_TIMEOUT)
        except httpx.HTTPError:
            # Never echo the raw error — it may reveal an internal host.
            return Response(
                {"error": "provider_unreachable", "detail": "Could not reach the dictation provider."},
                status=status.HTTP_502_BAD_GATEWAY,
            )

        failure = _classify(resp.status_code)
        if failure is not None:
            code, http_status, detail = failure
            return Response({"error": code, "detail": detail}, status=http_status)

        return Response({"text": _extract_text(resp, response_format)})


def _content_length_over_cap(request) -> bool:
    raw = request.META.get("CONTENT_LENGTH")
    if not raw:
        return False
    try:
        return int(raw) > MAX_AUDIO_BYTES + _CONTENT_LENGTH_SLACK
    except (TypeError, ValueError):
        return False


def _audio_too_large() -> Response:
    return Response(
        {"error": "audio_too_large", "detail": "Recording exceeds the 25 MB limit."},
        status=status.HTTP_413_REQUEST_ENTITY_TOO_LARGE,
    )


def _classify(code: int) -> tuple[str, int, str] | None:
    """Map a provider HTTP status onto the stable dictation error taxonomy.

    Returns ``(error_code, http_status, detail)`` for a failure, or ``None`` for
    success. Deliberately small and dictation-specific so the composer never
    renders a dictation failure as a chat failure.
    """
    if code in (401, 403):
        return "provider_auth_failed", status.HTTP_502_BAD_GATEWAY, "The dictation provider rejected the API key."
    if code == 404:
        return "model_invalid", status.HTTP_400_BAD_REQUEST, "The transcription model or endpoint was not found."
    if code == 413:
        return (
            "audio_too_large",
            status.HTTP_413_REQUEST_ENTITY_TOO_LARGE,
            "The provider rejected the recording as too large.",
        )
    if code >= 500 or code == 429:
        return "provider_unreachable", status.HTTP_502_BAD_GATEWAY, "The dictation provider is unavailable."
    if code >= 400:
        # The endpoint answered and the key was accepted, but it would not
        # transcribe this recording (e.g. an unsupported audio format).
        return (
            "transcription_failed",
            status.HTTP_502_BAD_GATEWAY,
            "The dictation provider could not transcribe the audio.",
        )
    return None


def _extract_text(resp: httpx.Response, response_format: str) -> str:
    """Pull the transcript out of the provider response, whatever its shape.

    ``json``/``verbose_json`` return an object with a ``text`` field; ``text``
    (and subtitle formats) return the transcript as the raw body.
    """
    if response_format in _TEXT_FORMATS:
        return resp.text
    try:
        payload = resp.json()
    except ValueError:
        return resp.text
    if isinstance(payload, dict):
        return payload.get("text", "") or ""
    return ""
