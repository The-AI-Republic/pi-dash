"""Contract: the cutover edge preserves the client Host on proxied requests.

PIDASHCONV-1303: ``proxy_request`` (``rust-api/crates/api/src/edge.rs``)
skipped ``header::HOST`` when copying request headers, so reqwest set Host
from the upstream URL and Django built host-dependent output from the
internal upstream authority. Django never sets ``USE_X_FORWARDED_HOST``,
so ``request.get_host()`` on a proxied request returned e.g.
``127.0.0.1:8000`` instead of the public host.

Reflector: under ``settings.test`` (``DEBUG=True``) Django answers an
unknown path with the technical 404 page, whose "Request URL" row echoes
the absolute request URL built from ``request.get_host()``. The pinned
behavior: ``GET /does-not-exist/`` with ``Host: <public host>`` 404s
with the public host in the page — identically whether the request hits
Django directly or through the Rust edge.

(The issue's suggested reflector, the configured OAuth initiate
``redirect_uri``, no longer proxies: all four providers' initiate GETs
are Rust-owned since D-17, and the webhook and MinIO reflectors are
likewise Rust-owned / env-gated. The 404 echo is the remaining
DB-free, auth-free, flag-independent ``get_host()`` reflector. It pins
``DEBUG=True`` behavior, like the status-only 404 test in
``test_web_edge.py`` notes; under ``DEBUG=False`` the custom handler404
JSON carries no host.)

DB-free and auth-free: no fixtures beyond the shared HTTP client.
"""

import httpx

from _harness.client import base_url, client  # noqa: F401  (fixtures)

PUBLIC_HOST = "pidash.example.com"
UNKNOWN_PATH = "/does-not-exist/"


def test_proxied_request_preserves_client_host(client: httpx.Client):
    resp = client.get(UNKNOWN_PATH, headers={"Host": PUBLIC_HOST})
    assert resp.status_code == 404, f"expected 404, got {resp.status_code}: {resp.text[:200]}"
    expected_url = f"http://{PUBLIC_HOST}{UNKNOWN_PATH}".encode()
    assert expected_url in resp.content, (
        f"404 page does not echo the client host (first 400 bytes): {resp.content[:400]!r}"
    )
