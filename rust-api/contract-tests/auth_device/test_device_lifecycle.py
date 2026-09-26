"""Device-flow lifecycle: start -> approve -> token poll -> machine token -> revoke.

One end-to-end pass through the exact calls the installed runner makes
(``runner/src/cli/auth/login.rs``), plus exact-shape assertions for the
``start`` payload the runner parses. Error variants that need no live
grant use ``Seed.device_code`` inserts (no HTTP start, no throttle cost).
"""

import re
import time
import uuid

from .conftest import approve, device_start, poll_token, session_for

USER_CODE_RE = re.compile(r"^[BCDFGHJKLMNPQRSTVWXZ23456789]{4}-[BCDFGHJKLMNPQRSTVWXZ23456789]{4}$")


def test_start_shape_exact(api, web_base, db):
    """The runner parses exactly these five fields; nothing may drift."""
    r = device_start(api)
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == {
        "device_code",
        "user_code",
        "verification_uri",
        "expires_in",
        "interval",
    }
    assert isinstance(body["device_code"], str) and len(body["device_code"]) >= 32
    assert USER_CODE_RE.match(body["user_code"]), body["user_code"]
    assert body["verification_uri"] == f"{web_base}/auth/device/"
    assert body["expires_in"] == 600
    assert body["interval"] == 5

    with db.cursor() as cur:
        cur.execute(
            "SELECT approved, denied, consumed, user_id, workspace_id,"
            " expires_at > now() AS live FROM cli_device_codes"
            " WHERE device_code = %s",
            (body["device_code"],),
        )
        row = cur.fetchone()
    assert row is not None, "start must persist the grant row"
    approved, denied, consumed, user_id, workspace_id, live = row
    assert (approved, denied, consumed, user_id, workspace_id, live) == (
        False,
        False,
        False,
        None,
        None,
        True,
    )


def test_start_codes_unique(api):
    first = device_start(api).json()
    second = device_start(api).json()
    assert first["device_code"] != second["device_code"]
    assert first["user_code"] != second["user_code"]


def test_full_lifecycle(api, seed, tenant_a, secret, db):
    """start -> pending poll -> approve -> token -> machine token -> revoke."""
    codes = device_start(api).json()

    pending = poll_token(api, codes["device_code"])
    assert pending.status_code == 400, pending.text
    assert pending.json() == {"error": "authorization_pending"}

    cookies = session_for(seed, tenant_a, secret)
    ok = approve(api, cookies, codes["user_code"])
    assert ok.status_code == 200, ok.text
    assert ok.json() == {
        "ok": True,
        "user_email": tenant_a["user"]["email"],
        "workspace_slug": tenant_a["workspace"]["slug"],
    }

    # Honor the poll gap like the runner does (it polls every `interval`
    # seconds): the pending poll above stamped last_polled_at, so an
    # immediate second poll would answer slow_down.
    time.sleep(3.5)

    minted = poll_token(api, codes["device_code"])
    assert minted.status_code == 200, minted.text
    token_body = minted.json()
    assert set(token_body) == {
        "access_token",
        "token_type",
        "user_email",
        "workspace_slug",
    }
    assert token_body["token_type"] == "X-Api-Key"
    assert token_body["access_token"].startswith("pi_dash_api_")
    assert token_body["user_email"] == tenant_a["user"]["email"]
    assert token_body["workspace_slug"] == tenant_a["workspace"]["slug"]
    bridge = token_body["access_token"]

    # The minted bridge token is a working CLI credential.
    ws = api.get(
        "/api/v1/auth/workspaces/", headers={"X-Api-Key": bridge}
    )
    assert ws.status_code == 200, ws.text
    assert tenant_a["workspace"]["slug"] in {
        w["slug"] for w in ws.json()["workspaces"]
    }

    dev_machine_id = str(uuid.uuid4())
    mt = api.post(
        "/api/v1/auth/machine-token/",
        json={
            "workspace_slug": tenant_a["workspace"]["slug"],
            "dev_machine_id": dev_machine_id,
            "host_label": "ct103-host",
        },
        headers={"X-Api-Key": bridge},
    )
    assert mt.status_code == 201, mt.text
    mt_body = mt.json()
    assert set(mt_body) == {
        "machine_token",
        "workspace_slug",
        "dev_machine_id",
        "host_label",
    }
    assert mt_body["machine_token"].startswith("mt_")
    assert mt_body["workspace_slug"] == tenant_a["workspace"]["slug"]
    assert mt_body["dev_machine_id"] == dev_machine_id
    assert mt_body["host_label"] == "ct103-host"
    machine_token = mt_body["machine_token"]

    with db.cursor() as cur:
        cur.execute(
            "SELECT is_active FROM api_tokens WHERE token = %s", (bridge,)
        )
        (bridge_active,) = cur.fetchone()
    assert bridge_active is False, "exchange must deactivate the bridge token"

    # The machine token authenticates as the user.
    assert (
        api.get(
            "/api/v1/auth/workspaces/",
            headers={"X-Api-Key": machine_token},
        ).status_code
        == 200
    )

    # Revoke (pidash auth logout) kills the machine token.
    gone = api.post(
        "/api/v1/auth/revoke/", headers={"X-Api-Key": machine_token}
    )
    assert gone.status_code == 200, gone.text
    assert gone.json() == {"ok": True}

    # Subsequent API use with the revoked token fails ...
    assert (
        api.get(
            "/api/v1/auth/workspaces/",
            headers={"X-Api-Key": machine_token},
        ).status_code
        == 403
    )
    # ... and the consumed grant can never be polled again.
    replay = poll_token(api, codes["device_code"])
    assert replay.status_code == 400, replay.text
    assert replay.json()["error"] == "invalid_grant"


def test_token_unknown_code(api):
    r = poll_token(api, "no-such-device-code")
    assert r.status_code == 400, r.text
    assert r.json() == {
        "error": "invalid_grant",
        "error_description": "Unknown device code.",
    }


def test_token_expired(api, seed):
    codes = seed.device_code(expires_in_seconds=-60)
    r = poll_token(api, codes["device_code"])
    assert r.status_code == 410, r.text
    assert r.json() == {"error": "expired_token"}


def test_token_denied_row(api, seed):
    codes = seed.device_code(denied=True)
    r = poll_token(api, codes["device_code"])
    assert r.status_code == 410, r.text
    assert r.json() == {"error": "access_denied"}


def test_approve_unknown_code(api, seed, tenant_a, secret):
    cookies = session_for(seed, tenant_a, secret)
    r = approve(api, cookies, "ZZZZ-9999")
    assert r.status_code == 404, r.text
    assert "error" in r.json()


def test_approve_expired(api, seed, tenant_a, secret):
    cookies = session_for(seed, tenant_a, secret)
    codes = seed.device_code(expires_in_seconds=-60)
    r = approve(api, cookies, codes["user_code"])
    assert r.status_code == 410, r.text
    assert "expired" in r.json()["error"]


def test_approve_consumed(api, seed, tenant_a, secret):
    cookies = session_for(seed, tenant_a, secret)
    codes = seed.device_code(consumed=True)
    r = approve(api, cookies, codes["user_code"])
    assert r.status_code == 410, r.text
    assert r.json() == {"error": "This code has already been used."}
