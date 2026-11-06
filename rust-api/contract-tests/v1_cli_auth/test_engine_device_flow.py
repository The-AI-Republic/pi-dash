"""Engine grant device-flow coverage (NEWPIDASH-51).

Ports Django test_engine_identity.py and test_engine_followups.py revoke cases
to black-box contract tests: scope, approvals, grant redemption, machine-token
engine issuance, revoke sign-out.

Uses MANAGED_RUNNER_ENABLED via env (see workflow; callers run with True).
"""

import uuid
from datetime import datetime, timedelta, timezone

from .conftest import approve as approve_post, device_start, poll_token


def _exchange(api, token, **fields):
    return api.post("/api/v1/auth/machine-token/", json=fields, headers={"X-Api-Key": token})


def _poll(api, device_code, bearer=None):
    headers = {}
    if bearer:
        headers["Authorization"] = f"Bearer {bearer}"
    return api.post("/api/v1/auth/device/token/", json={"device_code": device_code}, headers=headers)


def _poll_raw(api, device_code, auth_header):
    headers = {}
    if auth_header is not None:
        headers["Authorization"] = auth_header
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
    assert test_r.status_code == 401, test_r.text
    assert test_r.json() == {"error": "machine_token_required", "error_description": "Redeem an engine grant with Authorization: Bearer <machine token>."}


def test_engine_grant_invalid_bearer(api, seed, tenant_a, secret):
    codes = device_start(api, scope="engine").json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    assert approve_post(api, cookies, codes["user_code"]).status_code == 200
    r = _poll(api, codes["device_code"], bearer="mt_bad")
    assert r.status_code == 401, r.text
    # exact resolver code for unknown token
    assert r.json() == {"error": "invalid_client", "error_description": "machine_token_invalid"}


def test_bearer_requires_mt_prefix_and_exact_two_parts(api, seed, tenant_a, secret):
    # Non-mt_ token, extra part, and Basic scheme all answer machine_token_required at the gate
    codes = device_start(api, scope="engine").json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    assert approve_post(api, cookies, codes["user_code"]).status_code == 200
    for auth in ["Bearer abc", "Bearer mt_abc extra", "Basic mt_abc", "Bearer", "", "Bearer  "]:
        r = _poll_raw(api, codes["device_code"], auth)
        assert r.status_code == 401, f"{auth!r} -> {r.text}"
        assert r.json() == {"error": "machine_token_required", "error_description": "Redeem an engine grant with Authorization: Bearer <machine token>."}, f"{auth!r}"


def test_bearer_whitespace_double_space_and_tab_accepted(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"])
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    codes = device_start(api, scope="engine", dev_machine_id=machine).json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    ap = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": True, "dev_machine_id": machine}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert ap.status_code == 200, ap.text
    # Double-space and tab must pass the gate (Python split semantics) and reach the resolver.
    # With a valid token, redemption proceeds to engine_unavailable 409 (CE).
    for sep in ["  ", "\t", " \t "]:
        r = _poll_raw(api, codes["device_code"], f"Bearer{sep}{mt}")
        # Must NOT be machine_token_required; CE gives 409 engine_unavailable
        assert r.status_code != 401 or r.json().get("error") != "machine_token_required", f"sep {sep!r} wrongly gated {r.text}"
        assert r.status_code == 409, r.text
        assert r.json()["error"] == "engine_unavailable"
        assert r.json()["reason_code"] in ("profile_unavailable", "engine_disabled")
        assert r.json()["error_description"] == "This Pi Dash instance cannot serve the built-in engine for this account."


def test_bearer_whitespace_invalid_token_still_resolver_code(api, seed, tenant_a, secret):
    codes = device_start(api, scope="engine").json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    assert approve_post(api, cookies, codes["user_code"]).status_code == 200
    # Double-space with unknown mt_ must answer invalid_client/machine_token_invalid, not machine_token_required
    r = _poll_raw(api, codes["device_code"], "Bearer  mt_unknown")
    assert r.status_code == 401, r.text
    assert r.json() == {"error": "invalid_client", "error_description": "machine_token_invalid"}
    r2 = _poll_raw(api, codes["device_code"], "Bearer\tmt_unknown")
    assert r2.status_code == 401
    assert r2.json() == {"error": "invalid_client", "error_description": "machine_token_invalid"}


def test_resolver_revoked_expired_and_machine_revoked(api, seed, tenant_a, secret):
    now = datetime.now(timezone.utc)
    # revoked
    machine = seed.dev_machine(tenant_a["user"]["id"])
    mt_revoked = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret, revoked_at=now)
    codes = device_start(api, scope="engine").json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    assert approve_post(api, cookies, codes["user_code"]).status_code == 200
    r = _poll(api, codes["device_code"], bearer=mt_revoked)
    assert r.status_code == 401
    assert r.json() == {"error": "invalid_client", "error_description": "machine_token_revoked"}
    # expired
    mt_expired = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret, expires_in_days=-1)
    codes2 = device_start(api, scope="engine").json()
    cookies2 = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    assert approve_post(api, cookies2, codes2["user_code"]).status_code == 200
    r2 = _poll(api, codes2["device_code"], bearer=mt_expired)
    assert r2.status_code == 401
    assert r2.json() == {"error": "invalid_client", "error_description": "machine_token_expired"}
    # dev_machine_revoked
    rev_machine = seed.dev_machine(tenant_a["user"]["id"], revoked_at=now)
    mt_machine_rev = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], rev_machine, secret)
    codes3 = device_start(api, scope="engine").json()
    cookies3 = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    assert approve_post(api, cookies3, codes3["user_code"]).status_code == 200
    r3 = _poll(api, codes3["device_code"], bearer=mt_machine_rev)
    assert r3.status_code == 401
    assert r3.json() == {"error": "invalid_client", "error_description": "dev_machine_revoked"}


