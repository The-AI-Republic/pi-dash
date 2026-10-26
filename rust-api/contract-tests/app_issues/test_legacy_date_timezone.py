"""Legacy ``created_at``/``completed_at`` date filters use the actor timezone.

PIDASHCONV-1497: Django's ``created_at__date`` / ``completed_at__date``
lookups are ``TruncDate`` in the ``TimezoneMixin``-activated user zone, so
``(col AT TIME ZONE '<user tz>')::date`` decides the match. Pre-fix Rust
truncated with ``issue.<col>::date`` in the session zone (UTC): any user
outside UTC got different rows and counts near midnight.

2026-01-01T20:00:00Z is 2026-01-02 05:00 in Asia/Tokyo: the pinned issue
matches ``=2026-01-02;after`` and misses ``=2026-01-01;before`` in Tokyo,
while a UTC truncation does the opposite. Membership (not counts) is
asserted because the shared seed holds other NOW-dated issues.
"""

import pytest

# 2026-01-02 05:00 on the Tokyo wall clock.
_TZ_PINNED_AT = "2026-01-01T20:00:00+00:00"


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


@pytest.fixture()
def tokyo_pinned_issue(clients, seed, db_conn):
    """API-created issue with pinned dates + Tokyo member, both restored."""
    admin = clients["admin"]
    base = _base(seed["ws_slug"], seed["project"])
    resp = admin.post(f"{base}/issues/", json={"name": "TZ pinned"})
    assert resp.status_code == 201, resp.text
    issue_id = resp.json()["id"]
    with db_conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET created_at = %s, updated_at = %s,"
            " completed_at = %s WHERE id = %s",
            (_TZ_PINNED_AT, _TZ_PINNED_AT, _TZ_PINNED_AT, issue_id),
        )
        cur.execute(
            "UPDATE users SET user_timezone = %s WHERE id = %s",
            ("Asia/Tokyo", seed["member"]),
        )
    try:
        yield issue_id
    finally:
        with db_conn.cursor() as cur:
            cur.execute(
                "UPDATE users SET user_timezone = %s WHERE id = %s",
                ("UTC", seed["member"]),
            )
        admin.delete(f"{base}/issues/{issue_id}/")


def _ids(body):
    return {row["id"] for row in body["results"]}


def test_legacy_created_at_filter_uses_actor_timezone(
    clients, seed, tokyo_pinned_issue
):
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    resp = member.get(f"{base}/issues/?created_at=2026-01-02;after")
    assert resp.status_code == 200, resp.text
    assert tokyo_pinned_issue in _ids(resp.json())
    resp = member.get(f"{base}/issues/?created_at=2026-01-01;before")
    assert resp.status_code == 200, resp.text
    assert tokyo_pinned_issue not in _ids(resp.json())


def test_legacy_completed_at_filter_uses_actor_timezone(
    clients, seed, tokyo_pinned_issue
):
    member = clients["member"]
    base = _base(seed["ws_slug"], seed["project"])
    resp = member.get(f"{base}/issues/?completed_at=2026-01-02;after")
    assert resp.status_code == 200, resp.text
    assert tokyo_pinned_issue in _ids(resp.json())
    resp = member.get(f"{base}/issues/?completed_at=2026-01-01;before")
    assert resp.status_code == 200, resp.text
    assert tokyo_pinned_issue not in _ids(resp.json())
