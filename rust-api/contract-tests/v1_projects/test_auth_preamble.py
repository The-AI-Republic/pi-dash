"""Contract tests: shared v1 API-auth preamble edges (PIDASHCONV-737).

The per-domain preamble (``actor`` / ``resolve_api_token`` /
``resolve_machine_token`` / ``request_timezone``) must match
``api/middleware/api_authentication.py`` + ``BaseAPIView`` on four edges
no other suite pins (every suite uses active users, live tokens, valid
zones, member callers):

1. A soft-deleted ``api_tokens`` row 403s (the ``SoftDeletionManager``
   scope), it does not authenticate.
2. ``users.is_active`` is never consulted: an inactive user with a valid
   token and intact memberships is served.
3. The machine-token non-member deny path stamps ``revoked_at`` only —
   ``last_used_at`` stays NULL (``MachineToken.revoke()`` saves
   ``update_fields=["revoked_at"]``).
4. The stored zone activates after the permission gate
   (``TimezoneMixin.initial`` runs ``super().initial()`` first): an
   invalid zone + denied caller 403s; an invalid zone + allowed caller
   400s with the ``KeyError`` branch body (``ZoneInfoNotFoundError`` is
   a ``KeyError``), it never 500s.

PIDASHCONV-747 pins four adjacent latent divergences the 737 work found
and left out of scope (each live-probed on Django first; ``''`` is
unreachable via the API — ``choices=pytz.common_timezones`` — so these
tests seed the zone straight into ``users``, like the 737 tests do):

5. An EMPTY stored zone 500s for allowed callers: ``ZoneInfo('')``
   raises ``ValueError`` (not ``KeyError``), which falls through
   ``handle_exception`` to the generic 500 — while an empty zone +
   denied caller still 403s (the gate runs first).
6. The asset generic/user routes (``BaseAPIView`` with no further gate)
   activate the zone too: bad zone + any authenticated caller 400s,
   empty zone + any authenticated caller 500s, anonymous 401s.
7. Modules / states / stickies BAD-zone controls: 737 fixed their
   mapping but pinned no zone test for them; an invalid zone + member
   400s there too.

(Item 4 of PIDASHCONV-747 — the device-logout revoke re-read missing
``AND deleted_at IS NULL`` — is race-only with byte-identical
responses, so no black-box test can pin it; it is covered by code
review and the existing revoke suite staying green.)

Every test seeds its own rows and never mutates the shared session
``seed`` (unique tags, no teardown needed).
"""

import os
import secrets
import sys
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, djangocrypto, http  # noqa: E402

INVALID_TOKEN_BODY = {"detail": "Given API token is not valid"}
GATE_DENIAL_BODY = {"detail": "You do not have permission to perform this action."}
KEY_ERROR_BODY = {"error": "The required key does not exist."}
SERVER_ERROR_BODY = {"error": "Something went wrong please try again later"}
UNAUTHENTICATED_BODY = {"detail": "Authentication credentials were not provided."}
BAD_ZONE = "Not/AZone"
EMPTY_ZONE = ""


