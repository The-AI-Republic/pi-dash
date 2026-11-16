"""PATCH intake-issue estimate-key validation (PIDASHCONV-1585).

`IssueCreateSerializer` declares only ``state_id``/``parent_id``/
``assigned_pod_id`` (``serializers/issue.py:137-154``); ``estimate_point``
is the auto-generated FK field with the default-manager queryset
(``EstimatePoint.objects`` — live rows only), while ``estimate_point_id``
is not a serializer field at all, so DRF drops the key. A well-formed
but unknown pk 400s under ``estimate_point`` before ``validate()``;
``validate()`` then rejects ids from another project with the
non-field message (which still says ``estimate_point_id``).
"""

import uuid

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract


def _detail(tenant):
    urls = intake_urls(tenant)
    created = create_intake_issue(tenant["client"], urls["intake_issues"])
    return f"{urls['intake_issues']}{created['issue']['id']}/"


def _make_point(db, workspace_id, project_id):
    est_id, pt_id = str(uuid.uuid4()), str(uuid.uuid4())
    db.execute(
        "INSERT INTO estimates (id, workspace_id, project_id, name, description,"
        " type, last_used, created_at, updated_at)"
        " VALUES (%s,%s,%s,'ep-est','','points', false, now(), now())",
        (est_id, workspace_id, project_id),
    )
    db.execute(
        'INSERT INTO estimate_points (id, workspace_id, project_id, estimate_id,'
        ' "key", description, value, created_at, updated_at)'
        " VALUES (%s,%s,%s,%s,1,'','1',now(),now())",
        (pt_id, workspace_id, project_id, est_id),
    )
    return pt_id


def test_patch_bogus_estimate_point_400s(make_tenant):
    tenant = make_tenant()
    bogus = str(uuid.uuid4())
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"estimate_point": bogus}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "estimate_point": [f'Invalid pk "{bogus}" - object does not exist.']
    }


def test_patch_bogus_estimate_point_id_ignored(make_tenant):
    # `estimate_point_id` is not a serializer field: DRF drops the key,
    # so even a bogus id is a 200 no-op.
    tenant = make_tenant()
    bogus = str(uuid.uuid4())
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"estimate_point_id": bogus}}
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["estimate_point"] is None


def test_patch_estimate_point_wins_over_ignored_id_key(make_tenant):
    # With both keys present the validated `estimate_point` value is
    # stored and the dropped `estimate_point_id` value is irrelevant.
    tenant = make_tenant()
    db = tenant["_database"]
    live = _make_point(
        db, tenant["workspace"]["id"], tenant["project"]["id"]
    )
    bogus = str(uuid.uuid4())
    r = tenant["client"].patch(
        _detail(tenant),
        json={"issue": {"estimate_point": live, "estimate_point_id": bogus}},
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["estimate_point"] == live


def test_patch_bogus_estimate_point_error_ignores_id_key(make_tenant):
    # Both keys bogus: only the validated key reports an error.
    tenant = make_tenant()
    bogus, bogus_id = str(uuid.uuid4()), str(uuid.uuid4())
    r = tenant["client"].patch(
        _detail(tenant),
        json={
            "issue": {
                "estimate_point": bogus,
                "estimate_point_id": bogus_id,
            }
        },
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "estimate_point": [f'Invalid pk "{bogus}" - object does not exist.']
    }


def test_patch_deleted_estimate_point_400s(make_tenant):
    # Soft-deleted rows are excluded from the auto field's queryset.
    tenant = make_tenant()
    db = tenant["_database"]
    pt_id = _make_point(
        db, tenant["workspace"]["id"], tenant["project"]["id"]
    )
    db.execute(
        "UPDATE estimate_points SET deleted_at = now() WHERE id = %s",
        (pt_id,),
    )
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"estimate_point": pt_id}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "estimate_point": [f'Invalid pk "{pt_id}" - object does not exist.']
    }


def test_patch_foreign_estimate_point_rejected(make_tenant):
    # Field validation passes (live row), then validate() rejects the
    # other-project id with the non-field message.
    tenant = make_tenant()
    db = tenant["_database"]
    other = tenant["seeder"].create_project(tenant["workspace"]["id"])
    foreign = _make_point(db, tenant["workspace"]["id"], other["id"])
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"estimate_point": foreign}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "non_field_errors": [
            "Estimate point is not valid please pass a valid estimate_point_id"
        ]
    }


def test_patch_live_estimate_point_moves(make_tenant):
    # The one accepted value: a live id in this project is stored
    # (guards against over-narrowing the field queryset).
    tenant = make_tenant()
    db = tenant["_database"]
    live = _make_point(
        db, tenant["workspace"]["id"], tenant["project"]["id"]
    )
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"estimate_point": live}}
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["estimate_point"] == live


def test_patch_null_estimate_point_clears(make_tenant):
    # `estimate_point: null` clears a previously set value.
    tenant = make_tenant()
    db = tenant["_database"]
    live = _make_point(
        db, tenant["workspace"]["id"], tenant["project"]["id"]
    )
    detail = _detail(tenant)
    r = tenant["client"].patch(
        detail, json={"issue": {"estimate_point": live}}
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["estimate_point"] == live
    r = tenant["client"].patch(detail, json={"issue": {"estimate_point": None}})
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["estimate_point"] is None