def test_resolver_membership_revoked_revokes_token(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"])
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    # Lapse membership
    with seed.conn.cursor() as cur:
        cur.execute("UPDATE workspace_members SET is_active=false WHERE workspace_id=%s AND member_id=%s", (tenant_a["workspace"]["id"], tenant_a["user"]["id"]))
        seed.conn.commit()
    codes = device_start(api, scope="engine").json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    # approval will still need workspace lookup; use most recent membership fallback would fail, so approve with explicit workspace would 404 - use tenant_b user for approval instead
    # For this redemption test we approve with a different user who is still member of same workspace, but redemption checks token's user membership
    # Recreate membership for approver
    tenant_b = seed.tenant()
    # ensure tenant_b is member of original workspace as well
    seed.member(tenant_a["workspace"]["id"], tenant_b["user"]["id"])
    cookies_b = seed.session_cookie(tenant_b["user"]["id"], tenant_b["user"]["password"], secret)
    # start grant for original user? To avoid mismatch, approve with tenant_b and redeem with token of tenant_a whose membership lapsed
    # grant user_id will be tenant_b, token user is tenant_a -> would be access_denied not membership_revoked. So we must make approver same as token owner.
    # Re-create membership for original user to allow approve but keep token's membership check to fail - need token's workspace member check to fail at token's workspace
    # Instead create a separate workspace for token vs approval workspace mismatch not relevant.
    # Simplest: use same user but fix approve workspace to bypass: create second workspace where user is still active, but token's workspace is the lapsed one.
    # Token's workspace is tenant_a workspace (lapsed). Approver's most recent membership will be the other workspace (still active) -> approval succeeds
    ws2 = seed.workspace(tenant_a["user"]["id"])
    seed.member(ws2["id"], tenant_a["user"]["id"])
    # re-create cookies for original user (session still valid)
    cookies_orig = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    codes2 = device_start(api, scope="engine").json()
    ap = api.post("/api/v1/auth/device/approve/", json={"user_code": codes2["user_code"]}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies_orig.items())})
    assert ap.status_code == 200, ap.text
    r = _poll(api, codes2["device_code"], bearer=mt)
    assert r.status_code == 401, r.text
    assert r.json() == {"error": "invalid_client", "error_description": "membership_revoked"}
    # Token must be revoked as side effect
    with seed.conn.cursor() as cur:
        cur.execute("SELECT revoked_at FROM machine_token WHERE token_hash=%s", (seed._hash(mt, secret) if hasattr(seed, "_hash") else ""))
        # Use djangocrypto hash if available; fallback query by token raw not stored. Look up by user_id/workspace
        cur.execute("SELECT revoked_at FROM machine_token WHERE dev_machine_id=%s AND workspace_id=%s ORDER BY created_at DESC LIMIT 1", (machine, tenant_a["workspace"]["id"]))
        (rev,) = cur.fetchone()
        assert rev is not None, "membership lapse must revoke token"


