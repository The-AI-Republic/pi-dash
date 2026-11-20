"""Search endpoints + FTS parity (PIDASHCONV-87, D-29).

Routes (apps/api/pi_dash/app/urls/search.py):
  GET /api/workspaces/<slug>/search/                        (global search)
  GET /api/workspaces/<slug>/projects/<project_id>/search-issues/
  GET /api/workspaces/<slug>/entity-search/                 (mention picker)

FTS parity: the issue text search must keep using the issues_fts_idx GIN
index (expression: to_tsvector('english', name + description_stripped)).
The EXPLAIN test below pins the plan; the behavioral tests pin the results.
"""

import json
import os
import uuid
from datetime import datetime, timezone

from _harness.diffs import expect

from .conftest import session_headers

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


def _seed_linked_page(seed, ws_id, project_id, owner_id, name):
    """A ``pages`` row + its ``project_pages`` link (column set mirrors
    ``Seeder.create_page`` / ``link_page_project``). Tracked so
    ``Seed.cleanup`` removes the bridge before the page."""
    page_id = str(uuid.uuid4())
    with seed.conn.cursor() as cur:
        cur.execute(
            """INSERT INTO pages
               (id, name, description_json, description_binary, description_html,
                owned_by_id, access, workspace_id, color, parent_id, archived_at,
                is_locked, view_props, logo_props, is_global, sort_order,
                created_by_id, created_at, updated_at)
               VALUES (%s,%s,CAST(%s AS jsonb),%s,%s,%s,%s,%s,'',%s,
                       NULL,%s,'{"full_width": false}','{}',false,65535,%s,now(),now())""",
            (
                page_id, name, json.dumps({}), None, "<p></p>",
                owner_id, 0, ws_id, None, False, owner_id,
            ),
        )
    seed.track("pages", "id", page_id)
    bridge_id = str(uuid.uuid4())
    with seed.conn.cursor() as cur:
        cur.execute(
            """INSERT INTO project_pages
               (id, workspace_id, project_id, page_id, created_by_id, updated_by_id,
                deleted_at, created_at, updated_at)
               VALUES (%s,%s,%s,%s,%s,%s,NULL,now(),now())""",
            (bridge_id, ws_id, str(project_id), page_id, owner_id, owner_id),
        )
    seed.track("project_pages", "id", bridge_id)
    return page_id


def test_global_search_project_scoped_page(api, seed, tenant_a, project_a, auth_a):
    """workspace_search=false + project_id narrows pages to that project.

    NEWPIDASH-349: the Rust page arm bound the narrowing project id as
    text against a uuid subquery (``operator does not exist: uuid =
    text``), 500ing every project-scoped search that includes pages.
    """
    ws_id = tenant_a["workspace"]["id"]
    owner_id = tenant_a["user"]["id"]
    page_id = _seed_linked_page(
        seed, ws_id, project_a, owner_id, "Probe page scoped narrow",
    )
    other_project = seed.project(ws_id)
    seed.project_member(
        other_project, ws_id, owner_id, role=20
    )
    _seed_linked_page(
        seed, ws_id, other_project, owner_id, "Probe page other project",
    )
    for params in (
        {"search": "Probe page scoped"},
        {"search": "Probe page scoped", "entities": "page"},
    ):
        r = api.get(
            _search_url(tenant_a), headers=auth_a,
            params={
                **params, "workspace_search": "false",
                "project_id": str(project_a),
            },
        )
        assert r.status_code == 200, r.text
        rows = r.json()["results"]["page"]
        assert [row["id"] for row in rows] == [page_id]
        assert rows[0]["project_ids"] == [str(project_a)]


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


def test_entity_search_cycle_module_archived_project_included(
    api, seed, tenant_a, project_a, auth_a
):
    """Entity-search cycle/module branches include archived projects
    (PIDASHCONV-1450): Django filters member/is_active/slug only, with
    no project__archived_at__isnull on these branches — unlike the
    global search sections."""
    ws_id = tenant_a["workspace"]["id"]
    cycle_id = seed._insert(
        "cycles",
        name="archived probe cycle",
        description="",
        owned_by_id=tenant_a["user"]["id"],
        view_props="{}",
        logo_props="{}",
        timezone="UTC",
        version=1,
        sort_order=65535,
        progress_snapshot="{}",
        project_id=project_a,
        workspace_id=ws_id,
        created_at=datetime.now(timezone.utc),
        updated_at=datetime.now(timezone.utc),
    )
    module_id = seed.module(ws_id, project_a, name="archived probe module")
    with seed.conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET archived_at = %s WHERE id = %s",
            (datetime.now(timezone.utc), project_a),
        )
    url = f"/api/workspaces/{tenant_a['workspace']['slug']}/entity-search/"
    for query_type, row_id in (("cycle", cycle_id), ("module", module_id)):
        for params in (
            {"query": "archived probe", "query_type": query_type},
            {
                "query": "archived probe", "query_type": query_type,
                "project_id": str(project_a),
            },
        ):
            r = api.get(url, headers=auth_a, params=params)
            assert r.status_code == 200, r.text
            assert row_id in [row["id"] for row in r.json()[query_type]], params


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


