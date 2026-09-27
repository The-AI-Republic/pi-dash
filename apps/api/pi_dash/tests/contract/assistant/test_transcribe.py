# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the voice-dictation transcribe endpoint.

Covers the requirements in ``PDASHOSS01-151``: the endpoint gates on STT config
before dispatch, caps upload size server-side, forwards the recording as
OpenAI-compatible multipart to the resolved provider and returns ``{text}``,
never persists the audio, re-runs the SSRF guard at execution time, and maps
provider failures onto a small stable dictation error taxonomy.
"""

from __future__ import annotations

import httpx
import pytest
from django.core.files.uploadedfile import SimpleUploadedFile
from rest_framework.test import APIClient

from pi_dash.tests.contract.assistant.conftest import configure_stt

pytestmark = pytest.mark.django_db

URL = "/api/users/me/ai-assistant/transcribe/"


def client_for(user):
    c = APIClient()
    c.force_authenticate(user=user)
    return c


def audio(content: bytes = b"RIFFxxxxWAVE", name: str = "clip.webm") -> SimpleUploadedFile:
    return SimpleUploadedFile(name, content, content_type="audio/webm")


class FakeResp:
    """Minimal stand-in for an ``httpx.Response``."""

    def __init__(self, status_code: int, json_body=None, text: str = ""):
        self.status_code = status_code
        self._json = json_body
        self.text = text

    def json(self):
        if self._json is None:
            raise ValueError("no json body")
        return self._json


# --- gating ---


def test_transcribe_rejects_when_no_config(world, kms_crypto, mocker):
    post = mocker.patch("httpx.post")
    res = client_for(world.member).post(URL, {"file": audio()}, format="multipart")
    assert res.status_code == 422
    assert res.data["error"] == "stt_config_missing"
    # gate fires before any outbound request
    post.assert_not_called()


def test_transcribe_requires_a_file(world, kms_crypto, mocker):
    configure_stt(world.member)
    post = mocker.patch("httpx.post")
    res = client_for(world.member).post(URL, {}, format="multipart")
    assert res.status_code == 400
    assert res.data["error"] == "no_audio"
    post.assert_not_called()


def test_transcribe_requires_auth(world, kms_crypto):
    res = APIClient().post(URL, {"file": audio()}, format="multipart")
    assert res.status_code in (401, 403)


# --- size cap ---


def test_transcribe_rejects_oversize_upload(world, kms_crypto, mocker):
    configure_stt(world.member)
    mocker.patch("pi_dash.assistant.views.transcribe.MAX_AUDIO_BYTES", 4)
    post = mocker.patch("httpx.post")
    res = client_for(world.member).post(URL, {"file": audio(b"more-than-four-bytes")}, format="multipart")
    assert res.status_code == 413
    assert res.data["error"] == "audio_too_large"
    # nothing is streamed to the provider
    post.assert_not_called()


# --- happy path / forwarding ---


def test_transcribe_forwards_and_returns_text(world, kms_crypto, mocker):
    configure_stt(world.member, base_url="https://api.example.com/v1", model="whisper-1")
    post = mocker.patch("httpx.post", return_value=FakeResp(200, {"text": "hello world"}))

    res = client_for(world.member).post(URL, {"file": audio()}, format="multipart")
    assert res.status_code == 200
    assert res.data == {"text": "hello world"}

    post.assert_called_once()
    args, kwargs = post.call_args
    assert args[0] == "https://api.example.com/v1/audio/transcriptions"
    assert kwargs["data"]["model"] == "whisper-1"
    assert kwargs["headers"]["Authorization"].startswith("Bearer ")
    assert "file" in kwargs["files"]
    # optional fields are omitted when not sent
    assert "language" not in kwargs["data"]
    assert "response_format" not in kwargs["data"]


def test_transcribe_passes_optional_fields(world, kms_crypto, mocker):
    configure_stt(world.member)
    post = mocker.patch("httpx.post", return_value=FakeResp(200, {"text": "bonjour"}))

    res = client_for(world.member).post(
        URL,
        {"file": audio(), "language": "fr", "response_format": "json"},
        format="multipart",
    )
    assert res.status_code == 200
    _, kwargs = post.call_args
    assert kwargs["data"]["language"] == "fr"
    assert kwargs["data"]["response_format"] == "json"


def test_transcribe_plain_text_response_format(world, kms_crypto, mocker):
    configure_stt(world.member)
    mocker.patch("httpx.post", return_value=FakeResp(200, json_body=None, text="just the words"))

    res = client_for(world.member).post(
        URL, {"file": audio(), "response_format": "text"}, format="multipart"
    )
    assert res.status_code == 200
    assert res.data == {"text": "just the words"}


def test_transcribe_does_not_persist_audio(world, kms_crypto, mocker):
    """A transcribe call leaves no audio in the DB (no message/attachment rows)."""
    from pi_dash.assistant.models import AssistantMessage

    configure_stt(world.member)
    mocker.patch("httpx.post", return_value=FakeResp(200, {"text": "hi"}))
    before = AssistantMessage.objects.count()
    client_for(world.member).post(URL, {"file": audio()}, format="multipart")
    assert AssistantMessage.objects.count() == before


# --- SSRF re-check at execution time ---


def test_transcribe_reruns_ssrf_check(world, kms_crypto, settings, mocker):
    settings.ASSISTANT_BLOCK_PRIVATE_URLS = True
    configure_stt(world.member, base_url="http://127.0.0.1:9000/v1")
    post = mocker.patch("httpx.post")
    res = client_for(world.member).post(URL, {"file": audio()}, format="multipart")
    assert res.status_code == 400
    assert res.data["error"] == "base_url_blocked"
    post.assert_not_called()


# --- provider failure taxonomy ---


@pytest.mark.parametrize(
    "provider_status,expected_error,expected_http",
    [
        (401, "provider_auth_failed", 502),
        (403, "provider_auth_failed", 502),
        (404, "model_invalid", 400),
        (413, "audio_too_large", 413),
        (429, "provider_unreachable", 502),
        (500, "provider_unreachable", 502),
        (400, "transcription_failed", 502),
    ],
)
def test_transcribe_maps_provider_failures(
    world, kms_crypto, mocker, provider_status, expected_error, expected_http
):
    configure_stt(world.member)
    mocker.patch("httpx.post", return_value=FakeResp(provider_status, {"error": "x"}))
    res = client_for(world.member).post(URL, {"file": audio()}, format="multipart")
    assert res.status_code == expected_http
    assert res.data["error"] == expected_error


def test_transcribe_unreachable_provider(world, kms_crypto, mocker):
    configure_stt(world.member)
    mocker.patch("httpx.post", side_effect=httpx.ConnectError("boom"))
    res = client_for(world.member).post(URL, {"file": audio()}, format="multipart")
    assert res.status_code == 502
    assert res.data["error"] == "provider_unreachable"
