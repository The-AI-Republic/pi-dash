"""Contract tests for the D-17 device-session handlers B (PIDASHCONV-343).

Pins the live behavior of the last three device endpoints from
``pi_dash/authentication/views/cli/device.py`` against the backend under
test (Django today, the Rust port through the proxy tomorrow):

* ``GET /api/v1/auth/workspaces/`` — member-since order, ``{slug, name}``
  key order, inactive memberships excluded.
* ``POST /api/v1/auth/machine-token/`` — the 400/404/201 shapes in view
  order (F12 ``machine_token_*`` rows), bridge deactivation, ownership
  404, rotation for ``mt_`` callers.
* ``POST /api/v1/auth/revoke/`` — idempotent ok for both caller kinds
  (F12 ``revoke_*`` rows).

Auth ground truth (probed live, 2026-09-30): no credential answers 401
``{"detail": "Authentication credentials were not provided."``; a bad,
expired, or inactive token answers 403
``{"detail": "Given API token is not valid"}`` (DRF coerces the 401
because ``APIKeyAuthentication`` defines no ``authenticate_header``).
A revoked-then-reused token therefore 403s at the gate — the handler's
missing/inactive-row ok branches are defensive (race-only) and stay
unreached over HTTP.

HTTP goes through httpx; seeding is raw SQL via the shared harness (no
Django imports). Nothing here asserts internals — only status lines,
JSON bodies, and the rows the endpoints promise to write.
"""

import os
import re
import secrets
import sys
import uuid

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import httpx  # noqa: E402
import pytest  # noqa: E402

from _harness import config, db, factory, tokens  # noqa: E402
from _harness.client import base_url, client  # noqa: E402,F401

DEVICE_FLOW_DESCRIPTION = "Issued by pidash auth login (device-code flow)."

UNAUTHENTICATED = {"detail": "Authentication credentials were not provided."}
INVALID_TOKEN = {"detail": "Given API token is not valid"}
METHOD_NOT_ALLOWED = lambda m: {"detail": f'Method "{m}" not allowed.'}  # noqa: E731


def database_url():
    return config.database_url()


def secret_key():
    return config.secret_key()


def mint_api_token(user_id, workspace_id=None, *, description="", active=True, expired=False):
    raw = "pi_dash_api_" + uuid.uuid4().hex
    expired_sql = "now() - interval '1 hour'" if expired else "NULL"
    db.execute(
        database_url(),
        f"""INSERT INTO api_tokens (id, token, label, user_type, user_id, workspace_id,
            description, is_active, is_service, allowed_rate_limit, expired_at,
            created_at, updated_at)
            VALUES (%s,%s,'ct343',0,%s,%s,%s,%s,false,'60/min',{expired_sql},now(),now())""",
        (str(uuid.uuid4()), raw, user_id, workspace_id, description, active),
    )
    return raw


def mint_machine_token(user_id, workspace_id, dev_machine_id, *, host_label="h"):
    raw = "mt_" + secrets.token_urlsafe(32)
    db.execute(
        database_url(),
        """INSERT INTO machine_token (id, user_id, dev_machine_id, workspace_id,
            host_label, token_hash, token_fingerprint, label, is_service,
            created_at, last_used_at, revoked_at)
            VALUES (%s,%s,%s,%s,%s,%s,%s,%s,true,now(),NULL,NULL)""",
        (
            str(uuid.uuid4()), user_id, dev_machine_id, workspace_id, host_label,
            tokens.hash_token(raw, secret_key()), tokens.fingerprint(raw),
            f"machine: {host_label[:96]}",
        ),
    )
    return raw


def make_dev_machine(owner_id, *, host_label="h", label="h"):
    mid = str(uuid.uuid4())
    db.execute(
        database_url(),
        """INSERT INTO dev_machine (id, owner_id, host_label, label, visibility,
            provisioning, last_seen_at, revoked_at, created_at, updated_at)
            VALUES (%s,%s,%s,%s,0,'manual',now(),NULL,now(),now())""",
        (mid, owner_id, host_label, label),
    )
    return mid


