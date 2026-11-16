"""POST /api/v1/auth/device/token/ — CLI polls the grant for an APIToken."""

from .conftest import approve, device_start, poll_token


def _approved_codes(api, seed, tenant, secret):
    codes = device_start(api).json()
    cookies = seed.session_cookie(
        tenant["user"]["id"], tenant["user"]["password"], secret
    )
    assert approve(api, cookies, codes["user_code"]).status_code == 200
    return codes


def test_token_pending_shape(api):
    codes = device_start(api).json()
    r = poll_token(api, codes["device_code"])
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "authorization_pending"}


def test_token_missing_code(api):
    r = api.post("/api/v1/auth/device/token/", json={})
    assert r.status_code == 400, r.text
    assert r.json() == {
        "error": "invalid_request",
        "error_description": "device_code is required.",
    }


def test_token_unknown_code(api):
    r = poll_token(api, "no-such-device-code")
    assert r.status_code == 400, r.text
    assert r.json() == {
        "error": "invalid_grant",
        "error_description": "Unknown device code.",
    }


def test_token_consumed(api, seed):
    codes = seed.device_code(consumed=True)
    r = poll_token(api, codes["device_code"])
    assert r.status_code == 400, r.text
    assert r.json() == {
        "error": "invalid_grant",
        "error_description": "Device code already consumed.",
    }


def test_token_denied(api, seed):
    codes = seed.device_code(denied=True)
    r = poll_token(api, codes["device_code"])
    assert r.status_code == 410, r.text
    assert r.json() == {"error": "access_denied"}


def test_token_expired(api, seed):
    codes = seed.device_code(expires_in_seconds=-60)
    r = poll_token(api, codes["device_code"])
    assert r.status_code == 410, r.text
    assert r.json() == {"error": "expired_token"}


def test_token_slow_down(api):
    codes = device_start(api).json()
    assert poll_token(api, codes["device_code"]).status_code == 400
    r = poll_token(api, codes["device_code"])
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "slow_down"}


def test_token_happy_path(api, seed, tenant_a, secret, db):
    codes = _approved_codes(api, seed, tenant_a, secret)
    r = poll_token(api, codes["device_code"])
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == {
        "access_token",
        "token_type",
        "user_email",
        "workspace_slug",
    }
    assert body["token_type"] == "X-Api-Key"
    assert body["user_email"] == tenant_a["user"]["email"]
    assert body["workspace_slug"] == tenant_a["workspace"]["slug"]
    assert isinstance(body["access_token"], str) and body["access_token"]

    with db.cursor() as cur:
        cur.execute(
            "SELECT description FROM api_tokens WHERE token = %s",
            (body["access_token"],),
        )
        token_row = cur.fetchone()
        cur.execute(
            "SELECT consumed FROM cli_device_codes WHERE device_code = %s",
            (codes["device_code"],),
        )
        (consumed,) = cur.fetchone()
    assert token_row is not None, "poll must mint an APIToken row"
    assert token_row[0] == "Issued by pidash auth login (device-code flow)."
    assert consumed is True, "grant must be consumed after mint"

    # The minted token is a working CLI credential (wire compatibility).
    workspaces = api.get(
        "/api/v1/auth/workspaces/",
        headers={"X-Api-Key": body["access_token"]},
    )
    assert workspaces.status_code == 200, workspaces.text
    slugs = [w["slug"] for w in workspaces.json()["workspaces"]]
    assert tenant_a["workspace"]["slug"] in slugs


def test_token_single_use(api, seed, tenant_a, secret):
    codes = _approved_codes(api, seed, tenant_a, secret)
    assert poll_token(api, codes["device_code"]).status_code == 200
    r = poll_token(api, codes["device_code"])
    assert r.status_code == 400, r.text
    assert r.json()["error"] == "invalid_grant"
