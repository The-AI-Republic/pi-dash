"""DELETE /api/v1/runners/<id>/ — CLI-friendly cascade delete."""


def _seed_runner(seed, tenant):
    project = seed.project(tenant["workspace"]["id"])
    pod = seed.pod(tenant["workspace"]["id"], project)
    return seed.runner(tenant["user"]["id"], tenant["workspace"]["id"], pod)


def _delete(api, token, runner_id, query=""):
    return api.delete(
        f"/api/v1/runners/{runner_id}/{query}",
        headers={"X-Api-Key": token} if token else {},
    )


def test_runner_delete_shape(api, seed, tenant_a, db):
    token = seed.api_token(
        tenant_a["user"]["id"], tenant_a["workspace"]["id"]
    )
    runner_id = _seed_runner(seed, tenant_a)
    r = _delete(api, token, runner_id)
    assert r.status_code == 204, r.text
    assert r.content == b""

    with db.cursor() as cur:
        cur.execute("SELECT count(*) FROM runner WHERE id = %s", (runner_id,))
        (remaining,) = cur.fetchone()
    assert remaining == 0, "delete must hard-delete the runner row"


def test_runner_delete_purge_flag(api, seed, tenant_a):
    token = seed.api_token(
        tenant_a["user"]["id"], tenant_a["workspace"]["id"]
    )
    explicit_false = _seed_runner(seed, tenant_a)
    r = _delete(api, token, explicit_false, "?purge_local=false")
    assert r.status_code == 204, r.text

    bad_value = _seed_runner(seed, tenant_a)
    r = _delete(api, token, bad_value, "?purge_local=maybe")
    assert r.status_code == 400, r.text
    assert r.json() == {
        "error": "purge_local must be one of: true, false, 1, 0, yes, no"
    }


def test_runner_delete_missing(api, seed, tenant_a):
    import uuid

    token = seed.api_token(
        tenant_a["user"]["id"], tenant_a["workspace"]["id"]
    )
    r = _delete(api, token, str(uuid.uuid4()))
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "not found"}


def test_runner_delete_requires_auth(api, seed, tenant_a):
    runner_id = _seed_runner(seed, tenant_a)
    assert _delete(api, None, runner_id).status_code == 401
    assert _delete(api, "bogus", runner_id).status_code == 403


def test_runner_delete_other_owner_isolation(api, seed, tenant_a, tenant_b):
    """Private runners are owner-only: another user sees 404, not 403."""
    token_b = seed.api_token(
        tenant_b["user"]["id"], tenant_b["workspace"]["id"]
    )
    runner_id = _seed_runner(seed, tenant_a)
    r = _delete(api, token_b, runner_id)
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "not found"}
