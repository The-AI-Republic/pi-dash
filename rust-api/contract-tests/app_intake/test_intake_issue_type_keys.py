"""Nested issue type_id/is_epic keys in intake detail (PIDASHCONV-1695, NEWFRONT-984).

Django's NEWFRONT-984 change adds ``type_id``/``is_epic`` to
``IssueSerializer``, so the ``issue`` nested in
``IntakeIssueDetailSerializer`` — and the ``issue_current_instance`` dump,
which renders through the same shape — carries both keys in
``Meta.fields`` order: after ``parent_id`` (``cycle_id`` stays
``SkipField``ed; the intake queryset never annotates it).

The intake-write shapes stay put: ``duplicate_issue_detail`` and the
``IntakeIssueSerializer`` nested issue are ``IssueIntakeSerializer`` (no
new keys on Django main), pinned by
``test_duplicate_issue_detail_keeps_intake_shape``.
"""

from datetime import datetime, timezone
import uuid

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract

# Nested `issue` key order in GET/PATCH intake-issues/<pk>/, verified
# against live Django@main: IssueSerializer Meta.fields order (type_id /
# is_epic ride after parent_id; cycle_id, module_ids and the counts
# SkipField out) plus the detail-only tail minus unannotated
# is_subscribed / is_intake.
NESTED_ISSUE_KEYS_ORDERED = [
    "id", "name", "state_id", "sort_order", "completed_at",
    "estimate_point", "priority", "complexity_score", "start_date",
    "target_date", "sequence_id", "project_id", "parent_id", "type_id",
    "is_epic", "assigned_pod_id", "agent_executor", "label_ids",
    "assignee_ids", "created_at", "updated_at", "created_by",
    "updated_by", "is_draft", "archived_at", "is_synced",
    "description_html", "agent_ticker", "agent_status",
    "relations_summary", "has_open_blockers",
]

# `duplicate_issue_detail` is IssueIntakeSerializer over the plain
# duplicate_to row: label_ids SkipFields out (no annotation), Django@main.
DUPLICATE_DETAIL_KEYS_ORDERED = [
    "id", "name", "priority", "sequence_id", "project_id", "created_at",
    "created_by",
]

TYPE_COLUMNS = (
    "created_at, updated_at, id, name, description, logo_props,"
    " workspace_id, is_active, is_default, is_epic, level"
)


def _make_type(tenant, name, *, is_epic):
    tid = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    tenant["_database"].execute(
        "INSERT INTO issue_types (%s) VALUES (%%s,%%s,%%s,%%s,%%s,%%s,%%s,TRUE,FALSE,%%s,0.0)"
        % TYPE_COLUMNS,
        (now, now, tid, name, "", "{}", tenant["workspace"]["id"], is_epic),
    )
    return tid


def _type_issue(tenant, issue_id, type_id):
    tenant["_database"].execute(
        "UPDATE issues SET type_id=%s WHERE id=%s", (type_id, issue_id)
    )


def test_nested_issue_key_order_untyped(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(
        admin["client"], urls["intake_issues"], name="Type-keys probe"
    )
    issue_id = created["issue"]["id"]
    response = admin["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert response.status_code == 200, response.text
    nested = response.json()["issue"]
    assert list(nested) == NESTED_ISSUE_KEYS_ORDERED
    assert nested["type_id"] is None
    assert nested["is_epic"] is False


def test_nested_issue_epic_typed(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(
        admin["client"], urls["intake_issues"], name="Epic-typed probe"
    )
    issue_id = created["issue"]["id"]
    epic_type = _make_type(admin, "Epic", is_epic=True)
    _type_issue(admin, issue_id, epic_type)
    response = admin["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert response.status_code == 200, response.text
    nested = response.json()["issue"]
    assert list(nested) == NESTED_ISSUE_KEYS_ORDERED
    assert nested["type_id"] == epic_type
    assert nested["is_epic"] is True


def test_nested_issue_non_epic_typed(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(
        admin["client"], urls["intake_issues"], name="Bug-typed probe"
    )
    issue_id = created["issue"]["id"]
    bug_type = _make_type(admin, "Bug", is_epic=False)
    _type_issue(admin, issue_id, bug_type)
    response = admin["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert response.status_code == 200, response.text
    nested = response.json()["issue"]
    assert list(nested) == NESTED_ISSUE_KEYS_ORDERED
    assert nested["type_id"] == bug_type
    assert nested["is_epic"] is False


def test_patch_nested_issue_carries_type_keys(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(
        admin["client"], urls["intake_issues"], name="Patch type-keys probe"
    )
    issue_id = created["issue"]["id"]
    epic_type = _make_type(admin, "Epic", is_epic=True)
    _type_issue(admin, issue_id, epic_type)
    response = admin["client"].patch(
        f"{urls['intake_issues']}{issue_id}/",
        json={"issue": {"name": "Patched typed probe"}},
    )
    assert response.status_code == 200, response.text
    nested = response.json()["issue"]
    assert list(nested) == NESTED_ISSUE_KEYS_ORDERED
    assert nested["type_id"] == epic_type
    assert nested["is_epic"] is True
    assert nested["name"] == "Patched typed probe"


def test_duplicate_issue_detail_keeps_intake_shape(make_tenant):
    tenant = make_tenant()
    urls = intake_urls(tenant)
    workspace_id = tenant["workspace"]["id"]
    project_id = tenant["project"]["id"]
    # duplicate_to binds the triage-hiding default manager, so the target
    # must be a non-triage issue (see test_intake_issue_duplicate.py).
    state = tenant["seeder"].create_state(
        workspace_id, project_id, name="Dupe target"
    )
    target = tenant["seeder"].create_issue(workspace_id, project_id, state["id"])
    created = create_intake_issue(
        tenant["client"], urls["intake_issues"], name="Dupe shape probe"
    )
    issue_id = created["issue"]["id"]
    marked = tenant["client"].patch(
        f"{urls['intake_issues']}{issue_id}/",
        json={"status": 2, "duplicate_to": target["id"]},
    )
    assert marked.status_code == 200, marked.text[:500]
    response = tenant["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert response.status_code == 200, response.text
    duplicate = response.json()["duplicate_issue_detail"]
    assert list(duplicate) == DUPLICATE_DETAIL_KEYS_ORDERED
