"""Contract tests: api-v1 member endpoints (urls/member.py, 5 URL entries).

Covers workspace member list, project member list/create (both the
`members/` and `project-members/` aliases), member detail get/patch/delete,
one denied case and one tenant-isolation case. Non-canonical UUID detail
spellings 404 on both aliases (PIDASHCONV-1393).
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402
from _harness.diffs import expect, is_rust  # noqa: E402

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


def test_null_email_member_shapes(seed, conn):
    # users.email is nullable (db/models/user.py:61); the serializer renders
    # null — a NULL row must not 500 any UserLite path (PIDASHCONV-513).
    tag = db.new_tag()
    user = db.create_user(conn, tag, first_name="CtNull", email=None)
    assert user["email"] is None
    db.add_workspace_member(conn, seed["ws_a"]["id"], user["id"], db.MEMBER)

    ws_body = http.get(
        seed["keys"][KEY],
        f"/api/v1/workspaces/{seed['ws_a']['slug']}/members/").json()
    ws_entry = next(u for u in ws_body if u["id"] == user["id"])
    assert set(ws_entry.keys()) == USER_KEYS | {"role"}
    assert ws_entry["email"] is None
    assert ws_entry["role"] == db.MEMBER

    created = http.post(seed["keys"][KEY], proj_base(seed) + "/members/",
                        json={"member": user["id"], "role": db.MEMBER},
                        expect=201).json()
    for suffix in ("/members/", "/project-members/"):
        listed = http.get(seed["keys"][KEY], proj_base(seed) + suffix).json()
        entry = next(u for u in listed if u["id"] == user["id"])
        assert set(entry.keys()) == USER_KEYS
        assert entry["email"] is None
    # Detail GET returns the user profile on both aliases.
    for suffix in ("/members/", "/project-members/"):
        got = http.get(seed["keys"][KEY],
                       f"{proj_base(seed)}{suffix}{created['id']}/").json()
        assert set(got.keys()) == USER_KEYS
        assert got["id"] == user["id"]
        assert got["email"] is None


def _spellings(canonical):
    # Every spelling `uuid.UUID()` (and Rust's `Uuid::parse_str`) accepts
    # but Django's `<uuid:...>` converter (`[0-9a-f]{8}-...`, matched
    # case-sensitively) does not route (PIDASHCONV-1393).
    return {
        "uppercase": canonical.upper(),
        "simple": canonical.replace("-", ""),
        "braced": "{" + canonical + "}",
    }


def _assert_resolver_404(resp):
    # Routing precedes auth and the view entirely: the answer is the
    # resolver's 404, never the view's body or a 401. The harness serves
    # Django with `pi_dash.settings.test` (`DEBUG = True`), so the
    # resolver miss is Django's technical HTML page on both backends
    # (PIDASHCONV-1193 note); the production JSON handler404 only
    # appears with `DEBUG = False` and is unreachable here.
    assert resp.status_code == 404, resp.text[:400]
    assert resp.headers.get("content-type", "").startswith("text/html"), \
        resp.headers.get("content-type")


def test_detail_non_canonical_spelling_404s(seed, conn):
    # `members/<uuid:pk>/` and `project-members/<uuid:pk>/`
    # (`member.py:21,31`): uppercase, plain-hex and braced spellings of a
    # real membership id resolver-404 on GET, PATCH and DELETE — for
    # anonymous callers too — and change nothing.
    tag = db.new_tag()
    user = db.create_user(conn, tag, first_name="CtSpell")
    db.add_workspace_member(conn, seed["ws_a"]["id"], user["id"], db.MEMBER)
    created = http.post(seed["keys"][KEY], proj_base(seed) + "/members/",
                        json={"member": user["id"], "role": db.MEMBER},
                        expect=201).json()
    mid = created["id"]
    for suffix in ("/members/", "/project-members/"):
        for segment in _spellings(mid).values():
            url = f"{proj_base(seed)}{suffix}{segment}/"
            _assert_resolver_404(http.get(None, url, expect=404))
            _assert_resolver_404(http.get(seed["keys"][KEY], url, expect=404))
            _assert_resolver_404(http.patch(
                seed["keys"][KEY], url, json={"role": db.GUEST}, expect=404))
            _assert_resolver_404(http.delete(seed["keys"][KEY], url, expect=404))
    row = db.fetch_one(
        conn, "SELECT role, is_active FROM project_members WHERE id = %s", (mid,))
    assert row["role"] == db.MEMBER
    assert row["is_active"] is True
    # Canonical control: the gate must not reject the spelling Django routes.
    got = http.get(seed["keys"][KEY], f"{proj_base(seed)}/members/{mid}/").json()
    assert got["id"] == user["id"]


CLASS_DENIAL = {"detail": "You do not have permission to perform this action."}
WS_ADMIN_TARGET = {"error": "You cannot update the role of a workspace admin"}
SELF_ROLE = {"error": "You cannot update your own role"}
ASSIGN_GUARD = {"error": "You cannot assign a role equal to or higher than your own"}
EQUAL_TARGET = {"error": "You cannot update the role of a member with a role equal to or higher than your own"}
GUEST_CAP = {"error": "You cannot add a user with role higher than the workspace role"}


def _seed_matrix(seed, conn):
    """Fresh NEWPIDASH-264 role matrix in the shared seed project.

    proj_admin/ws_admin/member/ws_guest/proj_admin2 cover the app
    `test_project_member_role_app.py` standings (ws MEMBER+proj ADMIN,
    ws ADMIN+proj MEMBER, ws MEMBER+proj MEMBER, ws GUEST+proj GUEST,
    second proj ADMIN for the equal-target guard). Per-test fresh rows
    so Django's vulnerable writes never pollute another case.
    """
    tag = db.new_tag()
    proj_admin = db.create_user(conn, tag + "pa", first_name="NEProjAdm")
    ws_admin = db.create_user(conn, tag + "wa", first_name="NEWsAdm")
    member_u = db.create_user(conn, tag + "mb", first_name="NEMember")
    ws_guest = db.create_user(conn, tag + "wg", first_name="NEWsGuest")
    proj_admin2 = db.create_user(conn, tag + "pb", first_name="NEProjAdm2")
    db.add_workspace_member(conn, seed["ws_a"]["id"], proj_admin["id"], db.MEMBER)
    db.add_workspace_member(conn, seed["ws_a"]["id"], ws_admin["id"], db.ADMIN)
    db.add_workspace_member(conn, seed["ws_a"]["id"], member_u["id"], db.MEMBER)
    db.add_workspace_member(conn, seed["ws_a"]["id"], ws_guest["id"], db.GUEST)
    db.add_workspace_member(conn, seed["ws_a"]["id"], proj_admin2["id"], db.MEMBER)
    m_pa = db.add_project_member(conn, seed["ws_a"]["id"], seed["project"]["id"], proj_admin["id"], db.ADMIN)
    m_wa = db.add_project_member(conn, seed["ws_a"]["id"], seed["project"]["id"], ws_admin["id"], db.MEMBER)
    m_mb = db.add_project_member(conn, seed["ws_a"]["id"], seed["project"]["id"], member_u["id"], db.MEMBER)
    m_wg = db.add_project_member(conn, seed["ws_a"]["id"], seed["project"]["id"], ws_guest["id"], db.GUEST)
    m_pb = db.add_project_member(conn, seed["ws_a"]["id"], seed["project"]["id"], proj_admin2["id"], db.ADMIN)
    owner_row = db.fetch_one(
        conn,
        "SELECT id FROM project_members WHERE project_id = %s AND member_id = %s AND is_active = true",
        (seed["project"]["id"], seed["owner"]["id"]),
    )
    return {
        "pa_key": db.create_api_token(conn, proj_admin["id"], tag + "pa")["token"],
        "wa_key": db.create_api_token(conn, ws_admin["id"], tag + "wa")["token"],
        "m_pa": m_pa,
        "m_wa": m_wa,
        "m_mb": m_mb,
        "m_wg": m_wg,
        "m_pb": m_pb,
        "owner_mid": owner_row["id"],
    }


def _role(conn, mid):
    return db.fetch_one(conn, "SELECT role FROM project_members WHERE id = %s", (mid,))["role"]


def test_patch_ws_admin_target_protected(seed, conn):
    """NEWPIDASH-264 DIFF-24: proj Admin cannot change a ws admin's role."""
    m = _seed_matrix(seed, conn)
    res = http.patch(m["pa_key"], f"{proj_base(seed)}/members/{m['m_wa']['id']}/",
                     json={"role": db.GUEST}, expect=expect("DIFF-24", django=200, rust=403))
    if is_rust():
        assert res.json() == WS_ADMIN_TARGET
        assert _role(conn, m["m_wa"]["id"]) == db.MEMBER
    else:
        assert _role(conn, m["m_wa"]["id"]) == db.GUEST


