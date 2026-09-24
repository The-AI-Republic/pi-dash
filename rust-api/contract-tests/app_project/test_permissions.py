"""Cross-cutting permission + tenant-isolation cases for D-25.

Coverage floor: at least one denied-permission case and one
tenant-isolation case per domain, plus the tripwire that fails when a
permission class is deliberately removed (project create is gated ONLY by
its ``allow_permission`` decorator, so deleting that line turns the 403
below into a 201).
"""

import httpx


def test_unauthenticated_list_401(world):
    user = world.user("noauth")
    ws = world.workspace(user, "noauthws")
    world.ws_member(ws, user, 20)
    resp = httpx.get(
        f"{world.base_url}/api/workspaces/{ws['slug']}/projects/", timeout=30)
    assert resp.status_code == 401
    assert resp.json() == {"detail": "Authentication credentials were not provided."}


def test_outsider_list_403(world):
    owner = world.user("owner")
    ws = world.workspace(owner, "closedws")
    world.ws_member(ws, owner, 20)
    outsider = world.user("outsider")
    outsider_client = world.client(outsider)
    resp = outsider_client.get(f"/api/workspaces/{ws['slug']}/projects/")
    assert resp.status_code == 403
    assert resp.json() == {"error": "You don't have the required permissions."}


def test_guest_cannot_create_project(world):
    # Tripwire: project create is gated ONLY by its allow_permission
    # decorator, so deleting that line turns this 403 into a 201.
    client, _, ws, project = world.full_stack()
    _, guest_client = world.add_member(ws, project, ws_role=5, proj_role=5,
                                       name="guestx")
    resp = guest_client.post(f"/api/workspaces/{ws['slug']}/projects/",
                             json={"name": "Guest Project", "identifier": "GUESTX"})
    assert resp.status_code == 403


def test_member_cannot_invite(world):
    # Invitations are admin-only; the decorator is the sole gate, so removing
    # it turns this 403 into a 500 (the known .delay bug path).
    client, _, ws, project = world.full_stack()
    _, member_client = world.add_member(ws, project, ws_role=15, proj_role=15,
                                        name="inviter")
    resp = member_client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/invitations/",
        json={"emails": [{"email": "x@example.com", "role": 5}]})
    assert resp.status_code == 403


def test_guest_cannot_create_state(world):
    client, _, ws, project = world.full_stack()
    _, guest_client = world.add_member(ws, project, ws_role=5, proj_role=5,
                                       name="stateguest")
    resp = guest_client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/",
        json={"name": "Guest State", "color": "#000", "group": "unstarted",
              "sequence": 1})
    assert resp.status_code == 403


def test_tenant_isolation_list(world):
    client_a, _, ws_a, project_a = world.full_stack()
    tenant = world.user("tenant")
    ws_b = world.workspace(tenant, "tenantws")
    world.ws_member(ws_b, tenant, 20)
    tenant_client = world.client(tenant)
    # Tenant admin sees only their own workspace: the other workspace 403s
    # at the workspace-membership gate instead of leaking rows.
    resp = tenant_client.get(f"/api/workspaces/{ws_a['slug']}/projects/")
    assert resp.status_code == 403
    resp = tenant_client.get(f"/api/workspaces/{ws_b['slug']}/projects/")
    assert resp.status_code == 200
    assert resp.json() == []


def test_tenant_isolation_retrieve(world):
    _, _, ws_a, project_a = world.full_stack()
    tenant = world.user("tenant2")
    ws_b = world.workspace(tenant, "tenantws2")
    world.ws_member(ws_b, tenant, 20)
    tenant_client = world.client(tenant)
    # The workspace-membership gate rejects before the project lookup,
    # so a cross-workspace retrieve is deterministically 403, never 404.
    resp = tenant_client.get(
        f"/api/workspaces/{ws_a['slug']}/projects/{project_a['id']}/")
    assert resp.status_code == 403
    assert resp.json() == {"error": "You don't have the required permissions."}


def test_secret_project_nonmember(world):
    # SECRET (network 0) project: a workspace member who is not a project
    # member gets 403; on a PUBLIC project the same call is 409.
    from _harness import seed as _seed
    owner = world.user("seowner")
    ws = world.workspace(owner, "secws")
    world.ws_member(ws, owner, 20)
    owner_client = world.client(owner)
    secret = _seed.project(world.conn, ws["id"], network=0)
    _seed.project_member(world.conn, secret["id"], ws["id"], owner["id"], role=20)
    stranger = world.user("stranger")
    world.ws_member(ws, stranger, 15)
    stranger_client = world.client(stranger)
    resp = stranger_client.get(
        f"/api/workspaces/{ws['slug']}/projects/{secret['id']}/")
    assert resp.status_code == 403
    assert resp.json() == {"error": "You do not have permission"}
    with world.conn.cursor() as cur:
        cur.execute("UPDATE projects SET network=2 WHERE id=%s", (str(secret["id"]),))
    world.conn.commit()
    resp = stranger_client.get(
        f"/api/workspaces/{ws['slug']}/projects/{secret['id']}/")
    assert resp.status_code == 409
    assert resp.json() == {"error": "You are not a member of this project"}
