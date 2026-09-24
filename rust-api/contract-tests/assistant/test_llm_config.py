# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""BYOK LLM config and title generation.

Covers ``users/me/ai-assistant/config/`` (lifecycle, key secrecy, serializer
validation, the SSRF save-time guard) and ``generate-title/`` (the
deterministic gates: config missing, blank description, guest denial).
Successful provider calls need a live LLM, so they are out of scope here;
the ``/test/`` probe is covered through its missing-config shape.
"""

from .conftest import PUBLIC_BASE, threads_url

CONFIG_URL = "/api/users/me/ai-assistant/config/"
TEST_URL = "/api/users/me/ai-assistant/config/test/"


def _unset_shape(body):
    assert set(body.keys()) >= {
        "provider_kind",
        "base_url",
        "model_name",
        "has_api_key",
        "last_verified_at",
    }
    assert body["has_api_key"] is False
    assert body["last_verified_at"] is None


def test_llm_config_lifecycle(world, member):
    assert member.get(CONFIG_URL).status_code == 200
    _unset_shape(member.get(CONFIG_URL).json())

    res = member.put(
        CONFIG_URL,
        json={
            "provider_kind": "openai_compatible",
            "base_url": PUBLIC_BASE,
            "model_name": "m",
            "api_key": "test-key-123",
        },
    )
    assert res.status_code == 200
    body = res.json()
    assert body["has_api_key"] is True
    assert body["base_url"] == PUBLIC_BASE
    assert body["model_name"] == "m"
    assert "api_key" not in body

    # Updating the model without resending the key keeps the stored key.
    res = member.put(CONFIG_URL, json={"model_name": "m2"})
    assert res.status_code == 200
    assert res.json()["has_api_key"] is True
    assert res.json()["model_name"] == "m2"

    assert member.delete(CONFIG_URL).status_code == 204
    assert member.get(CONFIG_URL).json()["has_api_key"] is False


def test_llm_config_is_per_user(world, member, admin):
    admin.put(
        CONFIG_URL,
        json={
            "provider_kind": "openai_compatible",
            "base_url": PUBLIC_BASE,
            "model_name": "m",
            "api_key": "test-key-123",
        },
    )
    assert member.get(CONFIG_URL).json()["has_api_key"] is False


def test_llm_config_validation(world, member):
    # Non-http(s) scheme.
    res = member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible", "base_url": "ftp://x/v1",
              "model_name": "m", "api_key": "test-key-123"},
    )
    assert res.status_code == 400

    # Credentials embedded in the URL.
    res = member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible", "base_url": "https://u:p@8.8.8.8/v1",
              "model_name": "m", "api_key": "test-key-123"},
    )
    assert res.status_code == 400

    # OpenAI-compatible providers require a base URL and model name.
    res = member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible", "base_url": "",
              "model_name": "m", "api_key": "test-key-123"},
    )
    assert res.status_code == 400

    res = member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible", "base_url": PUBLIC_BASE,
              "model_name": "", "api_key": "test-key-123"},
    )
    assert res.status_code == 400

    # Keys that are implausibly short are rejected.
    res = member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible", "base_url": PUBLIC_BASE,
              "model_name": "m", "api_key": "short"},
    )
    assert res.status_code == 400


def test_llm_config_rejects_blocked_url_at_save(world, member):
    res = member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible",
              "base_url": "http://169.254.169.254/v1",
              "model_name": "m", "api_key": "test-key-123"},
    )
    assert res.status_code == 400
    assert res.json()["error"] == "base_url_blocked"


def test_llm_test_missing_config(world, member):
    res = member.post(TEST_URL)
    assert res.status_code == 200
    body = res.json()
    assert body["ok"] is False
    assert body["error_code"] == "llm_config_missing"


def test_llm_test_requires_auth(world, anon):
    assert anon.post(TEST_URL).status_code == 401


def test_generate_title_requires_llm_config(world, member):
    res = member.post(
        f"{threads_url(world)}/generate-title/",
        json={"description": "Ship the new dashboard export."},
    )
    assert res.status_code == 422
    assert res.json()["error"] == "llm_config_missing"


def test_generate_title_requires_description(world, member):
    member.put(
        CONFIG_URL,
        json={"provider_kind": "openai_compatible", "base_url": PUBLIC_BASE,
              "model_name": "m", "api_key": "test-key-123"},
    )
    res = member.post(f"{threads_url(world)}/generate-title/", json={"description": "   "})
    assert res.status_code == 400
    assert res.json()["error"] == "description_required"


def test_generate_title_blocked_for_guest(world, guest):
    res = guest.post(
        f"{threads_url(world)}/generate-title/",
        json={"description": "Ship the new dashboard export."},
    )
    assert res.status_code == 403