def test_patch_ws_admin_cannot_change_another_ws_admin(seed, conn):
    """NEWPIDASH-264 DIFF-24: ws admin vs another ws admin body differs."""
    m = _seed_matrix(seed, conn)
    res = http.patch(m["wa_key"], f"{proj_base(seed)}/members/{m['owner_mid']}/",
                     json={"role": db.GUEST}, expect=403)
    assert res.json() == expect("DIFF-24", django=CLASS_DENIAL, rust=WS_ADMIN_TARGET)
    if is_rust():
        assert _role(conn, m["owner_mid"]) == db.ADMIN


def test_patch_ws_admin_can_demote_non_admin(seed, conn):
    """NEWPIDASH-264 DIFF-24: gate override lets ws-admin Member through."""
    m = _seed_matrix(seed, conn)
    res = http.patch(m["wa_key"], f"{proj_base(seed)}/members/{m['m_mb']['id']}/",
                     json={"role": db.GUEST}, expect=expect("DIFF-24", django=403, rust=200))
    if is_rust():
        assert res.json()["role"] == db.GUEST
        assert _role(conn, m["m_mb"]["id"]) == db.GUEST


def test_patch_ws_admin_can_promote_to_admin(seed, conn):
    """NEWPIDASH-264 DIFF-24: ws-admin Member promotes to project Admin."""
    m = _seed_matrix(seed, conn)
    res = http.patch(m["wa_key"], f"{proj_base(seed)}/members/{m['m_mb']['id']}/",
                     json={"role": db.ADMIN}, expect=expect("DIFF-24", django=403, rust=200))
    if is_rust():
        assert res.json()["role"] == db.ADMIN
        assert _role(conn, m["m_mb"]["id"]) == db.ADMIN


