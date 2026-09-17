# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the BYO speech-to-text (dictation) config endpoints.

Mirrors the BYOK LLM config coverage in ``test_endpoints.py`` /
``test_serializers_ssrf.py``: the config lifecycle never exposes the stored
key, the SSRF guard runs at save and at test time, and a successful connection
test stamps ``last_verified_at``.
"""

from __future__ import annotations

import types

import pytest
from rest_framework.test import APIClient

from pi_dash.assistant import ssrf
from pi_dash.assistant.serializers import UserSTTConfigSerializer
from pi_dash.tests.contract.assistant.conftest import configure_stt

pytestmark = pytest.mark.django_db

CONFIG_URL = "/api/users/me/ai-assistant/stt-config/"
TEST_URL = "/api/users/me/ai-assistant/stt-config/test/"


def client_for(user):
    c = APIClient()
    c.force_authenticate(user=user)
    return c


# --- config CRUD ---


def test_stt_config_lifecycle(world, kms_crypto):
    c = client_for(world.member)

    # unset -> 200 with has_api_key False and no provider_kind field
    res = c.get(CONFIG_URL)
    assert res.status_code == 200
    assert res.data["has_api_key"] is False
    assert res.data["last_verified_at"] is None
    assert "provider_kind" not in res.data

    # set
    res = c.put(
        CONFIG_URL,
        {
            "base_url": "https://api.example.com/v1",
            "model_name": "whisper-1",
            "api_key": "sk-12345678",
        },
        format="json",
    )
    assert res.status_code == 200
    assert res.data["has_api_key"] is True
    assert res.data["base_url"] == "https://api.example.com/v1"
    assert res.data["model_name"] == "whisper-1"

    # the key is never returned
    assert "api_key" not in res.data

    # update model without resending the key keeps has_api_key True
    res = c.put(CONFIG_URL, {"model_name": "whisper-large"}, format="json")
    assert res.status_code == 200
    assert res.data["has_api_key"] is True
    assert res.data["model_name"] == "whisper-large"

    # delete
    assert c.delete(CONFIG_URL).status_code == 204
    assert c.get(CONFIG_URL).data["has_api_key"] is False


def test_stt_config_put_requires_model_name(world, kms_crypto):
    c = client_for(world.member)
    res = c.put(
        CONFIG_URL,
        {"base_url": "https://api.example.com/v1", "model_name": "", "api_key": "sk-12345678"},
        format="json",
    )
    assert res.status_code == 400
    assert "model_name" in res.data


def test_stt_config_put_rejects_ssrf_at_save(world, kms_crypto, settings):
    settings.ASSISTANT_BLOCK_PRIVATE_URLS = True
    c = client_for(world.member)
    res = c.put(
        CONFIG_URL,
        {"base_url": "http://169.254.169.254/v1", "model_name": "whisper-1", "api_key": "sk-12345678"},
        format="json",
    )
    assert res.status_code == 400
    assert res.data["error"] == "base_url_blocked"


def test_stt_config_isolated_per_user(world, kms_crypto):
    configure_stt(world.admin)
    # member has no config even though admin does
    res = client_for(world.member).get(CONFIG_URL)
    assert res.data["has_api_key"] is False


# --- connection test ---


def test_stt_test_missing_config(world, kms_crypto):
    res = client_for(world.member).post(TEST_URL)
    assert res.status_code == 200
    assert res.data["ok"] is False
    assert res.data["error_code"] == "stt_config_missing"


def test_stt_test_success_stamps_verified(world, kms_crypto, mocker):
    configure_stt(world.member, verified=False)
    mocker.patch("httpx.post", return_value=types.SimpleNamespace(status_code=200))

    res = client_for(world.member).post(TEST_URL)
    assert res.status_code == 200
    assert res.data["ok"] is True

    # last_verified_at is now stamped and visible via GET
    got = client_for(world.member).get(CONFIG_URL)
    assert got.data["last_verified_at"] is not None


def test_stt_test_bad_probe_still_counts_as_reachable(world, kms_crypto, mocker):
    # A 400 means the endpoint answered and the key was accepted (our probe
    # payload is intentionally invalid) -> connection is considered good.
    configure_stt(world.member, verified=False)
    mocker.patch("httpx.post", return_value=types.SimpleNamespace(status_code=400))
    res = client_for(world.member).post(TEST_URL)
    assert res.data["ok"] is True


def test_stt_test_auth_failure(world, kms_crypto, mocker):
    configure_stt(world.member)
    mocker.patch("httpx.post", return_value=types.SimpleNamespace(status_code=401))
    res = client_for(world.member).post(TEST_URL)
    assert res.data["ok"] is False
    assert res.data["error_code"] == "provider_auth_failed"


def test_stt_test_unreachable(world, kms_crypto, mocker):
    import httpx

    configure_stt(world.member)
    mocker.patch("httpx.post", side_effect=httpx.ConnectError("boom"))
    res = client_for(world.member).post(TEST_URL)
    assert res.data["ok"] is False
    assert res.data["error_code"] == "provider_unreachable"


def test_stt_test_reruns_ssrf_check(world, kms_crypto, settings, mocker):
    settings.ASSISTANT_BLOCK_PRIVATE_URLS = True
    configure_stt(world.member, base_url="http://127.0.0.1:9000/v1")
    post = mocker.patch("httpx.post")
    res = client_for(world.member).post(TEST_URL)
    assert res.data["ok"] is False
    assert res.data["error_code"] == "base_url_blocked"
    # the guard fires before any outbound request
    post.assert_not_called()


def test_stt_test_guest_authenticated_only(world, kms_crypto):
    # unauthenticated caller is rejected by the endpoint's auth
    res = APIClient().post(TEST_URL)
    assert res.status_code in (401, 403)


# --- serializer ---


def _valid(data):
    s = UserSTTConfigSerializer(data=data)
    return s.is_valid(), s.errors


def test_serializer_requires_model_name():
    ok, errors = _valid({"base_url": "https://x/v1", "model_name": ""})
    assert not ok
    assert "model_name" in errors


def test_serializer_requires_base_url():
    ok, errors = _valid({"model_name": "whisper-1"})
    assert not ok
    assert "base_url" in errors


def test_serializer_rejects_non_http_base_url():
    ok, errors = _valid({"base_url": "ftp://x/v1", "model_name": "whisper-1"})
    assert not ok


def test_serializer_rejects_base_url_with_credentials():
    ok, errors = _valid({"base_url": "https://u:p@x/v1", "model_name": "whisper-1"})
    assert not ok


def test_serializer_strips_trailing_slash():
    s = UserSTTConfigSerializer(data={"base_url": "https://example.com/v1/", "model_name": "whisper-1"})
    assert s.is_valid(), s.errors
    assert s.validated_data["base_url"] == "https://example.com/v1"


def test_serializer_rejects_short_api_key():
    ok, errors = _valid({"base_url": "https://x/v1", "model_name": "whisper-1", "api_key": "short"})
    assert not ok


def test_ssrf_blocks_loopback_when_enabled(settings):
    settings.ASSISTANT_BLOCK_PRIVATE_URLS = True
    assert ssrf.is_blocked("http://127.0.0.1:9000/v1")
