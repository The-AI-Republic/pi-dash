"""GET /api/v1/workspaces/<slug>/projects/<project_id>/runners/ — the CLI's
cloud runner list (``pidash project runners``, PDASHOSS01-245).

Pins the token-auth list against both servers: auth, identifier/UUID
resolution, the shared visibility rules (membership, private-runner
ownership, project/pod narrowing, bundled exclusion), the view's error
bodies, and that the payload is the web AI Workers panel's payload
(``GET /api/runners/``) rendered in the caller's time zone.
"""

import uuid


def _url(slug, project, query=""):
    return f"/api/v1/workspaces/{slug}/projects/{project}/runners/{query}"


def _get(api, token, slug, project, query=""):
    return api.get(
        _url(slug, project, query),
        headers={"X-Api-Key": token} if token else {},
    )


def _project(seed, tenant, identifier=None):
    identifier = identifier or ("PR" + uuid.uuid4().hex[:8]).upper()
    project_id = seed.project(tenant["workspace"]["id"], identifier=identifier)
    pod_id = seed.pod(tenant["workspace"]["id"], project_id)
    return project_id, identifier, pod_id


def _extra_pod(seed, workspace_id, project_id):
    return seed._insert(
        "pod",
        name=f"ct245-pod-{uuid.uuid4().hex[:6]}",
        description="",
        is_default=False,
        workspace_id=workspace_id,
        project_id=project_id,
        created_at="2026-10-01T00:00:00+00:00",
        updated_at="2026-10-01T00:00:00+00:00",
    )


def _token(seed, tenant):
    return seed.api_token(tenant["user"]["id"], tenant["workspace"]["id"])


def _set(db, table, row_id, **cols):
    sets = ", ".join(f'"{c}" = %s' for c in cols)
    with db.cursor() as cur:
        cur.execute(
            f'UPDATE {table} SET {sets} WHERE id = %s',
            [*cols.values(), row_id],
        )


def _ids(response):
    return [row["id"] for row in response.json()]


def test_lists_project_runners_with_status_and_heartbeat(api, seed, db, tenant_a):
    project_id, identifier, pod_id = _project(seed, tenant_a)
    runner_id = seed.runner(tenant_a["user"]["id"], tenant_a["workspace"]["id"], pod_id)
    _set(
        db,
        "runner",
        runner_id,
        status="online",
        last_heartbeat_at="2026-10-02T01:02:03.456789+00:00",
    )
    token = _token(seed, tenant_a)

    r = _get(api, token, tenant_a["workspace"]["slug"], identifier)
    assert r.status_code == 200, r.text
    assert r.headers["content-type"] == "application/json"
    body = r.json()
    assert [row["id"] for row in body] == [runner_id]
    row = body[0]
    assert row["status"] == "online"
    assert row["last_heartbeat_at"] == "2026-10-02T01:02:03.456789Z"
    assert row["pod"] == pod_id
    assert row["pod_detail"]["project"] == project_id
    assert row["pod_detail"]["project_identifier"] == identifier
    assert list(row) == [
        "id", "name", "status", "host_label", "provisioning", "os", "arch",
        "runner_version", "dev_metadata", "protocol_version", "capabilities",
        "last_heartbeat_at", "owner", "dev_machine", "dev_machine_detail",
        "visibility", "pod", "pod_detail", "live_state", "enrolled_at",
        "revoked_at", "revoked_reason", "created_at", "updated_at",
    ]
    for secret_field in ("refresh_token_hash", "enrollment_token_hash"):
        assert secret_field not in row


def test_payload_matches_web_panel(api, seed, db, tenant_a, secret):
    """Same rows and bytes as the session-auth AI Workers list (UTC user)."""
    project_id, identifier, pod_id = _project(seed, tenant_a)
    owner = tenant_a["user"]["id"]
    ws = tenant_a["workspace"]["id"]
    first = seed.runner(owner, ws, pod_id)
    second = seed.runner(owner, ws, pod_id)
    _set(db, "runner", first, updated_at="2026-10-01T00:00:00+00:00")
    _set(db, "runner", second, updated_at="2026-10-02T00:00:00+00:00")
    cookies = seed.session_cookie(owner, tenant_a["user"]["password"], secret)

    web = api.get(
        f"/api/runners/?workspace={ws}&project={project_id}",
        headers={"Cookie": "; ".join(f"{k}={v}" for k, v in cookies.items())},
    )
    v1 = _get(api, _token(seed, tenant_a), tenant_a["workspace"]["slug"], identifier)
    assert web.status_code == 200, web.text
    assert v1.status_code == 200, v1.text
    assert _ids(v1) == [second, first], "newest-updated first"
    assert v1.content == web.content


