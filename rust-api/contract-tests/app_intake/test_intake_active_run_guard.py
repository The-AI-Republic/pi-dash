"""Contract tests for the intake-issue active-run guard (PIDASHCONV-1428).

`PATCH intake-issues/<pk>/` changing `assigned_pod_id` or `agent_executor`
consults `Issue.has_active_run` (`db/models/issue.py:232-248`, an
`agent_run` existence check over `NON_TERMINAL_STATUSES`): with no live
run the change saves (200); with a live run it answers 400 with the
serializer message (`app/serializers/issue.py`, pod and executor arms).

The Rust port queried a nonexistent `agent_runs` table, so both PATCHes
500d. These tests fail on the pre-fix binary and pass on both backends
after the fix.
"""

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract

EXECUTOR_GUARD = "cannot change the execution target while the issue has an active run"
POD_GUARD = "cannot reassign pod while the issue has an active run"


def _tenant_with_pods(make_tenant):
    """Fresh tenant with a default pod plus a second pod in the project."""
    tenant = make_tenant()
    seeder = tenant["seeder"]
    workspace_id = tenant["workspace"]["id"]
    project_id = tenant["project"]["id"]
    default_pod = seeder.create_pod(
        workspace_id, project_id, name="Default pod", is_default=True
    )
    other_pod = seeder.create_pod(workspace_id, project_id, name="Other pod")
    tenant["default_pod"] = default_pod
    tenant["other_pod"] = other_pod
    return tenant


def _intake_issue_with_default_pod(tenant, name):
    """Create an intake issue; it must pick up the project default pod."""
    urls = intake_urls(tenant)
    created = create_intake_issue(tenant["client"], urls["intake_issues"], name=name)
    issue = created["issue"]
    assert issue["assigned_pod_id"] == tenant["default_pod"]["id"]
    assert issue["agent_executor"] is None
    return urls, issue


def test_patch_executor_no_run_200(make_tenant):
    tenant = _tenant_with_pods(make_tenant)
    urls, issue = _intake_issue_with_default_pod(tenant, "Executor no-run")
    response = tenant["client"].patch(
        f"{urls['intake_issues']}{issue['id']}/",
        json={"issue": {"agent_executor": "local_runner"}},
    )
    assert response.status_code == 200, response.text
    assert response.json()["issue"]["agent_executor"] == "local_runner"


def test_patch_pod_no_run_200(make_tenant):
    tenant = _tenant_with_pods(make_tenant)
    urls, issue = _intake_issue_with_default_pod(tenant, "Pod no-run")
    response = tenant["client"].patch(
        f"{urls['intake_issues']}{issue['id']}/",
        json={"issue": {"assigned_pod_id": tenant["other_pod"]["id"]}},
    )
    assert response.status_code == 200, response.text
    assert response.json()["issue"]["assigned_pod_id"] == tenant["other_pod"]["id"]


def _seed_running_run(tenant, issue):
    return tenant["seeder"].create_agent_run(
        tenant["workspace"]["id"],
        tenant["user"]["id"],
        issue["assigned_pod_id"],
        issue["id"],
        status="running",
    )


def test_patch_executor_live_run_400(make_tenant):
    tenant = _tenant_with_pods(make_tenant)
    urls, issue = _intake_issue_with_default_pod(tenant, "Executor live-run")
    _seed_running_run(tenant, issue)
    response = tenant["client"].patch(
        f"{urls['intake_issues']}{issue['id']}/",
        json={"issue": {"agent_executor": "local_runner"}},
    )
    assert response.status_code == 400, response.text
    assert response.json() == {"agent_executor": [EXECUTOR_GUARD]}


def test_patch_pod_live_run_400(make_tenant):
    tenant = _tenant_with_pods(make_tenant)
    urls, issue = _intake_issue_with_default_pod(tenant, "Pod live-run")
    _seed_running_run(tenant, issue)
    response = tenant["client"].patch(
        f"{urls['intake_issues']}{issue['id']}/",
        json={"issue": {"assigned_pod_id": tenant["other_pod"]["id"]}},
    )
    assert response.status_code == 400, response.text
    assert response.json() == {"assigned_pod_id": [POD_GUARD]}
