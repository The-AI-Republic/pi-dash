"""Contract tests for PATCH intake-issues/<pk>/ agent_executor (PIDASHCONV-1429).

Covers the executor-validation cases against the current Django
`normalize_issue_executor` (`runner/services/issue_assignment.py`, built
engine as runner §6 — pods-only dispatch):

- cloud pin: earlier clients' `cloud_agent` is stored as `local_runner` (200).
- managed pin: earlier clients' `managed_runner` is stored as `local_runner` (200).
- null clear with a pinned executor and a live run: refused with the
  mid-flight message; the stored pin is untouched.
- PIDASHCONV-1500 coincidence: a deprecated pin under a live run that is
  already pinned to `local_runner` normalizes to `local_runner`, so it is
  unchanged and reports 200 (no mid-flight error).

Regimes: pods-only wins regardless of `CLOUD_AGENT_ENABLED` /
`MANAGED_RUNNER_ENABLED` — the deprecated writes never gate, they always
normalize. The null case still needs the mid-flight check.
"""

import os

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract

MID_FLIGHT = "cannot change the execution target while the issue has an active run"


def _issue_url(tenant):
    return intake_urls(tenant)["intake_issues"]


def test_patch_cloud_agent_pin(make_tenant):
    """Cloud pin: pods-only normalizes to local_runner (200, stored)."""
    tenant = make_tenant()
    created = create_intake_issue(tenant["client"], _issue_url(tenant), name="Cloud pin")
    issue_id = created["issue"]["id"]
    response = tenant["client"].patch(
        f"{_issue_url(tenant)}{issue_id}/",
        json={"issue": {"agent_executor": "cloud_agent"}},
    )
    assert response.status_code == 200, response.text
    assert response.json()["issue"]["agent_executor"] == "local_runner"


def test_patch_managed_runner_disabled(make_tenant):
    """Managed pin: pods-only normalizes to local_runner (200, stored)."""
    tenant = make_tenant()
    created = create_intake_issue(tenant["client"], _issue_url(tenant), name="Managed pin")
    issue_id = created["issue"]["id"]
    response = tenant["client"].patch(
        f"{_issue_url(tenant)}{issue_id}/",
        json={"issue": {"agent_executor": "managed_runner"}},
    )
    assert response.status_code == 200, response.text
    assert response.json()["issue"]["agent_executor"] == "local_runner"
    again = tenant["client"].get(f"{_issue_url(tenant)}{issue_id}/")
    assert again.status_code == 200, again.text
    assert again.json()["issue"]["agent_executor"] == "local_runner"


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


def test_patch_cloud_pin_under_live_run_reports_gate_only(make_tenant):
    """Deprecated cloud pin under live run: normalizes to local_runner (200).

    Pods-only: `cloud_agent` writes always normalize to `local_runner`.
    Stored value is already `local_runner`, so the normalized write is a
    no-op — no mid-flight error, 200.
    """
    tenant = make_tenant()
    issue_id = _seed_pinned_live_issue(tenant, "Cloud pin live")
    response = tenant["client"].patch(
        f"{_issue_url(tenant)}{issue_id}/",
        json={"issue": {"agent_executor": "cloud_agent"}},
    )
    assert response.status_code == 200, response.text
    assert response.json()["issue"]["agent_executor"] == "local_runner"
    assert _stored_executor(tenant, issue_id) == "local_runner"


def test_patch_managed_pin_under_live_run_reports_gate_only(make_tenant):
    """Deprecated managed pin under live run: normalizes to local_runner (200)."""
    tenant = make_tenant()
    issue_id = _seed_pinned_live_issue(tenant, "Managed pin live")
    response = tenant["client"].patch(
        f"{_issue_url(tenant)}{issue_id}/",
        json={"issue": {"agent_executor": "managed_runner"}},
    )
    assert response.status_code == 200, response.text
    assert response.json()["issue"]["agent_executor"] == "local_runner"
    assert _stored_executor(tenant, issue_id) == "local_runner"
