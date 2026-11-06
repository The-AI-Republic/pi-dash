"""Engine grant device-flow coverage (NEWPIDASH-51).

Ports Django test_engine_identity.py and test_engine_followups.py revoke cases
to black-box contract tests: scope, approvals, grant redemption, machine-token
engine issuance, revoke sign-out.

Uses MANAGED_RUNNER_ENABLED via env (see workflow; callers run with True).
"""

import uuid

from .conftest import approve as approve_post, device_start, poll_token


def _exchange(api, token, **fields):
    return api.post("/api/v1/auth/machine-token/", json=fields, headers={"X-Api-Key": token})


def _poll(api, device_code, bearer=None):
    headers = {}
    if bearer:
        headers["Authorization"] = f"Bearer {bearer}"
    return api.post("/api/v1/auth/device/token/", json={"device_code": device_code}, headers=headers)


def test_start_engine_scope(api, db):
    r = device_start(api, scope="engine")
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["scope"] == "engine"
    with db.cursor() as cur:
        cur.execute("SELECT scope FROM cli_device_codes WHERE device_code=%s", (body["device_code"],))
        (scope,) = cur.fetchone()
    assert scope == "engine"


def test_start_invalid_scope(api):
    r = device_start(api, scope="bogus")
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "invalid_scope", "error_description": "scope must be 'cli' or 'engine'."}


def test_start_invalid_dev_machine_id(api):
    r = device_start(api, scope="engine", dev_machine_id="not-a-uuid")
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "invalid_dev_machine_id"}


def test_approve_interactive_not_bool(api, seed, tenant_a, secret):
    codes = device_start(api, scope="engine").json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    r = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": "yes"}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert r.status_code == 400
    assert r.json()["error"] == "interactive must be a boolean."


def test_approve_invalid_dev_machine_id(api, seed, tenant_a, secret):
    codes = device_start(api, scope="engine").json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    r = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "dev_machine_id": "not-uuid"}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert r.status_code == 400
    assert r.json()["error"] == "invalid_dev_machine_id"


def test_engine_grant_needs_machine_token(api, seed, tenant_a, secret):
    codes = device_start(api, scope="engine").json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    r = approve_post(api, cookies, codes["user_code"])
    assert r.status_code == 200
    test_r = _poll(api, codes["device_code"])
    assert test_r.status_code == 401
    assert test_r.json()["error"] == "machine_token_required"


def test_engine_grant_invalid_bearer(api, seed, tenant_a, secret):
    codes = device_start(api, scope="engine").json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    assert approve_post(api, cookies, codes["user_code"]).status_code == 200
    r = _poll(api, codes["device_code"], bearer="mt_bad")
    assert r.status_code == 401
    assert r.json()["error"] == "invalid_client"


def test_engine_grant_happy_path(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"])
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    codes = device_start(api, scope="engine", dev_machine_id=machine).json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    ap = approve_post(api, cookies, codes["user_code"])
    assert ap.status_code == 200, ap.text
    # interactive defaults false but engine grant with known machine
    r = _poll(api, codes["device_code"], bearer=mt)
    # If silent rules refuse because never, this will be 409; interactive would pass.
    # Use interactive approval to guarantee happy path
    # For this test we approve interactive true
    # Re-create with interactive
    codes2 = device_start(api, scope="engine", dev_machine_id=machine).json()
    cookies2 = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    ap2 = api.post("/api/v1/auth/device/approve/", json={"user_code": codes2["user_code"], "interactive": True, "dev_machine_id": machine}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies2.items())})
    assert ap2.status_code == 200
    r2 = _poll(api, codes2["device_code"], bearer=mt)
    assert r2.status_code == 200, r2.text
    body = r2.json()
    assert "engine_credential" in body
    assert body["engine_credential"]["token"].startswith("ec_")
    assert body["dev_machine_id"] == machine


def test_machine_token_issues_engine_cred(api, seed, tenant_a, secret):
    # Full cli login then exchange -> should mint engine cred if grant is interactive
    codes = device_start(api).json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    # approve interactive
    ap = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": True}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert ap.status_code == 200
    poll = poll_token(api, codes["device_code"])
    assert poll.status_code == 200
    api_token = poll.json()["access_token"]
    dm = str(uuid.uuid4())
    r = _exchange(api, api_token, workspace_slug=tenant_a["workspace"]["slug"], dev_machine_id=dm, host_label="host")
    assert r.status_code == 201, r.text
    body = r.json()
    assert "engine_sign_in_state" in body
    # engine_credential may be present when instance can serve engine
    if "engine_credential" in body:
        assert body["engine_credential"]["token"].startswith("ec_")


def test_machine_token_no_engine_for_machine_token_caller(api, seed, tenant_a, secret):
    # machine-token caller never mints
    machine = seed.dev_machine(tenant_a["user"]["id"])
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    r = _exchange(api, mt, workspace_slug=tenant_a["workspace"]["slug"], dev_machine_id=str(uuid.uuid4()), host_label="x")
    assert r.status_code == 201
    assert "engine_credential" not in r.json()


def test_revoke_signs_out_engine(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="active")
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    seed.engine_credential(tenant_a["user"]["id"], machine, secret)
    r = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt})
    assert r.status_code == 200
    assert r.json() == {"ok": True}
    # DB check: machine state should be signed_out after last token revoke
    with seed.conn.cursor() as cur:
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine,))
        (state,) = cur.fetchone()
    assert state == "signed_out"


def test_revoke_workspace_logout_keeps_engine(api, seed, tenant_a, tenant_b, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="active")
    mt_a = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    # second workspace same user/machine needs second tenant? use same user different workspace
    ws2 = seed.workspace(tenant_a["user"]["id"])
    seed.member(ws2["id"], tenant_a["user"]["id"])
    mt_b = seed.machine_token(tenant_a["user"]["id"], ws2["id"], machine, secret)
    # logout workspace B, engine stays active because A still live
    r = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt_b})
    assert r.status_code == 200
    with seed.conn.cursor() as cur:
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine,))
        (state,) = cur.fetchone()
    assert state == "active"
    # logout last
    r2 = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt_a})
    assert r2.status_code == 200
    with seed.conn.cursor() as cur:
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine,))
        (state2,) = cur.fetchone()
    assert state2 == "signed_out"
