"""Enroll edge gating (PIDASHCONV-791).

R1: Django's ``<uuid:>`` converter (lowercase-only hex) 404s non-canonical
path segments before auth runs. Rust falls through to Django, which renders
its own framework 404 (HTML) — byte-identical by construction. The status
plus the HTML content type pin the fallthrough: an unfixed Rust answers
401 anon / 400-or-410 authed, or a JSON 404 body for an uppercase id.

R2: a valid-JSON non-object Redis ticket blob 500s (Django indexes
``payload["user_id"]`` -> ``TypeError``). Only the status is pinned — the
500 body is framework-defined on each backend.
"""

from __future__ import annotations

import os
import uuid

import pytest
import redis

from _harness.web import web_post

WEB = "/api/runners"
DAEMON = "/api/v1/runner"

pytestmark = pytest.mark.contract

UPPER_WS = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"


def _assert_framework_404(response) -> None:
    assert response.status_code == 404, response.text
    assert "text/html" in response.headers.get("content-type", "")


@pytest.mark.parametrize("segment", ["not-a-uuid", "123", UPPER_WS])
def test_web_ticket_non_canonical_workspace_404s(user_client, anon_client, segment):
    authed = web_post(
        user_client, f"{WEB}/machine-tokens/{segment}/tickets/", {"host_label": "h"}
    )
    _assert_framework_404(authed)
    anon = anon_client.post(
        f"{WEB}/machine-tokens/{segment}/tickets/", json={"host_label": "h"}
    )
    _assert_framework_404(anon)


def test_web_ticket_uppercase_workspace_uuid_404s(user_client, anon_client, daemon_world):
    """Uppercase of a real member workspace id: membership would pass, but the
    ``<uuid:>`` converter still 404s before the view runs."""
    segment = daemon_world["workspace"]["id"].upper()
    authed = web_post(
        user_client, f"{WEB}/machine-tokens/{segment}/tickets/", {"host_label": "h"}
    )
    _assert_framework_404(authed)
    anon = anon_client.post(
        f"{WEB}/machine-tokens/{segment}/tickets/", json={"host_label": "h"}
    )
    _assert_framework_404(anon)


@pytest.mark.parametrize("segment", ["not-a-uuid", "123", UPPER_WS])
def test_web_revive_non_canonical_runner_404s(user_client, anon_client, segment):
    authed = web_post(user_client, f"{WEB}/{segment}/revive/", {})
    _assert_framework_404(authed)
    anon = anon_client.post(f"{WEB}/{segment}/revive/", json={})
    _assert_framework_404(anon)


def _ticket_redis():
    url = os.environ.get("CONTRACT_REDIS_URL") or os.environ.get("REDIS_URL", "")
    assert url, "CONTRACT_REDIS_URL (or REDIS_URL) must point at the backend's Redis"
    return redis.Redis.from_url(url)


@pytest.mark.parametrize("blob", ["[1,2]", '"just-a-string"', "42", "true", "null"])
def test_daemon_redeem_non_object_ticket_is_500(anon_client, blob):
    # No throttle reset: five anon posts fit the 30/min window, and the
    # suite shares its Redis db with other runs (never FLUSHDB here).
    ticket = uuid.uuid4().hex
    key = f"machine_token_ticket:{ticket}"
    rdb = _ticket_redis()
    rdb.setex(key, 60, blob)
    try:
        response = anon_client.post(f"{DAEMON}/machine-tokens/", json={"ticket": ticket})
    finally:
        rdb.delete(key)
    assert response.status_code == 500, response.text
