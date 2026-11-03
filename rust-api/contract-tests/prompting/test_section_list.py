"""Section-list endpoint (PIDASHCONV-18, D-04).

Route (apps/api/pi_dash/prompting/urls.py):
  GET /api/workspaces/<slug>/prompt-sections?kind=&scope=
"""

SECTION_KEYS = {
    "key", "title", "customizable", "body", "default_body", "source",
    "version", "needs_attention", "editable_at_workspace",
    "editable_at_personal",
}

# coding-task recipe order, pinned from prompting/recipes.py.
CODING_TASK_ORDER = (
    "intro", "repo-context", "relationships", "session-framing",
    "pidash-cli", "task-lifecycle", "default-posture", "autonomy",
    "state-routing", "analyze-and-scope", "workpad-setup",
    "implementation", "blocking", "guardrails", "workpad-template",
    "ending-run",
)


def _list_url(tenant, kind="coding-task", scope=None):
    url = f"/api/workspaces/{tenant['workspace']['slug']}/prompt-sections"
    q = f"?kind={kind}"
    if scope:
        q += f"&scope={scope}"
    return url + q


def test_list_shape(api, tenant_a, auth_a):
    r = api.get(_list_url(tenant_a), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["kind"] == "coding-task"
    assert body["scope"] == "user"
    sections = body["sections"]
    assert [s["key"] for s in sections] == list(CODING_TASK_ORDER)
    for s in sections:
        assert set(s) == SECTION_KEYS, sorted(set(s) ^ SECTION_KEYS)
        assert s["source"] == "default"
        assert s["version"] == 0
        assert s["needs_attention"] is False
        assert s["body"]
        assert s["default_body"]
        assert s["title"]


def test_list_scope_workspace(api, tenant_a, auth_a):
    r = api.get(_list_url(tenant_a, scope="workspace"), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["scope"] == "workspace"
    assert [s["key"] for s in body["sections"]] == list(CODING_TASK_ORDER)


def test_list_scheduler_kind(api, tenant_a, auth_a):
    r = api.get(_list_url(tenant_a, kind="scheduler"), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["kind"] == "scheduler"
    assert [s["key"] for s in body["sections"]] == [
        "scheduler-intro", "session-framing", "pidash-cli",
        "scheduler-task", "guardrails", "scheduler-ending",
    ]


def test_list_tier_capabilities(api, tenant_a, auth_a):
    """Every section carries its governance tier + capability flags."""
    r = api.get(_list_url(tenant_a), headers=auth_a)
    assert r.status_code == 200, r.text
    by_key = {s["key"]: s for s in r.json()["sections"]}
    locked = by_key["pidash-cli"]
    assert locked["customizable"] == "locked"
    assert locked["editable_at_workspace"] is False
    assert locked["editable_at_personal"] is False
    open_ = by_key["implementation"]
    assert open_["customizable"] == "overridable"
    assert open_["editable_at_workspace"] is True
    assert open_["editable_at_personal"] is True


def test_list_unknown_kind_400(api, tenant_a, auth_a):
    r = api.get(_list_url(tenant_a, kind="bogus"), headers=auth_a)
    assert r.status_code == 400, r.text
    assert "kinds" in r.json()


def test_list_unknown_workspace_404(api, auth_a):
    r = api.get(
        "/api/workspaces/no-such-workspace-ct18/prompt-sections",
        headers=auth_a,
    )
    assert r.status_code == 404, r.text


def test_list_anonymous_denied(api, tenant_a):
    """Removing IsAuthenticated would turn this 401 into a 200."""
    r = api.get(_list_url(tenant_a))
    assert r.status_code == 401, r.text


def test_list_outsider_forbidden(api, tenant_a, auth_b):
    """An authenticated user outside the workspace cannot read sections."""
    r = api.get(_list_url(tenant_a), headers=auth_b)
    assert r.status_code == 403, r.text


def test_list_tenant_isolation(api, tenant_a, tenant_b, auth_b):
    """Tenant B's credentials see nothing of tenant A's workspace."""
    r = api.get(_list_url(tenant_a), headers=auth_b)
    assert r.status_code == 403, r.text
    # ... and B's own workspace still resolves for B.
    r = api.get(_list_url(tenant_b), headers=auth_b)
    assert r.status_code == 200, r.text