def _project_list(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects/"


def _project_detail(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects/{seed['project']['id']}/"


def _project_members(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects/{seed['project']['id']}/members/"


def _project_states(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects/{seed['project']['id']}/states/"


def _cycles(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects/{seed['project']['id']}/cycles/"


def _modules(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects/{seed['project']['id']}/modules/"


def _exec(conn, query, params=()):
    with conn.cursor() as cur:
        cur.execute(query, params)
    conn.commit()


def _mint_machine_token(conn, user_id, workspace_id, secret):
    """Seed one dev-machine + machine-token row; return the raw `mt_` token."""
    machine_id = str(uuid.uuid4())
    _exec(
        conn,
        """INSERT INTO dev_machine
               (id, owner_id, host_label, label, visibility, provisioning,
                created_at, updated_at)
           VALUES (%s, %s, 'ct737', 'ct737', 0, 'manual', now(), now())""",
        (machine_id, user_id),
    )
    raw = "mt_" + secrets.token_urlsafe(24)
    _exec(
        conn,
        """INSERT INTO machine_token
               (id, user_id, workspace_id, dev_machine_id, host_label,
                token_hash, token_fingerprint, label, is_service, created_at)
           VALUES (%s, %s, %s, %s, 'ct737', %s, %s, 'ct737', true, now())""",
        (
            str(uuid.uuid4()),
            user_id,
            workspace_id,
            machine_id,
            djangocrypto.machine_token_hash(raw, secret),
            djangocrypto.machine_token_fingerprint(raw),
        ),
    )
    return raw


def test_soft_deleted_api_token_403(seed, conn):
    tag = db.new_tag()
    doomed = db.create_api_token(conn, seed["owner"]["id"], tag + "d")
    control = db.create_api_token(conn, seed["owner"]["id"], tag + "c")["token"]
    _exec(conn, "UPDATE api_tokens SET deleted_at = now() WHERE id = %s", (doomed["id"],))
    r = http.get(doomed["token"], _project_list(seed), expect=403)
    assert r.json() == INVALID_TOKEN_BODY
    # Control: a live token for the same user still authenticates.
    http.get(control, _project_list(seed), expect=200)


def test_inactive_user_with_valid_token_served(seed, conn):
    tag = db.new_tag()
    user = db.create_user(conn, tag + "i")
    db.add_workspace_member(conn, seed["ws_a"]["id"], user["id"], db.MEMBER)
    db.add_project_member(conn, seed["ws_a"]["id"], seed["project"]["id"], user["id"], db.MEMBER)
    key = db.create_api_token(conn, user["id"], tag + "ik")["token"]
    _exec(conn, "UPDATE users SET is_active = false WHERE id = %s", (user["id"],))
    r = http.get(key, _project_detail(seed), expect=200)
    assert r.json()["id"] == seed["project"]["id"]


def test_machine_token_nonmember_revoked_without_last_used(seed, conn):
    tag = db.new_tag()
    secret = os.environ["SECRET_KEY"]
    user = db.create_user(conn, tag + "n")  # no memberships anywhere
    raw = _mint_machine_token(conn, user["id"], seed["ws_a"]["id"], secret)
    r = http.get(raw, _project_list(seed), expect=403)
    assert r.json() == INVALID_TOKEN_BODY
    row = db.fetch_one(
        conn,
        "SELECT revoked_at IS NOT NULL AS revoked, last_used_at FROM machine_token"
        " WHERE user_id = %s",
        (user["id"],),
    )
    assert row["revoked"] is True
    assert row["last_used_at"] is None


def test_machine_token_member_ok_stamps_last_used(seed, conn):
    tag = db.new_tag()
    secret = os.environ["SECRET_KEY"]
    raw = _mint_machine_token(conn, seed["owner"]["id"], seed["ws_a"]["id"], secret)
    http.get(raw, _project_list(seed), expect=200)
    row = db.fetch_one(
        conn,
        "SELECT revoked_at, last_used_at IS NOT NULL AS stamped FROM machine_token"
        " WHERE token_hash = %s",
        (djangocrypto.machine_token_hash(raw, secret),),
    )
    assert row["revoked_at"] is None
    assert row["stamped"] is True


def test_invalid_zone_denied_caller_403(seed, conn):
    tag = db.new_tag()
    user = db.create_user(conn, tag + "x")  # outsider: no memberships
    key = db.create_api_token(conn, user["id"], tag + "xk")["token"]
    _exec(conn, "UPDATE users SET user_timezone = %s WHERE id = %s", (BAD_ZONE, user["id"]))
    r = http.get(key, _project_detail(seed), expect=403)
    assert r.json() == GATE_DENIAL_BODY


def test_invalid_zone_member_400_key_error(seed, conn):
    tag = db.new_tag()
    user = db.create_user(conn, tag + "z")
    db.add_workspace_member(conn, seed["ws_a"]["id"], user["id"], db.MEMBER)
    db.add_project_member(conn, seed["ws_a"]["id"], seed["project"]["id"], user["id"], db.MEMBER)
    key = db.create_api_token(conn, user["id"], tag + "zk")["token"]
    _exec(conn, "UPDATE users SET user_timezone = %s WHERE id = %s", (BAD_ZONE, user["id"]))
    r = http.get(key, _project_detail(seed), expect=400)
    assert r.json() == KEY_ERROR_BODY


def _soft_deleted_key(conn, user_id):
    tag = db.new_tag()
    doomed = db.create_api_token(conn, user_id, tag + "d")
    _exec(conn, "UPDATE api_tokens SET deleted_at = now() WHERE id = %s", (doomed["id"],))
    return doomed["token"]


def test_soft_deleted_token_403_members(seed, conn):
    r = http.get(_soft_deleted_key(conn, seed["owner"]["id"]), _project_members(seed), expect=403)
    assert r.json() == INVALID_TOKEN_BODY


def test_soft_deleted_token_403_states(seed, conn):
    r = http.get(_soft_deleted_key(conn, seed["owner"]["id"]), _project_states(seed), expect=403)
    assert r.json() == INVALID_TOKEN_BODY


def test_soft_deleted_token_403_cycles(seed, conn):
    r = http.get(_soft_deleted_key(conn, seed["owner"]["id"]), _cycles(seed), expect=403)
    assert r.json() == INVALID_TOKEN_BODY


def test_soft_deleted_token_403_modules(seed, conn):
    r = http.get(_soft_deleted_key(conn, seed["owner"]["id"]), _modules(seed), expect=403)
    assert r.json() == INVALID_TOKEN_BODY


def _zoneless_outsider_key(conn):
    tag = db.new_tag()
    user = db.create_user(conn, tag + "x")
    key = db.create_api_token(conn, user["id"], tag + "xk")["token"]
    _exec(conn, "UPDATE users SET user_timezone = %s WHERE id = %s", (BAD_ZONE, user["id"]))
    return key


def _zoneless_member_key(seed, conn):
    tag = db.new_tag()
    user = db.create_user(conn, tag + "z")
    db.add_workspace_member(conn, seed["ws_a"]["id"], user["id"], db.MEMBER)
    db.add_project_member(conn, seed["ws_a"]["id"], seed["project"]["id"], user["id"], db.MEMBER)
    key = db.create_api_token(conn, user["id"], tag + "zk")["token"]
    _exec(conn, "UPDATE users SET user_timezone = %s WHERE id = %s", (BAD_ZONE, user["id"]))
    return key


def test_invalid_zone_denied_caller_403_members(seed, conn):
    r = http.get(_zoneless_outsider_key(conn), _project_members(seed), expect=403)
    assert r.json() == GATE_DENIAL_BODY


def test_invalid_zone_denied_caller_403_cycles(seed, conn):
    r = http.get(_zoneless_outsider_key(conn), _cycles(seed), expect=403)
    assert r.json() == GATE_DENIAL_BODY


def test_invalid_zone_member_400_members(seed, conn):
    r = http.get(_zoneless_member_key(seed, conn), _project_members(seed), expect=400)
    assert r.json() == KEY_ERROR_BODY


def test_invalid_zone_member_400_cycles(seed, conn):
    r = http.get(_zoneless_member_key(seed, conn), _cycles(seed), expect=400)
    assert r.json() == KEY_ERROR_BODY


def test_soft_deleted_token_403_stickies(seed, conn):
    url = f"/api/v1/workspaces/{seed['ws_a']['slug']}/stickies/"
    r = http.get(_soft_deleted_key(conn, seed["owner"]["id"]), url, expect=403)
    assert r.json() == INVALID_TOKEN_BODY


def test_soft_deleted_token_403_generic_assets(seed, conn):
    import uuid as _uuid

    url = (
        f"/api/v1/workspaces/{seed['ws_a']['slug']}/assets/{_uuid.uuid4()}/"
    )
    r = http.get(_soft_deleted_key(conn, seed["owner"]["id"]), url, expect=403)
    assert r.json() == INVALID_TOKEN_BODY


def test_soft_deleted_token_403_user_assets(seed, conn):
    import uuid as _uuid

    url = f"/api/v1/assets/user-assets/{_uuid.uuid4()}/"
    r = http.patch(
        _soft_deleted_key(conn, seed["owner"]["id"]), url, json={}, expect=403
    )
    assert r.json() == INVALID_TOKEN_BODY


def test_soft_deleted_token_403_device_workspaces(seed, conn):
    r = http.get(
        _soft_deleted_key(conn, seed["owner"]["id"]),
        "/api/v1/auth/workspaces/",
        expect=403,
    )
    assert r.json() == INVALID_TOKEN_BODY


# ---------------------------------------------------------------------------
# PIDASHCONV-747: empty-zone mapping, asset zone parsing, BAD-zone controls
# ---------------------------------------------------------------------------


def _stickies(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/stickies/"


def _generic_asset(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/assets/{uuid.uuid4()}/"


def _user_asset():
    return f"/api/v1/assets/user-assets/{uuid.uuid4()}/"


def _users_me():
    return "/api/v1/users/me/"


def _empty_zone_outsider_key(conn):
    tag = db.new_tag()
    user = db.create_user(conn, tag + "e")
    key = db.create_api_token(conn, user["id"], tag + "ek")["token"]
    _exec(conn, "UPDATE users SET user_timezone = %s WHERE id = %s", (EMPTY_ZONE, user["id"]))
    return key


def _empty_zone_member_key(seed, conn):
    tag = db.new_tag()
    user = db.create_user(conn, tag + "e")
    db.add_workspace_member(conn, seed["ws_a"]["id"], user["id"], db.MEMBER)
    db.add_project_member(conn, seed["ws_a"]["id"], seed["project"]["id"], user["id"], db.MEMBER)
    key = db.create_api_token(conn, user["id"], tag + "ek")["token"]
    _exec(conn, "UPDATE users SET user_timezone = %s WHERE id = %s", (EMPTY_ZONE, user["id"]))
    return key


def test_empty_zone_member_500_projects(seed, conn):
    r = http.get(_empty_zone_member_key(seed, conn), _project_detail(seed), expect=500)
    assert r.json() == SERVER_ERROR_BODY


def test_empty_zone_denied_caller_403_projects(seed, conn):
    r = http.get(_empty_zone_outsider_key(conn), _project_detail(seed), expect=403)
    assert r.json() == GATE_DENIAL_BODY


def test_empty_zone_member_500_members(seed, conn):
    r = http.get(_empty_zone_member_key(seed, conn), _project_members(seed), expect=500)
    assert r.json() == SERVER_ERROR_BODY


def test_empty_zone_denied_caller_403_members(seed, conn):
    r = http.get(_empty_zone_outsider_key(conn), _project_members(seed), expect=403)
    assert r.json() == GATE_DENIAL_BODY


def test_empty_zone_member_500_cycles(seed, conn):
    r = http.get(_empty_zone_member_key(seed, conn), _cycles(seed), expect=500)
    assert r.json() == SERVER_ERROR_BODY


def test_empty_zone_denied_caller_403_cycles(seed, conn):
    r = http.get(_empty_zone_outsider_key(conn), _cycles(seed), expect=403)
    assert r.json() == GATE_DENIAL_BODY


def test_empty_zone_member_500_modules(seed, conn):
    r = http.get(_empty_zone_member_key(seed, conn), _modules(seed), expect=500)
    assert r.json() == SERVER_ERROR_BODY


def test_empty_zone_denied_caller_403_modules(seed, conn):
    r = http.get(_empty_zone_outsider_key(conn), _modules(seed), expect=403)
    assert r.json() == GATE_DENIAL_BODY


def test_empty_zone_member_500_states(seed, conn):
    r = http.get(_empty_zone_member_key(seed, conn), _project_states(seed), expect=500)
    assert r.json() == SERVER_ERROR_BODY


def test_empty_zone_denied_caller_403_states(seed, conn):
    r = http.get(_empty_zone_outsider_key(conn), _project_states(seed), expect=403)
    assert r.json() == GATE_DENIAL_BODY


def test_empty_zone_member_500_stickies(seed, conn):
    r = http.get(_empty_zone_member_key(seed, conn), _stickies(seed), expect=500)
    assert r.json() == SERVER_ERROR_BODY


def test_empty_zone_denied_caller_403_stickies(seed, conn):
    r = http.get(_empty_zone_outsider_key(conn), _stickies(seed), expect=403)
    assert r.json() == GATE_DENIAL_BODY


def test_empty_zone_500_users_me(seed, conn):
    # No membership gate beyond `IsAuthenticated`: any authenticated
    # caller with an empty zone 500s.
    r = http.get(_empty_zone_outsider_key(conn), _users_me(), expect=500)
    assert r.json() == SERVER_ERROR_BODY


def test_invalid_zone_member_400_modules(seed, conn):
    r = http.get(_zoneless_member_key(seed, conn), _modules(seed), expect=400)
    assert r.json() == KEY_ERROR_BODY


def test_invalid_zone_member_400_states(seed, conn):
    r = http.get(_zoneless_member_key(seed, conn), _project_states(seed), expect=400)
    assert r.json() == KEY_ERROR_BODY


def test_invalid_zone_member_400_stickies(seed, conn):
    r = http.get(_zoneless_member_key(seed, conn), _stickies(seed), expect=400)
    assert r.json() == KEY_ERROR_BODY


def test_invalid_zone_400_generic_assets(seed, conn):
    # The asset id is random: the zone 400 lands before the row lookup.
    r = http.get(_zoneless_outsider_key(conn), _generic_asset(seed), expect=400)
    assert r.json() == KEY_ERROR_BODY


def test_empty_zone_500_generic_assets(seed, conn):
    r = http.get(_empty_zone_outsider_key(conn), _generic_asset(seed), expect=500)
    assert r.json() == SERVER_ERROR_BODY


def test_anonymous_401_generic_assets(seed, conn):
    r = http.get(None, _generic_asset(seed), expect=401)
    assert r.json() == UNAUTHENTICATED_BODY


def test_invalid_zone_400_user_assets(seed, conn):
    r = http.patch(_zoneless_outsider_key(conn), _user_asset(), json={}, expect=400)
    assert r.json() == KEY_ERROR_BODY


def test_empty_zone_500_user_assets(seed, conn):
    r = http.patch(_empty_zone_outsider_key(conn), _user_asset(), json={}, expect=500)
    assert r.json() == SERVER_ERROR_BODY


def test_anonymous_401_user_assets(seed, conn):
    r = http.patch(None, _user_asset(), json={}, expect=401)
    assert r.json() == UNAUTHENTICATED_BODY


def test_invalid_zone_400_user_asset_upload(seed, conn):
    payload = {
        "name": "profile.jpg",
        "type": "image/jpeg",
        "size": 1024000,
        "entity_type": "USER_AVATAR",
    }
    r = http.post(
        _zoneless_outsider_key(conn), "/api/v1/assets/user-assets/", json=payload, expect=400
    )
    assert r.json() == KEY_ERROR_BODY


def test_empty_zone_500_user_asset_upload(seed, conn):
    payload = {
        "name": "profile.jpg",
        "type": "image/jpeg",
        "size": 1024000,
        "entity_type": "USER_AVATAR",
    }
    r = http.post(
        _empty_zone_outsider_key(conn), "/api/v1/assets/user-assets/", json=payload, expect=500
    )
    assert r.json() == SERVER_ERROR_BODY
