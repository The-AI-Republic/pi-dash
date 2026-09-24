"""Contract tests: api-v1 member endpoints (urls/member.py, 5 URL entries).

Covers workspace member list, project member list/create (both the
`members/` and `project-members/` aliases), member detail get/patch/delete,
one denied case and one tenant-isolation case.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402

KEY = "members"

USER_KEYS = {"avatar", "avatar_url", "display_name", "email", "first_name", "id", "last_name"}


def proj_base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects/{seed['project']['id']}"


def test_workspace_members_list(seed):
    body = http.get(
        seed["keys"][KEY],
        f"/api/v1/workspaces/{seed['ws_a']['slug']}/members/").json()
    assert isinstance(body, list)
    by_id = {u["id"]: u for u in body}
    assert seed["owner"]["id"] in by_id
    assert seed["member"]["id"] in by_id
    assert seed["outsider"]["id"] not in by_id
    owner = by_id[seed["owner"]["id"]]
    assert set(owner.keys()) == USER_KEYS | {"role"}
    assert owner["role"] == db.ADMIN


def test_project_members_list_aliases(seed):
    first = http.get(seed["keys"][KEY], proj_base(seed) + "/members/").json()
    second = http.get(seed["keys"][KEY], proj_base(seed) + "/project-members/").json()
    assert isinstance(first, list)
    assert {u["id"] for u in first} == {u["id"] for u in second}
    assert {u["id"] for u in first} >= {seed["owner"]["id"], seed["member"]["id"]}
    assert all(set(u.keys()) == USER_KEYS for u in first)


def test_project_members_list_by_slug(seed):
    body = http.get(
        seed["keys"][KEY],
        f"/api/v1/workspaces/{seed['ws_a']['slug']}"
        f"/projects/{seed['project']['identifier']}/members/").json()
    assert {u["id"] for u in body} >= {seed["owner"]["id"], seed["member"]["id"]}


def test_create_member(seed, conn):
    tag = db.new_tag()
    user = db.create_user(conn, tag, first_name="CtNew")
    db.add_workspace_member(conn, seed["ws_a"]["id"], user["id"], db.MEMBER)
    body = http.post(seed["keys"][KEY], proj_base(seed) + "/members/",
                     json={"member": user["id"], "role": db.MEMBER},
                     expect=201).json()
    assert set(body.keys()) == {"id", "member", "role"}
    assert body["member"] == user["id"]
    assert body["role"] == db.MEMBER
    # Visible in the list afterwards.
    listed = http.get(seed["keys"][KEY], proj_base(seed) + "/members/").json()
    assert user["id"] in {u["id"] for u in listed}


def test_create_member_validation(seed, conn):
    tag = db.new_tag()
    user = db.create_user(conn, tag, first_name="CtBad")
    db.add_workspace_member(conn, seed["ws_a"]["id"], user["id"], db.MEMBER)
    # Unknown role.
    r = http.post(seed["keys"][KEY], proj_base(seed) + "/members/",
                  json={"member": user["id"], "role": 99}, expect=400)
    assert "role" in r.json()
    # User exists but is not in the workspace.
    r = http.post(seed["keys"][KEY], proj_base(seed) + "/members/",
                  json={"member": seed["outsider"]["id"], "role": db.MEMBER},
                  expect=400)
    assert "member" in r.json()
    # Missing member.
    r = http.post(seed["keys"][KEY], proj_base(seed) + "/members/",
                  json={"role": db.MEMBER}, expect=400)
    assert "member" in r.json()


def test_detail_get_patch_delete(seed, conn):
    tag = db.new_tag()
    user = db.create_user(conn, tag, first_name="CtTmp")
    db.add_workspace_member(conn, seed["ws_a"]["id"], user["id"], db.MEMBER)
    created = http.post(seed["keys"][KEY], proj_base(seed) + "/members/",
                        json={"member": user["id"], "role": db.MEMBER},
                        expect=201).json()
    mid = created["id"]
    # Detail GET returns the user profile, not the membership row.
    got = http.get(seed["keys"][KEY], f"{proj_base(seed)}/members/{mid}/").json()
    assert set(got.keys()) == USER_KEYS
    assert got["id"] == user["id"]
    # PATCH changes the role.
    patched = http.patch(seed["keys"][KEY], f"{proj_base(seed)}/members/{mid}/",
                         json={"role": db.GUEST}).json()
    assert patched["role"] == db.GUEST
    # DELETE is a soft delete (is_active flips, row stays).
    http.delete(seed["keys"][KEY], f"{proj_base(seed)}/members/{mid}/")
    row = db.fetch_one(conn, "SELECT is_active FROM project_members WHERE id = %s", (mid,))
    assert row["is_active"] is False


def test_denied_guest_patch(seed, conn):
    # guest is a workspace MEMBER but holds no project membership: non-safe
    # project-member methods require a project ADMIN or MEMBER row -> 403.
    # Permission runs before lookup, so use a real membership id to prove the
    # denial is about the role, not the row.
    row = db.fetch_one(
        conn,
        "SELECT id FROM project_members WHERE project_id = %s AND is_active = true LIMIT 1",
        (seed["project"]["id"],),
    )
    http.patch(seed["keys"]["guest"], f"{proj_base(seed)}/members/{row['id']}/",
               json={"role": db.GUEST}, expect=403)


def test_denied_outsider(seed):
    http.get(seed["keys"]["outsider"], proj_base(seed) + "/members/", expect=403)
    http.post(seed["keys"]["outsider"], proj_base(seed) + "/members/",
              json={"member": seed["guest"]["id"], "role": db.MEMBER}, expect=403)


def test_isolation_other_workspace(seed):
    # owner_b sees only ws_b membership through ws_b's slug ...
    body = http.get(seed["keys"]["owner_b"],
                    f"/api/v1/workspaces/{seed['ws_b']['slug']}/members/").json()
    assert {u["id"] for u in body} == {seed["owner_b"]["id"]}
    # ... and cannot reach ws_a's project members at all.
    http.get(seed["keys"]["owner_b"], proj_base(seed) + "/members/", expect=403)
