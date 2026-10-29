"""Intake PATCH ``{"issue": {"state": null}}`` (PIDASHCONV-1405).

``Issue.save`` takes the ``state is None`` branch (``db/models/issue.py``):
it resolves the project's default non-triage state through
``State.objects`` (whose manager excludes ``group='triage'``) and does
*not* touch ``completed_at`` — the recompute lives only in the ``else``
branch. An explicitly supplied ``completed_at`` therefore sticks, and
with no default state the issue keeps ``state_id`` NULL.
"""

from datetime import datetime, timezone

from . import seed_d21

STAMP = "2025-03-04T05:06:07Z"
STAMP_DT = datetime(2025, 3, 4, 5, 6, 7, tzinfo=timezone.utc)
SUPPLIED = "2026-01-01T00:00:00Z"
SUPPLIED_DT = datetime(2026, 1, 1, 0, 0, 0, tzinfo=timezone.utc)


def _base(org, project_id=None):
    return (
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{project_id or org.project['id']}/intake-issues/"
    )


def _row(org, issue_id):
    return org.conn.execute(
        """SELECT i.state_id, s.name, s."group", i.completed_at
           FROM issues i LEFT JOIN states s ON s.id = i.state_id
           WHERE i.id = %s""",
        (issue_id,),
    ).fetchone()


def test_intake_patch_state_null_keeps_completed_at(org):
    default = seed_d21.create_state(
        org.conn, workspace_id=org.workspace["id"], project_id=org.project["id"],
        name="Backlog", group="backlog", default=True,
    )
    created = org.create_intake_issue(name="keep stamp")
    org.conn.execute(
        "UPDATE issues SET completed_at = %s WHERE id = %s", (STAMP, created["issue"])
    )
    r = org.request(
        "PATCH", f"{_base(org)}{created['issue']}/", "admin",
        json={"issue": {"state": None}},
    )
    assert r.status_code == 200
    detail = r.json()["issue_detail"]
    assert detail["state"]["id"] == default["id"]
    assert detail["completed_at"] == STAMP
    state_id, _name, group, completed_at = _row(org, created["issue"])
    assert str(state_id) == default["id"]
    assert group == "backlog"
    assert completed_at == STAMP_DT


def test_intake_patch_state_null_takes_supplied_completed_at(org):
    default = seed_d21.create_state(
        org.conn, workspace_id=org.workspace["id"], project_id=org.project["id"],
        name="Backlog", group="backlog", default=True,
    )
    created = org.create_intake_issue(name="take supplied")
    org.conn.execute(
        "UPDATE issues SET completed_at = %s WHERE id = %s", (STAMP, created["issue"])
    )
    r = org.request(
        "PATCH", f"{_base(org)}{created['issue']}/", "admin",
        json={"issue": {"state": None, "completed_at": SUPPLIED}},
    )
    assert r.status_code == 200
    detail = r.json()["issue_detail"]
    assert detail["state"]["id"] == default["id"]
    assert detail["completed_at"] == SUPPLIED
    state_id, _name, _group, completed_at = _row(org, created["issue"])
    assert str(state_id) == default["id"]
    assert completed_at == SUPPLIED_DT


def test_intake_patch_state_null_without_default_state(org):
    """No default state: no fallback to the auto-created Triage row
    (``is_triage=FALSE`` but ``group='triage'``) — the issue keeps
    ``state_id`` NULL and its ``completed_at``."""
    created = org.create_intake_issue(name="no default")
    org.conn.execute(
        "UPDATE issues SET completed_at = %s WHERE id = %s", (STAMP, created["issue"])
    )
    r = org.request(
        "PATCH", f"{_base(org)}{created['issue']}/", "admin",
        json={"issue": {"state": None}},
    )
    assert r.status_code == 200
    detail = r.json()["issue_detail"]
    assert detail["state"] is None
    assert detail["completed_at"] == STAMP
    state_id, _name, group, completed_at = _row(org, created["issue"])
    assert state_id is None
    assert group is None
    assert completed_at == STAMP_DT
