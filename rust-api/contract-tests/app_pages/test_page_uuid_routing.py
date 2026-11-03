"""Page-id route UUID gating (PIDASHCONV-1170).

Django's ``<uuid:page_id>`` / ``<uuid:pk>`` converter
(``django/urls/converters.py:25-26``) matches lowercase hyphenated UUIDs
only; uppercase, 32-hex simple, braced and ``urn:uuid:`` spellings get a
resolver 404 *before* authentication runs. Rust's ``parse_page_id`` used
``raw.parse::<uuid::Uuid>()`` (all four spellings accepted), so it
authenticated and served URLs Django never routes.

These tests run against both backends (``BASE_URL``). The un-fixed Rust
answers 401 anonymously and a gate JSON 403/404 or a 200/204 authed; the
fixed Rust (and Django) answer a framework 404 (HTML, no ``Allow`` header).
"""

from __future__ import annotations

import pytest

from .conftest import page_url

pytestmark = pytest.mark.contract

# A synthetic canonical id used for spellings that must not touch a row.
SYNTHETIC = "12345678-1234-abcd-ef01-234567890abc"


def _spellings(canonical: str):
    """The four non-canonical spellings Django's ``<uuid:>`` never routes."""
    return {
        "uppercase": canonical.upper(),
        "simple32": canonical.replace("-", ""),
        "braced": f"%7B{canonical}%7D",
        "urn": f"urn:uuid:{canonical}",
    }


def _assert_framework_404(response) -> None:
    assert response.status_code == 404, response.text
    assert "text/html" in response.headers.get("content-type", ""), response.headers
    # Framework 404s never pass a DRF view, so Django stamps no Allow; the
    # Rust proxy fallthrough must not add one either (PIDASHCONV-1048).
    assert "allow" not in response.headers, response.headers


SPELLINGS = list(_spellings(SYNTHETIC).items())


@pytest.mark.parametrize("label,segment", SPELLINGS)
def test_retrieve_non_canonical_404s(user_client, anon_client, world, label, segment):
    """GET .../pages/<non-canonical>/ — the detail route."""
    url = page_url(world, segment)
    _assert_framework_404(user_client.get(url))
    _assert_framework_404(anon_client.get(url))


@pytest.mark.parametrize("label,segment", SPELLINGS)
def test_lock_non_canonical_404s(user_client, anon_client, world, label, segment):
    """POST .../pages/<non-canonical>/lock/ — a detail action route."""
    url = f"{page_url(world, segment)}lock/"
    _assert_framework_404(user_client.post(url))
    _assert_framework_404(anon_client.post(url))


@pytest.mark.parametrize("label,segment", SPELLINGS)
def test_version_detail_pk_non_canonical_404s(user_client, anon_client, world, label, segment):
    """GET .../pages/<page>/versions/<non-canonical pk>/ — the pk converter."""
    url = f"{page_url(world, world['page']['id'])}versions/{segment}/"
    _assert_framework_404(user_client.get(url))
    _assert_framework_404(anon_client.get(url))


@pytest.mark.parametrize("label,segment", SPELLINGS)
def test_versions_list_page_non_canonical_404s(user_client, anon_client, world, label, segment):
    """GET .../pages/<non-canonical page>/versions/ — the page_id converter."""
    url = f"{page_url(world, segment)}versions/"
    _assert_framework_404(user_client.get(url))
    _assert_framework_404(anon_client.get(url))


def test_retrieve_uppercase_real_page_id_404s(user_client, anon_client, world):
    """Uppercase of the *real* page id: the row exists and the caller may
    read it, but the ``<uuid:>`` converter still 404s first."""
    segment = world["page"]["id"].upper()
    url = page_url(world, segment)
    _assert_framework_404(user_client.get(url))
    _assert_framework_404(anon_client.get(url))
