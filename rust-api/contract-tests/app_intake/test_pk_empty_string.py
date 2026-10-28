"""Empty-string pk keys on intake POST/PATCH (PIDASHCONV-1607).

DRF ``RelatedField.run_validation`` forces ``""`` to ``None`` before
validation, so ``""`` on a relational field answers the field's
``allow_null`` rule instead of the UUID parse: every scalar pk key on
this path is ``allow_null=True`` (declared fields say so; the auto
twins inherit it from their null FKs), so ``""`` behaves exactly like
explicit null — including the ``state`` twin overriding triage with
the save-default fill, and PATCH clears. List children carry
``allow_null=False``, so ``[""]`` fails null per index, while a whole
``""`` list fails ``not_a_list`` (``ListField`` is not relational).
"""

import uuid

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract

# Scalar pk keys whose ``""`` lands null on POST (state/state_id have
# their own tests: state fills the default, state_id is dead input the
# view overwrites with triage).
POST_NULL_KEYS = ["parent", "parent_id", "estimate_point", "type", "assigned_pod_id"]
POST_NULL_RESPONSE_KEY = {
    "parent": "parent_id",
    "parent_id": "parent_id",
    "estimate_point": "estimate_point",
    "type": "type_id",
    "assigned_pod_id": "assigned_pod_id",
}
PATCH_NULL_KEYS = ["state_id", "parent", "parent_id", "estimate_point", "type", "assigned_pod_id"]
LIST_KEYS = ["label_ids", "assignee_ids"]


def _detail(tenant):
    urls = intake_urls(tenant)
    created = create_intake_issue(tenant["client"], urls["intake_issues"])
    return f"{urls['intake_issues']}{created['issue']['id']}/"


def _post_create(tenant):
    return intake_urls(tenant)["intake_issues"]


@pytest.mark.parametrize("key", POST_NULL_KEYS)
def test_post_empty_scalar_key_lands_null(make_tenant, key):
    # `""` behaves like explicit null: the triage issue is created with
    # the field unset.
    tenant = make_tenant()
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", key: ""}}
    )
    assert r.status_code == 200, r.text[:500]
    response_key = POST_NULL_RESPONSE_KEY[key]
    assert r.json()["issue"].get(response_key) is None


def test_post_empty_state_fills_default_like_null(make_tenant):
    # `""` is present-None, not absent: the auto twin beats the
    # server-set triage id and replays the save-default fill, exactly
    # like explicit null.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    default = seeder.create_state(
        tenant["workspace"]["id"], tenant["project"]["id"], name="Default pick"
    )
    tenant["_database"].execute(
        'UPDATE states SET "default" = true WHERE id = %s', (default["id"],)
    )
    r_empty = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "state": ""}}
    )
    r_null = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "state": None}}
    )
    assert r_empty.status_code == 200, r_empty.text[:500]
    assert r_null.status_code == 200, r_null.text[:500]
    assert r_empty.json()["issue"]["state_id"] == default["id"]
    assert r_null.json()["issue"]["state_id"] == default["id"]


def test_post_empty_state_id_still_lands_triage(make_tenant):
    # `state_id` input is dead on this path (the view overwrites it
    # with triage before validation), so `""` there also creates a
    # triage issue — same row as a plain create.
    tenant = make_tenant()
    plain = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "plain"}}
    )
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", "state_id": ""}}
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["state_id"] == plain.json()["issue"]["state_id"]


def test_post_empty_parent_twin_wins_like_null(make_tenant):
    # The auto `parent` twin wins over `parent_id` (declared fields
    # apply first), and `""` counts as the null twin: a valid
    # `parent_id` alongside `parent: ""` still lands null.
    tenant = make_tenant()
    other = create_intake_issue(
        tenant["client"], _post_create(tenant), name="parent pick"
    )
    r = tenant["client"].post(
        _post_create(tenant),
        json={
            "issue": {
                "name": "c",
                "parent_id": other["issue"]["id"],
                "parent": "",
            }
        },
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["parent_id"] is None


@pytest.mark.parametrize("key", LIST_KEYS)
def test_post_empty_list_key_not_a_list(make_tenant, key):
    tenant = make_tenant()
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", key: ""}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        key: ['Expected a list of items but got type "str".']
    }


