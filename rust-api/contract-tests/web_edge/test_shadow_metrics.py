"""Contract suite: shadow metrics must not leak (PIDASHCONV-1305).

``GET /internal/shadow-metrics`` is a Rust-only operator tool: Django has
no ``/internal/*`` route and 404s it. The contract pins that reference on
both backends:

- an anonymous ``GET /internal/shadow-metrics`` is not served with path
  keys (Django 404s; Rust must 404 for callers without the metrics
  token, never 200 with a ``routes`` map);
- a write to a token-bearing path never lands the token in metrics, even
  with shadow mode on (``PIDASH_RUST_SHADOW=1``).

DB-free like the rest of ``web_edge/``: the posted uid/token are
unregistered markers — what matters is that the path *string* never
appears in the served metrics, not whether the reset itself succeeds, so
nothing is asserted about the write responses.

Fails before the fix on a shadow-enabled Rust edge (200 with the raw
token path as a route key); passes after it (404) and on Django always.
"""

import httpx

from _harness.client import base_url, client  # noqa: F401  (fixtures)

UIDB64 = "Mg"
TOKEN = "cv1305contract-9f8e7d6c5b4a3948576d5c4b3a2918070605"


def _reset_path() -> str:
    return f"/auth/reset-password/{UIDB64}/{TOKEN}/"


def test_anonymous_shadow_metrics_not_served_with_path_keys(
    client: httpx.Client,
):
    # Django 404s /internal/* (no such route); Rust must 404 anonymous
    # callers too, never 200 with a routes map.
    resp = client.get("/internal/shadow-metrics")
    assert resp.status_code == 404
    assert "routes" not in resp.text


def test_token_bearing_write_never_appears_in_metrics(client: httpx.Client):
    # The issue's literal step (POST to the reset path), the same path via
    # a method that still proxies, and a POST through the plain fallback:
    # none of them may land the token string in the served metrics.
    client.post(_reset_path(), data={"password": "weak"})
    client.put(_reset_path(), data={"password": "weak"})
    client.post(f"/api/v1/unported-probe-1305/{TOKEN}/", data={"x": "1"})

    resp = client.get("/internal/shadow-metrics")
    assert resp.status_code == 404
    assert TOKEN not in resp.text
    assert "routes" not in resp.text
