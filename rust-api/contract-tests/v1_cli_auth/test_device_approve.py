"""POST /api/v1/auth/device/approve/ — session user stamps the grant."""

import pytest

from conftest import approve, device_start


@pytest.fixture()
def session_a(api, seed, tenant_a, secret):
    del api
    return seed.session_cookie(
        tenant_a["user"]["id"], tenant_a["user"]["password"], secret
    )


def _fresh_codes(api):
    return device_start(api).json()


def test_approve_shape(api, session_a, tenant_a, db):
    codes = _fresh_codes(api)
    r = approve(api, session_a, codes["user_code"])
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == {"ok", "user_email", "workspace_slug"}
    assert body["ok"] is True
    assert body["user_email"] == tenant_a["user"]["email"]
    assert body["workspace_slug"] == tenant_a["workspace"]["slug"]

    with db.cursor() as cur:
        cur.execute(
            "SELECT user_id, workspace_id, approved FROM cli_device_codes"
            " WHERE device_code = %s",
            (codes["device_code"],),
        )
        user_id, workspace_id, approved = cur.fetchone()
    assert approved is True
    assert str(user_id) == tenant_a["user"]["id"]
    assert str(workspace_id) == tenant_a["workspace"]["id"]


def test_approve_denied_without_session(api):
    # No session cookie: IsAuthenticated denies with 401.
    codes = _fresh_codes(api)
    r = approve(api, {}, codes["user_code"])
    assert r.status_code == 401, r.text


def test_approve_missing_code(api, session_a):
    r = approve(api, session_a, "")
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "user_code is required."}


def test_approve_bad_length(api, session_a):
    r = approve(api, session_a, "ABC")
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "user_code must be 8 characters."}


def test_approve_unknown_code(api, session_a):
    r = approve(api, session_a, "ZZZZ-9999")
    assert r.status_code == 404, r.text
    assert "error" in r.json()


def test_approve_lenient_format(api, session_a, tenant_a):
    codes = _fresh_codes(api)
    messy = "  " + codes["user_code"].lower().replace("-", " - ") + "  "
    r = approve(api, session_a, messy)
    assert r.status_code == 200, r.text
    assert r.json()["user_email"] == tenant_a["user"]["email"]


def test_approve_consumed(api, session_a, seed):
    codes = seed.device_code(consumed=True)
    r = approve(api, session_a, codes["user_code"])
    assert r.status_code == 410, r.text
    assert r.json() == {"error": "This code has already been used."}


def test_approve_denied_row(api, session_a, seed):
    codes = seed.device_code(denied=True)
    r = approve(api, session_a, codes["user_code"])
    assert r.status_code == 410, r.text
    assert r.json() == {"error": "This code has been denied."}


def test_approve_expired(api, session_a, seed):
    codes = seed.device_code(expires_in_seconds=-60)
    r = approve(api, session_a, codes["user_code"])
    assert r.status_code == 410, r.text
    assert "expired" in r.json()["error"]


def test_approve_second_user_conflict(api, seed, tenant_a, tenant_b, secret):
    codes = device_start(api).json()
    cookies_a = seed.session_cookie(
        tenant_a["user"]["id"], tenant_a["user"]["password"], secret
    )
    cookies_b = seed.session_cookie(
        tenant_b["user"]["id"], tenant_b["user"]["password"], secret
    )
    assert approve(api, cookies_a, codes["user_code"]).status_code == 200
    r = approve(api, cookies_b, codes["user_code"])
    assert r.status_code == 409, r.text
    assert r.json() == {
        "error": "This code has already been approved by another user."
    }


def test_approve_idempotent_same_user(api, session_a):
    codes = _fresh_codes(api)
    assert approve(api, session_a, codes["user_code"]).status_code == 200
    r = approve(api, session_a, codes["user_code"])
    assert r.status_code == 200, r.text
    assert r.json()["ok"] is True
