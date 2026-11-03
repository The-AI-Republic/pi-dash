"""GET /api/v1/auth/workspaces/ — membership picker for `pidash auth login`."""


def test_workspaces_shape(api, seed, tenant_a, api_key_a):
    second = seed.workspace(tenant_a["user"]["id"], name="CT80 Second")
    seed.member(second["id"], tenant_a["user"]["id"])
    r = api.get(
        "/api/v1/auth/workspaces/", headers={"X-Api-Key": api_key_a}
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == {"workspaces"}
    assert isinstance(body["workspaces"], list)
    for entry in body["workspaces"]:
        assert set(entry) == {"slug", "name"}
    slugs = [w["slug"] for w in body["workspaces"]]
    # Member-since order: the first workspace stays first.
    assert slugs[0] == tenant_a["workspace"]["slug"]
    assert second["slug"] in slugs


def test_workspaces_requires_auth(api):
    r = api.get("/api/v1/auth/workspaces/")
    assert r.status_code == 401, r.text


def test_workspaces_bad_token(api):
    # An invalid token fails authentication: 403 with the auth detail.
    r = api.get(
        "/api/v1/auth/workspaces/", headers={"X-Api-Key": "bogus"}
    )
    assert r.status_code == 403, r.text
    assert r.json() == {"detail": "Given API token is not valid"}


def test_workspaces_revoked_token(api, seed, tenant_a):
    token = seed.api_token(
        tenant_a["user"]["id"], tenant_a["workspace"]["id"]
    )
    assert (
        api.get(
            "/api/v1/auth/workspaces/", headers={"X-Api-Key": token}
        ).status_code
        == 200
    )
    with seed.conn.cursor() as cur:
        cur.execute(
            "UPDATE api_tokens SET is_active = false WHERE token = %s",
            (token,),
        )
    r = api.get("/api/v1/auth/workspaces/", headers={"X-Api-Key": token})
    assert r.status_code == 403, r.text


def test_workspaces_tenant_isolation(api, seed, tenant_a, tenant_b):
    """A caller sees exactly their own workspaces — nothing leaks across."""
    key_a = seed.api_token(
        tenant_a["user"]["id"], tenant_a["workspace"]["id"]
    )
    key_b = seed.api_token(
        tenant_b["user"]["id"], tenant_b["workspace"]["id"]
    )
    slugs_a = {
        w["slug"]
        for w in api.get(
            "/api/v1/auth/workspaces/", headers={"X-Api-Key": key_a}
        ).json()["workspaces"]
    }
    slugs_b = {
        w["slug"]
        for w in api.get(
            "/api/v1/auth/workspaces/", headers={"X-Api-Key": key_b}
        ).json()["workspaces"]
    }
    assert slugs_a == {tenant_a["workspace"]["slug"]}
    assert slugs_b == {tenant_b["workspace"]["slug"]}


def test_workspaces_empty(api, seed):
    user = seed.user()
    key = seed.api_token(user["id"])
    r = api.get("/api/v1/auth/workspaces/", headers={"X-Api-Key": key})
    assert r.status_code == 200, r.text
    assert r.json() == {"workspaces": []}
