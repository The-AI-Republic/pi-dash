# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.assistant import ssrf
from pi_dash.assistant.serializers import UserLLMConfigSerializer


def _valid(data):
    s = UserLLMConfigSerializer(data=data)
    return s.is_valid(), s.errors


def test_serializer_requires_model_name():
    ok, errors = _valid({"provider_kind": "openai_compatible", "base_url": "https://x/v1", "model_name": ""})
    assert not ok
    assert "model_name" in errors


def test_serializer_openai_requires_base_url():
    ok, errors = _valid({"provider_kind": "openai_compatible", "model_name": "m"})
    assert not ok
    assert "base_url" in errors


def test_serializer_rejects_non_http_base_url():
    ok, errors = _valid(
        {"provider_kind": "openai_compatible", "base_url": "ftp://x/v1", "model_name": "m"}
    )
    assert not ok


def test_serializer_strips_trailing_slash():
    s = UserLLMConfigSerializer(
        data={"provider_kind": "openai_compatible", "base_url": "https://example.com/v1/", "model_name": "m"}
    )
    assert s.is_valid(), s.errors
    assert s.validated_data["base_url"] == "https://example.com/v1"


def test_serializer_anthropic_no_base_url_ok():
    ok, errors = _valid({"provider_kind": "anthropic", "model_name": "claude-x"})
    assert ok, errors


def test_serializer_rejects_short_api_key():
    ok, errors = _valid(
        {"provider_kind": "anthropic", "model_name": "claude-x", "api_key": "short"}
    )
    assert not ok


def test_ssrf_off_by_default(settings):
    settings.ASSISTANT_BLOCK_PRIVATE_URLS = False
    assert not ssrf.blocking_enabled()
    assert not ssrf.is_blocked("http://localhost:8000/v1")


def test_ssrf_blocks_loopback_when_enabled(settings):
    settings.ASSISTANT_BLOCK_PRIVATE_URLS = True
    assert ssrf.is_blocked("http://127.0.0.1:8000/v1")
    assert ssrf.is_blocked("http://localhost/v1")


def test_ssrf_blocks_link_local_metadata(settings):
    settings.ASSISTANT_BLOCK_PRIVATE_URLS = True
    assert ssrf.is_blocked("http://169.254.169.254/latest/meta-data/")
