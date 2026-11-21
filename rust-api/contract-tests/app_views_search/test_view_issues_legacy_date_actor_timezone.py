"""NEWPIDASH-286: workspace issues list legacy date filters use actor zone.

Django's ``created_at__date`` / ``updated_at__date`` / ``completed_at__date``
are ``TruncDate`` in the ``TimezoneMixin``-activated ``user_timezone``, so
``(col AT TIME ZONE '<user tz>')::date`` decides the match. Rust
``view_issues_list`` compiled the legacy predicates in ``chrono_tz::UTC``
while the rich tree on the same route already used the actor zone
(NEWPIDASH-211 twin, fixed there for cycle issues).

2026-11-13T20:00:00Z is 2026-11-14 10:00 in Pacific/Kiritimati (UTC+14):
the pinned issue matches ``created_at=2026-11-14;after`` and misses
``created_at=2026-11-13;before`` in Kiritimati, while a UTC truncation
does the opposite.
"""

import pytest

pytestmark = pytest.mark.contract

PINNED_AT = "2026-11-13T20:00:00+00:00"
ACTOR_TZ = "Pacific/Kiritimati"
# Local (Kiritimati) tomorrow vs UTC today for the pinned instant.
TOMORROW = "2026-11-14"
TODAY = "2026-11-13"


def _set_user_timezone(seed, user_id, tz):
    with seed.conn.cursor() as cur:
        cur.execute(
            "update users set user_timezone=%s where id=%s",
            (tz, user_id),
        )
    seed.conn.commit()


def _pin_issue_dates(seed, issue_id):
    with seed.conn.cursor() as cur:
        cur.execute(
            "update issues set created_at=%s, updated_at=%s, completed_at=%s where id=%s",
            (PINNED_AT, PINNED_AT, PINNED_AT, issue_id),
        )
    seed.conn.commit()


def _ids(body):
    results = body.get("results", [])
    assert isinstance(results, list), body
    return {row["id"] for row in results}


def _seed_kiritimati_issue(seed, tenant, project_id):
    _set_user_timezone(seed, tenant["user"]["id"], ACTOR_TZ)
    issue_id = seed.issue(
        tenant["workspace"]["id"], project_id, name="Kiritimati pinned issue"
    )
    _pin_issue_dates(seed, issue_id)
    return issue_id


def _issues_url(tenant):
    return f"/api/workspaces/{tenant['workspace']['slug']}/issues/"


def test_view_issues_legacy_created_at_uses_actor_timezone(
    api, seed, tenant_a, project_a, auth_a
):
    issue_id = _seed_kiritimati_issue(seed, tenant_a, project_a)
    base = _issues_url(tenant_a)
    # Django: local date 2026-11-14 is > 2026-11-14? off-by-one is
    # ``after`` -> >=, so the pinned row is included.
    r = api.get(f"{base}?created_at={TOMORROW};after", headers=auth_a)
    assert r.status_code == 200, r.text
    assert issue_id in _ids(r.json()), r.json()
    # Local date 2026-11-14 is not <= 2026-11-13, so excluded.
    r = api.get(f"{base}?created_at={TODAY};before", headers=auth_a)
    assert r.status_code == 200, r.text
    assert issue_id not in _ids(r.json()), r.json()


def test_view_issues_legacy_updated_at_uses_actor_timezone(
    api, seed, tenant_a, project_a, auth_a
):
    issue_id = _seed_kiritimati_issue(seed, tenant_a, project_a)
    base = _issues_url(tenant_a)
    r = api.get(f"{base}?updated_at={TOMORROW};after", headers=auth_a)
    assert r.status_code == 200, r.text
    assert issue_id in _ids(r.json()), r.json()
    r = api.get(f"{base}?updated_at={TODAY};before", headers=auth_a)
    assert r.status_code == 200, r.text
    assert issue_id not in _ids(r.json()), r.json()


def test_view_issues_legacy_completed_at_uses_actor_timezone(
    api, seed, tenant_a, project_a, auth_a
):
    issue_id = _seed_kiritimati_issue(seed, tenant_a, project_a)
    base = _issues_url(tenant_a)
    r = api.get(f"{base}?completed_at={TOMORROW};after", headers=auth_a)
    assert r.status_code == 200, r.text
    assert issue_id in _ids(r.json()), r.json()
    r = api.get(f"{base}?completed_at={TODAY};before", headers=auth_a)
    assert r.status_code == 200, r.text
    assert issue_id not in _ids(r.json()), r.json()