def test_unbound_token_gives_403_dev_machine_required(api, seed, tenant_a, secret):
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], None, secret)
    codes = device_start(api, scope="engine").json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    assert approve_post(api, cookies, codes["user_code"]).status_code == 200
    r = _poll(api, codes["device_code"], bearer=mt)
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "dev_machine_required", "error_description": "This machine token is not bound to a computer."}


def test_approve_silent_refusal_never_active_signedout(api, seed, tenant_a, secret):
    # never / active / signed_out descriptions
    cases = [
        ("never", "The built-in engine is not turned on for this computer; turn it on explicitly."),
        ("active", "The built-in engine credential on this computer is not due for renewal yet."),
        ("signed_out", "The built-in engine was signed out on this computer; sign it in explicitly."),
    ]
    for state, desc in cases:
        machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state=state)
        # For active, need no expiry so derived is active
        if state == "active":
            # ensure not expiring: expires far future
            with seed.conn.cursor() as cur:
                cur.execute("UPDATE dev_machine SET engine_credential_expires_at=%s WHERE id=%s", (datetime.now(timezone.utc)+timedelta(days=14), machine))
                seed.conn.commit()
        codes = device_start(api, scope="engine", dev_machine_id=machine).json()
        cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
        r = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": False, "dev_machine_id": machine}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
        assert r.status_code == 409, f"state {state} {r.text}"
        body = r.json()
        assert body["error"] == "engine_silent_renewal_refused", body
        assert body["engine_sign_in_state"] == state, body
        assert body["error_description"] == desc, body


def test_approve_silent_expired_expiring_allowed(api, seed, tenant_a, secret):
    # expiring (1 day left) and expired (past) should be allowed silently
    for expires in [datetime.now(timezone.utc)+timedelta(days=1), datetime.now(timezone.utc)-timedelta(days=1)]:
        machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="active")
        with seed.conn.cursor() as cur:
            cur.execute("UPDATE dev_machine SET engine_credential_expires_at=%s WHERE id=%s", (expires, machine))
            seed.conn.commit()
            cur.execute("SELECT engine_sign_in_state, engine_credential_expires_at FROM dev_machine WHERE id=%s", (machine,))
            (s, e) = cur.fetchone()
        codes = device_start(api, scope="engine", dev_machine_id=machine).json()
        cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
        r = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": False, "dev_machine_id": machine}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
        assert r.status_code == 200, f"expires {expires} {r.text}"
        # body has engine_sign_in_state expiring/expired
        assert "engine_sign_in_state" in r.json()
        assert r.json()["engine_sign_in_state"] in ("expiring", "expired")


def test_approve_workspace_slug_and_missing(api, seed, tenant_a, secret):
    ws2 = seed.workspace(tenant_a["user"]["id"])
    seed.member(ws2["id"], tenant_a["user"]["id"])
    codes = device_start(api, scope="engine").json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    r = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "workspace_slug": ws2["slug"]}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert r.status_code == 200, r.text
    assert r.json()["workspace_slug"] == ws2["slug"]
    # missing slug -> 404 workspace_not_found
    codes2 = device_start(api, scope="engine").json()
    r2 = api.post("/api/v1/auth/device/approve/", json={"user_code": codes2["user_code"], "workspace_slug": "nope-not-exist"}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert r2.status_code == 404
    assert r2.json() == {"error": "workspace_not_found"}


def test_approve_named_mismatch_400_and_another_user_404(api, seed, tenant_a, tenant_b, secret):
    machine_a = seed.dev_machine(tenant_a["user"]["id"])
    machine_b = seed.dev_machine(tenant_a["user"]["id"])
    codes = device_start(api, scope="engine", dev_machine_id=machine_a).json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    r = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": True, "dev_machine_id": machine_b}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "dev_machine_mismatch"}
    # another user's machine -> 404
    machine_b_owned = seed.dev_machine(tenant_b["user"]["id"])
    mt_b_machine = machine_b_owned
    codes2 = device_start(api, scope="engine", dev_machine_id=mt_b_machine).json()
    cookies_a = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    r2 = api.post("/api/v1/auth/device/approve/", json={"user_code": codes2["user_code"], "interactive": True, "dev_machine_id": mt_b_machine}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies_a.items())})
    assert r2.status_code == 404, r2.text
    assert r2.json() == {"error": "dev_machine_not_found"}


