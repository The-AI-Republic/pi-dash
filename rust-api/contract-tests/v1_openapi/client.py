"""HTTP surface of the api-v1 OpenAPI schema domain (D-23).

Served routes — live exactly when ``ENABLE_DRF_SPECTACULAR=1``:

- ``GET /api/schema/`` — the OpenAPI document itself (YAML by default,
  ``?format=json`` for JSON). The document *describes* ``/api/v1/``: the
  ``SCHEMA_PATH_PREFIX`` plus the ``preprocess_filter_api_v1_paths`` hook
  keep every path under ``/api/v1/``, drop all ``PUT`` operations and any
  path containing ``server``.
- ``GET /api/schema/swagger-ui/`` — Swagger UI HTML, wired to ``/api/schema/``.
- ``GET /api/schema/redoc/`` — ReDoc HTML, wired to ``/api/schema/``.

All three views are intentionally public (``SERVE_PERMISSIONS`` defaults to
``AllowAny``): the document carries no tenant data, so anonymous, keyed and
cross-tenant fetches must return identical bytes. The surface is read-only:
every unsafe method is rejected with 405.

``pi_dash/api/urls/schema.py`` defines the same three views under
``schema/`` but is *not* imported by ``pi_dash/api/urls/__init__.py`` — it
is currently unwired (no ``/api/v1/schema/*`` routes exist), so this suite
pins only the served ``/api/schema/*`` trio.
"""

from __future__ import annotations

import os
import time

import httpx

from _harness import env

SCHEMA = "/api/schema/"
SWAGGER_UI = "/api/schema/swagger-ui/"
REDOC = "/api/schema/redoc/"

#: Every drf-spectacular endpoint in this domain.
ENDPOINTS = (SCHEMA, SWAGGER_UI, REDOC)


def client(api_key: str | None = None, timeout: float = 120.0) -> httpx.Client:
    """Anonymous client, or tenant-keyed when ``api_key`` is given.

    The schema views ignore API keys by design (public document); the key
    only matters for the tenant-isolation tests, which prove it changes
    nothing.
    """
    headers: dict[str, str] = {}
    if api_key is not None:
        headers["X-Api-Key"] = api_key
    return httpx.Client(base_url=env.BASE_URL, headers=headers, timeout=timeout)


def get_schema(api_key: str | None = None, format: str | None = None) -> httpx.Response:
    params = {"format": format} if format is not None else None
    with client(api_key) as c:
        return c.get(SCHEMA, params=params)


def get_ui(path: str, api_key: str | None = None) -> httpx.Response:
    with client(api_key) as c:
        return c.get(path)


#: Seconds to wait out a hot anonymous throttle window (the limit is
#: 30/minute per IP on a shared Redis cache, so a previous run's burst can
#: still be counting down). Override with ``CONTRACT_ANON_WAIT``.
ANON_WAIT = float(os.environ.get("CONTRACT_ANON_WAIT", "75"))


def get_patient(
    path: str, params: dict | None = None, api_key: str | None = None
) -> httpx.Response:
    """GET that waits out ``429``s from the global ``AnonRateThrottle``.

    These views take session auth only, so API keys do not identify the
    caller and every request here counts toward the anonymous budget. A
    throttled reply carries no information about the endpoint, so it is
    retried until the window resets; any *other* status (200, 403, 404,
    405, …) is returned immediately — in particular a permission change
    surfaces fast instead of hiding behind the wait.
    """
    deadline = time.monotonic() + ANON_WAIT
    while True:
        with client(api_key) as c:
            r = c.get(path, params=params)
        if r.status_code != 429 or time.monotonic() >= deadline:
            return r
        time.sleep(2)


def request_patient(method: str, path: str) -> httpx.Response:
    """Unsafe-method twin of :func:`get_patient`: waits out ``429``s, then
    returns the first real verdict (normally 405 on this surface)."""
    deadline = time.monotonic() + ANON_WAIT
    while True:
        with client() as c:
            r = c.request(method, path)
        if r.status_code != 429 or time.monotonic() >= deadline:
            return r
        time.sleep(2)
