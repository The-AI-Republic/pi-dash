"""Tenant-isolation case for the OAuth domain (PIDASHCONV-102).

The ``spaces/`` callback variants are tenant-scoped twins of the app
callbacks: same provider error codes, different redirect bases. The gitea
pair is the clean 302-vs-302 comparison — both carry ``5123`` while
landing on different bases. The google pair records the starkest actual
difference in the domain: the app callback 302s with ``5115`` while the
spaces twin 500s on every input (see ``test_callback.py``).

Behavioral differences app vs spaces/, read off the Python sources (not
assumed) and recorded here for the Rust port — translate, don't redesign:

- Success login: app callbacks construct the provider with
  ``callback=post_user_auth_workflow`` (joins pending workspace/project
  invitations); the space callbacks pass no callback, so no invitation
  join runs on that path.
- Success target: app uses ``get_redirection_path`` (onboarding, last or
  fallback workspace slug, invitations, create-workspace) joined onto the
  app base; space uses the session ``next_path`` (validated) joined onto
  the space base, falling back to the space base itself.
- ``next_path`` persistence: the app google/github/gitlab initiates store
  ``next_path`` in the session, as does the app/space gitea pair; the
  space google/github/gitlab initiates do NOT store it, so their
  callbacks always see ``next_path=None``.
- Error redirect builders: google/github/gitlab (app and space) share
  ``get_safe_redirect_url``; gitea (app and space) builds the same values
  by hand with ``urljoin`` + ``urlencode`` (see the ``next_path`` encoding
  pins in ``test_initiate.py``).
- Liveness gap (port-this-bug): spaces google/github/gitlab callbacks 500
  on every input because a ``base_host`` local shadows the helper; the app
  twins 302. The Rust port must reproduce the 500 until the Python side is
  fixed separately.
"""

import os
import sys
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402

BOGUS_CODE = "contract-test-invalid-code"


def test_app_and_space_gitea_callbacks_share_code_but_not_base(
    client: httpx.Client, app_base: str, space_base: str
):
    # The app gitea hit uses a fresh client (no session host), so its
    # Location is the relative quirk form — the shared error_code is what
    # ties the tenants together here.
    app = client.get("/auth/gitea/callback/", params={"code": BOGUS_CODE})
    space = client.get(
        "/auth/spaces/gitea/callback/", params={"code": BOGUS_CODE, "state": "no-such-state"}
    )
    assert app.status_code == 302, app.text[:200]
    assert space.status_code == 302, space.text[:200]
    app_parts, space_parts = urlsplit(app.headers["location"]), urlsplit(
        space.headers["location"]
    )
    assert parse_qs(app_parts.query).get("error_code") == ["5123"]
    assert parse_qs(space_parts.query).get("error_code") == ["5123"]
    assert f"{space_parts.scheme}://{space_parts.netloc}" == space_base
    assert space_parts.path.startswith("/spaces"), space_parts.path
    assert app.headers["location"] != space.headers["location"]


def test_app_and_space_google_callbacks_diverge(
    client: httpx.Client, app_base: str
):
    # Same provider, same bogus code: the app tenant 302s with the
    # provider error while the spaces tenant 500s (shadowed base_host).
    app = client.get(
        "/auth/google/callback/", params={"code": BOGUS_CODE, "state": "no-such-state"}
    )
    space = client.get(
        "/auth/spaces/google/callback/", params={"code": BOGUS_CODE, "state": "no-such-state"}
    )
    assert app.status_code == 302, app.text[:200]
    app_parts = urlsplit(app.headers["location"])
    assert f"{app_parts.scheme}://{app_parts.netloc}" == app_base
    assert parse_qs(app_parts.query).get("error_code") == ["5115"]
    assert space.status_code == 500


def test_app_and_space_initiates_share_code_but_not_base(
    client: httpx.Client, app_base: str, space_base: str
):
    app = client.get("/auth/github/")
    space = client.get("/auth/spaces/github/")
    assert app.status_code == 302, app.text[:200]
    assert space.status_code == 302, space.text[:200]
    app_parts, space_parts = urlsplit(app.headers["location"]), urlsplit(
        space.headers["location"]
    )
    assert parse_qs(app_parts.query).get("error_code") == ["5110"]
    assert parse_qs(space_parts.query).get("error_code") == ["5110"]
    assert f"{app_parts.scheme}://{app_parts.netloc}" == app_base
    assert f"{space_parts.scheme}://{space_parts.netloc}" == space_base
    assert app.headers["location"] != space.headers["location"]