def test_identifier_and_uuid_resolve_to_same_list(api, seed, tenant_a):
    project_id, identifier, pod_id = _project(seed, tenant_a)
    runner_id = seed.runner(tenant_a["user"]["id"], tenant_a["workspace"]["id"], pod_id)
    token = _token(seed, tenant_a)
    slug = tenant_a["workspace"]["slug"]

    by_identifier = _get(api, token, slug, identifier)
    by_uuid = _get(api, token, slug, project_id)
    by_lower = _get(api, token, slug, identifier.lower())
    assert by_identifier.status_code == 200, by_identifier.text
    assert _ids(by_identifier) == [runner_id]
    assert by_uuid.content == by_identifier.content
    assert by_lower.content == by_identifier.content


def test_other_projects_runners_are_not_listed(api, seed, tenant_a):
    _, identifier, pod_id = _project(seed, tenant_a)
    _, _, other_pod = _project(seed, tenant_a)
    owner = tenant_a["user"]["id"]
    ws = tenant_a["workspace"]["id"]
    mine = seed.runner(owner, ws, pod_id)
    seed.runner(owner, ws, other_pod)

    r = _get(api, _token(seed, tenant_a), tenant_a["workspace"]["slug"], identifier)
    assert r.status_code == 200, r.text
    assert _ids(r) == [mine]


def test_other_users_private_runners_are_hidden(api, seed, tenant_a):
    """A second workspace member never sees the owner's private runner."""
    _, identifier, pod_id = _project(seed, tenant_a)
    seed.runner(tenant_a["user"]["id"], tenant_a["workspace"]["id"], pod_id)
    other = seed.user()
    seed.member(tenant_a["workspace"]["id"], other["id"])
    token = seed.api_token(other["id"], tenant_a["workspace"]["id"])

    r = _get(api, token, tenant_a["workspace"]["slug"], identifier)
    assert r.status_code == 200, r.text
    assert r.json() == []


def test_non_member_is_forbidden(api, seed, tenant_a, tenant_b):
    project_id, identifier, pod_id = _project(seed, tenant_a)
    seed.runner(tenant_a["user"]["id"], tenant_a["workspace"]["id"], pod_id)
    token_b = _token(seed, tenant_b)
    slug = tenant_a["workspace"]["slug"]

    for project in (identifier, project_id):
        r = _get(api, token_b, slug, project)
        assert r.status_code == 403, (project, r.text)
        assert r.json() == {"error": "forbidden"}
    # The membership gate runs before the filters are validated.
    r = _get(api, token_b, slug, identifier, "?pod=not-a-uuid")
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "forbidden"}


def test_unknown_project(api, seed, tenant_a, tenant_b):
    token = _token(seed, tenant_a)
    slug = tenant_a["workspace"]["slug"]

    r = _get(api, token, slug, "NOPE" + uuid.uuid4().hex[:6].upper())
    assert r.status_code == 404, r.text
    assert r.json() == {"detail": "Project not found"}

    r = _get(api, token, slug, str(uuid.uuid4()))
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "not found"}

    # Another workspace's project UUID is unknown under this slug.
    foreign_project, _, _ = _project(seed, tenant_b)
    r = _get(api, token, slug, foreign_project)
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "not found"}


def test_soft_deleted_project_is_not_found(api, seed, db, tenant_a):
    project_id, _, _ = _project(seed, tenant_a)
    _set(db, "projects", project_id, deleted_at="2026-10-01T00:00:00+00:00")
    r = _get(api, _token(seed, tenant_a), tenant_a["workspace"]["slug"], project_id)
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "not found"}