@pytest.fixture()
def ctx():
    """Two users, three workspaces, staggered memberships; wiped afterwards."""
    tag = uuid.uuid4().hex[:8]
    u1 = factory.create_user(f"ct343a-{tag}@example.com")
    u2 = factory.create_user(f"ct343b-{tag}@example.com")
    w1 = factory.create_workspace(f"ct343a-{tag}", u1["id"], name="CT-A")
    w2 = factory.create_workspace(f"ct343b-{tag}", u1["id"], name="CT-B")
    w3 = factory.create_workspace(f"ct343c-{tag}", u2["id"], name="CT-C")
    # Member-since order: w2 older than w1 (explicit stamps; same-instant
    # ties would leave ORDER BY created_at nondeterministic).
    factory.add_member(w2["id"], u1["id"])
    factory.add_member(w1["id"], u1["id"])
    factory.add_member(w3["id"], u2["id"])
    db.execute(
        database_url(),
        "UPDATE workspace_members SET created_at = now() - interval '2 minutes'"
        " WHERE workspace_id = %s AND member_id = %s",
        (w2["id"], u1["id"]),
    )
    db.execute(
        database_url(),
        "UPDATE workspace_members SET created_at = now() - interval '1 minute'"
        " WHERE workspace_id = %s AND member_id = %s",
        (w1["id"], u1["id"]),
    )
    handles = {"tag": tag, "u1": u1, "u2": u2, "w1": w1, "w2": w2, "w3": w3}
    yield handles
    like = f"%{tag}%"
    user_ids = [r["id"] for r in db.fetchall(
        database_url(), "SELECT id FROM users WHERE email LIKE %s", (like,))]
    ws_ids = [r["id"] for r in db.fetchall(
        database_url(), "SELECT id FROM workspaces WHERE slug LIKE %s", (like,))]
    if user_ids:
        db.execute(database_url(),
            "DELETE FROM machine_token WHERE user_id = ANY(%s)", (user_ids,))
        db.execute(database_url(),
            "DELETE FROM dev_machine WHERE owner_id = ANY(%s)", (user_ids,))
        db.execute(database_url(),
            "DELETE FROM api_tokens WHERE user_id = ANY(%s)", (user_ids,))
    if ws_ids or user_ids:
        db.execute(database_url(),
            "DELETE FROM workspace_members WHERE workspace_id = ANY(%s)"
            " OR member_id = ANY(%s)", (ws_ids or [], user_ids or []))
    factory.cleanup_run(tag)


# ---------------------------------------------------------------------------
# workspaces list
# ---------------------------------------------------------------------------

def test_workspaces_unauthenticated(client: httpx.Client):
    resp = client.get("/api/v1/auth/workspaces/")
    assert resp.status_code == 401, (resp.status_code, resp.text[:200])
    assert resp.json() == UNAUTHENTICATED


def test_workspaces_bad_expired_and_inactive_tokens(client: httpx.Client, ctx):
    bad = {"X-Api-Key": "pi_dash_api_no-such-token"}
    resp = client.get("/api/v1/auth/workspaces/", headers=bad)
    assert resp.status_code == 403, (resp.status_code, resp.text[:200])
    assert resp.json() == INVALID_TOKEN
    for raw in (
        mint_api_token(ctx["u1"]["id"], expired=True),
        mint_api_token(ctx["u1"]["id"], active=False),
    ):
        resp = client.get("/api/v1/auth/workspaces/", headers={"X-Api-Key": raw})
        assert resp.status_code == 403, (resp.status_code, resp.text[:200])
        assert resp.json() == INVALID_TOKEN


def test_workspaces_member_since_order_and_key_order(client: httpx.Client, ctx):
    raw = mint_api_token(ctx["u1"]["id"])
    resp = client.get("/api/v1/auth/workspaces/", headers={"X-Api-Key": raw})
    assert resp.status_code == 200, (resp.status_code, resp.text[:200])
    assert resp.json() == {
        "workspaces": [
            {"slug": ctx["w2"]["slug"], "name": "CT-B"},
            {"slug": ctx["w1"]["slug"], "name": "CT-A"},
        ]
    }
    # Byte order: slug before name in every entry (Python dict order).
    text = resp.text
    assert text.index('"slug"') < text.index('"name"'), text[:200]


