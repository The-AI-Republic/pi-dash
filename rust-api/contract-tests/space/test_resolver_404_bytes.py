"""Resolver-404 bytes: non-UUID ``<uuid:pk>`` segment (PIDASHCONV-1215).

Django's global ``handler404`` returns a plain ``JsonResponse`` whose bytes
are ``json.dumps`` defaults — ``b'{"error": "Page not found."}'`` (28 bytes,
space after the colon). The Rust handler matches the segment and re-emits
that body from its own constant, so the whitespace must match exactly; a
parsed-JSON comparison cannot see it.
"""

import pytest

pytestmark = pytest.mark.contract

RESOLVER_404 = b'{"error": "Page not found."}'


def assert_resolver_404(response):
    assert response.status_code == 404, response.text
    ctype = response.headers.get("content-type", "")
    if ctype.startswith("application/json"):
        assert response.content == RESOLVER_404, response.content
    else:
        # Harness Django (DEBUG=True) renders the HTML technical 404 for an
        # unmatched path, so only the status and content type are comparable.
        assert "text/html" in ctype, ctype


def test_entity_asset_non_uuid_pk_is_resolver_404(world, anon_client):
    # GET /api/public/assets/v2/anchor/<str:anchor>/<uuid:pk>/
    response = anon_client.get(
        f"/api/public/assets/v2/anchor/{world['anchor']}/not-a-uuid/"
    )
    assert_resolver_404(response)
