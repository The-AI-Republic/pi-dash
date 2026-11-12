# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The default per-token rate limit on ``/api/v1/`` (PDASHOSS01-309).

A normal (``is_service=False``) API token is throttled by ``ApiKeyRateThrottle``.
With ``API_KEY_RATE_LIMIT`` unset the limit is 120 requests per minute
(2 per second); the env var still overrides it.

``settings/test.py`` sets ``API_KEY_RATE_LIMIT=100000/minute`` for the rest of
the suite, and the throttle reads its rate at import time, so these tests drop
the env var and re-derive the rate the way the module does.
"""

from __future__ import annotations

import uuid

import pytest
from django.core.cache import cache
from rest_framework import status as http_status
from rest_framework.test import APIClient

from pi_dash.api.rate_limit import (
    DEFAULT_API_KEY_RATE_LIMIT,
    ApiKeyRateThrottle,
    get_api_key_rate_limit,
)
from pi_dash.db.models import APIToken

ME_URL = "/api/v1/users/me/"


def test_default_is_120_per_minute_when_env_unset(monkeypatch):
    monkeypatch.delenv("API_KEY_RATE_LIMIT", raising=False)

    assert DEFAULT_API_KEY_RATE_LIMIT == "120/minute"
    assert get_api_key_rate_limit() == "120/minute"


def test_env_var_overrides_default(monkeypatch):
    monkeypatch.setenv("API_KEY_RATE_LIMIT", "7/minute")

    assert get_api_key_rate_limit() == "7/minute"


@pytest.fixture
def unthrottled_default_rate(monkeypatch):
    """Run ApiKeyRateThrottle at the rate it gets when the env var is unset."""
    monkeypatch.delenv("API_KEY_RATE_LIMIT", raising=False)
    monkeypatch.setattr(ApiKeyRateThrottle, "rate", get_api_key_rate_limit())


@pytest.fixture
def normal_token_client(db, create_user):
    # Unique token so the Redis-backed throttle history never leaks between runs.
    token = APIToken.objects.create(
        user=create_user,
        label="rate-limit test",
        token=f"rate-limit-{uuid.uuid4().hex}",
        is_service=False,
    )
    cache_key = f"{ApiKeyRateThrottle.scope}:{token.token}"
    cache.delete(cache_key)
    client = APIClient()
    client.credentials(HTTP_X_API_KEY=token.token)
    yield client
    cache.delete(cache_key)


@pytest.mark.django_db
def test_normal_token_gets_120_requests_then_429(unthrottled_default_rate, normal_token_client):
    for i in range(120):
        resp = normal_token_client.get(ME_URL)
        assert resp.status_code == http_status.HTTP_200_OK, f"request {i + 1} got {resp.status_code}"

    resp = normal_token_client.get(ME_URL)
    assert resp.status_code == http_status.HTTP_429_TOO_MANY_REQUESTS


@pytest.mark.django_db
def test_env_override_applies_to_requests(monkeypatch, normal_token_client):
    monkeypatch.setenv("API_KEY_RATE_LIMIT", "3/minute")
    monkeypatch.setattr(ApiKeyRateThrottle, "rate", get_api_key_rate_limit())

    for _ in range(3):
        assert normal_token_client.get(ME_URL).status_code == http_status.HTTP_200_OK
    assert normal_token_client.get(ME_URL).status_code == http_status.HTTP_429_TOO_MANY_REQUESTS