def test_workspaces_empty_and_inactive_membership(client: httpx.Client, ctx):
    # u2 sees only w3.
    raw = mint_api_token(ctx["u2"]["id"])
    resp = client.get("/api/v1/auth/workspaces/", headers={"X-Api-Key": raw})
    assert resp.json() == {"workspaces": [{"slug": ctx["w3"]["slug"], "name": "CT-C"}]}
    # Deactivating the membership empties the list.
    db.execute(
        database_url(),
        "UPDATE workspace_members SET is_active = false WHERE workspace_id = %s",
        (ctx["w3"]["id"],),
    )
    try:
        resp = client.get("/api/v1/auth/workspaces/", headers={"X-Api-Key": raw})
        assert resp.json() == {"workspaces": []}
    finally:
        db.execute(
            database_url(),
            "UPDATE workspace_members SET is_active = true WHERE workspace_id = %s",
            (ctx["w3"]["id"],),
        )


def test_workspaces_wrong_method(client: httpx.Client, ctx):
    raw = mint_api_token(ctx["u1"]["id"])
    resp = client.post("/api/v1/auth/workspaces/", headers={"X-Api-Key": raw}, json={})
    assert resp.status_code == 405, (resp.status_code, resp.text[:200])
    assert resp.json() == METHOD_NOT_ALLOWED("POST")


# ---------------------------------------------------------------------------
# machine-token exchange
# ---------------------------------------------------------------------------

def test_machine_token_unauthenticated(client: httpx.Client):
    resp = client.post("/api/v1/auth/machine-token/", json={})
    assert resp.status_code == 401, (resp.status_code, resp.text[:200])
    assert resp.json() == UNAUTHENTICATED


def test_machine_token_validation_order(client: httpx.Client, ctx):
    raw = mint_api_token(ctx["u1"]["id"])
    headers = {"X-Api-Key": raw}
    dm = str(uuid.uuid4())
    ws = ctx["w1"]["slug"]
    cases = [
        ({}, {"error": "workspace_slug is required"}),
        ({"workspace_slug": ws}, {"error": "dev_machine_id is required"}),
        (
            {"workspace_slug": ws, "dev_machine_id": dm},
            {"error": "host_label is required"},
        ),
        (
            {"workspace_slug": ws, "dev_machine_id": "not-a-uuid", "host_label": "h"},
            {"error": "invalid_dev_machine_id"},
        ),
    ]
    for payload, expected in cases:
        resp = client.post("/api/v1/auth/machine-token/", headers=headers, json=payload)
        assert resp.status_code == 400, (payload, resp.status_code, resp.text[:200])
        assert resp.json() == expected, payload


def test_machine_token_unknown_and_foreign_workspace(client: httpx.Client, ctx):
    raw = mint_api_token(ctx["u1"]["id"])
    headers = {"X-Api-Key": raw}
    dm = str(uuid.uuid4())
    for slug in ("no-such-workspace", ctx["w3"]["slug"]):
        resp = client.post(
            "/api/v1/auth/machine-token/",
            headers=headers,
            json={"workspace_slug": slug, "dev_machine_id": dm, "host_label": "h"},
        )
        assert resp.status_code == 404, (slug, resp.status_code, resp.text[:200])
        assert resp.json() == {"error": "workspace_not_found"}, slug


def test_machine_token_bad_json_and_scalar_body(client: httpx.Client, ctx):
    raw = mint_api_token(ctx["u1"]["id"])
    headers = {"X-Api-Key": raw, "Content-Type": "application/json"}
    resp = client.post("/api/v1/auth/machine-token/", headers=headers, content=b"{bad")
    assert resp.status_code == 400, (resp.status_code, resp.text[:200])
    assert resp.json()["detail"].startswith("JSON parse error"), resp.text[:200]
    # A truthy non-string field is the `.strip()` AttributeError 500
    # (Django answers HTML there, so only the status is pinned).
    resp = client.post(
        "/api/v1/auth/machine-token/",
        headers=headers,
        json={"workspace_slug": 123, "dev_machine_id": str(uuid.uuid4()), "host_label": "h"},
    )
    assert resp.status_code == 500, (resp.status_code, resp.text[:200])


def test_machine_token_wrong_method(client: httpx.Client, ctx):
    raw = mint_api_token(ctx["u1"]["id"])
    resp = client.get("/api/v1/auth/machine-token/", headers={"X-Api-Key": raw})
    assert resp.status_code == 405, (resp.status_code, resp.text[:200])
    assert resp.json() == METHOD_NOT_ALLOWED("GET")


