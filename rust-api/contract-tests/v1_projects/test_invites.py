"""Contract tests: api-v1 invite endpoints (urls/invite.py, 1 router entry).

The router mounts WorkspaceInvitationsViewset at
`workspaces/<slug>/invitations/` with list/retrieve/create/partial_update/
destroy. Only workspace ADMINs may touch invites.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402

KEY = "invites"

INVITE_KEYS = {"id", "email", "role", "created_at", "updated_at", "responded_at", "accepted"}


def base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/invitations"


def test_list(seed, conn):
    tag = db.new_tag()
    inv = db.create_invite(conn, seed["ws_a"]["id"], tag, created_by_id=seed["owner"]["id"])
    body = http.get(seed["keys"][KEY], base(seed) + "/").json()
    assert isinstance(body, list)
    by_id = {i["id"]: i for i in body}
    assert inv["id"] in by_id
    assert set(by_id[inv["id"]].keys()) == INVITE_KEYS
    assert by_id[inv["id"]]["email"] == inv["email"]
    assert by_id[inv["id"]]["accepted"] is False


def test_create(seed):
    tag = db.new_tag()
    email = f"ct-new-{tag}@example.com"
    body = http.post(seed["keys"][KEY], base(seed) + "/",
                     json={"email": email, "role": db.MEMBER}, expect=201).json()
    assert set(body.keys()) == INVITE_KEYS
    assert body["email"] == email
    assert body["role"] == db.MEMBER
    assert body["accepted"] is False


def test_create_validation(seed, conn):
    tag = db.new_tag()
    inv = db.create_invite(conn, seed["ws_a"]["id"], tag, created_by_id=seed["owner"]["id"])
    # Duplicate email in the same workspace.
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"email": inv["email"], "role": db.MEMBER}, expect=400)
    assert "non_field_errors" in r.json() or "email" in r.json()
    # Malformed email.
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"email": "not-an-email", "role": db.MEMBER}, expect=400)
    assert "email" in r.json()
    # Unknown role.
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"email": f"ct-r-{tag}@example.com", "role": 99}, expect=400)
    assert "role" in r.json()


def test_retrieve(seed, conn):
    tag = db.new_tag()
    inv = db.create_invite(conn, seed["ws_a"]["id"], tag, created_by_id=seed["owner"]["id"])
    body = http.get(seed["keys"][KEY], f"{base(seed)}/{inv['id']}/").json()
    assert set(body.keys()) == INVITE_KEYS
    assert body["email"] == inv["email"]


def test_partial_update_role(seed, conn):
    tag = db.new_tag()
    inv = db.create_invite(conn, seed["ws_a"]["id"], tag, created_by_id=seed["owner"]["id"])
    body = http.patch(seed["keys"][KEY], f"{base(seed)}/{inv['id']}/",
                      json={"role": db.GUEST}).json()
    assert body["role"] == db.GUEST
    # Email is immutable after creation.
    r = http.patch(seed["keys"][KEY], f"{base(seed)}/{inv['id']}/",
                   json={"email": f"ct-changed-{tag}@example.com"}, expect=400)
    assert r.json()["code"] == "EMAIL_CANNOT_BE_UPDATED"


def test_destroy(seed, conn):
    tag = db.new_tag()
    inv = db.create_invite(conn, seed["ws_a"]["id"], tag, created_by_id=seed["owner"]["id"])
    http.delete(seed["keys"][KEY], f"{base(seed)}/{inv['id']}/")
    http.get(seed["keys"][KEY], f"{base(seed)}/{inv['id']}/", expect=404)


def test_destroy_accepted_400(seed, conn):
    tag = db.new_tag()
    inv = db.create_invite(conn, seed["ws_a"]["id"], tag, created_by_id=seed["owner"]["id"])
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE workspace_member_invites SET accepted = true, responded_at = now() WHERE id = %s",
            (inv["id"],),
        )
    r = http.delete(seed["keys"][KEY], f"{base(seed)}/{inv['id']}/", expect=400)
    assert r.json()["code"] == "INVITE_ALREADY_ACCEPTED"


def test_denied_member(seed):
    # Workspace MEMBER (not ADMIN) may not manage invites.
    http.get(seed["keys"]["member_self"], base(seed) + "/", expect=403)
    http.post(seed["keys"]["member_self"], base(seed) + "/",
              json={"email": "ct-nope@example.com", "role": db.MEMBER}, expect=403)


def test_isolation_other_workspace(seed, conn):
    tag = db.new_tag()
    inv = db.create_invite(conn, seed["ws_a"]["id"], tag, created_by_id=seed["owner"]["id"])
    inv_b = db.create_invite(conn, seed["ws_b"]["id"], tag + "b",
                             created_by_id=seed["owner_b"]["id"])
    # Data isolation: each workspace list carries only its own invites.
    ids_a = {i["id"] for i in http.get(seed["keys"][KEY], base(seed) + "/").json()}
    assert inv["id"] in ids_a
    assert inv_b["id"] not in ids_a
    ids_b = {i["id"] for i in http.get(
        seed["keys"]["owner_b"],
        f"/api/v1/workspaces/{seed['ws_b']['slug']}/invitations/").json()}
    assert inv_b["id"] in ids_b
    assert inv["id"] not in ids_b
    # Permission runs before queryset scoping, so cross-workspace retrieve is
    # 403 (not a 404 leak): neither side can fetch the other's invite.
    http.get(seed["keys"]["owner_b"], f"{base(seed)}/{inv['id']}/", expect=403)
    http.get(seed["keys"][KEY],
             f"/api/v1/workspaces/{seed['ws_b']['slug']}/invitations/{inv_b['id']}/",
             expect=403)