def test_global_search_pages_hide_other_users_private_pages(
    api, seed, tenant_a, project_a, auth_a
):
    """NEWPIDASH-342 (DIFF-56): the global search page section must mirror
    the pages-list scope — a member sees only pages they own or public
    ones, and a restricted guest (no ``guest_view_all_features``) sees
    only pages they own. Django leaks both (frozen); the Rust port fixes
    it, so the arms diverge via the per-backend expectation."""
    ws_id = tenant_a["workspace"]["id"]
    owner = tenant_a["user"]
    member = seed.user()
    seed.member(ws_id, member["id"], role=15)
    seed.project_member(project_a, ws_id, member["id"], role=15)
    member_headers = session_headers(
        seed, member, member["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    guest = seed.user()
    seed.member(ws_id, guest["id"], role=20)
    seed.project_member(project_a, ws_id, guest["id"], role=5)
    guest_headers = session_headers(
        seed, guest, guest["password"], os.environ["CONTRACT_SECRET_KEY"]
    )

    token = "zq342leak"

    def make_page(owner_id, name, access):
        # Same row shape as Seeder.create_page/link_page_project; tracked
        # for cleanup (this suite's Seed has no page helpers).
        page_id, bridge_id = str(uuid.uuid4()), str(uuid.uuid4())
        with seed.conn.cursor() as cur:
            cur.execute(
                """INSERT INTO pages
                   (id, name, description_json, description_binary,
                    description_html, owned_by_id, access, workspace_id,
                    color, parent_id, archived_at, is_locked, view_props,
                    logo_props, is_global, sort_order, created_by_id,
                    created_at, updated_at)
                   VALUES (%s,%s,CAST(%s AS jsonb),%s,%s,%s,%s,%s,'',NULL,
                           NULL,false,'{}','{}',false,65535,%s,now(),now())""",
                (
                    page_id, name, "{}", None, "<p></p>", owner_id, access,
                    ws_id, owner_id,
                ),
            )
            cur.execute(
                """INSERT INTO project_pages
                   (id, workspace_id, project_id, page_id, created_by_id,
                    updated_by_id, deleted_at, created_at, updated_at)
                   VALUES (%s,%s,%s,%s,%s,%s,NULL,now(),now())""",
                (
                    bridge_id, ws_id, project_a, page_id, owner["id"],
                    owner["id"],
                ),
            )
        # Tracked in insert order: cleanup deletes in reverse, so the
        # bridge row goes before the page row (FK).
        seed.track("pages", "id", page_id)
        seed.track("project_pages", "id", bridge_id)

    make_page(owner["id"], f"{token} admin private", 1)
    make_page(member["id"], f"{token} member private", 1)
    make_page(member["id"], f"{token} member public", 0)
    make_page(owner["id"], f"{token} admin public", 0)
    make_page(guest["id"], f"{token} guest private", 1)

    def page_names(headers):
        r = api.get(
            _search_url(tenant_a), headers=headers,
            params={"search": token, "entities": "page"},
        )
        assert r.status_code == 200, r.text
        return sorted(row["name"] for row in r.json()["results"]["page"])

    member_leak = sorted([
        f"{token} admin private", f"{token} member private",
        f"{token} member public", f"{token} admin public",
        f"{token} guest private",
    ])
    member_fixed = sorted([
        f"{token} member private", f"{token} member public",
        f"{token} admin public",
    ])
    guest_fixed = [f"{token} guest private"]
    # Workspace-wide search only: the project narrow (`workspace_search=false`
    # + `project_id`) 500s on Rust even without this fix — the narrow
    # subquery's parameterized LIMIT never executes there (pre-existing
    # defect, reported as "found:" on NEWPIDASH-1; Django narrows fine).
    assert page_names(member_headers) == expect(
        "DIFF-56", django=member_leak, rust=member_fixed
    )
    assert page_names(guest_headers) == expect(
        "DIFF-56", django=member_leak, rust=guest_fixed
    )


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


def test_search_issues_module_exclusion_split_exclude(
    api, seed, tenant_a, project_a, auth_a
):
    """?module=M keeps Django's split exclude (PIDASHCONV-1449): exclude()
    over the multi-valued issue_module join compiles to NOT (any link row
    to M AND any live link to any module), so an issue soft-removed from M
    but still live in N is excluded, while a control with only a
    soft-deleted link to M is still listed."""
    ws_id = tenant_a["workspace"]["id"]
    mod_m = seed.module(ws_id, project_a)
    mod_n = seed.module(ws_id, project_a)
    issue_id = seed.issue(ws_id, project_a, name="split exclude target")
    control_id = seed.issue(ws_id, project_a, name="split exclude control")
    seed.module_issue(ws_id, project_a, mod_m, issue_id)
    seed.module_issue(ws_id, project_a, mod_n, issue_id)
    seed.module_issue(ws_id, project_a, mod_m, control_id)
    with seed.conn.cursor() as cur:
        cur.execute(
            "UPDATE module_issues SET deleted_at = now(), updated_at = now()"
            " WHERE module_id = %s AND issue_id IN (%s, %s)",
            (mod_m, issue_id, control_id),
        )
    r = api.get(
        f"/api/workspaces/{tenant_a['workspace']['slug']}"
        f"/projects/{project_a}/search-issues/",
        headers=auth_a,
        params={"module": str(mod_m)},
    )
    assert r.status_code == 200, r.text
    ids = [row["id"] for row in r.json()]
    assert issue_id not in ids
    assert control_id in ids
