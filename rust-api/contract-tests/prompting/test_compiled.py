"""Compiled-template endpoint (PIDASHCONV-18, D-04).

Route (apps/api/pi_dash/prompting/urls.py):
  GET /api/workspaces/<slug>/prompts/<kind>/compiled?scope=

Also pins the installed-runner wire surface: the compiled coding-task
template carries the `pidash` CLI instructions (pidash-cli section),
which installed runners consume verbatim.
"""


def _compiled_url(tenant, kind, scope=None):
    url = (f"/api/workspaces/{tenant['workspace']['slug']}"
           f"/prompts/{kind}/compiled")
    return url + (f"?scope={scope}" if scope else "")


def test_compiled_shape(api, tenant_a, auth_a):
    r = api.get(_compiled_url(tenant_a, "coding-task"), headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["kind"] == "coding-task"
    assert body["scope"] == "user"
    assert "{{ issue.identifier }}" in body["template_body"]
    # The per-section breakdown lives on the section-list endpoint.
    assert "sections" not in body


def test_compiled_runner_wire_compat(api, tenant_a, auth_a):
    """Installed runners execute the compiled prompt: the CLI wire docs
    (pidash-cli section) must ship inside it byte-identically."""
    r = api.get(_compiled_url(tenant_a, "coding-task"), headers=auth_a)
    assert r.status_code == 200, r.text
    template = r.json()["template_body"]
    assert "pidash issue get" in template
    assert "pidash run yield" in template


def test_compiled_unknown_kind_400(api, tenant_a, auth_a):
    r = api.get(_compiled_url(tenant_a, "bogus"), headers=auth_a)
    assert r.status_code == 400, r.text


def test_compiled_unknown_workspace_404(api, auth_a):
    r = api.get(
        "/api/workspaces/no-such-workspace-ct18/prompts/coding-task/compiled",
        headers=auth_a,
    )
    assert r.status_code == 404, r.text


def test_compiled_anonymous_denied(api, tenant_a):
    r = api.get(_compiled_url(tenant_a, "coding-task"))
    assert r.status_code == 401, r.text


def test_compiled_outsider_forbidden(api, tenant_a, auth_b):
    r = api.get(_compiled_url(tenant_a, "coding-task"), headers=auth_b)
    assert r.status_code == 403, r.text


def test_compiled_tenant_isolation(api, tenant_a, tenant_b, auth_b):
    r = api.get(_compiled_url(tenant_a, "coding-task"), headers=auth_b)
    assert r.status_code == 403, r.text
    r = api.get(_compiled_url(tenant_b, "coding-task"), headers=auth_b)
    assert r.status_code == 200, r.text


def test_compiled_dual_compilation_with_user_override(api, seed, tenant_a,
                                                      auth_a):
    seed.override(tenant_a["workspace"]["id"], "implementation", "MINE ONLY",
                  user_id=tenant_a["user"]["id"])
    r = api.get(_compiled_url(tenant_a, "coding-task", scope="user"),
                headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert "MINE ONLY" in body["template_body"]
    assert "automatic_template_body" in body
    assert "MINE ONLY" not in body["automatic_template_body"]


def test_compiled_no_dual_without_user_override(api, tenant_a, auth_a):
    r = api.get(_compiled_url(tenant_a, "coding-task", scope="user"),
                headers=auth_a)
    assert r.status_code == 200, r.text
    assert "automatic_template_body" not in r.json()


def test_compiled_workspace_scope_ignores_user_override(api, seed, tenant_a,
                                                        auth_a):
    seed.override(tenant_a["workspace"]["id"], "implementation", "MINE ONLY",
                  user_id=tenant_a["user"]["id"])
    r = api.get(_compiled_url(tenant_a, "coding-task", scope="workspace"),
                headers=auth_a)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["scope"] == "workspace"
    assert "MINE ONLY" not in body["template_body"]
    assert "automatic_template_body" not in body
