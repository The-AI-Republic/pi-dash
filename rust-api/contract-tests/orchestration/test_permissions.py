"""Permission floor for the engine surface: anonymous denied, tenant
isolation, project membership. If ProjectEntityPermission is removed, the
denied cases below fail — that is the deliberate-removal check."""

import httpx

from _harness import api, env, world


def _anon():
    return httpx.Client(base_url=env.BASE_URL, timeout=30)


def test_anonymous_patch_denied(api_client):
    w, _ = api_client
    issue = world.make_issue(w, "anon", "Backlog")

    r = _anon().patch(
        f"/api/v1/workspaces/{w.workspace_slug}/projects/{w.project_id}/work-items/{issue}/",
        json={"state": w.states["In Progress"]["id"]},
    )
    assert r.status_code in (401, 403), r.text


def test_anonymous_workpad_denied(api_client):
    w, _ = api_client
    issue = world.make_issue(w, "anonpad", "Backlog")

    r = _anon().get(
        f"/api/v1/workspaces/{w.workspace_slug}/projects/{w.project_id}/work-items/{issue}/workpad/"
    )
    assert r.status_code in (401, 403), r.text


def test_cross_workspace_isolation(api_client):
    w, _ = api_client
    issue = world.make_issue(w, "iso", "Backlog")

    other = world.build("outsider")
    other_client = api.Api(other.api_key)

    r = other_client.get_issue(w.workspace_slug, w.project_id, issue)
    assert r.status_code in (403, 404), r.text
    r = other_client.patch_issue(w.workspace_slug, w.project_id, issue,
                                 {"state": w.states["In Progress"]["id"]})
    assert r.status_code in (403, 404), r.text
    r = other_client.get_workpad(w.workspace_slug, w.project_id, issue)
    assert r.status_code in (403, 404), r.text


def test_workspace_member_without_project_role_denied(api_client):
    w, _ = api_client
    issue = world.make_issue(w, "norole", "Backlog")

    member = world.make_user(world.slug("ws-only") + "@example.com")
    world.add_workspace_member(w.workspace_id, member, world.ROLE_MEMBER)
    member_client = api.Api(world.make_token(member, w.workspace_id))

    r = member_client.patch_issue(w.workspace_slug, w.project_id, issue,
                                  {"state": w.states["In Progress"]["id"]})
    assert r.status_code in (403, 404), r.text