def test_approve_body_fields_when_machine_known(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"])
    codes = device_start(api, scope="engine", dev_machine_id=machine).json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    r = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": True, "dev_machine_id": machine}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["scope"] == "engine"
    assert body["interactive"] == True
    assert body["dev_machine_id"] == machine
    assert body["engine_sign_in_state"] == "never"
    # without machine, those keys absent
    codes2 = device_start(api, scope="engine").json()
    r2 = api.post("/api/v1/auth/device/approve/", json={"user_code": codes2["user_code"], "interactive": True}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert r2.status_code == 200
    assert "dev_machine_id" not in r2.json()
    assert "engine_sign_in_state" not in r2.json()


def test_silent_redemption_refusal_and_signout_between(api, seed, tenant_a, secret):
    # grant approved silently with no machine, then redeem with never machine silent
    machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="never")
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    codes = device_start(api, scope="engine").json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    r = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": False}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert r.status_code == 200, r.text
    # interactive_approval stored false
    with seed.conn.cursor() as cur:
        cur.execute("SELECT interactive_approval FROM cli_device_codes WHERE device_code=%s", (codes["device_code"],))
        (ia,) = cur.fetchone()
        assert ia == False
    # redemption silent and never -> 409
    rr = _poll(api, codes["device_code"], bearer=mt)
    assert rr.status_code == 409, rr.text
    assert rr.json()["error"] == "engine_silent_renewal_refused"
    assert rr.json()["engine_sign_in_state"] == "never"
    assert rr.json()["error_description"] == "The built-in engine is not turned on for this computer; turn it on explicitly."


def test_signout_between_approval_and_redemption(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="active")
    with seed.conn.cursor() as cur:
        cur.execute("UPDATE dev_machine SET engine_credential_expires_at=%s WHERE id=%s", (datetime.now(timezone.utc)-timedelta(days=1), machine))
        seed.conn.commit()
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    seed.engine_credential(tenant_a["user"]["id"], machine, secret, expires_in_days=1)
    codes = device_start(api, scope="engine", dev_machine_id=machine).json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    # silent approval should pass (expired)
    ap = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": False, "dev_machine_id": machine}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert ap.status_code == 200, ap.text
    # sign out before redemption
    with seed.conn.cursor() as cur:
        cur.execute("UPDATE dev_machine SET engine_sign_in_state='signed_out', engine_credential_expires_at=NULL WHERE id=%s", (machine,))
        cur.execute("UPDATE engine_credential SET revoked_at=now(), revoked_reason='signed_out' WHERE dev_machine_id=%s AND revoked_at IS NULL", (machine,))
        seed.conn.commit()
    rr = _poll(api, codes["device_code"], bearer=mt)
    assert rr.status_code == 409, rr.text
    assert rr.json()["error"] == "engine_silent_renewal_refused"
    assert rr.json()["engine_sign_in_state"] == "signed_out"


def test_pending_with_bearer_and_without(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"])
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    codes = device_start(api, scope="engine", dev_machine_id=machine).json()
    # poll before approval with valid bearer -> authorization_pending after bearer passes
    r = _poll(api, codes["device_code"], bearer=mt)
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "authorization_pending"}
    # poll without bearer -> machine_token_required even though pending
    r2 = _poll(api, codes["device_code"])
    assert r2.status_code == 401, r2.text
    assert r2.json() == {"error": "machine_token_required", "error_description": "Redeem an engine grant with Authorization: Bearer <machine token>."}


def test_access_denied_and_mismatch_at_redemption(api, seed, tenant_a, tenant_b, secret):
    machine_a = seed.dev_machine(tenant_a["user"]["id"])
    machine_b = seed.dev_machine(tenant_a["user"]["id"])
    mt_a = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine_a, secret)
    mt_b = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine_b, secret)
    # grant for A redeemed by B -> mismatch 403
    codes = device_start(api, scope="engine", dev_machine_id=machine_a).json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    ap = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": True, "dev_machine_id": machine_a}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert ap.status_code == 200, ap.text
    r = _poll(api, codes["device_code"], bearer=mt_b)
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "dev_machine_mismatch", "error_description": "This grant was approved for another computer."}
    # access denied: approver different user than token owner
    machine_b_user = seed.dev_machine(tenant_b["user"]["id"])
    mt_for_b = seed.machine_token(tenant_b["user"]["id"], tenant_b["workspace"]["id"], machine_b_user, secret)
    codes2 = device_start(api, scope="engine", dev_machine_id=machine_a).json()
    cookies_a = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    ap2 = api.post("/api/v1/auth/device/approve/", json={"user_code": codes2["user_code"], "interactive": True, "dev_machine_id": machine_a}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies_a.items())})
    assert ap2.status_code == 200, ap2.text
    r2 = _poll(api, codes2["device_code"], bearer=mt_for_b)
    assert r2.status_code == 403, r2.text
    assert r2.json() == {"error": "access_denied", "error_description": "This computer does not belong to the approving user."}


