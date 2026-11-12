"""Permission + tenant-isolation floor for the device flow (PIDASHCONV-103).

- Denied case: an unauthenticated ``approve`` is rejected AND the grant
  stays pending (a later ``token/`` poll still reports pending).
- Isolation: user B cannot approve/consume user A's grant, cannot mint a
  machine token for A's workspace, and neither user's workspace list
  leaks the other's private workspace.
"""

import uuid

from .conftest import approve, device_start, poll_token, session_for


def test_approve_denied_without_session_stays_pending(api, db):
    """Unauthenticated approve -> 401/403 and the code stays unapproved."""
    codes = device_start(api).json()

    r = approve(api, {}, codes["user_code"])
    assert r.status_code in (401, 403), r.text

    with db.cursor() as cur:
        cur.execute(
            "SELECT approved, user_id FROM cli_device_codes"
            " WHERE device_code = %s",
            (codes["device_code"],),
        )
        approved, user_id = cur.fetchone()
    assert approved is False
    assert user_id is None

    # The grant is still pending: the CLI poll reports authorization_pending.
    pending = poll_token(api, codes["device_code"])
    assert pending.status_code == 400, pending.text
    assert pending.json() == {"error": "authorization_pending"}


def test_tenant_isolation_device_flow(api, seed, tenant_a, tenant_b, secret, db):
    """B cannot take over A's grant; workspace lists never cross."""
    codes = device_start(api).json()
    cookies_a = session_for(seed, tenant_a, secret)
    cookies_b = session_for(seed, tenant_b, secret)

    assert approve(api, cookies_a, codes["user_code"]).status_code == 200

    # B cannot approve A's already-approved code ...
    clash = approve(api, cookies_b, codes["user_code"])
    assert clash.status_code == 409, clash.text
    assert clash.json() == {
        "error": "This code has already been approved by another user."
    }

    # ... and B cannot mint a machine token for A's workspace.
    key_b = seed.api_token(
        tenant_b["user"]["id"], tenant_b["workspace"]["id"]
    )
    grab = api.post(
        "/api/v1/auth/machine-token/",
        json={
            "workspace_slug": tenant_a["workspace"]["slug"],
            "dev_machine_id": str(uuid.uuid4()),
            "host_label": "ct103-host",
        },
        headers={"X-Api-Key": key_b},
    )
    assert grab.status_code == 404, grab.text
    assert grab.json() == {"error": "workspace_not_found"}

    # The grant still mints exactly once, for A; nobody can consume it twice.
    first = poll_token(api, codes["device_code"])
    assert first.status_code == 200, first.text
    assert first.json()["user_email"] == tenant_a["user"]["email"]
    second = poll_token(api, codes["device_code"])
    assert second.status_code == 400, second.text
    assert second.json()["error"] == "invalid_grant"

    # Workspace lists are disjoint: neither side leaks the other's workspace.
    slugs_a = {
        w["slug"]
        for w in api.get(
            "/api/v1/auth/workspaces/",
            headers={"X-Api-Key": seed.api_token(
                tenant_a["user"]["id"], tenant_a["workspace"]["id"]
            )},
        ).json()["workspaces"]
    }
    slugs_b = {
        w["slug"]
        for w in api.get(
            "/api/v1/auth/workspaces/", headers={"X-Api-Key": key_b}
        ).json()["workspaces"]
    }
    assert tenant_a["workspace"]["slug"] in slugs_a
    assert tenant_b["workspace"]["slug"] in slugs_b
    assert tenant_b["workspace"]["slug"] not in slugs_a
    assert tenant_a["workspace"]["slug"] not in slugs_b


