"""Search endpoints + FTS parity (PIDASHCONV-87, D-29).

Routes (apps/api/pi_dash/app/urls/search.py):
  GET /api/workspaces/<slug>/search/                        (global search)
  GET /api/workspaces/<slug>/projects/<project_id>/search-issues/
  GET /api/workspaces/<slug>/entity-search/                 (mention picker)

FTS parity: the issue text search must keep using the issues_fts_idx GIN
index (expression: to_tsvector('english', name + description_stripped)).
The EXPLAIN test below pins the plan; the behavioral tests pin the results.
"""

import os

from conftest import session_headers

GLOBAL_RESULT_ENTITIES = {
    "workspace", "project", "issue", "cycle", "module", "issue_view",
    "page", "intake",
}

PROJECT_SEARCH_KEYS = {
    "name", "id", "start_date", "sequence_id", "project__name",
    "project__identifier", "project_id", "workspace__slug", "state__name",
    "state__group", "state__color",
}


def _search_url(tenant):
    return f"/api/workspaces/{tenant['workspace']['slug']}/search/"


def test_global_search_shape(api, seed, tenant_a, auth_a):
    r = api.get(_search_url(tenant_a), headers=auth_a, params={"search": "zzz-no-hit"})
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == {"results"}
    assert set(body["results"]) == GLOBAL_RESULT_ENTITIES, sorted(
        set(body["results"]) ^ GLOBAL_RESULT_ENTITIES
    )


def test_global_search_entities_param(api, seed, tenant_a, auth_a):
    r = api.get(
        _search_url(tenant_a), headers=auth_a,
        params={"search": "zzz-no-hit", "entities": "issue,project"},
    )
    assert r.status_code == 200, r.text
    assert set(r.json()["results"]) == {"issue", "project"}


def test_global_search_issue_body_match(api, seed, tenant_a, project_a, auth_a):
    """FTS over name + description_stripped surfaces the issue (reading
    material: test_global_search.test_issue_body_match_surfaces_issue)."""
    issue_id = seed.issue(
        tenant_a["workspace"]["id"], project_a,
        name="unrelated title",
        description="browser console has 38 same errors",
    )
    r = api.get(
        _search_url(tenant_a), headers=auth_a,
        params={
            "search": "38 same errors",
            "workspace_search": "true",
            "entities": "issue",
        },
    )
    assert r.status_code == 200, r.text
    ids = [row["id"] for row in r.json()["results"]["issue"]]
    assert issue_id in ids


def test_global_search_comment_match_surfaces_parent(
    api, seed, tenant_a, project_a, auth_a
):
    """Global search widens to comment text (include_comments=True)."""
    issue_id = seed.issue(
        tenant_a["workspace"]["id"], project_a,
        name="unrelated title", description="unrelated description",
    )
    seed.issue_comment(
        tenant_a["workspace"]["id"], project_a, issue_id,
        text="release blocker from upload retry loop",
        actor_id=tenant_a["user"]["id"],
    )
    r = api.get(
        _search_url(tenant_a), headers=auth_a,
        params={
            "search": "upload retry loop",
            "workspace_search": "true",
            "entities": "issue",
        },
    )
    assert r.status_code == 200, r.text
    ids = [row["id"] for row in r.json()["results"]["issue"]]
    assert issue_id in ids


def test_global_search_issue_row_shape(api, seed, tenant_a, project_a, auth_a):
    seed.issue(
        tenant_a["workspace"]["id"], project_a,
        name="shape probe issue", description="shape probe body",
    )
    r = api.get(
        _search_url(tenant_a), headers=auth_a,
        params={
            "search": "shape probe", "workspace_search": "true", "entities": "issue",
        },
    )
    assert r.status_code == 200, r.text
    rows = r.json()["results"]["issue"]
    assert rows, "expected the seeded issue in results"
    assert set(rows[0]) == {
        "name", "id", "sequence_id", "project__identifier", "project_id",
        "workspace__slug",
    }


def test_global_search_project_scoped(api, seed, tenant_a, project_a, auth_a):
    """workspace_search=false + project_id narrows issues to that project."""
    other_project = seed.project(tenant_a["workspace"]["id"])
    seed.project_member(
        other_project, tenant_a["workspace"]["id"], tenant_a["user"]["id"], role=20
    )
    seed.issue(
        tenant_a["workspace"]["id"], other_project,
        name="elsewhere entirely", description="elsewhere body token",
    )
    r = api.get(
        _search_url(tenant_a), headers=auth_a,
        params={
            "search": "elsewhere", "workspace_search": "false",
            "project_id": str(project_a), "entities": "issue",
        },
    )
    assert r.status_code == 200, r.text
    assert r.json()["results"]["issue"] == []


def test_project_search_shape(api, seed, tenant_a, project_a, auth_a):
    issue_id = seed.issue(
        tenant_a["workspace"]["id"], project_a,
        name="project search target", description="findable body",
    )
    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}"
        f"/projects/{project_a}/search-issues/",
        headers=auth_a,
        params={"search": "findable"},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert isinstance(body, list) and len(body) == 1
    assert set(body[0]) == PROJECT_SEARCH_KEYS, sorted(
        set(body[0]) ^ PROJECT_SEARCH_KEYS
    )
    assert body[0]["id"] == issue_id


