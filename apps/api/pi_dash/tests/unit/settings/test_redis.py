# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from unittest.mock import Mock, patch

import pytest
from django.test import override_settings

from pi_dash.settings import redis as redis_settings


@pytest.mark.unit
@override_settings(
    REDIS_URL="redis://:password@redis.example.test:6379/5",
    REDIS_SSL=False,
    REDIS_SOCKET_CONNECT_TIMEOUT=1.25,
    REDIS_SOCKET_TIMEOUT=2.5,
    REDIS_HEALTH_CHECK_INTERVAL=15,
    REDIS_MAX_CONNECTIONS=32,
)
def test_redis_instance_reuses_process_local_client_with_timeouts():
    redis_settings.close_redis_instance()
    client = Mock()
    with patch("pi_dash.settings.redis.redis.Redis.from_url", return_value=client) as from_url:
        first = redis_settings.redis_instance()
        second = redis_settings.redis_instance()

    assert first is client
    assert second is client
    from_url.assert_called_once_with(
        "redis://:password@redis.example.test:6379/5",
        db=0,
        socket_connect_timeout=1.25,
        socket_timeout=2.5,
        health_check_interval=15,
        max_connections=32,
    )
    redis_settings.close_redis_instance()


@pytest.mark.unit
@override_settings(
    REDIS_URL="redis://:password@redis.example.test:6379/5",
    REDIS_SSL=False,
    REDIS_MAX_CONNECTIONS=None,
)
def test_redis_instance_does_not_cap_connections_by_default():
    redis_settings.close_redis_instance()
    client = Mock()
    with patch(
        "pi_dash.settings.redis.redis.Redis.from_url",
        return_value=client,
    ) as from_url:
        assert redis_settings.redis_instance() is client

    assert "max_connections" not in from_url.call_args.kwargs
    redis_settings.close_redis_instance()


@pytest.mark.unit
@override_settings(
    REDIS_URL="rediss://serviceuser:password@redis.example.test:6380/5",
    REDIS_SSL=True,
    REDIS_SOCKET_CONNECT_TIMEOUT=1.25,
    REDIS_SOCKET_TIMEOUT=2.5,
    REDIS_HEALTH_CHECK_INTERVAL=15,
    REDIS_MAX_CONNECTIONS=None,
)
def test_redis_instance_ssl_preserves_default_user_and_db_zero():
    redis_settings.close_redis_instance()
    client = Mock()
    with patch("pi_dash.settings.redis.redis.Redis", return_value=client) as redis_cls:
        assert redis_settings.redis_instance() is client

    redis_cls.assert_called_once_with(
        host="redis.example.test",
        port=6380,
        password="password",
        db=0,
        ssl=True,
        ssl_cert_reqs=None,
        socket_connect_timeout=1.25,
        socket_timeout=2.5,
        health_check_interval=15,
    )
    redis_settings.close_redis_instance()


@pytest.mark.unit
@override_settings(REDIS_URL=None, REDIS_SSL=False)
def test_redis_instance_requires_redis_url():
    redis_settings.close_redis_instance()

    with pytest.raises(RuntimeError, match="REDIS_URL"):
        redis_settings.redis_instance()


@pytest.mark.unit
@override_settings(
    REDIS_URL="redis://redis-a.example.test:6379/5",
    REDIS_SSL=False,
)
def test_redis_instance_rebuilds_when_connection_settings_change(settings):
    redis_settings.close_redis_instance()
    first_client = Mock()
    second_client = Mock()
    with patch(
        "pi_dash.settings.redis.redis.Redis.from_url",
        side_effect=[first_client, second_client],
    ):
        assert redis_settings.redis_instance() is first_client
        settings.REDIS_URL = "redis://redis-b.example.test:6379/5"
        assert redis_settings.redis_instance() is second_client

    first_client.close.assert_called_once()
    redis_settings.close_redis_instance()
