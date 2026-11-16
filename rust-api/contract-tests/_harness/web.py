"""Session-cookie JSON request helpers for web-API contract suites (PIDASHCONV-98).

``login_session`` (in ``_harness.auth``) leaves a logged-in httpx client
holding the session cookie + ``csrftoken``. Django's session auth rejects
unsafe methods without the CSRF header (403 ``CSRF Failed``), which would
mask each endpoint's real gate — these helpers attach exactly what the web
client sends (``X-CSRFToken`` + ``Referer``). Read-only GETs go through the
plain client; they need no CSRF header.
"""

from __future__ import annotations

import httpx


def csrf_headers(client: httpx.Client) -> dict:
    csrf = client.cookies.get("csrftoken")
    assert csrf, "no csrftoken cookie on the session client"
    return {"X-CSRFToken": csrf, "Referer": str(client.base_url)}


def web_request(client, method: str, url: str, payload=None, *, params=None):
    """Unsafe-method JSON request carrying the session CSRF pair."""
    return client.request(
        method,
        url,
        json=payload if payload is not None else {},
        params=params,
        headers=csrf_headers(client),
    )


def web_post(client, url: str, payload: dict):
    return web_request(client, "POST", url, payload)


def web_patch(client, url: str, payload: dict):
    return web_request(client, "PATCH", url, payload)


def web_put(client, url: str, payload: dict):
    return web_request(client, "PUT", url, payload)


def web_delete(client, url: str, payload=None, *, params=None):
    return web_request(client, "DELETE", url, payload, params=params)
