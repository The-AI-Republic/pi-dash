# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``clear_cache`` against TLS Redis (PIDASHCONV-1269).

Django disables TLS certificate verification for ``rediss://`` URLs
(``ssl_cert_reqs=None`` in ``pi_dash/settings/redis.py`` and
``ssl_cert_reqs: False`` in the ``CACHES`` pool kwargs), so a
self-signed or private-CA Redis is reachable. The Rust port must match:
``pidash-api ops clear_cache`` prints ``Cache Cleared`` and flushes the
database instead of failing the handshake.

Each test seeds the ``REDIS_URL`` server, drives one backend
(``manage.py clear_cache`` for Django, ``pidash-api ops clear_cache``
for Rust), and asserts the success line plus an empty database
afterwards. Against a self-signed ``rediss://`` server the Django leg
passes and the Rust leg fails before the fix (handshake failure), and
both pass after it. Against plain ``redis://`` both legs pass
throughout (parity regression).

Run from ``rust-api/contract-tests`` (the binary must be built)::

    export REDIS_URL=rediss://127.0.0.1:6390/0  # self-signed TLS Redis
    export DJANGO_SETTINGS_MODULE=pi_dash.settings.test
    cargo build --bin pidash-api  # in rust-api/
    pytest -p no:django ops/test_tls_cache.py -v

``PIDASH_MANAGE_PY`` / ``PIDASH_API_BIN`` (or ``PIDASH_RUST_BIN``)
override the driver paths. The suite runs both legs against database 7
of the ``REDIS_URL`` server (derived, scheme/host/port preserved), so
its ``FLUSHDB`` never touches sibling suites' keys.
"""

from __future__ import annotations

import os
import subprocess
import sys
import urllib.parse
from pathlib import Path

import pytest
import redis

REPO_ROOT = Path(__file__).resolve().parents[3]
MANAGE_PY = os.environ.get("PIDASH_MANAGE_PY", str(REPO_ROOT / "apps/api/manage.py"))
RUST_BIN = (
    os.environ.get("PIDASH_API_BIN")
    or os.environ.get("PIDASH_RUST_BIN")
    or str(REPO_ROOT / "rust-api/target/debug/pidash-api")
)

CACHE_CLEARED = b"Cache Cleared\n"

# Dedicated database number: the full-clear path runs a real FLUSHDB,
# like Django (same convention as the boot-storage suite's db 6).
TEST_DB = 7


def _require_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        pytest.fail(f"tls-cache suite needs {name} (see test module docstring)")
    return value


def suite_redis_url() -> str:
    """``REDIS_URL`` retargeted at the suite's dedicated database."""
    parts = urllib.parse.urlsplit(_require_env("REDIS_URL"))
    if not parts.scheme or not parts.hostname:
        pytest.fail(f"tls-cache suite needs a URL REDIS_URL, got {parts.geturl()!r}")
    rebuilt = urllib.parse.urlunsplit(
        (parts.scheme, parts.netloc, f"/{TEST_DB}", parts.query, parts.fragment)
    )
    return rebuilt


def suite_client() -> redis.Redis:
    """Seeding/verification client (no cert verification, like Django)."""
    url = suite_redis_url()
    if "rediss" in url:
        return redis.Redis.from_url(url, decode_responses=True, ssl_cert_reqs=None)
    return redis.Redis.from_url(url, decode_responses=True)


def run_django(*args: str) -> subprocess.CompletedProcess:
    env = dict(os.environ)
    env["REDIS_URL"] = suite_redis_url()
    return subprocess.run(
        [sys.executable, MANAGE_PY, *args],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=300,
    )


def run_rust(*args: str) -> subprocess.CompletedProcess:
    env = dict(os.environ)
    env["REDIS_URL"] = suite_redis_url()
    if not os.path.isfile(RUST_BIN):
        pytest.fail(
            f"pidash-api binary not found at {RUST_BIN} "
            "(build it: cargo build --bin pidash-api, or set PIDASH_API_BIN)"
        )
    return subprocess.run(
        [RUST_BIN, "ops", *args],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=300,
    )


def seed(client: redis.Redis) -> None:
    client.flushdb()
    client.set(":1:tlsprobe", "v")
    client.set("plaintls", "v")
    assert client.dbsize() == 2


def test_django_clear_cache_tls():
    """Django oracle: self-signed TLS Redis clears (ssl_cert_reqs=None)."""
    client = suite_client()
    seed(client)
    proc = run_django("clear_cache")
    assert proc.returncode == 0
    assert proc.stdout == CACHE_CLEARED
    assert proc.stderr == b""
    assert client.dbsize() == 0


def test_rust_clear_cache_tls():
    """Rust port: same URL, same line, empty database afterwards."""
    client = suite_client()
    seed(client)
    proc = run_rust("clear_cache")
    assert proc.returncode == 0
    assert proc.stdout == CACHE_CLEARED
    assert proc.stderr == b""
    assert client.dbsize() == 0
