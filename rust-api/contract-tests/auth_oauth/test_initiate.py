"""Contract suite: OAuth initiate routes (PIDASHCONV-102).

Covers all 8 initiate endpoints — ``/auth/{google,github,gitlab,gitea}/``
and ``/auth/spaces/{google,github,gitlab,gitea}/`` — which are plain Django
``View`` GET handlers returning 302 redirects.

With no provider configured (see ``conftest.oauth_unconfigured``) every
initiate takes the same branch: the provider constructor raises
``AuthenticationException`` with the provider's ``*_NOT_CONFIGURED`` code
before any outbound HTTP, and the view 302s to the app (or space) base
carrying ``error_code`` + ``error_message``. Each test pins the status,
the redirect base (app vs spaces/), and the Python-defined error code:

- google 5105, github 5110, gitlab 5111, gitea 5112.

The configured branch (302 straight to the provider authorization URL with
``client_id``/``redirect_uri``/``state`` params) needs live provider
credentials and is therefore out of scope — see "Known oracle limits".
"""

import os
import sys
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

# Third element: expected redirect path. google/github/gitlab share
# get_safe_redirect_url (base + "/?..."); gitea builds with urljoin, so an
# empty path (urljoin("http://host", "?error...") drops the slash).
APP_INITIATE = [
    ("/auth/google/", "5105", "/"),
    ("/auth/github/", "5110", "/"),
    ("/auth/gitlab/", "5111", "/"),
    ("/auth/gitea/", "5112", ""),
]

SPACE_INITIATE = [
    ("/auth/spaces/google/", "5105", "/spaces/"),
    ("/auth/spaces/github/", "5110", "/spaces/"),
    ("/auth/spaces/gitlab/", "5111", "/spaces/"),
    ("/auth/spaces/gitea/", "5112", "/spaces/"),
]


def _location_parts(resp: httpx.Response):
    assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:200]}"
    return urlsplit(resp.headers["location"])


def _raw_location(parts) -> str:
    return parts.geturl()


@pytest.mark.parametrize("path,error_code,expected_path", APP_INITIATE)
def test_app_initiate_redirects_with_not_configured_code(
    client: httpx.Client, app_base: str, path: str, error_code: str, expected_path: str
):
    parts = _location_parts(client.get(path))
    assert f"{parts.scheme}://{parts.netloc}" == app_base
    assert parts.path == expected_path, parts.geturl()
    query = parse_qs(parts.query)
    assert query.get("error_code") == [error_code], parts.query
    assert "error_message" in query


@pytest.mark.parametrize("path,error_code,expected_path", SPACE_INITIATE)
def test_space_initiate_redirects_with_not_configured_code(
    client: httpx.Client, space_base: str, path: str, error_code: str, expected_path: str
):
    parts = _location_parts(client.get(path))
    assert f"{parts.scheme}://{parts.netloc}" == space_base
    assert parts.path == expected_path, parts.geturl()
    query = parse_qs(parts.query)
    assert query.get("error_code") == [error_code], parts.query
    assert "error_message" in query


def test_app_initiate_carries_valid_next_path(client: httpx.Client, app_base: str):
    # get_safe_redirect_url echoes a validated relative next_path alongside
    # the error params (google is representative of the google/github/gitlab
    # app + space views, which all share that helper).
    parts = _location_parts(client.get("/auth/google/", params={"next_path": "/test-path"}))
    assert f"{parts.scheme}://{parts.netloc}" == app_base
    query = parse_qs(parts.query)
    assert query.get("next_path") == ["/test-path"]
    assert query.get("error_code") == ["5105"]


def test_app_initiate_drops_absolute_next_path(client: httpx.Client, app_base: str):
    # An absolute URL is not a safe next_path: it must not survive into the
    # redirect target.
    parts = _location_parts(
        client.get("/auth/google/", params={"next_path": "https://evil.example/x"})
    )
    assert "evil.example" not in _raw_location(parts)
    query = parse_qs(parts.query)
    assert query.get("error_code") == ["5105"]


def test_gitea_initiate_carries_valid_next_path(client: httpx.Client, app_base: str):
    # The gitea views build the error redirect with urljoin + urlencode
    # instead of get_safe_redirect_url, so next_path arrives percent-encoded
    # inside the params — same values, different wire shape. Port it as is.
    parts = _location_parts(client.get("/auth/gitea/", params={"next_path": "/test-path"}))
    assert f"{parts.scheme}://{parts.netloc}" == app_base
    query = parse_qs(parts.query)
    assert query.get("next_path") == ["/test-path"]
    assert query.get("error_code") == ["5112"]


def test_space_gitea_initiate_carries_valid_next_path(client: httpx.Client, space_base: str):
    parts = _location_parts(
        client.get("/auth/spaces/gitea/", params={"next_path": "/test-path"})
    )
    assert f"{parts.scheme}://{parts.netloc}" == space_base
    query = parse_qs(parts.query)
    assert query.get("next_path") == ["/test-path"]
    assert query.get("error_code") == ["5112"]
