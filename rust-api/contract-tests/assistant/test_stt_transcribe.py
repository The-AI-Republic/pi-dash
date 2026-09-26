# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""BYO speech-to-text config and voice transcription.

Covers ``users/me/ai-assistant/stt-config/`` (lifecycle without a
``provider_kind`` field, per-user isolation, validation, the SSRF guard) and
``users/me/ai-assistant/transcribe/`` (the config gate, the audio requirement,
auth, and the 25 MB server-side cap). Provider round-trips need a live STT
endpoint, so they stay out; the ``/test/`` probe is covered through its
missing-config shape.
"""

from .conftest import PUBLIC_BASE

CONFIG_URL = "/api/users/me/ai-assistant/stt-config/"
TEST_URL = "/api/users/me/ai-assistant/stt-config/test/"
TRANSCRIBE_URL = "/api/users/me/ai-assistant/transcribe/"

AUDIO = ("clip.webm", b"RIFFxxxxWAVE", "audio/webm")


def _configure_stt(client, **overrides):
    payload = {"base_url": PUBLIC_BASE, "model_name": "whisper-1",
               "api_key": "test-key-123"}
    payload.update(overrides)
    res = client.put(CONFIG_URL, json=payload)
    assert res.status_code == 200, res.text
    return res.json()


def test_stt_config_lifecycle(world, member):
    got = member.get(CONFIG_URL)
    assert got.status_code == 200
    body = got.json()
    assert body["has_api_key"] is False
    assert body["last_verified_at"] is None
    assert "provider_kind" not in body

    res = member.put(
        CONFIG_URL,
        json={"base_url": PUBLIC_BASE, "model_name": "whisper-1",
              "api_key": "test-key-123"},
    )
    assert res.status_code == 200
    body = res.json()
    assert body["has_api_key"] is True
    assert body["base_url"] == PUBLIC_BASE
    assert body["model_name"] == "whisper-1"
    assert "api_key" not in body

    res = member.put(CONFIG_URL, json={"model_name": "whisper-large"})
    assert res.json()["has_api_key"] is True
    assert res.json()["model_name"] == "whisper-large"

    assert member.delete(CONFIG_URL).status_code == 204
    assert member.get(CONFIG_URL).json()["has_api_key"] is False


def test_stt_config_isolated_per_user(world, member, admin):
    _configure_stt(admin)
    assert member.get(CONFIG_URL).json()["has_api_key"] is False


def test_stt_config_validation(world, member):
    res = member.put(
        CONFIG_URL,
        json={"base_url": PUBLIC_BASE, "model_name": "", "api_key": "test-key-123"},
    )
    assert res.status_code == 400

    res = member.put(
        CONFIG_URL,
        json={"base_url": "ftp://x/v1", "model_name": "whisper-1",
              "api_key": "test-key-123"},
    )
    assert res.status_code == 400

    res = member.put(
        CONFIG_URL,
        json={"base_url": PUBLIC_BASE, "model_name": "whisper-1", "api_key": "short"},
    )
    assert res.status_code == 400


def test_stt_config_rejects_blocked_url_at_save(world, member):
    res = member.put(
        CONFIG_URL,
        json={"base_url": "http://127.0.0.1:9000/v1", "model_name": "whisper-1",
              "api_key": "test-key-123"},
    )
    assert res.status_code == 400
    assert res.json()["error"] == "base_url_blocked"


def test_stt_test_missing_config(world, member):
    res = member.post(TEST_URL)
    assert res.status_code == 200
    body = res.json()
    assert body["ok"] is False
    assert body["error_code"] == "stt_config_missing"


def test_transcribe_rejects_without_config(world, member):
    res = member.post(TRANSCRIBE_URL, files={"file": AUDIO})
    assert res.status_code == 422
    assert res.json()["error"] == "stt_config_missing"


def test_transcribe_requires_a_file(world, member):
    _configure_stt(member)
    res = member.post(TRANSCRIBE_URL, files={})
    assert res.status_code == 400
    assert res.json()["error"] == "no_audio"


def test_transcribe_requires_auth(world, anon):
    assert anon.post(TRANSCRIBE_URL, files={"file": AUDIO}).status_code == 401


def test_transcribe_rejects_oversize_upload(world, member):
    # Layering that matters for the port: with the default FILE_SIZE_LIMIT
    # (5 MB) the body-size middleware answers 413 before the request reaches
    # the view, so the wire error is REQUEST_BODY_TOO_LARGE — the view's own
    # 25 MB audio_too_large branch only runs when the limit is raised above
    # it. Pin the wire behaviour, not the unreachable branch.
    _configure_stt(member)
    big = ("clip.webm", b"x" * (27 * 1024 * 1024), "audio/webm")
    res = member.post(TRANSCRIBE_URL, files={"file": big}, timeout=120.0)
    assert res.status_code == 413
    assert res.json()["error"] == "REQUEST_BODY_TOO_LARGE"