def test_patch_project_admin_cannot_promote(seed, conn):
    """NEWPIDASH-264 DIFF-24: assign equal-or-higher guard."""
    m = _seed_matrix(seed, conn)
    res = http.patch(m["pa_key"], f"{proj_base(seed)}/members/{m['m_mb']['id']}/",
                     json={"role": db.ADMIN}, expect=expect("DIFF-24", django=200, rust=403))
    if is_rust():
        assert res.json() == ASSIGN_GUARD
        assert _role(conn, m["m_mb"]["id"]) == db.MEMBER
    else:
        assert _role(conn, m["m_mb"]["id"]) == db.ADMIN


def test_patch_project_admin_cannot_change_equal_target(seed, conn):
    """NEWPIDASH-264 DIFF-24: equal-or-higher target guard."""
    m = _seed_matrix(seed, conn)
    res = http.patch(m["pa_key"], f"{proj_base(seed)}/members/{m['m_pb']['id']}/",
                     json={"role": db.GUEST}, expect=expect("DIFF-24", django=200, rust=403))
    if is_rust():
        assert res.json() == EQUAL_TARGET
        assert _role(conn, m["m_pb"]["id"]) == db.ADMIN
    else:
        assert _role(conn, m["m_pb"]["id"]) == db.GUEST


def test_patch_project_admin_self_edit(seed, conn):
    """NEWPIDASH-264 DIFF-24: self 400 for non-ws-admin."""
    m = _seed_matrix(seed, conn)
    res = http.patch(m["pa_key"], f"{proj_base(seed)}/members/{m['m_pa']['id']}/",
                     json={"role": db.GUEST}, expect=expect("DIFF-24", django=200, rust=400))
    if is_rust():
        assert res.json() == SELF_ROLE
        assert _role(conn, m["m_pa"]["id"]) == db.ADMIN
    else:
        assert _role(conn, m["m_pa"]["id"]) == db.GUEST


def test_patch_guest_cap_project_admin(seed, conn):
    """NEWPIDASH-264 DIFF-24: guest cap for proj Admin."""
    m = _seed_matrix(seed, conn)
    res = http.patch(m["pa_key"], f"{proj_base(seed)}/members/{m['m_wg']['id']}/",
                     json={"role": db.MEMBER}, expect=expect("DIFF-24", django=200, rust=400))
    if is_rust():
        assert res.json() == GUEST_CAP
        assert _role(conn, m["m_wg"]["id"]) == db.GUEST
    else:
        assert _role(conn, m["m_wg"]["id"]) == db.MEMBER


def test_patch_guest_cap_ws_admin(seed, conn):
    """NEWPIDASH-264 DIFF-24: guest cap for ws-admin (gate 403 vs cap 400)."""
    m = _seed_matrix(seed, conn)
    res = http.patch(m["wa_key"], f"{proj_base(seed)}/members/{m['m_wg']['id']}/",
                     json={"role": db.MEMBER}, expect=expect("DIFF-24", django=403, rust=400))
    if is_rust():
        assert res.json() == GUEST_CAP
        assert _role(conn, m["m_wg"]["id"]) == db.GUEST
    else:
        assert res.json() == CLASS_DENIAL
        assert _role(conn, m["m_wg"]["id"]) == db.GUEST
