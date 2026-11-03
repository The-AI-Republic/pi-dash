"""POST /api/v1/auth/revoke/ — `pidash auth logout` kills the caller token."""


def _revoke(api, token):
    headers = {"X-Api-Key": token} if token else {}
    return api.post("/api/v1/auth/revoke/", headers=headers)


def test_revoke_shape(api, seed, tenant_a, db):
    token = seed.api_token(
        tenant_a["user"]["id"], tenant_a["workspace"]["id"]
    )
    r = _revoke(api, token)
    assert r.status_code == 200, r.text
    assert r.json() == {"ok": True}

    with db.cursor() as cur:
        cur.execute(
            "SELECT is_active FROM api_tokens WHERE token = %s", (token,)
        )
        (active,) = cur.fetchone()
    assert active is False

    # The revoked token no longer authenticates.
    again = api.get(
        "/api/v1/auth/workspaces/", headers={"X-Api-Key": token}
    )
    assert again.status_code == 403, again.text


def test_revoke_requires_auth(api):
    assert _revoke(api, None).status_code == 401
    assert _revoke(api, "bogus").status_code == 403


def test_revoke_machine_token(api, seed, tenant_a, secret, db):
    from _harness import djangocrypto

    machine = seed.dev_machine(tenant_a["user"]["id"])
    raw = seed.machine_token(
        tenant_a["user"]["id"],
        tenant_a["workspace"]["id"],
        machine,
        secret,
    )
    r = _revoke(api, raw)
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
