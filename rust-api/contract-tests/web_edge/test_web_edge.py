"""Contract suite: web edge (``pi_dash/web/urls.py``).

Two public, DB-free endpoints mounted at the site root:

- ``GET /`` — health probe, ``pi_dash.web.views.health_check``
- ``GET /robots.txt`` — crawler rules, ``pi_dash.web.views.robots_txt``

Both are plain Django function views with no auth and no tenant
scoping, so the coverage floor maps onto this domain as:

- shape: exact status, content-type and body bytes per endpoint;
- denied-permission case: the views are public by contract — requests
  bearing invalid credentials must still be served 200 with identical
  bytes (the suite goes red if a permission gate is added; demonstrated
  in the PR with a one-line ``login_required`` patch, reverted);
- tenant-isolation case: responses set no cookies and carry no
  tenant/user identity — credentialed and bare requests are
  byte-identical.

Quirks pinned here (port them, don't fix them):

- Unsafe methods without a CSRF token hit the project's custom
  ``CSRF_FAILURE_VIEW`` (``pi_dash.authentication.views.common``),
  which renders ``templates/csrf_failure.html`` with status **200**,
  not 403. The page embeds the deployment's root URL, so only the
  status, content-type and template marker are asserted, not full bytes.
- Unknown paths 404, but the body is settings-dependent (Django's
  technical 404 page under ``DEBUG=True`` vs ``handler404`` JSON
  otherwise), so only the status is asserted.
"""

import httpx
import pytest

from _harness.client import base_url, client  # noqa: F401  (fixtures)

HEALTH_BODY = b'{"status": "OK"}'
ROBOTS_BODY = b"User-agent: *\nDisallow: /"
CSRF_MARKER = b"<!-- templates/csrf_failure.html -->"

FOREIGN_HEADERS = {
    # Belong to nobody: must neither grant nor deny anything here.
    "Authorization": "Bearer deadbeef-not-a-real-token",
    "Cookie": "sessionid=deadbeef-not-a-real-session",
}


def test_root_health_shape(client: httpx.Client):
    resp = client.get("/")
    assert resp.status_code == 200
    assert resp.headers["content-type"] == "application/json"
    assert resp.content == HEALTH_BODY
    assert resp.json() == {"status": "OK"}


def test_robots_txt_shape(client: httpx.Client):
    resp = client.get("/robots.txt")
    assert resp.status_code == 200
    assert resp.headers["content-type"] == "text/plain"
    assert resp.content == ROBOTS_BODY


def test_head_requests_have_no_body(client: httpx.Client):
    for path in ("/", "/robots.txt"):
        resp = client.head(path)
        assert resp.status_code == 200
        assert resp.content == b""


def test_unsafe_method_without_csrf_token_serves_failure_page(
    client: httpx.Client,
):
    # Custom CSRF_FAILURE_VIEW answers 200 (not 403) with the failure page.
    for path in ("/", "/robots.txt"):
        resp = client.post(path)
        assert resp.status_code == 200
        assert resp.headers["content-type"].startswith("text/html")
        assert CSRF_MARKER in resp.content


def test_public_access_with_foreign_credentials(client: httpx.Client):
    # Denied-permission case for this domain: there are no permission
    # classes — both views are public — so invalid credentials must not
    # change anything. Adding a permission gate turns these red.
    for path, body in (("/", HEALTH_BODY), ("/robots.txt", ROBOTS_BODY)):
        plain = client.get(path)
        assert plain.status_code == 200
        foreign = client.get(path, headers=FOREIGN_HEADERS)
        assert foreign.status_code == 200
        assert foreign.content == plain.content == body


def test_responses_carry_no_tenant_state(client: httpx.Client):
    # Tenant-isolation case for this domain: nothing here is per-tenant,
    # so responses must set no cookies and identical requests from
    # different callers must be byte-identical.
    for path in ("/", "/robots.txt"):
        bare = client.get(path)
        other = client.get(path, headers=FOREIGN_HEADERS)
        assert "set-cookie" not in bare.headers
        assert "set-cookie" not in other.headers
        assert bare.content == other.content


@pytest.mark.parametrize("path", ["/does-not-exist/", "/robots.txt/"])
def test_unknown_path_404(client: httpx.Client, path: str):
    # Body varies with DEBUG (technical page vs handler404 JSON); the 404
    # status is the contract.
    assert client.get(path).status_code == 404
