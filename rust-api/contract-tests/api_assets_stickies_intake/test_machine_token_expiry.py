"""Expired machine-token (60-day idle expiry) rejection on v1 sticky/intake.

Django #2910 routes every v1 ``mt_`` check through
``resolve_machine_token``: a token whose ``expires_at`` is in the past is
refused with 403 ``{"detail": "Given API token is not valid"}`` and the
row is not touched (no ``last_used_at`` bump, no ``expires_at`` renewal),
so the token cannot revive itself. The v1 sticky and intake authenticators
must match (PIDASHCONV-1714).

Each case seeds an unrevoked machine token for an active member with
``expires_at`` in the past and asserts the 403 plus an untouched row. The
regression case seeds a still-valid token and expects 200 on both routes.
"""

from datetime import datetime, timedelta, timezone

from . import seed_d21


def _past_iso(minutes=1):
    return (datetime.now(timezone.utc) - timedelta(minutes=minutes)).isoformat()


def _future_iso(days=30):
    return (datetime.now(timezone.utc) + timedelta(days=days)).isoformat()


def _seeded_row(conn, user_id, workspace_id, host_label, expires_at, last_used_at):
    raw = seed_d21.create_machine_token(
        conn,
        user_id=user_id,
        workspace_id=workspace_id,
        host_label=host_label,
        expires_at=expires_at,
        last_used_at=last_used_at,
    )
    with conn.cursor() as cur:
        cur.execute(
            "SELECT id, expires_at, last_used_at FROM machine_token "
            "WHERE user_id = %s AND host_label = %s",
            (user_id, host_label),
        )
        row = cur.fetchone()
    assert row is not None
    return raw, row


def _assert_untouched(conn, token_id, before):
    with conn.cursor() as cur:
        cur.execute(
            "SELECT expires_at, last_used_at FROM machine_token WHERE id = %s",
            (token_id,),
        )
        after = cur.fetchone()
    assert after is not None
    # ``before`` is the full (id, expires_at, last_used_at) tuple; ``after``
    # is (expires_at, last_used_at) — both must match, i.e. no touch.
    assert after == before[1:], (before, after)


def test_expired_machine_token_sticky_list_403_no_revive(org):
    raw, before = _seeded_row(
        org.conn, org.member["id"], org.workspace["id"],
        "expired-sticky-1714", _past_iso(), None,
    )
    r = org.client().request(
        "GET",
        f"/api/v1/workspaces/{org.workspace['slug']}/stickies/",
        headers={"X-Api-Key": raw},
    )
    assert r.status_code == 403, r.text[:500]
    assert r.json() == {"detail": "Given API token is not valid"}
    _assert_untouched(org.conn, before[0], before)


def test_expired_machine_token_intake_list_403_no_revive(org):
    raw, before = _seeded_row(
        org.conn, org.guest["id"], org.workspace["id"],
        "expired-intake-1714", _past_iso(), None,
    )
    r = org.client().request(
        "GET",
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/intake-issues/",
        headers={"X-Api-Key": raw},
    )
    assert r.status_code == 403, r.text[:500]
    assert r.json() == {"detail": "Given API token is not valid"}
    _assert_untouched(org.conn, before[0], before)


def test_valid_machine_token_sticky_and_intake_list_200(org):
    raw, _before = _seeded_row(
        org.conn, org.admin["id"], org.workspace["id"],
        "valid-1714", _future_iso(), None,
    )
    r = org.client().request(
        "GET",
        f"/api/v1/workspaces/{org.workspace['slug']}/stickies/",
        headers={"X-Api-Key": raw},
    )
    assert r.status_code == 200, r.text[:500]
    r = org.client().request(
        "GET",
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/intake-issues/",
        headers={"X-Api-Key": raw},
    )
    assert r.status_code == 200, r.text[:500]
