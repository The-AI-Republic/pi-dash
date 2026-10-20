"""Resolver-404 bytes: non-UUID ``<uuid:>`` segments (PIDASHCONV-1215).

Django's global ``handler404`` (``pi_dash/urls.py:15`` ->
``app/views/error_404.py``) is a plain ``JsonResponse``, so a path segment
that misses a ``<uuid:>`` converter answers the 28-byte body
``b'{"error": "Page not found."}'`` — ``json.dumps`` defaults, i.e. with the
space after the colon.

Rust matches those segments with a bare ``{asset_id}``/``{workspace_id}``
param and answers the same 404 from its own constant *after* the parse
fails, so the bytes must match Django exactly. Every suite compared the
parsed JSON (``response.json() == {...}``), which cannot see whitespace —
these pin the exact bytes instead.
"""

import pytest

pytestmark = pytest.mark.contract

RESOLVER_404 = b'{"error": "Page not found."}'


def assert_resolver_404(response):
    assert response.status_code == 404, response.text
    ctype = response.headers.get("content-type", "")
    if ctype.startswith("application/json"):
        # The prod resolver-404 bytes, exactly (JsonResponse defaults).
        assert response.content == RESOLVER_404, response.content
    else:
        # Harness Django (DEBUG=True) renders the HTML technical 404 for an
        # unmatched path, so only the status and content type are comparable.
        assert "text/html" in ctype, ctype


def test_v1_file_asset_non_uuid_workspace_is_resolver_404(org):
    # GET /api/workspaces/file-assets/<uuid:workspace_id>/<str:asset_key>/
    response = org.request(
        "GET", "/api/workspaces/file-assets/not-a-uuid/some-key/", None
    )
    assert_resolver_404(response)


def test_v2_workspace_asset_non_uuid_pk_is_resolver_404(org):
    # GET /api/assets/v2/workspaces/<str:slug>/<uuid:asset_id>/
    response = org.request("GET", "/api/assets/v2/workspaces/ws-x/not-a-uuid/", None)
    assert_resolver_404(response)


def test_v2_check_non_uuid_asset_id_is_resolver_404(org):
    # GET /api/assets/v2/workspaces/<str:slug>/check/<uuid:asset_id>/
    response = org.request(
        "GET", "/api/assets/v2/workspaces/ws-x/check/not-a-uuid/", None
    )
    assert_resolver_404(response)