def test_pod_filter(api, seed, tenant_a):
    project_id, identifier, pod_id = _project(seed, tenant_a)
    ws = tenant_a["workspace"]["id"]
    owner = tenant_a["user"]["id"]
    second_pod = _extra_pod(seed, ws, project_id)
    in_default = seed.runner(owner, ws, pod_id)
    in_second = seed.runner(owner, ws, second_pod)
    token = _token(seed, tenant_a)
    slug = tenant_a["workspace"]["slug"]

    r = _get(api, token, slug, identifier, f"?pod={second_pod}")
    assert r.status_code == 200, r.text
    assert _ids(r) == [in_second]

    r = _get(api, token, slug, identifier, f"?pod={pod_id}")
    assert _ids(r) == [in_default]

    r = _get(api, token, slug, identifier, f"?pod={uuid.uuid4()}")
    assert r.status_code == 200, r.text
    assert r.json() == []

    r = _get(api, token, slug, identifier, "?pod=")
    assert sorted(_ids(r)) == sorted([in_default, in_second])

    r = _get(api, token, slug, identifier, "?pod=not-a-uuid")
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "Please provide valid detail"}


def test_bundled_runners_excluded_unless_requested(api, seed, db, tenant_a):
    _, identifier, pod_id = _project(seed, tenant_a)
    owner = tenant_a["user"]["id"]
    ws = tenant_a["workspace"]["id"]
    manual = seed.runner(owner, ws, pod_id)
    bundled = seed.runner(owner, ws, pod_id)
    _set(db, "runner", bundled, provisioning="desktop_bundled")
    token = _token(seed, tenant_a)
    slug = tenant_a["workspace"]["slug"]

    assert _ids(_get(api, token, slug, identifier)) == [manual]
    for flag in ("1", "true", "yes"):
        r = _get(api, token, slug, identifier, f"?include_bundled={flag}")
        assert sorted(_ids(r)) == sorted([manual, bundled]), flag
    for flag in ("True", "0", "false", ""):
        r = _get(api, token, slug, identifier, f"?include_bundled={flag}")
        assert _ids(r) == [manual], flag


def test_datetimes_render_in_user_timezone(api, seed, db, tenant_a):
    _, identifier, pod_id = _project(seed, tenant_a)
    runner_id = seed.runner(tenant_a["user"]["id"], tenant_a["workspace"]["id"], pod_id)
    _set(
        db,
        "runner",
        runner_id,
        last_heartbeat_at="2026-10-02T01:02:03+00:00",
        created_at="2026-10-01T12:00:00.5+00:00",
    )
    _set(db, "users", tenant_a["user"]["id"], user_timezone="Asia/Kolkata")
    token = _token(seed, tenant_a)

    r = _get(api, token, tenant_a["workspace"]["slug"], identifier)
    assert r.status_code == 200, r.text
    row = r.json()[0]
    assert row["last_heartbeat_at"] == "2026-10-02T06:32:03+05:30"
    assert row["created_at"] == "2026-10-01T17:30:00.500000+05:30"


def test_unknown_user_timezone_is_bad_request(api, seed, db, tenant_a):
    _, identifier, _ = _project(seed, tenant_a)
    _set(db, "users", tenant_a["user"]["id"], user_timezone="Not/AZone")
    r = _get(api, _token(seed, tenant_a), tenant_a["workspace"]["slug"], identifier)
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "The required key does not exist."}


def test_requires_api_key(api, seed, tenant_a):
    _, identifier, _ = _project(seed, tenant_a)
    slug = tenant_a["workspace"]["slug"]

    r = _get(api, None, slug, identifier)
    assert r.status_code == 401, r.text
    assert r.json() == {"detail": "Authentication credentials were not provided."}

    r = _get(api, "bogus", slug, identifier)
    assert r.status_code == 403, r.text
    assert r.json() == {"detail": "Given API token is not valid"}


def test_only_get_is_allowed(api, seed, tenant_a):
    _, identifier, _ = _project(seed, tenant_a)
    token = _token(seed, tenant_a)
    r = api.post(
        _url(tenant_a["workspace"]["slug"], identifier),
        headers={"X-Api-Key": token},
        json={},
    )
    assert r.status_code == 405, r.text
    assert r.json() == {"detail": 'Method "POST" not allowed.'}