def test_workspaces_item_shape_and_auth(api, seed, tenant_a, api_key_a):
    r = api.get(
        "/api/v1/auth/workspaces/", headers={"X-Api-Key": api_key_a}
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == {"workspaces"}
    assert isinstance(body["workspaces"], list)
    for entry in body["workspaces"]:
        assert set(entry) == {"slug", "name"}
    assert {w["slug"] for w in body["workspaces"]} == {
        tenant_a["workspace"]["slug"]
    }

    assert api.get("/api/v1/auth/workspaces/").status_code == 401
    bad = api.get(
        "/api/v1/auth/workspaces/", headers={"X-Api-Key": "bogus"}
    )
    assert bad.status_code == 403, bad.text
    assert bad.json() == {"detail": "Given API token is not valid"}


def test_machine_token_validation(api, seed, tenant_a):
    """Missing/invalid exchange fields fail exactly as Python defines."""
    token = seed.api_token(
        tenant_a["user"]["id"], tenant_a["workspace"]["id"]
    )
    base = {
        "workspace_slug": tenant_a["workspace"]["slug"],
        "dev_machine_id": str(uuid.uuid4()),
        "host_label": "ct103-host",
    }
    expected = {
        "workspace_slug": {"error": "workspace_slug is required"},
        "dev_machine_id": {"error": "dev_machine_id is required"},
        "host_label": {"error": "host_label is required"},
    }
    for drop, body in expected.items():
        fields = {k: v for k, v in base.items() if k != drop}
        r = api.post(
            "/api/v1/auth/machine-token/",
            json=fields,
            headers={"X-Api-Key": token},
        )
        assert r.status_code == 400, (drop, r.text)
        assert r.json() == body, (drop, r.text)

    bad_uuid = api.post(
        "/api/v1/auth/machine-token/",
        json={**base, "dev_machine_id": "not-a-uuid"},
        headers={"X-Api-Key": token},
    )
    assert bad_uuid.status_code == 400, bad_uuid.text
    assert bad_uuid.json() == {"error": "invalid_dev_machine_id"}

    unknown_ws = api.post(
        "/api/v1/auth/machine-token/",
        json={**base, "workspace_slug": "no-such-workspace"},
        headers={"X-Api-Key": token},
    )
    assert unknown_ws.status_code == 404, unknown_ws.text
    assert unknown_ws.json() == {"error": "workspace_not_found"}

    assert (
        api.post(
            "/api/v1/auth/machine-token/",
            json=base,
            headers={"X-Api-Key": "bogus"},
        ).status_code
        == 403
    )


def test_revoke_api_token(api, seed, tenant_a, db):
    token = seed.api_token(
        tenant_a["user"]["id"], tenant_a["workspace"]["id"]
    )
    r = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": token})
    assert r.status_code == 200, r.text
    assert r.json() == {"ok": True}

    with db.cursor() as cur:
        cur.execute(
            "SELECT is_active FROM api_tokens WHERE token = %s", (token,)
        )
        (active,) = cur.fetchone()
    assert active is False

    # The revoked token no longer authenticates.
    assert (
        api.get(
            "/api/v1/auth/workspaces/", headers={"X-Api-Key": token}
        ).status_code
        == 403
    )


def test_revoke_machine_token(api, seed, tenant_a, secret, db):
    from _harness import djangocrypto

    machine = seed.dev_machine(tenant_a["user"]["id"])
    raw = seed.machine_token(
        tenant_a["user"]["id"],
        tenant_a["workspace"]["id"],
        machine,
        secret,
    )
    r = api.post("/api/v1/auth/revoke/", headers={"X-Api-Key": raw})
    assert r.status_code == 200, r.text
    assert r.json() == {"ok": True}

    with db.cursor() as cur:
        cur.execute(
            "SELECT revoked_at FROM machine_token WHERE token_hash = %s",
            (djangocrypto.machine_token_hash(raw, secret),),
        )
        (revoked_at,) = cur.fetchone()
    assert revoked_at is not None

    assert (
        api.get(
            "/api/v1/auth/workspaces/", headers={"X-Api-Key": raw}
        ).status_code
        == 403
    )


def test_revoke_requires_auth(api):
    assert api.post("/api/v1/auth/revoke/").status_code == 401
    assert (
        api.post(
            "/api/v1/auth/revoke/", headers={"X-Api-Key": "bogus"}
        ).status_code
        == 403
    )