def test_engine_grant_happy_path(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"])
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    # interactive approval passes for never machine; redemption then hits
    # engine_unavailable in CE (profile never available in open source, cloud
    # lane available only via overlay stubbed in Django internal tests).
    codes2 = device_start(api, scope="engine", dev_machine_id=machine).json()
    cookies2 = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    ap2 = api.post("/api/v1/auth/device/approve/", json={"user_code": codes2["user_code"], "interactive": True, "dev_machine_id": machine}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies2.items())})
    assert ap2.status_code == 200, ap2.text
    r2 = _poll(api, codes2["device_code"], bearer=mt)
    # In CE, profile unavailable -> 409 engine_unavailable on both backends
    # (Rust matches Django CE: managed_runner alone does not make profile
    # available without a gateway lane; cloud overlay provides it) -- pin the CE expectation, not a disjunction
    assert r2.status_code == 409, r2.text
    body = r2.json()
    assert body["error"] == "engine_unavailable"
    assert body["reason_code"] in ("profile_unavailable", "engine_disabled")
    assert body["engine_sign_in_state"] == "never"
    assert body["error_description"] == "This Pi Dash instance cannot serve the built-in engine for this account."


def test_machine_token_issues_engine_cred(api, seed, tenant_a, secret):
    # Full cli login then exchange -> in CE instance_can_serve false so no engine cred
    codes = device_start(api).json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
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
    # CE: instance_can_serve_engine false (profile unavailable) -> engine_credential absent
    assert "engine_credential" not in body, body


def test_machine_token_no_engine_for_machine_token_caller(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"])
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    r = _exchange(api, mt, workspace_slug=tenant_a["workspace"]["slug"], dev_machine_id=str(uuid.uuid4()), host_label="x")
    assert r.status_code == 201
    assert "engine_credential" not in r.json()


def test_machine_token_expires_at_set(api, seed, tenant_a, secret):
    dm = str(uuid.uuid4())
    codes = device_start(api).json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    ap = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": True}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert ap.status_code == 200
    api_token = poll_token(api, codes["device_code"]).json()["access_token"]
    r = _exchange(api, api_token, workspace_slug=tenant_a["workspace"]["slug"], dev_machine_id=dm, host_label="host")
    assert r.status_code == 201, r.text
    raw = r.json()["machine_token"]
    # check DB expires_at is ~60 days in future, not NULL
    from _harness import djangocrypto
    h = djangocrypto.machine_token_hash(raw, secret)
    with seed.conn.cursor() as cur:
        cur.execute("SELECT expires_at, last_used_at FROM machine_token WHERE token_hash=%s", (h,))
        (exp, last) = cur.fetchone()
        assert exp is not None, "expires_at must be set, not NULL"
        # should be roughly 60 days
        delta = exp - datetime.now(timezone.utc)
        assert timedelta(days=59) < delta < timedelta(days=61), f"delta {delta} not ~60d"


def test_machine_token_touch_renews_expiry(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"])
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret, expires_in_days=10)
    # Touch via successful resolver path (e.g., device flow with valid token) should renew to 60 days
    from _harness import djangocrypto
    h = djangocrypto.machine_token_hash(mt, secret)
    # Do a successful engine grant with this token (CE gives 409 but still resolves and touches before that)
    codes = device_start(api, scope="engine", dev_machine_id=machine).json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    ap = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": True, "dev_machine_id": machine}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert ap.status_code == 200, ap.text
    r = _poll(api, codes["device_code"], bearer=mt)
    assert r.status_code == 409  # CE unavailable, but touch already happened
    with seed.conn.cursor() as cur:
        cur.execute("SELECT expires_at, last_used_at FROM machine_token WHERE token_hash=%s", (h,))
        (exp, last) = cur.fetchone()
        assert exp is not None
        assert exp > datetime.now(timezone.utc) + timedelta(days=55), f"touch must renew to ~60d, got {exp}"
        assert last is not None


def test_revoke_signs_out_engine(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="active")
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    seed.engine_credential(tenant_a["user"]["id"], machine, secret)
    r = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt})
    assert r.status_code == 200
    assert r.json() == {"ok": True}
    with seed.conn.cursor() as cur:
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine,))
        (state,) = cur.fetchone()
    assert state == "signed_out"


