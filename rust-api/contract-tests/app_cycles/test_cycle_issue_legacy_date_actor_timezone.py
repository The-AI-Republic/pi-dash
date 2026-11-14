"""NEWPIDASH-211: cycle-issues legacy date filters truncate in the actor zone.

Django's ``created_at__date`` / ``updated_at__date`` / ``completed_at__date``
are ``TruncDate`` in the ``TimezoneMixin``-activated ``user_timezone``, so
``(col AT TIME ZONE '<user tz>')::date`` decides the match. Rust
``cycle_filtered_set`` truncated in UTC, diverging from the project
issues list (``gate.timezone``) and from Django.

2026-11-13T20:00:00Z is 2026-11-14 10:00 in Pacific/Kiritimati (UTC+14):
the pinned issue matches ``created_at=2026-11-14;after`` and misses
``created_at=2026-11-13;before`` in Kiritimati, while a UTC truncation
does the opposite.
"""

import pytest

from .conftest import create_issue_for, cycle_urls

pytestmark = pytest.mark.contract

PINNED_AT = "2026-11-13T20:00:00+00:00"
ACTOR_TZ = "Pacific/Kiritimati"
# Local (Kiritimati) tomorrow vs UTC today for the pinned instant.
TOMORROW = "2026-11-14"
TODAY = "2026-11-13"


def _set_user_timezone(tenant, tz):
    with tenant["seed"].conn.cursor() as cur:
        cur.execute(
            "update users set user_timezone=%s where id=%s",
            (tz, tenant["user"]["id"]),
        )
    tenant["seed"].conn.commit()


def _pin_issue_dates(tenant, issue_id):
    with tenant["seed"].conn.cursor() as cur:
        cur.execute(
            "update issues set created_at=%s, updated_at=%s, completed_at=%s where id=%s",
            (PINNED_AT, PINNED_AT, PINNED_AT, issue_id),
        )
    tenant["seed"].conn.commit()


def _ids(body):
    results = body.get("results", [])
    # Flat list: results is a list of rows; grouped is a dict — this test
    # only uses the flat path.
    assert isinstance(results, list), body
    return {row["id"] for row in results}


def _seed_kiritimati_cycle(tenant):
    _set_user_timezone(tenant, ACTOR_TZ)
    issue = create_issue_for(tenant, name="Kiritimati pinned issue")
    _pin_issue_dates(tenant, issue["id"])
    urls = cycle_urls(tenant)
    r = tenant["client"].post(
        urls["cycle_issues"](tenant["cycle"]["id"]),
        json={"issues": [issue["id"]]},
    )
    assert r.status_code == 201, r.text[:500]
    return issue["id"]


def _teardown_bridge(tenant):
    with tenant["seed"].conn.cursor() as cur:
        cur.execute(
            "delete from cycle_issues where workspace_id=%s and issue_id is not null",
            (tenant["workspace"]["id"],),
        )
    tenant["seed"].conn.commit()


def test_cycle_legacy_created_at_uses_actor_timezone(make_tenant):
    tenant = make_tenant()
    try:
        issue_id = _seed_kiritimati_cycle(tenant)
        base = cycle_urls(tenant)["cycle_issues"](tenant["cycle"]["id"])
        # Django: local date 2026-11-14 is > 2026-11-14? off-by-one is
        # ``after`` -> >=, so the pinned row is included.
        r = tenant["client"].get(f"{base}?created_at={TOMORROW};after")
        assert r.status_code == 200, r.text[:500]
        assert issue_id in _ids(r.json()), r.json()
        # Local date 2026-11-14 is not <= 2026-11-13, so excluded.
        r = tenant["client"].get(f"{base}?created_at={TODAY};before")
        assert r.status_code == 200, r.text[:500]
        assert issue_id not in _ids(r.json()), r.json()
    finally:
        _teardown_bridge(tenant)


def test_cycle_legacy_updated_at_uses_actor_timezone(make_tenant):
    tenant = make_tenant()
    try:
        issue_id = _seed_kiritimati_cycle(tenant)
        base = cycle_urls(tenant)["cycle_issues"](tenant["cycle"]["id"])
        r = tenant["client"].get(f"{base}?updated_at={TOMORROW};after")
        assert r.status_code == 200, r.text[:500]
        assert issue_id in _ids(r.json()), r.json()
        r = tenant["client"].get(f"{base}?updated_at={TODAY};before")
        assert r.status_code == 200, r.text[:500]
        assert issue_id not in _ids(r.json()), r.json()
    finally:
        _teardown_bridge(tenant)


def test_cycle_legacy_completed_at_uses_actor_timezone(make_tenant):
    tenant = make_tenant()
    try:
        issue_id = _seed_kiritimati_cycle(tenant)
        base = cycle_urls(tenant)["cycle_issues"](tenant["cycle"]["id"])
        r = tenant["client"].get(f"{base}?completed_at={TOMORROW};after")
        assert r.status_code == 200, r.text[:500]
        assert issue_id in _ids(r.json()), r.json()
        r = tenant["client"].get(f"{base}?completed_at={TODAY};before")
        assert r.status_code == 200, r.text[:500]
        assert issue_id not in _ids(r.json()), r.json()
    finally:
        _teardown_bridge(tenant)
