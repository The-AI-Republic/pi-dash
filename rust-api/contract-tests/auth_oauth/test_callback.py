"""Contract suite: OAuth callback routes (PIDASHCONV-102).

Covers all 8 callback endpoints — ``/auth/{google,github,gitlab,gitea}/callback/``
and ``/auth/spaces/{google,github,gitlab,gitea}/callback/`` — which are
plain Django ``View`` GET handlers.

A callback first checks ``state`` against the session, then requires
``code``, and only then constructs the provider (which would exchange the
code over HTTP). With a fresh client the session carries no ``state``, so
``?code=<anything>`` stops at the state-mismatch branch — before any
provider traffic — and 302s to the app (or space) base carrying the
provider's ``*_OAUTH_PROVIDER_ERROR`` code:

- google 5115, github 5120, gitlab 5121, gitea 5123.

No session cookie may be set on this path (see ``test_permissions.py`` for
the denied-permission case built on the same branch).

The success branch (valid code exchanged, user logged in, 302 without
``error_code``) needs live provider credentials and is out of scope — see
"Known oracle limits" in ``test_permissions.py``.

Quirks pinned here (port them, don't fix them):

- App gitea builds its error redirect with
  ``urljoin(request.session.get("host"), ...)``. With no prior initiate
  the session has no host and ``urljoin(None, "?error...")`` returns the
  relative ref unchanged, so the 302 ``Location`` is the bare relative
  ``?error_code=5123&error_message=...`` — not an absolute URL like every
  other callback. (With a session host it is absolute.)
- Space google/github/gitlab callbacks assign
  ``base_host = request.session.get("host")`` at the top of ``get``,
  shadowing the imported ``base_host()`` helper for the whole function
  body. Every later ``base_host(request=...)`` call then raises
  ``TypeError: 'NoneType' object is not callable`` (or ``'str' object is
  not callable`` once a host is stored), so these three endpoints answer
  500 on every input — mismatch, missing code, and success branches alike.
  The spaces gitea twin never assigns that local and 302s normally.
"""

import os
import sys
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness.client import SESSION_COOKIE  # noqa: E402

APP_CALLBACKS = [
    ("/auth/google/callback/", "5115"),
    ("/auth/github/callback/", "5120"),
    ("/auth/gitlab/callback/", "5121"),
]

# spaces/{google,github,gitlab}/callback/ always 500 (see module docstring);
# spaces/gitea is the only spaces callback that redirects.
SPACE_CALLBACKS = [
    ("/auth/spaces/gitea/callback/", "5123"),
]

SPACE_500_CALLBACKS = [
    "/auth/spaces/google/callback/",
    "/auth/spaces/github/callback/",
    "/auth/spaces/gitlab/callback/",
]

BOGUS_CODE = "contract-test-invalid-code"


def _error_parts(resp: httpx.Response):
    assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:200]}"
    return urlsplit(resp.headers["location"])


def _assert_provider_error(parts, base: str, error_code: str) -> None:
    assert f"{parts.scheme}://{parts.netloc}" == base
    query = parse_qs(parts.query)
    assert query.get("error_code") == [error_code], parts.query
    assert "error_message" in query


def _assert_no_session_cookie(resp: httpx.Response) -> None:
    for raw in resp.headers.get_list("set-cookie"):
        assert not raw.startswith(SESSION_COOKIE + "="), f"session cookie set: {raw!r}"


@pytest.mark.parametrize("path,error_code", APP_CALLBACKS)
def test_app_callback_invalid_code_redirects_with_provider_error(
    client: httpx.Client, app_base: str, path: str, error_code: str
):
    # Fresh client: no session state, so the code never reaches the provider
    # (no outbound traffic) and the state-mismatch branch answers.
    resp = client.get(path, params={"code": BOGUS_CODE, "state": "no-such-state"})
    _assert_provider_error(_error_parts(resp), app_base, error_code)
    _assert_no_session_cookie(resp)


@pytest.mark.parametrize("path,error_code", SPACE_CALLBACKS)
def test_space_callback_invalid_code_redirects_with_provider_error(
    client: httpx.Client, space_base: str, path: str, error_code: str
):
    resp = client.get(path, params={"code": BOGUS_CODE, "state": "no-such-state"})
    _assert_provider_error(_error_parts(resp), space_base, error_code)
    _assert_no_session_cookie(resp)


@pytest.mark.parametrize("path", SPACE_500_CALLBACKS)
def test_space_callback_server_error_on_unshadowed_helper(
    client: httpx.Client, path: str
):
    # Port-this-bug: the local ``base_host`` variable shadows the helper,
    # so every branch raises TypeError and Django answers 500 — with a
    # code, without one, with or without a session.
    for params in (
        {"code": BOGUS_CODE, "state": "no-such-state"},
        {"code": BOGUS_CODE},
        {},
    ):
        resp = client.get(path, params=params)
        assert resp.status_code == 500, (path, params, resp.status_code)


def test_app_callback_missing_code_redirects_with_provider_error(
    client: httpx.Client, app_base: str
):
    # No code at all: same provider-error redirect (google app is
    # representative; every app callback shares the state-then-code order).
    resp = client.get("/auth/google/callback/")
    _assert_provider_error(_error_parts(resp), app_base, "5115")
    _assert_no_session_cookie(resp)


def test_app_gitea_callback_without_session_redirects_relative(client: httpx.Client):
    # Port-this-quirk: no session host, so urljoin returns the relative ref
    # and the 302 Location is bare ``?error_code=...`` instead of absolute.
    resp = client.get("/auth/gitea/callback/", params={"code": BOGUS_CODE})
    assert resp.status_code == 302, f"expected 302, got {resp.status_code}: {resp.text[:200]}"
    location = resp.headers["location"]
    assert not urlsplit(location).netloc, location
    query = parse_qs(urlsplit(location).query)
    assert query.get("error_code") == ["5123"], location
    assert "error_message" in query
    _assert_no_session_cookie(resp)