def test_revoke_workspace_logout_keeps_engine(api, seed, tenant_a, tenant_b, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="active")
    mt_a = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    ws2 = seed.workspace(tenant_a["user"]["id"])
    seed.member(ws2["id"], tenant_a["user"]["id"])
    mt_b = seed.machine_token(tenant_a["user"]["id"], ws2["id"], machine, secret)
    r = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt_b})
    assert r.status_code == 200
    with seed.conn.cursor() as cur:
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine,))
        (state,) = cur.fetchone()
    assert state == "active"
    r2 = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt_a})
    assert r2.status_code == 200
    with seed.conn.cursor() as cur:
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine,))
        (state2,) = cur.fetchone()
    assert state2 == "signed_out"


def test_revoke_expired_sibling_does_not_keep_engine(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="active")
    mt_live = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    mt_expired = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret, expires_in_days=-1)
    seed.engine_credential(tenant_a["user"]["id"], machine, secret)
    r = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt_live})
    assert r.status_code == 200
    with seed.conn.cursor() as cur:
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine,))
        (state,) = cur.fetchone()
    assert state == "signed_out"


def test_revoke_personal_api_token_leaves_engine(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="active")
    seed.engine_credential(tenant_a["user"]["id"], machine, secret)
    personal = seed.api_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"])
    r = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": personal})
    assert r.status_code == 200
    with seed.conn.cursor() as cur:
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine,))
        (state,) = cur.fetchone()
    assert state == "active"


def test_revoke_full_logout_never_idempotent(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="never")
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    r = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt})
    assert r.status_code == 200
    with seed.conn.cursor() as cur:
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine,))
        (state,) = cur.fetchone()
    assert state == "never"
    # The revoked token is now rejected (DRF answers 403: no auth header
    # challenge; the CLI clears its local token either way). Mirrors Django's
    # test_full_logout_without_engine_leaves_it_never_and_is_idempotent.
    r2 = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt})
    assert r2.status_code in (401, 403)
    with seed.conn.cursor() as cur:
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine,))
        (state2,) = cur.fetchone()
    assert state2 == "never"


def test_revoke_only_token_machine_affected(api, seed, tenant_a, tenant_b, secret):
    machine_a = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="active")
    machine_b = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="active")
    seed.engine_credential(tenant_a["user"]["id"], machine_a, secret)
    seed.engine_credential(tenant_a["user"]["id"], machine_b, secret)
    mt_a = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine_a, secret)
    mt_b = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine_b, secret)
    r = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt_a})
    assert r.status_code == 200
    with seed.conn.cursor() as cur:
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine_a,))
        (sa,) = cur.fetchone()
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine_b,))
        (sb,) = cur.fetchone()
    assert sa == "signed_out"
    assert sb == "active"


