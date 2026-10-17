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


def _spellings(runner_id):
    return {
        "uppercase": runner_id.upper(),
        "simple": runner_id.replace("-", ""),
        "braced": "{" + runner_id + "}",
        "urn": "urn:uuid:" + runner_id,
    }


def _runner_alive(db, runner_id):
    with db.cursor() as cur:
        cur.execute("SELECT count(*) FROM runner WHERE id = %s", (runner_id,))
        (remaining,) = cur.fetchone()
    return remaining


def _assert_resolver_404(r):
    # Django's <uuid:> converter rejects the segment before any view or
    # auth code runs: the answer is the resolver's HTML 404, not the
    # view's JSON {"error": "not found"}.
    assert r.status_code == 404, r.text
    assert r.headers["content-type"].startswith("text/html"), r.headers.get(
        "content-type"
    )


def test_runner_delete_noncanonical_spelling_404s_without_writes(
    api, seed, tenant_a, db
):
    """Uppercase/simple/braced/urn spellings never route: 404, no writes.

    PIDASHCONV-1095: Django's UUIDConverter regex only matches
    lowercase-hyphenated UUIDs, so every other spelling 404s at the
    resolver with the row untouched. Rust proxies these to Django.
    """
    token = seed.api_token(
        tenant_a["user"]["id"], tenant_a["workspace"]["id"]
    )
    for spelling in ("uppercase", "simple", "braced", "urn"):
        runner_id = _seed_runner(seed, tenant_a)
        segment = _spellings(runner_id)[spelling]
        r = _delete(api, token, segment)
        _assert_resolver_404(r)
        assert _runner_alive(db, runner_id) == 1, spelling


def test_runner_delete_uppercase_anonymous_404s(api, seed, tenant_a, db):
    """Resolver 404 precedes auth: anonymous callers also get 404."""
    runner_id = _seed_runner(seed, tenant_a)
    r = _delete(api, None, runner_id.upper())
    _assert_resolver_404(r)
    r = _delete(api, "bogus", runner_id.upper())
    _assert_resolver_404(r)
    assert _runner_alive(db, runner_id) == 1
