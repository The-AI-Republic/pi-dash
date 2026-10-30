"""Intake PATCH ``issue.assigned_pod`` validation (PIDASHCONV-1401).

The issue-half validator mirrors ``IssueSerializer.validate``: a pod from
another project 400s, and any pod change — assign, reassign or clear — 400s
while the issue has an active (non-terminal) agent run. Failures are field
errors under ``assigned_pod`` and write nothing.
"""

import uuid


def _base(org, project_id=None):
    return (
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{project_id or org.project['id']}/intake-issues/"
    )


def _mkpod(org, project_id):
    pid = str(uuid.uuid4())
    org.conn.execute(
        """INSERT INTO pod (id, name, description, is_default, workspace_id, project_id,
            created_at, updated_at)
           VALUES (%s,%s,'',false,%s,%s,now(),now())""",
        (pid, f"ct pod {pid[:8]}", org.workspace["id"], project_id),
    )
    return pid


def _mkrun(org, issue_id, pod_id, status="running"):
    rid = str(uuid.uuid4())
    org.conn.execute(
        """INSERT INTO agent_run
           (id, status, prompt, run_config, required_capabilities, thread_id,
            error_details, workspace_id, work_item_id, pod_id, created_by_id,
            llm_model, trigger, executor_kind, dispatch_attempts,
            cancel_reason, tool_plan, phase_kind, agent_metadata,
            usage, created_at)
           VALUES (%s,%s,'contract prompt','{}','{}','contract-thread','{}',
                   %s,%s,%s,%s,'contract-model', 'human', 'local_runner', 0,
                   '', '{}', 'work', '{}',
                   '{"input": 0, "output": 0, "total": 0}', now())""",
        (rid, status, org.workspace["id"], issue_id, pod_id, org.admin["id"]),
    )
    return rid


def _pod_of(org, issue_id):
    row = org.conn.execute(
        "SELECT assigned_pod_id FROM issues WHERE id = %s", (issue_id,)
    ).fetchone()
    return None if row[0] is None else str(row[0])


def test_intake_patch_assigned_pod_other_project_400(org):
    """(a) a pod from another project is rejected as a field error."""
    created = org.create_intake_issue(name="pod case a")
    other_pod = _mkpod(org, org.project2["id"])
    r = org.request(
        "PATCH", _base(org) + f"{created['issue']}/", "admin",
        json={"issue": {"assigned_pod": other_pod}},
    )
    assert r.status_code == 400
    assert r.json() == {"assigned_pod": ["pod is in a different project"]}
    assert _pod_of(org, created["issue"]) is None


def test_intake_patch_assigned_pod_reassign_with_active_run_400(org):
    """(b) reassigning the pod while a run is live is rejected; pod kept."""
    created = org.create_intake_issue(name="pod case b")
    pod = _mkpod(org, org.project["id"])
    other = _mkpod(org, org.project["id"])
    org.conn.execute(
        "UPDATE issues SET assigned_pod_id = %s WHERE id = %s", (pod, created["issue"])
    )
    _mkrun(org, created["issue"], pod)
    r = org.request(
        "PATCH", _base(org) + f"{created['issue']}/", "admin",
        json={"issue": {"assigned_pod": other}},
    )
    assert r.status_code == 400
    assert r.json() == {"assigned_pod": ["cannot reassign pod while the issue has an active run"]}
    assert _pod_of(org, created["issue"]) == pod


def test_intake_patch_assigned_pod_clear_with_active_run_400(org):
    """(c) clearing the pod while a run is live is rejected; pod kept."""
    created = org.create_intake_issue(name="pod case c")
    pod = _mkpod(org, org.project["id"])
    org.conn.execute(
        "UPDATE issues SET assigned_pod_id = %s WHERE id = %s", (pod, created["issue"])
    )
    _mkrun(org, created["issue"], pod)
    r = org.request(
        "PATCH", _base(org) + f"{created['issue']}/", "admin",
        json={"issue": {"assigned_pod": None}},
    )
    assert r.status_code == 400
    assert r.json() == {"assigned_pod": ["cannot reassign pod while the issue has an active run"]}
    assert _pod_of(org, created["issue"]) == pod


def test_intake_patch_assigned_pod_clear_without_active_run_ok(org):
    """Clearing the pod with no live run succeeds (gate is change + run)."""
    created = org.create_intake_issue(name="pod clear ok")
    pod = _mkpod(org, org.project["id"])
    org.conn.execute(
        "UPDATE issues SET assigned_pod_id = %s WHERE id = %s", (pod, created["issue"])
    )
    r = org.request(
        "PATCH", _base(org) + f"{created['issue']}/", "admin",
        json={"issue": {"assigned_pod": None}},
    )
    assert r.status_code == 200, r.text[:500]
    assert _pod_of(org, created["issue"]) is None


def test_intake_patch_assigned_pod_same_value_with_active_run_ok(org):
    """Re-sending the current pod while a run is live is a no-op, not a change."""
    created = org.create_intake_issue(name="pod same ok")
    pod = _mkpod(org, org.project["id"])
    org.conn.execute(
        "UPDATE issues SET assigned_pod_id = %s WHERE id = %s", (pod, created["issue"])
    )
    _mkrun(org, created["issue"], pod)
    r = org.request(
        "PATCH", _base(org) + f"{created['issue']}/", "admin",
        json={"issue": {"assigned_pod": pod}},
    )
    assert r.status_code == 200, r.text[:500]
    assert _pod_of(org, created["issue"]) == pod
