"""Denied access on the schema surface.

The document is public-read but read-*only*: there is no write permission
to hold, so every unsafe method is rejected with 405 on every endpoint,
unknown renderers 404, and the slashless URL redirects to the canonical one.
Together with the public-read pins, this is the permission contract: any
``SERVE_PERMISSIONS`` change (tightening *or* loosening) breaks this suite —
see the removal proof recorded in the PR.
"""

import httpx
import pytest

from _harness import env
from . import client as sc


@pytest.mark.contract
@pytest.mark.parametrize("path", sc.ENDPOINTS)
@pytest.mark.parametrize("method", ["POST", "PUT", "PATCH", "DELETE"])
def test_unsafe_methods_rejected(path, method):
    r = sc.request_patient(method, path)
    assert r.status_code == 405, (method, path, r.status_code)


@pytest.mark.contract
@pytest.mark.parametrize("path", sc.ENDPOINTS)
def test_allow_header_advertises_get(path):
    r = sc.request_patient("POST", path)
    assert r.status_code == 405
    assert "GET" in r.headers.get("allow", "")


@pytest.mark.contract
def test_unknown_format_404s():
    r = sc.get_patient(sc.SCHEMA, params={"format": "xml"})
    assert r.status_code == 404


@pytest.mark.contract
def test_slashless_redirects_to_canonical():
    # Redirects are served by CommonMiddleware outside DRF throttling, but a
    # hot window is still possible on a shared machine — retry once.
    r = httpx.get(env.BASE_URL + "/api/schema", follow_redirects=False, timeout=30)
    if r.status_code == 429:
        assert sc.get_patient("/api/schema").status_code == 301
        r = httpx.get(env.BASE_URL + "/api/schema", follow_redirects=False, timeout=30)
    assert r.status_code == 301
    assert r.headers["location"].endswith("/api/schema/")
