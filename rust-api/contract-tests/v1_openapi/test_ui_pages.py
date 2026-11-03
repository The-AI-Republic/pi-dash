"""Swagger UI and ReDoc pages: ``GET /api/schema/swagger-ui/`` and ``redoc/``."""

import re

import pytest

from . import client as sc

#: Per-render CSRF token baked into the Swagger page; the only part of
#: either UI page that legitimately varies request to request.
CSRF_PATTERN = re.compile(r'CSRFTOKEN"\] = "[^"]*";')


def normalize(body: str) -> str:
    return CSRF_PATTERN.sub('CSRFTOKEN"] = "";', body)


@pytest.mark.contract
def test_swagger_ui_shape():
    r = sc.get_patient(sc.SWAGGER_UI)
    assert r.status_code == 200
    assert "text/html" in r.headers["content-type"]
    body = r.text
    assert "<title>The Pi Dash REST API</title>" in body
    assert "swagger-ui-bundle" in body
    assert 'url: "/api/schema/"' in body
    assert CSRF_PATTERN.search(body), "expected per-render CSRF token"


@pytest.mark.contract
def test_redoc_shape():
    r = sc.get_patient(sc.REDOC)
    assert r.status_code == 200
    assert "text/html" in r.headers["content-type"]
    body = r.text
    assert "<title>The Pi Dash REST API</title>" in body
    assert "<redoc spec-url=\"/api/schema/\">" in body


@pytest.mark.contract
def test_anonymous_burst_is_throttled():
    """The global ``AnonRateThrottle`` (30/minute per IP) also guards this
    public surface: a sustained burst ends in 429 with ``Retry-After``.

    Runs last in this module — it deliberately exhausts the shared
    anonymous budget, so nothing after it may depend on an unthrottled
    anonymous reply. The throttle cache lives in Redis and is shared by
    every suite on this machine; concurrent contract runs may see 429s
    while this test's window counts down.
    """
    retry_after = None
    for _ in range(45):
        with sc.client() as c:
            r = c.get(sc.REDOC)
        if r.status_code == 429:
            retry_after = r.headers.get("retry-after")
            break
    assert retry_after is not None, "burst of 45 never throttled"
    assert int(retry_after) >= 1
