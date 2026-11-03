"""Contract tests for PATCH intake-issues/<pk>/ agent_executor (PIDASHCONV-1429).

Covers the three executor-validation cases against
`IssueCreateSerializer.validate` (`app/serializers/issue.py:275-321`):

- cloud pin: refused with Django's message unless `CLOUD_AGENT_ENABLED`.
- managed pin: refused with the `_MANAGED_UNAVAILABLE_DETAIL` text for the
  reason (disabled instance here); nothing is stored.
- null clear with a pinned executor and a live run: refused with the
  mid-flight message; the stored pin is untouched.
- PIDASHCONV-1500 coincidence: a changed pin under a live run that also
  fails a gate reports only the gate message (first-failure-wins), for
  both the cloud and the managed gate.

Regimes: CI runs both backends with the kill switches off, so the cloud
case asserts the 400 message by default. With
`CONTRACT_CLOUD_AGENT_ENABLED=true` (local servers started with
`CLOUD_AGENT_ENABLED=true`) it asserts the pin lands instead — the branch
that fails before the fix. The managed-disabled and null cases fail
before the fix in every regime. (Kill switches are process-global with
no per-test override; the null case additionally needs PIDASHCONV-1428.)
"""

import os

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract

CLOUD_ENABLED = os.environ.get("CONTRACT_CLOUD_AGENT_ENABLED", "false").lower() in (
    "1",
    "true",
    "yes",
)

CLOUD_UNAVAILABLE = "Pi Dash Cloud Agent is not available on this instance"
MANAGED_DISABLED = "Pi Dash Agent is not enabled on this instance"
MID_FLIGHT = "cannot change the execution target while the issue has an active run"


def _issue_url(tenant):
    return intake_urls(tenant)["intake_issues"]


def test_patch_cloud_agent_pin(make_tenant):
    """Cloud pin: 200 + stored when the instance enables it, else Django's 400."""
    tenant = make_tenant()
    created = create_intake_issue(tenant["client"], _issue_url(tenant), name="Cloud pin")
    issue_id = created["issue"]["id"]
    response = tenant["client"].patch(
        f"{_issue_url(tenant)}{issue_id}/",
        json={"issue": {"agent_executor": "cloud_agent"}},
    )
    if CLOUD_ENABLED:
        assert response.status_code == 200, response.text
        assert response.json()["issue"]["agent_executor"] == "cloud_agent"
    else:
        assert response.status_code == 400, response.text
        assert response.json() == {"agent_executor": [CLOUD_UNAVAILABLE]}


def test_patch_managed_runner_disabled(make_tenant):
    """Managed pin on a disabled instance: Django's 400, nothing stored."""
    tenant = make_tenant()
    created = create_intake_issue(tenant["client"], _issue_url(tenant), name="Managed pin")
    issue_id = created["issue"]["id"]
    response = tenant["client"].patch(
        f"{_issue_url(tenant)}{issue_id}/",
        json={"issue": {"agent_executor": "managed_runner"}},
    )
    assert response.status_code == 400, response.text
    assert response.json() == {"agent_executor": [MANAGED_DISABLED]}
    again = tenant["client"].get(f"{_issue_url(tenant)}{issue_id}/")
    assert again.status_code == 200, again.text
    assert again.json()["issue"]["agent_executor"] is None


def test_patch_clear_executor_mid_run_refused(make_tenant):
    """Null clear under a live run: Django's 400, the stored pin is untouched."""
    tenant = make_tenant()
    created = create_intake_issue(tenant["client"], _issue_url(tenant), name="Null clear")
    issue_id = created["issue"]["id"]
    tenant["seeder"].db.execute(
        "UPDATE issues SET agent_executor = 'local_runner' WHERE id = %s",
        (issue_id,),
    )
    pod = tenant["seeder"].create_pod(
        tenant["workspace"]["id"], tenant["project"]["id"], name="Executor pin pod"
    )
    tenant["seeder"].create_agent_run(
        tenant["workspace"]["id"],
        tenant["user"]["id"],
        pod["id"],
        issue_id,
        status="running",
    )
    response = tenant["client"].patch(
        f"{_issue_url(tenant)}{issue_id}/",
        json={"issue": {"agent_executor": None}},
    )
    assert response.status_code == 400, response.text
    assert response.json() == {"agent_executor": [MID_FLIGHT]}
    # The pin is untouched. Read from the database, not GET: rendering a
    # run-bearing issue is PIDASHCONV-1435's scope (500s on Rust today),
    # while this 400 short-circuits before the render.
    row = tenant["seeder"].db.fetchone(
        "SELECT agent_executor FROM issues WHERE id = %s",
        (issue_id,),
    )
    assert row["agent_executor"] == "local_runner"


def _seed_pinned_live_issue(tenant, name):
    """Issue pinned to local_runner with a running agent_run; returns its id."""
    created = create_intake_issue(tenant["client"], _issue_url(tenant), name=name)
    issue_id = created["issue"]["id"]
    tenant["seeder"].db.execute(
        "UPDATE issues SET agent_executor = 'local_runner' WHERE id = %s",
        (issue_id,),
    )
    pod = tenant["seeder"].create_pod(
        tenant["workspace"]["id"], tenant["project"]["id"], name=f"{name} pod"
    )
    tenant["seeder"].create_agent_run(
        tenant["workspace"]["id"],
        tenant["user"]["id"],
        pod["id"],
        issue_id,
        status="running",
    )
    return issue_id


def _stored_executor(tenant, issue_id):
    row = tenant["seeder"].db.fetchone(
        "SELECT agent_executor FROM issues WHERE id = %s",
        (issue_id,),
    )
    return row["agent_executor"]


@pytest.mark.skipif(
    CLOUD_ENABLED, reason="needs the cloud gate to refuse (kill switch off)"
)
def test_patch_cloud_pin_under_live_run_reports_gate_only(make_tenant):
    """PIDASHCONV-1500: gate refusal aborts validate(), mid-flight never runs.

    The pin changes under a live run AND the cloud gate refuses, so both
    arms would fire if validation collected errors. Django raises on the
    gate first (serializers/issue.py:283-309 runs before :313-321), so the
    400 carries exactly one agent_executor message and the pin is untouched.
    """
    tenant = make_tenant()
    issue_id = _seed_pinned_live_issue(tenant, "Cloud pin live")
    response = tenant["client"].patch(
        f"{_issue_url(tenant)}{issue_id}/",
        json={"issue": {"agent_executor": "cloud_agent"}},
    )
    assert response.status_code == 400, response.text
    assert response.json() == {"agent_executor": [CLOUD_UNAVAILABLE]}
    assert _stored_executor(tenant, issue_id) == "local_runner"


def test_patch_managed_pin_under_live_run_reports_gate_only(make_tenant):
    """PIDASHCONV-1500: managed-gate refusal likewise wins over mid-flight.

    Mirror of the cloud case on a managed-disabled instance: Django raises
    the managed message (serializers/issue.py:290-309) before the
    mid-flight check (:313-321) can run.
    """
    tenant = make_tenant()
    issue_id = _seed_pinned_live_issue(tenant, "Managed pin live")
    response = tenant["client"].patch(
        f"{_issue_url(tenant)}{issue_id}/",
        json={"issue": {"agent_executor": "managed_runner"}},
    )
    assert response.status_code == 400, response.text
    assert response.json() == {"agent_executor": [MANAGED_DISABLED]}
    assert _stored_executor(tenant, issue_id) == "local_runner"