@pytest.mark.parametrize("key", LIST_KEYS)
def test_post_empty_list_items_fail_null_indexed(make_tenant, key):
    tenant = make_tenant()
    bogus = "bogus-uuid"
    r = tenant["client"].post(
        _post_create(tenant), json={"issue": {"name": "c", key: ["", bogus]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        key: {
            "0": ["This field may not be null."],
            "1": [f"\u201c{bogus}\u201d is not a valid UUID."],
        }
    }


@pytest.mark.parametrize("key", PATCH_NULL_KEYS)
def test_patch_empty_scalar_key_clears(make_tenant, key):
    # `""` clears like null on PATCH (present-None, not absent).
    tenant = make_tenant()
    r = tenant["client"].patch(_detail(tenant), json={"issue": {key: ""}})
    assert r.status_code == 200, r.text[:500]


def test_patch_empty_state_matches_null(make_tenant):
    # `state: ""` transitions triage to the default fill, exactly like
    # explicit null (absent would leave triage untouched).
    tenant = make_tenant()
    urls = intake_urls(tenant)
    first = create_intake_issue(tenant["client"], urls["intake_issues"])
    second = create_intake_issue(
        tenant["client"], urls["intake_issues"], name="second"
    )
    triage_id = first["issue"]["state_id"]
    r_empty = tenant["client"].patch(
        f"{urls['intake_issues']}{first['issue']['id']}/",
        json={"issue": {"state": ""}},
    )
    r_null = tenant["client"].patch(
        f"{urls['intake_issues']}{second['issue']['id']}/",
        json={"issue": {"state": None}},
    )
    assert r_empty.status_code == 200, r_empty.text[:500]
    assert r_null.status_code == 200, r_null.text[:500]
    assert r_empty.json()["issue"]["state_id"] != triage_id
    assert r_empty.json()["issue"]["state_id"] == r_null.json()["issue"]["state_id"]


def test_patch_empty_parent_id_clears_set_value(make_tenant):
    # Set-then-clear proves `""` clears rather than leaving the value.
    tenant = make_tenant()
    urls = intake_urls(tenant)
    other = create_intake_issue(
        tenant["client"], urls["intake_issues"], name="parent pick"
    )
    target = create_intake_issue(tenant["client"], urls["intake_issues"])
    url = f"{urls['intake_issues']}{target['issue']['id']}/"
    set_r = tenant["client"].patch(
        url, json={"issue": {"parent_id": other["issue"]["id"]}}
    )
    assert set_r.status_code == 200, set_r.text[:500]
    assert set_r.json()["issue"]["parent_id"] == other["issue"]["id"]
    clear_r = tenant["client"].patch(url, json={"issue": {"parent_id": ""}})
    assert clear_r.status_code == 200, clear_r.text[:500]
    assert clear_r.json()["issue"]["parent_id"] is None


def test_patch_empty_parent_twin_clears_set_value(make_tenant):
    # Same clear through the auto `parent` twin (None needs no queryset
    # lookup, so a triage-held value clears under either key).
    tenant = make_tenant()
    urls = intake_urls(tenant)
    other = create_intake_issue(
        tenant["client"], urls["intake_issues"], name="parent pick"
    )
    target = create_intake_issue(tenant["client"], urls["intake_issues"])
    url = f"{urls['intake_issues']}{target['issue']['id']}/"
    set_r = tenant["client"].patch(
        url, json={"issue": {"parent_id": other["issue"]["id"]}}
    )
    assert set_r.status_code == 200, set_r.text[:500]
    clear_r = tenant["client"].patch(url, json={"issue": {"parent": ""}})
    assert clear_r.status_code == 200, clear_r.text[:500]
    assert clear_r.json()["issue"]["parent_id"] is None


def _seed_estimate_point(tenant):
    database = tenant["_database"]
    ws_id, proj_id = tenant["workspace"]["id"], tenant["project"]["id"]
    estimate_id, point_id = str(uuid.uuid4()), str(uuid.uuid4())
    database.execute(
        """INSERT INTO estimates
           (id, workspace_id, project_id, name, description, "type", last_used,
            created_at, updated_at)
           VALUES (%s,%s,%s,%s,'','points',false,now(),now())""",
        (estimate_id, ws_id, proj_id, f"ep-{point_id[:8]}"),
    )
    database.execute(
        """INSERT INTO estimate_points
           (id, workspace_id, project_id, estimate_id, "key", description,
            value, created_at, updated_at)
           VALUES (%s,%s,%s,%s,1,'','1',now(),now())""",
        (point_id, ws_id, proj_id, estimate_id),
    )
    return point_id


def _seed_issue_type(tenant):
    database = tenant["_database"]
    type_id = str(uuid.uuid4())
    database.execute(
        """INSERT INTO issue_types
           (id, workspace_id, name, description, logo_props, is_active,
            is_default, level, is_epic, created_at, updated_at)
           VALUES (%s,%s,%s,'','{}',true,false,0,false,now(),now())""",
        (type_id, tenant["workspace"]["id"], f"t-{type_id[:8]}"),
    )
    return type_id


def test_patch_empty_estimate_point_clears_set_value(make_tenant):
    tenant = make_tenant()
    point_id = _seed_estimate_point(tenant)
    url = _detail(tenant)
    set_r = tenant["client"].patch(url, json={"issue": {"estimate_point": point_id}})
    assert set_r.status_code == 200, set_r.text[:500]
    clear_r = tenant["client"].patch(url, json={"issue": {"estimate_point": ""}})
    assert clear_r.status_code == 200, clear_r.text[:500]
    assert clear_r.json()["issue"]["estimate_point"] is None


def test_patch_empty_type_clears_set_value(make_tenant):
    tenant = make_tenant()
    type_id = _seed_issue_type(tenant)
    url = _detail(tenant)
    set_r = tenant["client"].patch(url, json={"issue": {"type": type_id}})
    assert set_r.status_code == 200, set_r.text[:500]
    clear_r = tenant["client"].patch(url, json={"issue": {"type": ""}})
    assert clear_r.status_code == 200, clear_r.text[:500]
    landed = tenant["_database"].fetchval(
        "SELECT type_id FROM issues WHERE id = %s",
        (clear_r.json()["issue"]["id"],),
    )
    assert landed is None


def test_patch_empty_assigned_pod_clears_set_value(make_tenant):
    tenant = make_tenant()
    pod = tenant["seeder"].create_pod(
        tenant["workspace"]["id"], tenant["project"]["id"]
    )
    url = _detail(tenant)
    set_r = tenant["client"].patch(
        url, json={"issue": {"assigned_pod_id": pod["id"]}}
    )
    assert set_r.status_code == 200, set_r.text[:500]
    assert set_r.json()["issue"]["assigned_pod_id"] == pod["id"]
    clear_r = tenant["client"].patch(url, json={"issue": {"assigned_pod_id": ""}})
    assert clear_r.status_code == 200, clear_r.text[:500]
    assert clear_r.json()["issue"]["assigned_pod_id"] is None


def test_patch_empty_duplicate_to_clears_like_null(make_tenant):
    # `duplicate_to` is the intake branch's auto pk twin
    # (`allow_null=True`): `""` clears it exactly like null.
    tenant = make_tenant()
    urls = intake_urls(tenant)
    ws_id, proj_id = tenant["workspace"]["id"], tenant["project"]["id"]
    state = tenant["seeder"].create_state(ws_id, proj_id, name="Dupe target")
    target = tenant["seeder"].create_issue(ws_id, proj_id, state["id"])
    created = create_intake_issue(tenant["client"], urls["intake_issues"])
    url = f"{urls['intake_issues']}{created['issue']['id']}/"
    set_r = tenant["client"].patch(url, json={"duplicate_to": target["id"]})
    assert set_r.status_code == 200, set_r.text[:500]
    assert set_r.json()["duplicate_to"] == target["id"]
    clear_r = tenant["client"].patch(url, json={"duplicate_to": ""})
    assert clear_r.status_code == 200, clear_r.text[:500]
    assert clear_r.json()["duplicate_to"] is None
    null_r = tenant["client"].patch(url, json={"duplicate_to": None})
    assert null_r.status_code == 200, null_r.text[:500]
    assert null_r.json()["duplicate_to"] is None


@pytest.mark.parametrize("key", LIST_KEYS)
def test_patch_empty_list_key_not_a_list(make_tenant, key):
    tenant = make_tenant()
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {key: ""}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        key: ['Expected a list of items but got type "str".']
    }


@pytest.mark.parametrize("key", LIST_KEYS)
def test_patch_empty_list_items_fail_null_indexed(make_tenant, key):
    tenant = make_tenant()
    bogus = "bogus-uuid"
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {key: ["", bogus]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        key: {
            "0": ["This field may not be null."],
            "1": [f"\u201c{bogus}\u201d is not a valid UUID."],
        }
    }


@pytest.mark.parametrize("key", LIST_KEYS)
def test_patch_bogus_list_item_errors_are_indexed(make_tenant, key):
    # Per-item errors key by index like DRF `run_child_validation`
    # (previously flat on this path).
    tenant = make_tenant()
    bogus = "bogus-uuid"
    r = tenant["client"].patch(
        _detail(tenant), json={"issue": {key: [bogus]}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        key: {"0": [f"\u201c{bogus}\u201d is not a valid UUID."]}
    }
