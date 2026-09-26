"""Preview endpoint (PIDASHCONV-18, D-04).

Route (apps/api/pi_dash/prompting/urls.py):
  POST /api/workspaces/<slug>/prompts/<kind>/preview

Renders the composed prompt against a real issue (coding-task/review/
test) or scheduler binding (scheduler) without creating a run.
"""


def _preview_url(tenant, kind):
    return (f"/api/workspaces/{tenant['workspace']['slug']}"
            f"/prompts/{kind}/preview")


def _project_identifier(seed, project_id):
    with seed.conn.cursor() as cur:
        cur.execute("SELECT identifier FROM projects WHERE id = %s",
                    (project_id,))
        return cur.fetchone()[0]


def test_preview_issue_renders(api, seed, tenant_a, project_a, issue_a,
                               write_a):
    identifier = _project_identifier(seed, project_a)
    with seed.conn.cursor() as cur:
        cur.execute("SELECT sequence_id FROM issues WHERE id = %s",
                    (issue_a,))
        sequence_id = cur.fetchone()[0]
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": str(issue_a)})
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["kind"] == "coding-task"
    assert f"{identifier}-{sequence_id}" in body["prompt"]


def test_preview_issue_relationships(api, seed, tenant_a, project_a, write_a):
    """Blocked-by / blocking items render with open-state warnings."""
    state_id = seed.state(project_a, tenant_a["workspace"]["id"])
    issue_id = seed.issue(tenant_a["workspace"]["id"], project_a,
                          name="Handler", sequence_id=11, state_id=state_id)
    blocker_id = seed.issue(tenant_a["workspace"]["id"], project_a,
                            name="Model layer", sequence_id=12,
                            state_id=state_id)
    dependent_id = seed.issue(tenant_a["workspace"]["id"], project_a,
                              name="Follow-up", sequence_id=13,
                              state_id=state_id)
    ws = tenant_a["workspace"]["id"]
    seed.issue_relation(ws, project_a, issue_id, blocker_id)
    seed.issue_relation(ws, project_a, dependent_id, issue_id)
    identifier = _project_identifier(seed, project_a)
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": str(issue_id)})
    assert r.status_code == 200, r.text
    prompt = r.json()["prompt"]
    assert "Blocked by (must be done first):" in prompt
    assert f"{identifier}-12: Model layer (Todo)" in prompt
    assert f"- Warning: {identifier}-12 is still open" in prompt
    assert "Blocking (waiting on this item):" in prompt
    assert f"{identifier}-13: Follow-up (Todo)" in prompt


def test_preview_review_kind_against_issue(api, tenant_a, issue_a, write_a):
    r = api.post(_preview_url(tenant_a, "review"), headers=write_a,
                 json={"issue_id": str(issue_a)})
    assert r.status_code == 200, r.text
    assert "reviewing the work product" in r.json()["prompt"]


def test_preview_test_kind_shows_project_block(api, seed, tenant_a,
                                                project_a, write_a):
    with seed.conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET description = %s WHERE id = %s",
            ("Reject stubs; run two independent review passes.", project_a),
        )
    state_id = seed.state(project_a, tenant_a["workspace"]["id"],
                          name="In Test", group="test")
    issue_id = seed.issue(tenant_a["workspace"]["id"], project_a,
                          name="Port the parser", state_id=state_id)
    r = api.post(_preview_url(tenant_a, "test"), headers=write_a,
                 json={"issue_id": str(issue_id)})
    assert r.status_code == 200, r.text
    prompt = r.json()["prompt"]
    assert "Reject stubs; run two independent review passes." in prompt
    assert "Project-level instructions in the description apply to this test pass" in prompt


def test_preview_scheduler_renders(api, tenant_a, binding_a, write_a):
    r = api.post(_preview_url(tenant_a, "scheduler"), headers=write_a,
                 json={"binding_id": str(binding_a)})
    assert r.status_code == 200, r.text
    assert "Scan repo." in r.json()["prompt"]


def test_preview_missing_issue_id_400(api, tenant_a, write_a):
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={})
    assert r.status_code == 400, r.text


