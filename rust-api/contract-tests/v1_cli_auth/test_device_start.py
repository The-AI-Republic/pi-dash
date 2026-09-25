"""POST /api/v1/auth/device/start/ — shape of the RFC 8628 grant open."""

import re

from .conftest import device_start

USER_CODE_RE = re.compile(r"^[BCDFGHJKLMNPQRSTVWXZ23456789]{4}-[BCDFGHJKLMNPQRSTVWXZ23456789]{4}$")


def test_start_shape(api, web_base, db):
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