def test_project_search_no_comment_widening(api, seed, tenant_a, project_a, auth_a):
    """search-issues keeps the legacy contract (title/sequence/project-code
    only): a comment-text match must NOT surface the issue."""
    issue_id = seed.issue(
        tenant_a["workspace"]["id"], project_a,
        name="comment-only title", description="comment-only description",
    )
    seed.issue_comment(
        tenant_a["workspace"]["id"], project_a, issue_id,
        text="quetzal zebra xylophone",
        actor_id=tenant_a["user"]["id"],
    )
    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}"
        f"/projects/{project_a}/search-issues/",
        headers=auth_a,
        params={"search": "quetzal zebra xylophone"},
    )
    assert r.status_code == 200, r.text
    assert r.json() == []


def test_project_search_sequence_id(api, seed, tenant_a, project_a, auth_a):
    issue_id = seed.issue(
        tenant_a["workspace"]["id"], project_a,
        name="sequence target", description="no digits here", sequence_id=424242,
    )
    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}"
        f"/projects/{project_a}/search-issues/",
        headers=auth_a,
        params={"search": "424242"},
    )
    assert r.status_code == 200, r.text
    assert [row["id"] for row in r.json()] == [issue_id]


def test_project_search_int4_overflow_query_ok(api, seed, tenant_a, project_a, auth_a):
    """A digit token above int4 max must not 500 the endpoint (regression
    guard from pi_dash.search.issue)."""
    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}"
        f"/projects/{project_a}/search-issues/",
        headers=auth_a,
        params={"search": "2147483648"},
    )
    assert r.status_code == 200, r.text
    assert r.json() == []


def test_project_search_parent_exclusion(api, seed, tenant_a, project_a, auth_a):
    """parent=true drops the issue itself, its parent and its children —
    an unrelated match (the sibling) still surfaces."""
    parent_id = seed.issue(
        tenant_a["workspace"]["id"], project_a, name="parent epic",
    )
    child_id = seed.issue(
        tenant_a["workspace"]["id"], project_a, name="parent epic child",
    )
    sibling_id = seed.issue(
        tenant_a["workspace"]["id"], project_a, name="parent epic sibling",
    )
    with seed.conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET parent_id = %s WHERE id = %s", (parent_id, child_id)
        )
    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}"
        f"/projects/{project_a}/search-issues/",
        headers=auth_a,
        params={"search": "parent epic", "parent": "true", "issue_id": str(parent_id)},
    )
    assert r.status_code == 200, r.text
    assert [row["id"] for row in r.json()] == [sibling_id]


def test_entity_search_user_mention(api, seed, tenant_a, project_a, auth_a):
    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}/entity-search/",
        headers=auth_a,
        params={
            "query": "Contract", "query_type": "user_mention",
            "project_id": str(project_a),
        },
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == {"user_mention"}
    assert body["user_mention"], "expected the seeded member in mentions"
    assert set(body["user_mention"][0]) == {
        "member__avatar_url", "member__display_name", "member__id",
    }


def test_entity_search_issue_shape(api, seed, tenant_a, project_a, auth_a):
    seed.issue(
        tenant_a["workspace"]["id"], project_a,
        name="entity picker issue", description="entity picker body",
    )
    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}/entity-search/",
        headers=auth_a,
        params={
            "query": "entity picker", "query_type": "issue",
            "project_id": str(project_a),
        },
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert set(body) == {"issue"}
    assert body["issue"], "expected the seeded issue"
    assert set(body["issue"][0]) == {
        "name", "id", "sequence_id", "project__identifier", "project_id",
        "priority", "state_id", "type_id",
    }


def test_entity_search_without_project(api, seed, tenant_a, auth_a):
    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}/entity-search/",
        headers=auth_a,
        params={"query": "Contract", "query_type": "user_mention,project"},
    )
    assert r.status_code == 200, r.text
    assert set(r.json()) == {"user_mention", "project"}


def test_search_requires_auth(api, tenant_a):
    r = api.get(_search_url(tenant_a), params={"search": "x"})
    assert r.status_code == 401, r.text


def test_search_tenant_isolation(api, seed, tenant_a, tenant_b, project_a, auth_b):
    """Tenant B searching A's workspace slug sees no issue rows."""
    seed.issue(
        tenant_a["workspace"]["id"], project_a,
        name="A secret issue", description="A secret body",
    )
    r = api.get(
        _search_url(tenant_a), headers=auth_b,
        params={
            "search": "secret", "workspace_search": "true", "entities": "issue",
        },
    )
    assert r.status_code == 200, r.text
    assert r.json()["results"]["issue"] == []


def test_fts_uses_issues_fts_idx(db, seed, tenant_a, project_a):
    """EXPLAIN parity: the FTS predicate Django emits for issue search
    (to_tsvector('english', name + description_stripped) against a
    websearch query) must plan through the issues_fts_idx GIN index —
    byte-for-byte the same expression the index is built on. If a port
    rewrites the expression (different config, COALESCE shape, extra
    columns), the planner drops the index and this fails. enable_seqscan
    is off so the plan proves index *usability* for exactly this
    expression rather than the planner's cost choice on a tiny table."""
    seed.issue(
        tenant_a["workspace"]["id"], project_a,
        name="parity probe", description="release blocker parity body",
    )
    with db.cursor() as cur:
        cur.execute("SET enable_seqscan = off")
        cur.execute(
            "EXPLAIN SELECT id FROM issues WHERE "
            "to_tsvector('english'::regconfig, "
            "COALESCE(name, '') || ' ' || COALESCE(description_stripped, '')) "
            "@@ websearch_to_tsquery('english'::regconfig, 'release blocker')"
        )
        plan = "\n".join(row[0] for row in cur.fetchall())
    assert "issues_fts_idx" in plan, plan