def test_machine_token_valid_exchange(client: httpx.Client, ctx):
    bridge = mint_api_token(
        ctx["u1"]["id"], ctx["w1"]["id"], description=DEVICE_FLOW_DESCRIPTION)
    plain = mint_api_token(ctx["u1"]["id"], ctx["w1"]["id"], description="other")
    dm = str(uuid.uuid4())
    resp = client.post(
        "/api/v1/auth/machine-token/",
        headers={"X-Api-Key": bridge},
        json={"workspace_slug": ctx["w1"]["slug"], "dev_machine_id": dm, "host_label": "  myhost  "},
    )
    assert resp.status_code == 201, (resp.status_code, resp.text[:300])
    body = resp.json()
    assert set(body) == {"machine_token", "workspace_slug", "dev_machine_id", "host_label"}
    assert re.fullmatch(r"mt_[A-Za-z0-9_-]{43}", body["machine_token"]), body["machine_token"]
    assert body["workspace_slug"] == ctx["w1"]["slug"]
    assert body["dev_machine_id"] == dm
    assert body["host_label"] == "myhost"
    # Key order is the Python dict order.
    text = resp.text
    assert (
        text.index('"machine_token"') < text.index('"workspace_slug"')
        < text.index('"dev_machine_id"') < text.index('"host_label"')
    ), text[:200]
    # The bridge with the device-flow description is spent; any other
    # description is left alone.
    row = db.fetchone(
        database_url(), "SELECT is_active FROM api_tokens WHERE token = %s", (bridge,))
    assert row["is_active"] is False
    row = db.fetchone(
        database_url(), "SELECT is_active FROM api_tokens WHERE token = %s", (plain,))
    assert row["is_active"] is True
    # The dev machine and the minted token rows carry the Python columns.
    machine = db.fetchone(
        database_url(), "SELECT owner_id, host_label, label FROM dev_machine WHERE id = %s", (dm,))
    assert str(machine["owner_id"]) == ctx["u1"]["id"]
    assert machine["host_label"] == "myhost"
    assert machine["label"] == "myhost"
    token = db.fetchone(
        database_url(),
        "SELECT user_id, workspace_id, dev_machine_id, host_label, token_fingerprint,"
        " label, is_service, revoked_at FROM machine_token WHERE token_hash = %s",
        (tokens.hash_token(body["machine_token"], secret_key()),),
    )
    assert str(token["user_id"]) == ctx["u1"]["id"]
    assert str(token["workspace_id"]) == ctx["w1"]["id"]
    assert str(token["dev_machine_id"]) == dm
    assert token["host_label"] == "myhost"
    assert re.fullmatch(r"[0-9a-f]{12}", token["token_fingerprint"]), token["token_fingerprint"]
    assert token["label"] == "machine: myhost"
    assert token["is_service"] is True
    assert token["revoked_at"] is None


def test_machine_token_ownership_conflict(client: httpx.Client, ctx):
    first = mint_api_token(ctx["u1"]["id"], ctx["w1"]["id"])
    dm = str(uuid.uuid4())
    resp = client.post(
        "/api/v1/auth/machine-token/",
        headers={"X-Api-Key": first},
        json={"workspace_slug": ctx["w1"]["slug"], "dev_machine_id": dm, "host_label": "h"},
    )
    assert resp.status_code == 201, (resp.status_code, resp.text[:200])
    # u2's machine row is a different machine... now u2 claims u1's id.
    second = mint_api_token(ctx["u2"]["id"], ctx["w3"]["id"])
    resp = client.post(
        "/api/v1/auth/machine-token/",
        headers={"X-Api-Key": second},
        json={"workspace_slug": ctx["w3"]["slug"], "dev_machine_id": dm, "host_label": "h2"},
    )
    assert resp.status_code == 404, (resp.status_code, resp.text[:200])
    assert resp.json() == {"error": "dev_machine_not_found"}


