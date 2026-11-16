"""device/start/ per-IP throttle: 20/minute, then 429 with no row (PIDASHCONV-1091).

Django throttles anonymous ``device/start/`` posts per IP
(``DeviceCodeStartThrottle``, scope ``auth_device_start``, 20/minute);
the 21st call within a minute answers 429 and writes no row. The Rust
port must match. Throttle-state isolation comes from a unique
``X-Forwarded-For`` ident per test run, so this burst never touches the
bare-``REMOTE_ADDR`` bucket the rest of the suite shares — and a fresh
ident stays 200, proving the counter is per-ident, not global.
"""

import uuid

THROTTLE_LIMIT = 20
DENIED_BODY = '{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}'


def _row_count(db):
    with db.cursor() as cur:
        cur.execute("SELECT count(*) FROM cli_device_codes")
        return cur.fetchone()[0]


def test_start_throttled_after_twenty_per_minute(api, db):
    ident = f"throttle1091-{uuid.uuid4().hex}"
    headers = {"X-Forwarded-For": ident}
    minted = []
    try:
        before = _row_count(db)
        for i in range(THROTTLE_LIMIT):
            r = api.post("/api/v1/auth/device/start/", json={}, headers=headers)
            assert r.status_code == 200, (i + 1, r.text)
            minted.append(r.json()["device_code"])

        denied = api.post("/api/v1/auth/device/start/", json={}, headers=headers)
        assert denied.status_code == 429, denied.text
        assert denied.text == DENIED_BODY, denied.text
        retry_after = denied.headers.get("retry-after")
        assert retry_after is not None, "429 must carry Retry-After"
        assert int(retry_after) >= 1, retry_after

        assert _row_count(db) == before + THROTTLE_LIMIT, (
            "the denied call must not persist a row"
        )

        # A fresh ident is unaffected: the counter is per-IP, not global.
        fresh = api.post(
            "/api/v1/auth/device/start/",
            json={},
            headers={"X-Forwarded-For": f"throttle1091-{uuid.uuid4().hex}"},
        )
        assert fresh.status_code == 200, fresh.text
        minted.append(fresh.json()["device_code"])
    finally:
        if minted:
            with db.cursor() as cur:
                cur.execute(
                    "DELETE FROM cli_device_codes WHERE device_code = ANY(%s)",
                    (minted,),
                )
