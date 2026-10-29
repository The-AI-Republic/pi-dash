"""PATCH intake-issue parent-key validation (PIDASHCONV-1584).

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


def _detail(tenant):
    urls = intake_urls(tenant)
    created = create_intake_issue(tenant["client"], urls["intake_issues"])
    return f"{urls['intake_issues']}{created['issue']['id']}/"


def test_patch_bogus_parent_id_400s(make_tenant):
    tenant = make_tenant()
    bogus = str(uuid.uuid4())
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"parent_id": bogus}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "parent_id": [f'Invalid pk "{bogus}" - object does not exist.']
    }


def test_patch_bogus_parent_400s(make_tenant):
    tenant = make_tenant()
    bogus = str(uuid.uuid4())
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"parent": bogus}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "parent": [f'Invalid pk "{bogus}" - object does not exist.']
    }


def test_patch_bogus_parent_keys_merge(make_tenant):
    # Both keys are validated; errors merge with parent_id first (DRF
    # applies declared fields before auto fields).
    tenant = make_tenant()
    bogus_id, bogus = str(uuid.uuid4()), str(uuid.uuid4())
    r = tenant["client"].patch(
        _detail(tenant),
        json={"issue": {"parent_id": bogus_id, "parent": bogus}},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "parent_id": [f'Invalid pk "{bogus_id}" - object does not exist.'],
        "parent": [f'Invalid pk "{bogus}" - object does not exist.'],
    }


def test_patch_triage_parent_key_400s(make_tenant):
    # Triage rows are excluded from the auto field's queryset, so another
    # intake issue (triage state) 400s under the `parent` key.
    tenant = make_tenant()
    detail = _detail(tenant)
    triage = create_intake_issue(tenant["client"], intake_urls(tenant)["intake_issues"])
    triage_id = triage["issue"]["id"]
    r = tenant["client"].patch(
        detail, json={"issue": {"parent": triage_id}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "parent": [f'Invalid pk "{triage_id}" - object does not exist.']
    }


def test_patch_triage_parent_id_moves(make_tenant):
    # The same triage id resolves under the declared key (Issue.objects)
    # and passes validate() in the same project.
    tenant = make_tenant()
    detail = _detail(tenant)
    triage = create_intake_issue(tenant["client"], intake_urls(tenant)["intake_issues"])
    triage_id = triage["issue"]["id"]
    r = tenant["client"].patch(
        detail, json={"issue": {"parent_id": triage_id}}
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["parent_id"] == triage_id


def test_patch_parent_key_wins(make_tenant):
    # Both keys valid but different: the auto `parent` value lands.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    ws_id, proj_id = tenant["workspace"]["id"], tenant["project"]["id"]
    backlog = seeder.create_state(ws_id, proj_id, name="Live pick")
    first = seeder.create_issue(ws_id, proj_id, backlog["id"], name="Parent X")
    second = seeder.create_issue(ws_id, proj_id, backlog["id"], name="Parent Y")
    r = tenant["client"].patch(
        _detail(tenant),
        json={"issue": {"parent_id": first["id"], "parent": second["id"]}},
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["parent_id"] == second["id"]


def test_patch_null_parent_key_clears(make_tenant):
    # `parent: null` clears like `parent_id: null`.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    ws_id, proj_id = tenant["workspace"]["id"], tenant["project"]["id"]
    backlog = seeder.create_state(ws_id, proj_id, name="Clear pick")
    parent = seeder.create_issue(ws_id, proj_id, backlog["id"], name="Doomed parent")
    detail = _detail(tenant)
    r = tenant["client"].patch(detail, json={"issue": {"parent_id": parent["id"]}})
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["parent_id"] == parent["id"]
    r = tenant["client"].patch(detail, json={"issue": {"parent": None}})
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["parent_id"] is None


def test_patch_xproject_parent_key_rejected(make_tenant):
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
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"parent": foreign["id"]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "non_field_errors": ["Parent is not valid issue_id please pass a valid issue_id"]
    }
