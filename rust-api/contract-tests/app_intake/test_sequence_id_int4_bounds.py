"""Intake-issue ``sequence_id`` int4 bounds (NEWPIDASH-311).

``Issue.sequence_id`` is a Django ``IntegerField``, so DRF adds
``MinValueValidator(-2147483648)`` / ``MaxValueValidator(2147483647)``:
an out-of-range value 400s with the bound message and nothing is
written. The Rust PATCH arm validated the integer without the int4
bound and wrote it with ``as i32``, so 4294967297 silently wrapped to
1 (200); the create arm rejected out-of-range values but with the
generic message instead of Django's bound message.
"""

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract

MAX = 2147483647
MIN = -2147483648
MAX_MESSAGE = "Ensure this value is less than or equal to 2147483647."
MIN_MESSAGE = "Ensure this value is greater than or equal to -2147483648."


def _detail(tenant):
    urls = intake_urls(tenant)
    created = create_intake_issue(tenant["client"], urls["intake_issues"])
    return f"{urls['intake_issues']}{created['issue']['id']}/", created["issue"]["id"]


def _stored_sequence(tenant, issue_id):
    return tenant["_database"].fetchval(
        "SELECT sequence_id FROM issues WHERE id = %s",
        (issue_id,),
    )


def test_patch_sequence_id_above_max_400s(make_tenant):
    tenant = make_tenant()
    detail, issue_id = _detail(tenant)
    before = _stored_sequence(tenant, issue_id)
    r = tenant["client"].patch(detail, json={"issue": {"sequence_id": MAX + 2147483650}})
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"sequence_id": [MAX_MESSAGE]}
    assert _stored_sequence(tenant, issue_id) == before


def test_patch_sequence_id_at_max_200s(make_tenant):
    tenant = make_tenant()
    detail, issue_id = _detail(tenant)
    r = tenant["client"].patch(detail, json={"issue": {"sequence_id": MAX}})
    assert r.status_code == 200, r.text[:500]
    assert _stored_sequence(tenant, issue_id) == MAX


def test_patch_sequence_id_below_min_400s(make_tenant):
    tenant = make_tenant()
    detail, issue_id = _detail(tenant)
    before = _stored_sequence(tenant, issue_id)
    r = tenant["client"].patch(detail, json={"issue": {"sequence_id": MIN - 1}})
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"sequence_id": [MIN_MESSAGE]}
    assert _stored_sequence(tenant, issue_id) == before


def test_patch_sequence_id_at_min_200s(make_tenant):
    tenant = make_tenant()
    detail, issue_id = _detail(tenant)
    r = tenant["client"].patch(detail, json={"issue": {"sequence_id": MIN}})
    assert r.status_code == 200, r.text[:500]
    assert _stored_sequence(tenant, issue_id) == MIN


def test_create_sequence_id_above_max_400s(make_tenant):
    tenant = make_tenant()
    urls = intake_urls(tenant)
    r = tenant["client"].post(
        urls["intake_issues"],
        json={"issue": {"name": "Bounded", "sequence_id": MAX + 2147483650}},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"sequence_id": [MAX_MESSAGE]}
