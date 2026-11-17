"""NEWPIDASH-285: module-issues legacy date filters truncate in the actor zone.

Django's ``created_at__date`` / ``updated_at__date`` / ``completed_at__date``
are ``TruncDate`` in the ``TimezoneMixin``-activated ``user_timezone``, so
``(col AT TIME ZONE '<user tz>')::date`` decides the match. Rust
``module_filtered_set`` truncated in UTC, diverging from the project
issues list and from Django.

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


def _module_issues_url(tenant, project_id, module_id):
    # Mirror test_module_issues._module_issues_url
    return f"/api/workspaces/{tenant['workspace']['slug']}/projects/{project_id}/modules/{module_id}/issues/"


def _seed_kiritimati_module(seed, tenant_a, project_a, module_a):
    _set_user_timezone(seed, tenant_a["user"]["id"], ACTOR_TZ)
    issue_id = seed.issue(tenant_a["workspace"]["id"], project_a, name="Kiritimati module pinned issue")
    _pin_issue_dates(seed, issue_id)
    seed.module_issue(tenant_a["workspace"]["id"], project_a, module_a, issue_id)
    return issue_id


def test_module_legacy_created_at_uses_actor_timezone(api, seed, tenant_a, project_a, module_a, auth_a):
    issue_id = _seed_kiritimati_module(seed, tenant_a, project_a, module_a)
    base = _module_issues_url(tenant_a, project_a, module_a)
    r = api.get(f"{base}?created_at={TOMORROW};after", headers=auth_a)
    assert r.status_code == 200, r.text[:500]
    assert issue_id in _ids(r.json()), r.json()
    r = api.get(f"{base}?created_at={TODAY};before", headers=auth_a)
    assert r.status_code == 200, r.text[:500]
    assert issue_id not in _ids(r.json()), r.json()


def test_module_legacy_updated_at_uses_actor_timezone(api, seed, tenant_a, project_a, module_a, auth_a):
    issue_id = _seed_kiritimati_module(seed, tenant_a, project_a, module_a)
    base = _module_issues_url(tenant_a, project_a, module_a)
    r = api.get(f"{base}?updated_at={TOMORROW};after", headers=auth_a)
    assert r.status_code == 200, r.text[:500]
    assert issue_id in _ids(r.json()), r.json()
    r = api.get(f"{base}?updated_at={TODAY};before", headers=auth_a)
    assert r.status_code == 200, r.text[:500]
    assert issue_id not in _ids(r.json()), r.json()


def test_module_legacy_completed_at_uses_actor_timezone(api, seed, tenant_a, project_a, module_a, auth_a):
    issue_id = _seed_kiritimati_module(seed, tenant_a, project_a, module_a)
    base = _module_issues_url(tenant_a, project_a, module_a)
    r = api.get(f"{base}?completed_at={TOMORROW};after", headers=auth_a)
    assert r.status_code == 200, r.text[:500]
    assert issue_id in _ids(r.json()), r.json()
    r = api.get(f"{base}?completed_at={TODAY};before", headers=auth_a)
    assert r.status_code == 200, r.text[:500]
    assert issue_id not in _ids(r.json()), r.json()

