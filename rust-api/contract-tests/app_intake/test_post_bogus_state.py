"""POST intake-issue state-key validation (PIDASHCONV-1591).

``IssueCreateSerializer`` validates the nested ``issue`` object with two
fields over the same ``state`` source: the declared ``state_id``
(``State.all_state_objects`` queryset — but dead on this path, since the
intake view overwrites it with the server-side triage row before
validation) and the auto-generated ``state`` (the default manager,
``State.objects`` — live rows minus triage-group states).
A well-formed but unknown pk 400s under the ``state`` key, before
``validate()`` and before any save. A live non-triage id passes the
field check but ``validate()`` rejects it (the intake context only
accepts triage states). Explicit null replays ``Issue.save``'s
default-state fill instead of triage.
"""

import uuid

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract


def _post_create(tenant):
    return intake_urls(tenant)["intake_issues"]


def test_post_bogus_state_400s(make_tenant):
    tenant = make_tenant()
    bogus = str(uuid.uuid4())
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "state": bogus}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "state": [f'Invalid pk "{bogus}" - object does not exist.']
    }


def test_post_deleted_state_400s(make_tenant):
    # Soft-deleted rows are outside the auto field's queryset, so the
    # id 400s under the `state` key (unlike under the declared
    # `state_id`, whose all_state_objects queryset still resolves).
    tenant = make_tenant()
    seeder = tenant["seeder"]
    state = seeder.create_state(
        tenant["workspace"]["id"], tenant["project"]["id"], name="doomed state"
    )
    tenant["_database"].execute(
        "UPDATE states SET deleted_at = now() WHERE id = %s", (state["id"],)
    )
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "state": state["id"]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "state": [f'Invalid pk "{state["id"]}" - object does not exist.']
    }


def test_post_triage_state_400s(make_tenant):
    # Triage-group rows are excluded from the auto field's queryset, so
    # the project's own triage row 400s under the `state` key.
    tenant = make_tenant()
    triage = create_intake_issue(tenant["client"], _post_create(tenant))
    triage_id = triage["issue"]["state_id"]
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "state": triage_id}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "state": [f'Invalid pk "{triage_id}" - object does not exist.']
    }


def test_post_valid_state_rejected(make_tenant):
    # A live non-triage id passes the field check, then validate()
    # rejects it: the intake context only accepts triage states.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    live = seeder.create_state(
        tenant["workspace"]["id"], tenant["project"]["id"], name="Live pick"
    )
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "state": live["id"]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "non_field_errors": ["State is not valid please pass a valid state_id"]
    }


def test_post_xproject_state_rejected(make_tenant):
    # Same rejection for a live state from another project.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    other_ws = seeder.create_workspace(tenant["user"]["id"])
    other_proj = seeder.create_project(other_ws["id"])
    foreign = seeder.create_state(other_ws["id"], other_proj["id"], name="Foreign")
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "state": foreign["id"]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "non_field_errors": ["State is not valid please pass a valid state_id"]
    }


def test_post_null_state_fills_default(make_tenant):
    # `state: null` beats the server-set triage id (auto field wins)
    # and replays Issue.save's fill: the project's default state.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    default = seeder.create_state(
        tenant["workspace"]["id"], tenant["project"]["id"], name="Default pick"
    )
    tenant["_database"].execute(
        'UPDATE states SET "default" = true WHERE id = %s', (default["id"],)
    )
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "state": None}}
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["state_id"] == default["id"]


def test_post_null_state_falls_back(make_tenant):
    # With no default state, the fill takes the lowest-sequence
    # non-triage state (Meta.ordering).
    tenant = make_tenant()
    seeder = tenant["seeder"]
    ws_id, proj_id = tenant["workspace"]["id"], tenant["project"]["id"]
    hi = seeder.create_state(ws_id, proj_id, name="Hi seq")
    lo = seeder.create_state(ws_id, proj_id, name="Lo seq")
    tenant["_database"].execute(
        "UPDATE states SET sequence = 70000 WHERE id = %s", (hi["id"],)
    )
    tenant["_database"].execute(
        "UPDATE states SET sequence = 1000 WHERE id = %s", (lo["id"],)
    )
    tenant["_database"].execute(
        'UPDATE states SET "default" = false WHERE project_id = %s', (proj_id,)
    )
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "state": None}}
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["state_id"] == lo["id"]


def test_post_state_error_sorts_with_auto_fields(make_tenant):
    # `state` is an auto field near the end of DRF's field order, so
    # its error sorts after earlier auto fields (point) and before
    # later ones (estimate_point).
    tenant = make_tenant()
    bogus = str(uuid.uuid4())
    r = tenant["client"].post(
        _post_create(tenant),
        json={
            "issue": {
                "name": "c",
                "state": bogus,
                "estimate_point": bogus,
                "point": 99,
            }
        },
    )
    assert r.status_code == 400, r.text[:500]
    assert list(r.json().keys()) == ["point", "state", "estimate_point"]
    assert r.json()["state"] == [
        f'Invalid pk "{bogus}" - object does not exist.'
    ]


def test_post_bogus_state_id_declared_dead(make_tenant):
    # The declared `state_id` input is dead on this path (the view
    # overwrites it with triage before validation), so a bogus
    # `state_id` alongside a bogus `state` still errors only under
    # `state` — there is no winner question.
    tenant = make_tenant()
    bogus = str(uuid.uuid4())
    r = tenant["client"].post(
        _post_create(tenant),
        json={
            "issue": {
                "name": "c",
                "state_id": str(uuid.uuid4()),
                "state": bogus,
            }
        },
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "state": [f'Invalid pk "{bogus}" - object does not exist.']
    }
