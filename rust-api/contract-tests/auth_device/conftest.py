"""Fixtures + HTTP glue for the CLI device-flow contract suite (PIDASHCONV-103)."""

import httpx
import pytest

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _harness import env
from _harness.seed import Seed


@pytest.fixture(scope="session")
def api():
    with httpx.Client(base_url=env.base_url(), timeout=15) as client:
        yield client


@pytest.fixture()
def db():
    conn = env.connect()
    try:
        yield conn
    finally:
        conn.close()


@pytest.fixture()
def seed(db):
    s = Seed(db)
    yield s
    s.cleanup()


@pytest.fixture()
def secret():
    return env.contract_secret()


@pytest.fixture()
def web_base():
    return env.web_base()


@pytest.fixture()
def tenant_a(seed):
    return seed.tenant()


@pytest.fixture()
def tenant_b(seed):
    return seed.tenant()


@pytest.fixture()
def api_key_a(seed, tenant_a):
    return seed.api_token(
        tenant_a["user"]["id"], tenant_a["workspace"]["id"]
    )


_started_codes: list = []


@pytest.fixture(autouse=True)
def _sweep_started_codes():
    """Delete anonymous device-code rows this test created via start.

    Seed.sweep cannot attribute them (no user/workspace yet), so the
    start helper registers every minted code here for teardown.
    """
    del _started_codes[:]
    yield
    if _started_codes:
        conn = env.connect()
        try:
            with conn.cursor() as cur:
                cur.execute(
                    "DELETE FROM cli_device_codes"
                    " WHERE device_code = ANY(%s)",
                    (_started_codes,),
                )
        finally:
            conn.close()


def device_start(api, **json_body):
    r = api.post("/api/v1/auth/device/start/", json=json_body or {})
    if r.status_code == 200:
        code = (r.json() or {}).get("device_code")
        if code:
            _started_codes.append(code)
    return r


def approve(api, cookies, user_code):
    headers = {}
    if cookies:
        headers["Cookie"] = "; ".join(f"{k}={v}" for k, v in cookies.items())
    return api.post(
        "/api/v1/auth/device/approve/",
        json={"user_code": user_code},
        headers=headers,
    )


def poll_token(api, device_code):
    return api.post(
        "/api/v1/auth/device/token/", json={"device_code": device_code}
    )


def session_for(seed, tenant, secret):
    return seed.session_cookie(
        tenant["user"]["id"], tenant["user"]["password"], secret
    )