def test_preview_scheduler_without_binding_id_400(api, tenant_a, write_a):
    r = api.post(_preview_url(tenant_a, "scheduler"), headers=write_a,
                 json={})
    assert r.status_code == 400, r.text


def test_preview_unknown_issue_404(api, tenant_a, write_a):
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": "00000000-0000-0000-0000-000000000000"})
    assert r.status_code == 404, r.text


def test_preview_cross_workspace_issue_404(api, seed, tenant_a, tenant_b,
                                           project_a, write_a):
    """An issue from another workspace is not addressable here (isolation)."""
    project_b = seed.project(tenant_b["workspace"]["id"])
    issue_b = seed.issue(tenant_b["workspace"]["id"], project_b)
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": str(issue_b)})
    assert r.status_code == 404, r.text


def test_preview_unknown_binding_404(api, tenant_a, write_a):
    r = api.post(_preview_url(tenant_a, "scheduler"), headers=write_a,
                 json={"binding_id": "00000000-0000-0000-0000-000000000000"})
    assert r.status_code == 404, r.text


def test_preview_unknown_kind_400(api, tenant_a, issue_a, write_a):
    r = api.post(_preview_url(tenant_a, "bogus"), headers=write_a,
                 json={"issue_id": str(issue_a)})
    assert r.status_code == 400, r.text


def test_preview_workspace_scope_forbidden_for_member(api, member_a,
                                                     write_member, seed,
                                                     tenant_a, project_a):
    """Default scope is workspace: a non-admin member may not preview it."""
    state_id = seed.state(project_a, tenant_a["workspace"]["id"])
    issue_id = seed.issue(tenant_a["workspace"]["id"], project_a,
                          state_id=state_id)
    r = api.post(_preview_url(member_a, "coding-task"), headers=write_member,
                 json={"issue_id": str(issue_id)})
    assert r.status_code == 403, r.text


def test_preview_user_scope_allowed_for_member(api, member_a, write_member,
                                               seed, tenant_a, project_a):
    state_id = seed.state(project_a, tenant_a["workspace"]["id"])
    issue_id = seed.issue(tenant_a["workspace"]["id"], project_a,
                          state_id=state_id)
    r = api.post(_preview_url(member_a, "coding-task"), headers=write_member,
                 json={"issue_id": str(issue_id), "scope": "user"})
    assert r.status_code == 200, r.text
    assert r.json()["prompt"]


def test_preview_anonymous_denied(api, tenant_a, issue_a):
    r = api.post(_preview_url(tenant_a, "coding-task"),
                 json={"issue_id": str(issue_a)})
    assert r.status_code == 401, r.text


def test_preview_outsider_forbidden(api, tenant_a, issue_a, write_b):
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_b,
                 json={"issue_id": str(issue_a)})
    assert r.status_code == 403, r.text


def test_preview_draft_renders_overridable_section(api, tenant_a, issue_a,
                                                   write_a):
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": str(issue_a),
                       "section_key": "implementation",
                       "body": "DRAFT-PREVIEW-MARKER"})
    assert r.status_code == 200, r.text
    assert "DRAFT-PREVIEW-MARKER" in r.json()["prompt"]


def test_preview_draft_locked_section_forbidden(api, tenant_a, issue_a,
                                                write_a):
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": str(issue_a), "section_key": "pidash-cli",
                       "body": "SHOULD-BE-BLOCKED"})
    assert r.status_code == 403, r.text


def test_preview_draft_missing_body_400(api, tenant_a, issue_a, write_a):
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": str(issue_a),
                       "section_key": "implementation"})
    assert r.status_code == 400, r.text


def test_preview_draft_section_not_in_recipe_400(api, tenant_a, issue_a,
                                                 write_a):
    r = api.post(_preview_url(tenant_a, "coding-task"), headers=write_a,
                 json={"issue_id": str(issue_a),
                       "section_key": "scheduler-task",
                       "body": "not part of this kind"})
    assert r.status_code == 400, r.text
