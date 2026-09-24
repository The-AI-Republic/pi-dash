"""Section write endpoints (PIDASHCONV-18, D-04).

Routes (apps/api/pi_dash/prompting/urls.py):
  PUT    /api/workspaces/<slug>/prompt-sections/<key>?scope=
  DELETE /api/workspaces/<slug>/prompt-sections/<key>?scope=
"""

OVERRIDE_KEYS = {
    "id", "workspace", "user", "section_key", "body", "is_active",
    "version", "needs_attention", "is_workspace_level", "updated_by",
    "created_at", "updated_at",
}


def _detail_url(tenant, section_key):
    return (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/prompt-sections/{section_key}"
    )


def _list_url(tenant, scope="workspace"):
    return (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/prompt-sections?kind=coding-task&scope={scope}"
    )


def test_admin_put_workspace_override_shape(api, seed, tenant_a, auth_a, write_a):
    r = api.put(
        _detail_url(tenant_a, "implementation"),
        headers=write_a,
        json={"scope": "workspace", "body": "Custom workspace guidance."},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == OVERRIDE_KEYS, sorted(set(body) ^ OVERRIDE_KEYS)
    assert body["section_key"] == "implementation"
    assert body["body"] == "Custom workspace guidance."
    assert body["workspace"] == tenant_a["workspace"]["id"]
    assert body["user"] is None
    assert body["is_workspace_level"] is True
    assert body["is_active"] is True
    assert body["version"] == 1
    assert body["needs_attention"] is False
    # ... and the list now resolves the override.
    r = api.get(_list_url(tenant_a), headers=auth_a)
    assert r.status_code == 200, r.text
    impl = next(
        s for s in r.json()["sections"] if s["key"] == "implementation"
    )
    assert impl["source"] == "workspace"
    assert impl["body"] == "Custom workspace guidance."
    assert impl["version"] == 1


def test_put_bumps_version_in_place(api, seed, tenant_a, write_a):
    url = _detail_url(tenant_a, "implementation")
    r1 = api.put(url, headers=write_a,
                 json={"scope": "workspace", "body": "v1"})
    r2 = api.put(url, headers=write_a,
                 json={"scope": "workspace", "body": "v2"})
    assert r1.json()["version"] == 1
    assert r2.json()["version"] == 2
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT count(*) FROM prompt_section_override"
            " WHERE workspace_id = %s AND user_id IS NULL"
            " AND section_key = 'implementation' AND is_active",
            (tenant_a["workspace"]["id"],),
        )
        assert cur.fetchone()[0] == 1


def test_member_cannot_put_workspace_override(api, member_a, write_member):
    r = api.put(
        _detail_url(member_a, "implementation"),
        headers=write_member,
        json={"scope": "workspace", "body": "nope"},
    )
    assert r.status_code == 403, r.text


def test_member_can_put_own_user_override(api, seed, member_a, auth_member, write_member):
    r = api.put(
        _detail_url(member_a, "implementation"),
        headers=write_member,
        json={"scope": "user", "body": "My personal guidance."},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["user"] == member_a["user"]["id"]
    assert body["is_workspace_level"] is False
    with seed.conn.cursor() as cur:
        cur.execute(
            "SELECT count(*) FROM prompt_section_override"
            " WHERE workspace_id = %s AND user_id = %s"
            " AND section_key = 'implementation' AND is_active",
            (member_a["workspace"]["id"], member_a["user"]["id"]),
        )
        assert cur.fetchone()[0] == 1


def test_put_locked_section_forbidden_both_scopes(api, tenant_a, write_a):
    for scope in ("workspace", "user"):
        r = api.put(
            _detail_url(tenant_a, "pidash-cli"),
            headers=write_a,
            json={"scope": scope, "body": "hack the cli docs"},
        )
        assert r.status_code == 403, (scope, r.text)


def test_put_invalid_jinja_rejected(api, tenant_a, write_a):
    r = api.put(
        _detail_url(tenant_a, "implementation"),
        headers=write_a,
        json={"scope": "workspace", "body": "{% if x %}unclosed"},
    )
    assert r.status_code == 400, r.text


def test_put_unknown_variable_rejected(api, tenant_a, write_a):
    r = api.put(
        _detail_url(tenant_a, "implementation"),
        headers=write_a,
        json={"scope": "workspace", "body": "{{ totally_unknown }}"},
    )
    assert r.status_code == 400, r.text


def test_put_missing_body_400(api, tenant_a, write_a):
    r = api.put(
        _detail_url(tenant_a, "implementation"),
        headers=write_a,
        json={"scope": "workspace"},
    )
    assert r.status_code == 400, r.text


def test_put_unknown_section_404(api, tenant_a, write_a):
    r = api.put(
        _detail_url(tenant_a, "no-such-section"),
        headers=write_a,
        json={"scope": "workspace", "body": "x"},
    )
    assert r.status_code == 404, r.text


def test_put_anonymous_denied(api, tenant_a):
    r = api.put(
        _detail_url(tenant_a, "implementation"),
        json={"scope": "workspace", "body": "x"},
    )
    assert r.status_code == 401, r.text


def test_put_outsider_forbidden(api, tenant_a, write_b):
    """Tenant B cannot write tenant A's sections (tenant isolation)."""
    r = api.put(
        _detail_url(tenant_a, "implementation"),
        headers=write_b,
        json={"scope": "workspace", "body": "cross-tenant write"},
    )
    assert r.status_code == 403, r.text


def test_user_override_invisible_to_other_member(api, seed, tenant_a, auth_a,
                                                member_a, auth_member):
    """Personal overrides resolve per-user: A's row never leaks to a member."""
    seed.override(tenant_a["workspace"]["id"], "implementation",
                  "ADMIN-ONLY-MARKER", user_id=tenant_a["user"]["id"])
    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}"
        "/prompt-sections?kind=coding-task&scope=user",
        headers=auth_member,
    )
    assert r.status_code == 200, r.text
    impl = next(
        s for s in r.json()["sections"] if s["key"] == "implementation"
    )
    assert impl["source"] == "default"
    assert "ADMIN-ONLY-MARKER" not in impl["body"]


def test_delete_reverts_to_default(api, tenant_a, auth_a, write_a):
    url = _detail_url(tenant_a, "implementation")
    r = api.put(url, headers=write_a,
                json={"scope": "workspace", "body": "custom"})
    assert r.status_code == 200, r.text
    r = api.delete(url + "?scope=workspace", headers=write_a)
    assert r.status_code == 204, r.text
    r = api.get(_list_url(tenant_a), headers=auth_a)
    impl = next(
        s for s in r.json()["sections"] if s["key"] == "implementation"
    )
    assert impl["source"] == "default"


def test_delete_no_override_404(api, tenant_a, write_a):
    r = api.delete(
        _detail_url(tenant_a, "implementation") + "?scope=workspace",
        headers=write_a,
    )
    assert r.status_code == 404, r.text


def test_delete_outsider_forbidden(api, tenant_a, write_b):
    r = api.delete(
        _detail_url(tenant_a, "implementation") + "?scope=workspace",
        headers=write_b,
    )
    assert r.status_code == 403, r.text
