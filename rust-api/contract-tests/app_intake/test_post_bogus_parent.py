"""POST intake-issue parent-key validation (PIDASHCONV-1588).

``IssueCreateSerializer`` validates the nested ``issue`` object with two
fields over the same ``parent`` source: the declared ``parent_id``
(``Issue.objects`` queryset — soft-delete scope only) and the
auto-generated ``parent`` (the default manager, ``Issue.issue_objects`` —
live rows minus triage/draft/archived).
A well-formed but unknown pk 400s under whichever key was sent, before
``validate()`` and before any save. When both keys are sent, ``parent``
wins (DRF applies declared fields before auto fields). ``validate()``
itself only accepts same-project parents on this path.
"""

import uuid

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract


def _post_create(tenant):
    return intake_urls(tenant)["intake_issues"]


def test_post_bogus_parent_id_400s(make_tenant):
    tenant = make_tenant()
    bogus = str(uuid.uuid4())
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "parent_id": bogus}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "parent_id": [f'Invalid pk "{bogus}" - object does not exist.']
    }


def test_post_bogus_parent_400s(make_tenant):
    tenant = make_tenant()
    bogus = str(uuid.uuid4())
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "parent": bogus}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "parent": [f'Invalid pk "{bogus}" - object does not exist.']
    }


def test_post_bogus_parent_keys_merge(make_tenant):
    # Both keys are validated; errors merge with parent_id first (DRF
    # applies declared fields before auto fields), regardless of the
    # input key order.
    tenant = make_tenant()
    bogus_id, bogus = str(uuid.uuid4()), str(uuid.uuid4())
    r = tenant["client"].post(
        _post_create(tenant),
        json={"issue": {"name": "c", "parent": bogus, "parent_id": bogus_id}},
    )
    assert r.status_code == 400, r.text[:500]
    assert list(r.json().keys()) == ["parent_id", "parent"]
    assert r.json() == {
        "parent_id": [f'Invalid pk "{bogus_id}" - object does not exist.'],
        "parent": [f'Invalid pk "{bogus}" - object does not exist.'],
    }


def test_post_parent_error_sorts_with_auto_fields(make_tenant):
    # `parent` is an auto field near the end of DRF's field order, so
    # its error sorts after earlier auto fields (point) and after the
    # declared key (parent_id).
    tenant = make_tenant()
    bogus_id, bogus = str(uuid.uuid4()), str(uuid.uuid4())
    r = tenant["client"].post(
        _post_create(tenant),
        json={
            "issue": {
                "name": "c",
                "parent_id": bogus_id,
                "parent": bogus,
                "point": 99,
            }
        },
    )
    assert r.status_code == 400, r.text[:500]
    assert list(r.json().keys()) == ["parent_id", "point", "parent"]
    assert r.json()["parent"] == [
        f'Invalid pk "{bogus}" - object does not exist.'
    ]


def test_post_triage_parent_key_400s(make_tenant):
    # Triage rows are excluded from the auto field's queryset, so another
    # intake issue (triage state) 400s under the `parent` key.
    tenant = make_tenant()
    triage = create_intake_issue(tenant["client"], _post_create(tenant))
    triage_id = triage["issue"]["id"]
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "parent": triage_id}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "parent": [f'Invalid pk "{triage_id}" - object does not exist.']
    }


def test_post_triage_parent_id_sets(make_tenant):
    # The same triage id resolves under the declared key (Issue.objects)
    # and passes validate() in the same project.
    tenant = make_tenant()
    triage = create_intake_issue(tenant["client"], _post_create(tenant))
    triage_id = triage["issue"]["id"]
    r = tenant["client"].post(
        _post_create(tenant),
        json={"issue": {"name": "c", "parent_id": triage_id}},
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["parent_id"] == triage_id


def test_post_parent_key_wins(make_tenant):
    # Both keys valid but different: the auto `parent` value lands.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    ws_id, proj_id = tenant["workspace"]["id"], tenant["project"]["id"]
    backlog = seeder.create_state(ws_id, proj_id, name="Live pick")
    first = seeder.create_issue(ws_id, proj_id, backlog["id"], name="Parent X")
    second = seeder.create_issue(ws_id, proj_id, backlog["id"], name="Parent Y")
    r = tenant["client"].post(
        _post_create(tenant),
        json={"issue": {"name": "c", "parent_id": first["id"], "parent": second["id"]}},
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["parent_id"] == second["id"]


def test_post_null_parent_key_clears(make_tenant):
    # `parent: null` beats a valid `parent_id` (auto field wins even for
    # null), and alone it creates a parentless issue.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    ws_id, proj_id = tenant["workspace"]["id"], tenant["project"]["id"]
    backlog = seeder.create_state(ws_id, proj_id, name="Clear pick")
    parent = seeder.create_issue(ws_id, proj_id, backlog["id"], name="Doomed parent")
    r = tenant["client"].post(
        _post_create(tenant),
        json={"issue": {"name": "c", "parent_id": parent["id"], "parent": None}},
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["parent_id"] is None
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "parent": None}}
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["parent_id"] is None


def test_post_xproject_parent_key_rejected(make_tenant):
    # Field validation passes (both querysets are global), then
    # validate() rejects the cross-project id through the `parent` key.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    other_ws = seeder.create_workspace(tenant["user"]["id"])
    other_proj = seeder.create_project(other_ws["id"])
    other_state = seeder.create_state(other_ws["id"], other_proj["id"], name="Foreign")
    foreign = seeder.create_issue(
        other_ws["id"], other_proj["id"], other_state["id"], name="Foreign issue"
    )
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "parent": foreign["id"]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "non_field_errors": ["Parent is not valid issue_id please pass a valid issue_id"]
    }


def test_post_deleted_parent_key_400s(make_tenant):
    # Soft-deleted rows are outside both querysets, so the id 400s under
    # the `parent` key exactly like under `parent_id`.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    parent = seeder.create_issue(
        tenant["workspace"]["id"], tenant["project"]["id"], name="doomed parent"
    )
    tenant["_database"].execute(
        "UPDATE issues SET deleted_at = now() WHERE id = %s", (parent["id"],)
    )
    r = tenant["client"].post(
        _post_create(tenant),
        json={"issue": {"name": "c", "parent": parent["id"]}},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "parent": [f'Invalid pk "{parent["id"]}" - object does not exist.']
    }
