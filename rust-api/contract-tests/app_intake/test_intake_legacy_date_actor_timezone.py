"""NEWPIDASH-310: intake legacy date filters truncate in the actor zone.

Django's ``issue__created_at__date`` / ``issue__updated_at__date`` /
``issue__completed_at__date`` are ``TruncDate`` in the
``TimezoneMixin``-activated ``user_timezone``, so
``(issues.<col> AT TIME ZONE '<user tz>')::date`` decides the match.
Pre-fix Rust truncated with ``"issues"."<col>"::date`` in the session
zone (UTC): any user outside UTC got different rows and counts near
midnight. Twin of NEWPIDASH-211 (cycles) and NEWPIDASH-285/286/287.

2026-11-17T20:00:00Z is 2026-11-18 10:00 in Pacific/Kiritimati
(UTC+14): the pinned item matches ``=<local day>;after`` and misses
``=<UTC day>;before`` in Kiritimati, while a UTC truncation does the
opposite.
"""

import pytest

from .conftest import intake_urls

pytestmark = pytest.mark.contract

# 2026-11-18 10:00 on the Kiritimati wall clock.
PINNED_AT = "2026-11-17T20:00:00+00:00"
ACTOR_TZ = "Pacific/Kiritimati"
# Local (Kiritimati) day vs UTC day for the pinned instant.
LOCAL_DAY = "2026-11-18"
UTC_DAY = "2026-11-17"
# Midday UTC: the same calendar day in UTC and +14, so the idle columns
# never interfere with the column under test.
NEUTRAL_AT = "2026-11-10T12:00:00+00:00"


def _range(day):
    return f"{day};after,{day};before"


def _rows(payload):
    if isinstance(payload, dict) and "results" in payload:
        return payload["results"]
    assert isinstance(payload, list), repr(payload)[:500]
    return payload


def _seed(make_tenant, *, created=NEUTRAL_AT, updated=NEUTRAL_AT, completed=None):
    tenant = make_tenant()
    url = intake_urls(tenant)["intake_issues"]
    r = tenant["client"].post(url, json={"issue": {"name": "TZ pinned", "priority": "none"}})
    assert r.status_code == 200, r.text[:500]
    issue_id = r.json()["issue"]["id"]
    tenant["_database"].execute(
        "UPDATE issues SET created_at = %s, updated_at = %s, completed_at = %s WHERE id = %s",
        (created, updated, completed, issue_id),
    )
    tenant["_database"].execute(
        "UPDATE users SET user_timezone = %s WHERE id = %s",
        (ACTOR_TZ, tenant["user"]["id"]),
    )
    return tenant, url, issue_id


def _listed(tenant, url, param, day):
    r = tenant["client"].get(url, params={param: _range(day)})
    assert r.status_code == 200, r.text[:500]
    return {row["issue"]["id"] for row in _rows(r.json())}


def test_intake_list_created_at_uses_actor_timezone(make_tenant):
    tenant, url, issue_id = _seed(make_tenant, created=PINNED_AT)
    assert issue_id in _listed(tenant, url, "created_at", LOCAL_DAY)
    assert issue_id not in _listed(tenant, url, "created_at", UTC_DAY)


def test_intake_list_updated_at_uses_actor_timezone(make_tenant):
    tenant, url, issue_id = _seed(make_tenant, updated=PINNED_AT)
    assert issue_id in _listed(tenant, url, "updated_at", LOCAL_DAY)
    assert issue_id not in _listed(tenant, url, "updated_at", UTC_DAY)


def test_intake_list_completed_at_uses_actor_timezone(make_tenant):
    tenant, url, issue_id = _seed(make_tenant, completed=PINNED_AT)
    assert issue_id in _listed(tenant, url, "completed_at", LOCAL_DAY)
    assert issue_id not in _listed(tenant, url, "completed_at", UTC_DAY)
