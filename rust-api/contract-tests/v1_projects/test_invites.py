"""Contract tests: api-v1 invite endpoints (urls/invite.py, 1 router entry).

The router mounts WorkspaceInvitationsViewset at
`workspaces/<slug>/invitations/` with list/retrieve/create/partial_update/
destroy. Only workspace ADMINs may touch invites.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http, sessions  # noqa: E402

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


def api_root(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/"


def owner_session(seed, conn):
    return sessions.login(conn, seed["owner"]["id"], "!")


def test_api_root_index_lists_invitations(seed, conn):
    # DRF DefaultRouter api-root (PIDASHCONV-828 G3): the invite router is
    # mounted first, so its index wins and names only its own prefix. The
    # root view carries no API-key backend, so the body needs a session.
    r = http.make_client(owner_session(seed, conn)).get(api_root(seed))
    assert r.status_code == 200, r.text[:400]
    body = r.json()
    assert set(body.keys()) == {"invitations"}
    assert body["invitations"].endswith(f"{base(seed)}/")


def test_api_root_api_key_denied(seed):
    # No API-key backend on the api-root: keyed and anonymous callers 401
    # alike (both backends answer Django's bytes through the proxy).
    http.get(seed["keys"][KEY], api_root(seed), expect=401)
    http.get(None, api_root(seed), expect=401)


def test_api_root_format_suffix(seed, conn):
    # The api-root format variant (`/{slug}/.json`, G3): same index with the
    # suffix threaded into the reversed list URL.
    r = http.make_client(owner_session(seed, conn)).get(api_root(seed) + ".json")
    assert r.status_code == 200, r.text[:400]
    body = r.json()
    assert set(body.keys()) == {"invitations"}
    assert body["invitations"].endswith(f"{base(seed)}.json")


# Django answers every invite format-suffix URL with its generic 500: the
# viewset actions declare explicit params, so the router's `format` kwarg
# raises TypeError (`... got an unexpected keyword argument 'format'`).
# Rust proxies these spellings, so the bug shines through byte for byte.
SUFFIX_500 = {"error": "Something went wrong please try again later"}


def test_list_format_suffix_json(seed, conn):
    tag = db.new_tag()
    db.create_invite(conn, seed["ws_a"]["id"], tag, created_by_id=seed["owner"]["id"])
    for path in (base(seed) + ".json", base(seed) + ".json/"):
        r = http.get(seed["keys"][KEY], path, expect=500)
        assert r.json() == SUFFIX_500


def test_detail_format_suffix_json(seed, conn):
    tag = db.new_tag()
    inv = db.create_invite(conn, seed["ws_a"]["id"], tag, created_by_id=seed["owner"]["id"])
    # G5: the dotted spelling reaches Django (500), never the detail handler
    # on a garbage pk (which would 400 here).
    for path in (f"{base(seed)}/{inv['id']}.json", f"{base(seed)}/{inv['id']}.json/"):
        r = http.get(seed["keys"][KEY], path, expect=500)
        assert r.json() == SUFFIX_500


def test_detail_format_suffix_patch_and_delete(seed, conn):
    tag = db.new_tag()
    inv = db.create_invite(conn, seed["ws_a"]["id"], tag, created_by_id=seed["owner"]["id"])
    # PATCH rides the proxy with its real body; Django still 500s on the
    # `format` kwarg before touching anything, so DELETE 500s too and the
    # invite survives both.
    r = http.patch(seed["keys"][KEY], f"{base(seed)}/{inv['id']}.json/",
                   json={"role": db.GUEST}, expect=500)
    assert r.json() == SUFFIX_500
    r = http.delete(seed["keys"][KEY], f"{base(seed)}/{inv['id']}.json/", expect=500)
    assert r.json() == SUFFIX_500
    body = http.get(seed["keys"][KEY], f"{base(seed)}/{inv['id']}/").json()
    assert body["email"] == inv["email"]
