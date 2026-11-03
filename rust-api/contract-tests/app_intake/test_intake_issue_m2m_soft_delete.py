"""Contract tests for intake-issue m2m replacement soft-delete (PIDASHCONV-1438).

Covers `PATCH intake-issues/<pk>/` with `{"issue": {"assignee_ids": [...]}}`
and `{"issue": {"label_ids": [...]}}` — `apply_issue_update` in
`rust-api/crates/api/src/app_intake/issues.rs`, mirroring
`IssueCreateSerializer.update` (`app/serializers/issue.py:464-518`).
"""

import pytest

from _harness.seed import ADMIN
from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract


def _link_rows(database, table, id_column, issue_id):
    """``(target_id, is_live)`` pairs for one issue's link rows, id-ordered."""
    return sorted(
        (str(row[id_column]), row["deleted_at"] is None)
        for row in database.fetchall(
            f"SELECT {id_column}, deleted_at FROM {table} WHERE issue_id = %s",
            (issue_id,),
        )
    )


def _second_member(tenant):
    """A second ADMIN user on the tenant's workspace/project; returns its id."""
    seeder = tenant["seeder"]
    user = seeder.create_user()
    seeder.create_workspace_member(tenant["workspace"]["id"], user["id"], role=ADMIN)
    seeder.create_project_member(
        tenant["workspace"]["id"], tenant["project"]["id"], user["id"], role=ADMIN
    )
    return user["id"]


def test_patch_replace_assignees_soft_deletes_old_rows(make_tenant):
    """PATCH assignees ``[u1]`` -> ``[u2]`` keeps u1's row soft-stamped.

    Django replaces assignees with
    ``IssueAssignee.objects.filter(issue=instance).delete()``
    (``serializers/issue.py:474-475``). ``IssueAssignee`` uses the
    ``SoftDeletionManager``, so the queryset delete only stamps
    ``deleted_at`` on the live rows (``db/mixins.py:48-53``); the read
    annotation filters tombstones out
    (``views/intake/base.py:149-157``), so the detail body shows only
    the live id while both rows persist.
    """
    tenant = make_tenant()
    urls = intake_urls(tenant)
    first_id = tenant["user"]["id"]
    second_id = _second_member(tenant)
    created = create_intake_issue(tenant["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]

    response = tenant["client"].patch(
        f"{urls['intake_issues']}{issue_id}/",
        json={"issue": {"assignee_ids": [first_id]}},
    )
    assert response.status_code == 200, response.text
    response = tenant["client"].patch(
        f"{urls['intake_issues']}{issue_id}/",
        json={"issue": {"assignee_ids": [second_id]}},
    )
    assert response.status_code == 200, response.text

    assert _link_rows(
        tenant["_database"], "issue_assignees", "assignee_id", issue_id
    ) == sorted([(first_id, False), (second_id, True)])
    again = tenant["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert again.status_code == 200, again.text
    assert again.json()["issue"]["assignee_ids"] == [second_id]


def test_patch_replace_labels_soft_deletes_old_rows(make_tenant):
    """PATCH labels ``[l1]`` -> ``[l2]`` keeps l1's row soft-stamped.

    Same shape as the assignee case: ``IssueLabel.objects.filter(
    issue=instance).delete()`` (``serializers/issue.py:495-496``) is the
    soft queryset delete (``db/mixins.py:48-53``), and the read
    annotation hides tombstones (``views/intake/base.py:141-148``).
    """
    tenant = make_tenant()
    urls = intake_urls(tenant)
    seeder = tenant["seeder"]
    first = seeder.create_label(
        tenant["workspace"]["id"], tenant["project"]["id"], name="soft-l1"
    )
    second = seeder.create_label(
        tenant["workspace"]["id"], tenant["project"]["id"], name="soft-l2"
    )
    created = create_intake_issue(tenant["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]

    response = tenant["client"].patch(
        f"{urls['intake_issues']}{issue_id}/",
        json={"issue": {"label_ids": [first["id"]]}},
    )
    assert response.status_code == 200, response.text
    response = tenant["client"].patch(
        f"{urls['intake_issues']}{issue_id}/",
        json={"issue": {"label_ids": [second["id"]]}},
    )
    assert response.status_code == 200, response.text

    assert _link_rows(
        tenant["_database"], "issue_labels", "label_id", issue_id
    ) == sorted([(first["id"], False), (second["id"], True)])
    again = tenant["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert again.status_code == 200, again.text
    assert again.json()["issue"]["label_ids"] == [second["id"]]
