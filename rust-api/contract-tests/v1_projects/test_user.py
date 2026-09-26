"""Contract tests: api-v1 current-user endpoint (urls/user.py, 1 URL entry)."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import http  # noqa: E402

KEY = "user"


def test_me_shape(seed):
    body = http.get(seed["keys"][KEY], "/api/v1/users/me/").json()
    assert set(body.keys()) == {
        "avatar", "avatar_url", "display_name", "email", "first_name", "id", "last_name",
    }
    assert body["id"] == seed["owner"]["id"]
    assert body["email"] == seed["owner"]["email"]


def test_me_unauthenticated_401(seed):
    http.get(None, "/api/v1/users/me/", expect=401)


def test_me_bad_token_403(seed):
    # Quirk pinned: an *invalid* token answers 403 (not 401) carrying the
    # AuthenticationFailed message; only *missing* credentials answer 401.
    r = http.get("ct-bogus-token", "/api/v1/users/me/", expect=403)
    assert r.json() == {"detail": "Given API token is not valid"}