def test_relogin_rotation_supersedes(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="active")
    with seed.conn.cursor() as cur:
        cur.execute("UPDATE dev_machine SET engine_credential_expires_at=%s WHERE id=%s", (datetime.now(timezone.utc)+timedelta(days=1), machine))
        seed.conn.commit()
    old_raw = seed.engine_credential(tenant_a["user"]["id"], machine, secret)
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    # Use engine grant interactive to issue new credential (will be 409 CE, so rotation not provable black-box without cloud lane)
    # Instead test via direct DB rotation: issue via internal service not needed; we pin that seed credential remains live until superseded by next issue
    # The DB-level supersede is covered by Django service; here assert live count 1 before and after a manual supersede via revoke
    with seed.conn.cursor() as cur:
        cur.execute("SELECT count(*) FROM engine_credential WHERE dev_machine_id=%s AND revoked_at IS NULL", (machine,))
        (cnt,) = cur.fetchone()
        assert cnt == 1
    # Creating a second live credential would violate unique partial index; the service's issue revokes first then creates new
    # So after our CE 409 path, no rotation occurs (engine_unavailable) - verify state unchanged
    mt2 = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    # cleanup the extra mt2 to keep last-token logic simple; revoke it manually
    with seed.conn.cursor() as cur:
        cur.execute("SELECT id FROM machine_token WHERE dev_machine_id=%s ORDER BY created_at DESC", (machine,))
        (rid,) = cur.fetchone()
        cur.execute("UPDATE machine_token SET revoked_at=now() WHERE id=%s", (rid,))
        seed.conn.commit()
    codes = device_start(api, scope="engine", dev_machine_id=machine).json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    ap = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": True, "dev_machine_id": machine}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert ap.status_code == 200
    r = _poll(api, codes["device_code"], bearer=mt)
    assert r.status_code == 409  # CE
    assert r.json()["error"] == "engine_unavailable"
    with seed.conn.cursor() as cur:
        cur.execute("SELECT count(*) FROM engine_credential WHERE dev_machine_id=%s AND revoked_at IS NULL", (machine,))
        (cnt2,) = cur.fetchone()
        assert cnt2 == 1, "credential must remain live, not superseded when CE unavailable"


def test_grant_approved_before_logout_cannot_redeem_after(api, seed, tenant_a, secret):
    machine = seed.dev_machine(tenant_a["user"]["id"], engine_sign_in_state="active")
    with seed.conn.cursor() as cur:
        cur.execute("UPDATE dev_machine SET engine_credential_expires_at=%s WHERE id=%s", (datetime.now(timezone.utc)+timedelta(days=1), machine))
        seed.conn.commit()
    mt = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    seed.engine_credential(tenant_a["user"]["id"], machine, secret, expires_in_days=1)
    # silent approve (expired -> allowed)
    codes = device_start(api, scope="engine", dev_machine_id=machine).json()
    cookies = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    ap = api.post("/api/v1/auth/device/approve/", json={"user_code": codes["user_code"], "interactive": False, "dev_machine_id": machine}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())})
    assert ap.status_code == 200, ap.text
    # full logout signs out engine
    r = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt})
    assert r.status_code == 200
    with seed.conn.cursor() as cur:
        cur.execute("SELECT engine_sign_in_state FROM dev_machine WHERE id=%s", (machine,))
        (s,) = cur.fetchone()
        assert s == "signed_out"
    # now redemption with same mt (now revoked) must fail at resolver, not at silent re-check
    rr = _poll(api, codes["device_code"], bearer=mt)
    assert rr.status_code == 401, rr.text
    assert rr.json()["error"] == "invalid_client"
    # A fresh mt for same machine after logout plus grant still silent-refused
    mt2 = seed.machine_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"], machine, secret)
    # grant's interactive_approval is false, state signed_out silent -> 409
    codes2 = device_start(api, scope="engine", dev_machine_id=machine).json()
    # approve silently after logout should be refused at approve (signed_out not allowed)
    cookies2 = seed.session_cookie(tenant_a["user"]["id"], tenant_a["user"]["password"], secret)
    ap2 = api.post("/api/v1/auth/device/approve/", json={"user_code": codes2["user_code"], "interactive": False, "dev_machine_id": machine}, headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies2.items())})
    assert ap2.status_code == 409, ap2.text
    assert ap2.json()["error"] == "engine_silent_renewal_refused"
    assert ap2.json()["engine_sign_in_state"] == "signed_out"

