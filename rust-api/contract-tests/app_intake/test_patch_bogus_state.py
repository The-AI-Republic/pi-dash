"""PATCH intake-issue state-key validation (PIDASHCONV-1575, PIDASHCONV-1608).

`IssueCreateSerializer` validates the nested ``issue`` object with two
fields over the same ``state`` source: the declared ``state_id``
(``State.all_state_objects`` queryset) and the auto-generated ``state``
(the default manager, ``State.objects`` — live rows minus triage).
A well-formed but unknown pk 400s under whichever key was sent, before
``validate()`` and before any save. ``validate()`` itself only accepts
triage states on this path (``allow_triage_state=True``), so a live
non-triage id 400s with the non-field message instead.
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


def test_patch_bogus_state_id_400s(make_tenant):
    tenant = make_tenant()
    bogus = str(uuid.uuid4())
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"state_id": bogus}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "state_id": [f'Invalid pk "{bogus}" - object does not exist.']
    }


def test_patch_bogus_state_400s(make_tenant):
    tenant = make_tenant()
    bogus = str(uuid.uuid4())
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"state": bogus}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "state": [f'Invalid pk "{bogus}" - object does not exist.']
    }


def test_patch_bogus_state_keys_merge(make_tenant):
    # Both keys are validated; errors merge with state_id first (DRF
    # applies declared fields before auto fields).
    tenant = make_tenant()
    bogus_id, bogus = str(uuid.uuid4()), str(uuid.uuid4())
    r = tenant["client"].patch(
        _detail(tenant),
        json={"issue": {"state_id": bogus_id, "state": bogus}},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "state_id": [f'Invalid pk "{bogus_id}" - object does not exist.'],
        "state": [f'Invalid pk "{bogus}" - object does not exist.'],
    }


def test_patch_triage_state_key_400s(make_tenant):
    # Triage rows are excluded from the auto field's queryset, so the
    # triage id the issue already sits in 400s under the `state` key.
    tenant = make_tenant()
    detail = _detail(tenant)
    triage_id = tenant["_database"].fetchval(
        "SELECT id FROM states WHERE project_id = %s AND \"group\" = 'triage'"
        " AND deleted_at IS NULL",
        (tenant["project"]["id"],),
    )
    r = tenant["client"].patch(
        detail, json={"issue": {"state": str(triage_id)}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "state": [f'Invalid pk "{triage_id}" - object does not exist.']
    }


def test_patch_live_state_id_rejected_as_non_triage(make_tenant):
    # Field validation passes (all_state_objects), then validate()
    # rejects the non-triage id because allow_triage_state is set.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    live = seeder.create_state(
        tenant["workspace"]["id"], tenant["project"]["id"], name="Live pick"
    )
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"state_id": live["id"]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "non_field_errors": ["State is not valid please pass a valid state_id"]
    }


def test_patch_live_state_key_rejected_as_non_triage(make_tenant):
    # Same validate() rejection through the `state` key: the id is in
    # the default-manager queryset, but it is not a triage state.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    live = seeder.create_state(
        tenant["workspace"]["id"], tenant["project"]["id"], name="Live twin"
    )
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"state": live["id"]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "non_field_errors": ["State is not valid please pass a valid state_id"]
    }


def test_patch_null_state_key_clears_to_default(make_tenant):
    # `state: null` clears like `state_id: null`; Issue.save() then
    # re-defaults to the project's default state on both backends.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    live = seeder.create_state(
        tenant["workspace"]["id"], tenant["project"]["id"], name="Default pick"
    )
    tenant["_database"].execute(
        'UPDATE states SET "default" = true, sequence = 1 WHERE id = %s',
        (live["id"],),
    )
    r = tenant["client"].patch(_detail(tenant), json={"issue": {"state": None}})
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["state_id"] == live["id"]


def test_patch_triage_state_id_moves(make_tenant):
    # The one accepted value: a triage id in this project moves the
    # issue (guards against over-narrowing the validate() check).
    tenant = make_tenant()
    detail = _detail(tenant)
    triage_id = tenant["_database"].fetchval(
        "SELECT id FROM states WHERE project_id = %s AND \"group\" = 'triage'"
        " AND deleted_at IS NULL",
        (tenant["project"]["id"],),
    )
    r = tenant["client"].patch(
        detail, json={"issue": {"state_id": str(triage_id)}}
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["state_id"] == str(triage_id)


def test_patch_deleted_state_key_400s(make_tenant):
    # Soft-deleted rows are outside the auto field's queryset
    # (State.objects), so the id 400s under the `state` key.
    # (PIDASHCONV-1608; probed live on Django first.)
    tenant = make_tenant()
    seeder = tenant["seeder"]
    state = seeder.create_state(
        tenant["workspace"]["id"], tenant["project"]["id"], name="doomed state"
    )
    tenant["_database"].execute(
        "UPDATE states SET deleted_at = now() WHERE id = %s", (state["id"],)
    )
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"state": state["id"]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "state": [f'Invalid pk "{state["id"]}" - object does not exist.']
    }


def test_patch_deleted_state_id_rejected_as_non_triage(make_tenant):
    # The declared key resolves the soft-deleted row (all_state_objects
    # has no deleted_at scope), then validate() rejects it: the triage
    # check requires a live row. (PIDASHCONV-1608; probed live on Django
    # first — the body is the same non-field message as a live id.)
    tenant = make_tenant()
    seeder = tenant["seeder"]
    state = seeder.create_state(
        tenant["workspace"]["id"], tenant["project"]["id"], name="doomed id"
    )
    tenant["_database"].execute(
        "UPDATE states SET deleted_at = now() WHERE id = %s", (state["id"],)
    )
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"state_id": state["id"]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "non_field_errors": ["State is not valid please pass a valid state_id"]
    }


def test_patch_null_state_id_clears_to_default(make_tenant):
    # `state_id: null` clears exactly like `state: null`; Issue.save()
    # then re-defaults to the project's default state on both backends.
    # (PIDASHCONV-1608; probed live on Django first.)
    tenant = make_tenant()
    seeder = tenant["seeder"]
    live = seeder.create_state(
        tenant["workspace"]["id"], tenant["project"]["id"], name="Default twin"
    )
    tenant["_database"].execute(
        'UPDATE states SET "default" = true, sequence = 1 WHERE id = %s',
        (live["id"],),
    )
    r = tenant["client"].patch(_detail(tenant), json={"issue": {"state_id": None}})
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["state_id"] == live["id"]


def test_patch_xproject_state_key_rejected(make_tenant):
    # Field validation passes (the queryset is global), then validate()
    # rejects the cross-project id through the `state` key, like the
    # declared key. (PIDASHCONV-1608; probed live on Django first.)
    tenant = make_tenant()
    seeder = tenant["seeder"]
    other_ws = seeder.create_workspace(tenant["user"]["id"])
    other_proj = seeder.create_project(other_ws["id"])
    foreign = seeder.create_state(other_ws["id"], other_proj["id"], name="Foreign")
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {"state": foreign["id"]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "non_field_errors": ["State is not valid please pass a valid state_id"]
    }