def test_machine_token_rotation_via_machine_token(client: httpx.Client, ctx):
    bridge = mint_api_token(
        ctx["u1"]["id"], ctx["w1"]["id"], description=DEVICE_FLOW_DESCRIPTION)
    dm = str(uuid.uuid4())
    resp = client.post(
        "/api/v1/auth/machine-token/",
        headers={"X-Api-Key": bridge},
        json={"workspace_slug": ctx["w1"]["slug"], "dev_machine_id": dm, "host_label": "h"},
    )
    first_mt = resp.json()["machine_token"]
    resp = client.post(
        "/api/v1/auth/machine-token/",
        headers={"X-Api-Key": first_mt},
        json={"workspace_slug": ctx["w1"]["slug"], "dev_machine_id": dm, "host_label": "h2"},
    )
    assert resp.status_code == 201, (resp.status_code, resp.text[:300])
    second_mt = resp.json()["machine_token"]
    assert second_mt != first_mt
    assert resp.json()["host_label"] == "h2"
    # Rotation revokes the previous token for the pair...
    row = db.fetchone(
        database_url(),
        "SELECT revoked_at IS NOT NULL AS revoked FROM machine_token WHERE token_hash = %s",
        (tokens.hash_token(first_mt, secret_key()),),
    )
    assert row["revoked"] is True
    # ...and a machine-token caller keeps no bridge to spend: the spent
    # bridge stays spent, and the caller's own token is the rotated one.
    live = db.fetchone(
        database_url(),
        "SELECT revoked_at FROM machine_token WHERE token_hash = %s",
        (tokens.hash_token(second_mt, secret_key()),),
    )
    assert live["revoked_at"] is None
    machine = db.fetchone(
        database_url(), "SELECT host_label FROM dev_machine WHERE id = %s", (dm,))
    assert machine["host_label"] == "h2"


# ---------------------------------------------------------------------------
# revoke
# ---------------------------------------------------------------------------

def test_revoke_unauthenticated_and_bad_token(client: httpx.Client):
    resp = client.post("/api/v1/auth/revoke/")
    assert resp.status_code == 401, (resp.status_code, resp.text[:200])
    assert resp.json() == UNAUTHENTICATED
    resp = client.post("/api/v1/auth/revoke/", headers={"X-Api-Key": "pi_dash_api_nope"})
    assert resp.status_code == 403, (resp.status_code, resp.text[:200])
    assert resp.json() == INVALID_TOKEN


def test_revoke_api_token(client: httpx.Client, ctx):
    raw = mint_api_token(ctx["u1"]["id"], ctx["w1"]["id"])
    resp = client.post("/api/v1/auth/revoke/", headers={"X-Api-Key": raw})
    assert resp.status_code == 200, (resp.status_code, resp.text[:200])
    assert resp.json() == {"ok": True}
    row = db.fetchone(
        database_url(), "SELECT is_active FROM api_tokens WHERE token = %s", (raw,))
    assert row["is_active"] is False
    # The spent token no longer authenticates: the gate answers 403.
    resp = client.post("/api/v1/auth/revoke/", headers={"X-Api-Key": raw})
    assert resp.status_code == 403, (resp.status_code, resp.text[:200])
    assert resp.json() == INVALID_TOKEN


def test_revoke_machine_token(client: httpx.Client, ctx):
    bridge = mint_api_token(ctx["u1"]["id"], ctx["w1"]["id"])
    dm = str(uuid.uuid4())
    resp = client.post(
        "/api/v1/auth/machine-token/",
        headers={"X-Api-Key": bridge},
        json={"workspace_slug": ctx["w1"]["slug"], "dev_machine_id": dm, "host_label": "h"},
    )
    mt = resp.json()["machine_token"]
    resp = client.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt})
    assert resp.status_code == 200, (resp.status_code, resp.text[:200])
    assert resp.json() == {"ok": True}
    row = db.fetchone(
        database_url(),
        "SELECT revoked_at IS NOT NULL AS revoked FROM machine_token WHERE token_hash = %s",
        (tokens.hash_token(mt, secret_key()),),
    )
    assert row["revoked"] is True
    # Reuse after revoke fails at the gate.
    resp = client.post("/api/v1/auth/revoke/", headers={"X-Api-Key": mt})
    assert resp.status_code == 403, (resp.status_code, resp.text[:200])
    assert resp.json() == INVALID_TOKEN


def test_revoke_wrong_method(client: httpx.Client, ctx):
    raw = mint_api_token(ctx["u1"]["id"])
    resp = client.get("/api/v1/auth/revoke/", headers={"X-Api-Key": raw})
    assert resp.status_code == 405, (resp.status_code, resp.text[:200])
    assert resp.json() == METHOD_NOT_ALLOWED("GET")
