"""NEWPIDASH-153: grouped cycle-issues render sort_order as a JSON number.

Every row of a grouped or sub-grouped ``cycle-issues`` list carries
``sort_order`` from the grouping kernels as a string
(``py_float_str``); Django renders the DRF FloatField bare
(``65535.0``). The module engine splices the quotes back out
(``unquote_sort_order`` on ``results_json``); the cycle engine must do
the same or ``web_new``'s ``z.number()`` contract refuses the payload
and the cycle board renders nothing.

Both backends answer a JSON number on every row of every group
combination the board sends.
"""

import pytest

from _harness.seed import ADMIN
from .conftest import create_issue_for, cycle_urls

pytestmark = pytest.mark.contract


def _seed_cycle_issues(tenant):
    """Two issues linked to the tenant cycle, exercising grouped rows."""
    seed = tenant["seed"]
    ws_id = tenant["workspace"]["id"]
    proj_id = tenant["project"]["id"]
    other = seed.user(email=f"sort-other-{tenant['cycle']['id'][:8]}@contract.test")
    seed.member(ws_id, other["id"], role=ADMIN)
    seed.project_member(proj_id, ws_id, other["id"], role=ADMIN)
    label_id = seed.label(ws_id, proj_id, name="Sort label")
    module_id = seed.module(ws_id, proj_id, name="Sort module")
    i1 = create_issue_for(tenant, name="Sort issue one")
    i2 = create_issue_for(tenant, name="Sort issue two")
    seed.issue_label(ws_id, proj_id, i1["id"], label_id)
    seed.issue_assignee(ws_id, proj_id, i1["id"], other["id"])
    seed.module_issue(ws_id, proj_id, module_id, i1["id"])
    urls = cycle_urls(tenant)
    r = tenant["client"].post(
        urls["cycle_issues"](tenant["cycle"]["id"]),
        json={"issues": [i1["id"], i2["id"]]},
    )
    assert r.status_code == 201, r.text[:500]
    return i1, i2


def _teardown_bridges(tenant):
    ws_id = tenant["workspace"]["id"]
    with tenant["seed"].conn.cursor() as cur:
        # Server-minted activity rows reference the issues; Seed.cleanup
        # cannot delete the issues under that FK, so sweep them first.
        cur.execute(
            "delete from issue_activities where issue_id in ("
            "select id from issues where workspace_id=%s)",
            (ws_id,),
        )
        cur.execute(
            "delete from cycle_issues where workspace_id=%s",
            (ws_id,),
        )
    tenant["seed"].conn.commit()


def _sort_orders(node, acc):
    if isinstance(node, dict):
        if "sort_order" in node:
            acc.append(node["sort_order"])
        for value in node.values():
            _sort_orders(value, acc)
    elif isinstance(node, list):
        for item in node:
            _sort_orders(item, acc)


@pytest.mark.parametrize(
    "params",
    [
        "group_by=state_id",
        "group_by=assignees__id",
        "group_by=priority",
        "group_by=issue_module__module_id",
        "group_by=cycle_id",
        "group_by=state_id&sub_group_by=assignees__id",
    ],
)
def test_grouped_sort_order_is_float(make_tenant, params):
    tenant = make_tenant(role=ADMIN)
    try:
        _seed_cycle_issues(tenant)
        urls = cycle_urls(tenant)
        r = tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"]) + f"?{params}"
        )
        assert r.status_code == 200, r.text[:500]
        found = []
        _sort_orders(r.json()["results"], found)
        assert found, f"no grouped rows for {params}"
        for value in found:
            assert isinstance(value, float), (
                f"group_by {params}: sort_order is {type(value).__name__} "
                f"{value!r}, want float"
            )
    finally:
        _teardown_bridges(tenant)


def test_grouped_labels_sort_order_is_float(make_tenant):
    tenant = make_tenant(role=ADMIN)
    try:
        _seed_cycle_issues(tenant)
        urls = cycle_urls(tenant)
        r = tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"]) + "?group_by=labels__id"
        )
        assert r.status_code == 200, r.text[:500]
        found = []
        _sort_orders(r.json()["results"], found)
        assert found, "no grouped rows for labels__id"
        for value in found:
            assert isinstance(value, float), (
                f"group_by labels__id: sort_order is {type(value).__name__} "
                f"{value!r}, want float"
            )
    finally:
        _teardown_bridges(tenant)
