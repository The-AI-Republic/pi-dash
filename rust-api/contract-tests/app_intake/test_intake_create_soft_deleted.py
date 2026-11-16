"""Create-path soft-delete guards (PIDASHCONV-1431).

Every table behind the intake-create FK fields inherits ``SoftDeleteModel``,
whose default manager hides soft-deleted rows (``db/mixins.py``). Django's
``IssueCreateSerializer`` therefore 400s on a soft-deleted parent, label,
estimate point or type at field level, ``validate()`` silently drops
assignees whose project membership is soft-deleted
(``serializers/issue.py:337-354``), and the triage-state ``MAX(sequence)``
in ``State.save`` runs over the same live-only manager
(``db/models/state.py:131-138``).
"""

import uuid

import pytest

from .conftest import intake_urls

pytestmark = pytest.mark.contract


def _soft_delete(tenant, table, row_id):
    tenant["_database"].execute(
        f"UPDATE {table} SET deleted_at = now() WHERE id = %s", (row_id,)
    )


def test_create_rejects_deleted_parent(make_tenant):
    tenant = make_tenant()
    seeder = tenant["seeder"]
    parent = seeder.create_issue(
        tenant["workspace"]["id"], tenant["project"]["id"], name="doomed parent"
    )
    _soft_delete(tenant, "issues", parent["id"])
    urls = intake_urls(tenant)
    r = tenant["client"].post(
        urls["intake_issues"],
        json={"issue": {"name": "x", "parent_id": parent["id"]}},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "parent_id": [f'Invalid pk "{parent["id"]}" - object does not exist.']
    }


def test_create_rejects_deleted_label(make_tenant):
    tenant = make_tenant()
    seeder = tenant["seeder"]
    label = seeder.create_label(
        tenant["workspace"]["id"], tenant["project"]["id"], name="doomed label"
    )
    _soft_delete(tenant, "labels", label["id"])
    urls = intake_urls(tenant)
    r = tenant["client"].post(
        urls["intake_issues"],
        json={"issue": {"name": "x", "label_ids": [label["id"]]}},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "label_ids": {
            "0": [f'Invalid pk "{label["id"]}" - object does not exist.']
        }
    }


def test_create_rejects_deleted_estimate_point(make_tenant):
    tenant = make_tenant()
    db = tenant["_database"]
    ws, proj = tenant["workspace"]["id"], tenant["project"]["id"]
    est_id, pt_id = str(uuid.uuid4()), str(uuid.uuid4())
    db.execute(
        "INSERT INTO estimates (id, workspace_id, project_id, name, description,"
        " type, last_used, created_at, updated_at)"
        " VALUES (%s,%s,%s,'ep-est','', 'points', false, now(), now())",
        (est_id, ws, proj),
    )
    db.execute(
        'INSERT INTO estimate_points (id, workspace_id, project_id, estimate_id,'
        ' "key", description, value, created_at, updated_at)'
        " VALUES (%s,%s,%s,%s,1,'','1',now(),now())",
        (pt_id, ws, proj, est_id),
    )
    _soft_delete(tenant, "estimate_points", pt_id)
    urls = intake_urls(tenant)
    r = tenant["client"].post(
        urls["intake_issues"],
        json={"issue": {"name": "x", "estimate_point": pt_id}},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "estimate_point": [f'Invalid pk "{pt_id}" - object does not exist.']
    }


def test_create_rejects_deleted_type(make_tenant):
    tenant = make_tenant()
    db = tenant["_database"]
    type_id = str(uuid.uuid4())
    db.execute(
        "INSERT INTO issue_types (id, workspace_id, name, description, logo_props,"
        " is_epic, is_default, is_active, level, created_at, updated_at)"
        " VALUES (%s,%s,'doomed type','','{}',false,false,true,0,now(),now())",
        (type_id, tenant["workspace"]["id"]),
    )
    _soft_delete(tenant, "issue_types", type_id)
    urls = intake_urls(tenant)
    r = tenant["client"].post(
        urls["intake_issues"], json={"issue": {"name": "x", "type": type_id}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {
        "type": [f'Invalid pk "{type_id}" - object does not exist.']
    }


def test_create_drops_deleted_assignee(make_tenant):
    # The user row stays live (users are not soft-deletable) so field
    # validation passes; validate() drops the id because the membership
    # row is soft-deleted. Both backends answer 200 with no link.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    ws, proj = tenant["workspace"]["id"], tenant["project"]["id"]
    ghost = seeder.create_user()
    seeder.create_project_member(ws, proj, ghost["id"], role=15)
    tenant["_database"].execute(
        "UPDATE project_members SET deleted_at = now()"
        " WHERE project_id = %s AND member_id = %s",
        (proj, ghost["id"]),
    )
    urls = intake_urls(tenant)
    r = tenant["client"].post(
        urls["intake_issues"],
        json={"issue": {"name": "x", "assignee_ids": [ghost["id"]]}},
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["assignee_ids"] == []
    links = tenant["_database"].fetchval(
        "SELECT count(*) FROM issue_assignees WHERE assignee_id = %s",
        (ghost["id"],),
    )
    assert links == 0


def test_create_triage_sequence_ignores_deleted_states(make_tenant):
    # The tenant seeds one live state at 65535; a soft-deleted state at
    # 999999 must not move the new triage state's MAX+15000 sequence.
    tenant = make_tenant()
    seeder = tenant["seeder"]
    ws, proj = tenant["workspace"]["id"], tenant["project"]["id"]
    doomed = seeder.create_state(ws, proj, name="Doomed", group="backlog")
    tenant["_database"].execute(
        "UPDATE states SET sequence = 999999, deleted_at = now() WHERE id = %s",
        (doomed["id"],),
    )
    urls = intake_urls(tenant)
    r = tenant["client"].post(
        urls["intake_issues"], json={"issue": {"name": "triage seq probe"}}
    )
    assert r.status_code == 200, r.text[:500]
    seq = tenant["_database"].fetchval(
        "SELECT sequence FROM states WHERE project_id = %s AND \"group\" = 'triage'"
        " AND deleted_at IS NULL",
        (proj,),
    )
    assert seq == 80535.0
