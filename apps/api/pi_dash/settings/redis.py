# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import threading
from urllib.parse import urlparse

import redis
from django.conf import settings


_redis_client = None
_redis_client_key = None
_redis_lock = threading.Lock()


def _float_setting(name: str, default: float) -> float:
    raw = getattr(settings, name, default)
    try:
        return float(raw)
    except (TypeError, ValueError):
        return default


def _int_setting(name: str, default: int) -> int:
    raw = getattr(settings, name, default)
    try:
        return int(raw)
    except (TypeError, ValueError):
        return default


def _optional_int_setting(name: str) -> int | None:
    raw = getattr(settings, name, None)
    if raw in (None, ""):
        return None
    try:
        value = int(raw)
    except (TypeError, ValueError):
        return None
    return value if value > 0 else None


def close_redis_instance():
    """Close the cached Redis client for this process.

    Mostly useful for tests and one-off management commands that override
    settings at runtime. Gunicorn workers keep one cached client each.
    """

    global _redis_client, _redis_client_key
    with _redis_lock:
        if _redis_client is not None:
            _redis_client.close()
        _redis_client = None
        _redis_client_key = None


def redis_instance():
    """Return a process-local Redis client with bounded socket timeouts."""

    if not settings.REDIS_URL:
        raise RuntimeError("REDIS_URL is required to create a Redis client")

    connect_timeout = _float_setting("REDIS_SOCKET_CONNECT_TIMEOUT", 2.0)
    socket_timeout = _float_setting("REDIS_SOCKET_TIMEOUT", 5.0)
    health_check_interval = _int_setting("REDIS_HEALTH_CHECK_INTERVAL", 30)
    max_connections = _optional_int_setting("REDIS_MAX_CONNECTIONS")
    key = (
        settings.REDIS_URL,
        bool(settings.REDIS_SSL),
        connect_timeout,
        socket_timeout,
        health_check_interval,
        max_connections,
    )

    global _redis_client, _redis_client_key
    if _redis_client is not None and _redis_client_key == key:
        return _redis_client

    with _redis_lock:
        if _redis_client is not None and _redis_client_key == key:
            return _redis_client
        if _redis_client is not None:
            _redis_client.close()

        kwargs = {
            "socket_connect_timeout": connect_timeout,
            "socket_timeout": socket_timeout,
            "health_check_interval": health_check_interval,
        }
        if max_connections is not None:
            kwargs["max_connections"] = max_connections
        # connect to redis
        if settings.REDIS_SSL:
            url = urlparse(settings.REDIS_URL)
            _redis_client = redis.Redis(
                host=url.hostname,
                port=url.port,
                password=url.password,
                db=0,
                ssl=True,
                ssl_cert_reqs=None,
                **kwargs,
            )
        else:
            _redis_client = redis.Redis.from_url(settings.REDIS_URL, db=0, **kwargs)
        _redis_client_key = key
        return _redis_client
