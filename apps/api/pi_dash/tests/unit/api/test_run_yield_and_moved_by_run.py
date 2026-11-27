# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The run's two write-backs to the ticking clock through the external API:

- ``POST /api/v1/workspaces/<slug>/agent-runs/<run_id>/yield/`` — the outcome
  (``pidash run yield``), design §7.
- ``X-Pi-Dash-Run-Id`` on ``PATCH .../work-items/<pk>/`` — "this state move
  was made from inside an agent run", design §5.6.
"""

from __future__ import annotations

import uuid
from unittest import mock

import pytest
from crum import impersonate
from django.utils import timezone
from rest_framework import status as http_status

from pi_dash.db.models import Issue, Project, ProjectMember, State
from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker
from pi_dash.prompting.seed import seed_default_template
from pi_dash.runner.models import AgentRun, AgentRunStatus


@pytest.fixture(autouse=True)
def _no_celery():
    """The work-item PATCH fans activity out over Celery; not what these
    tests are about."""
    with mock.patch("pi_dash.api.views.issue.issue_activity.delay"), mock.patch(
        "pi_dash.api.views.issue.model_activity.delay"
    ), mock.patch("django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()):
        yield


@pytest.fixture
def project(db, workspace, create_user):
    with impersonate(create_user):
        project = Project.objects.create(
            name="Yield",
            identifier="YLD",
            workspace=workspace,
            created_by=create_user,
            agent_default_max_ticks=10,
        )
    ProjectMember.objects.create(project=project, member=create_user, role=20, is_active=True)
    return project


@pytest.fixture
def states(project, create_user, workspace):
    with impersonate(create_user):
        return {
            "todo": State.objects.create(
                name="Todo", project=project, workspace=workspace, group="unstarted", default=True
            ),
            "in_progress": State.objects.create(
                name="In Progress", project=project, workspace=workspace, group="started"
            ),
            "in_review": State.objects.create(name="In Review", project=project, workspace=workspace, group="review"),
        }


@pytest.fixture
def issue(db, workspace, project, states, create_user):
    seed_default_template()
    with impersonate(create_user):
        i = Issue.objects.create(
            name="Task",
            workspace=workspace,
            project=project,
            state=states["todo"],
            created_by=create_user,
        )
    Issue.all_objects.filter(pk=i.pk).update(state=states["in_progress"])
    i.refresh_from_db()
    return i


@pytest.fixture
def active_run(issue, create_user):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        work_item=issue,
        status=AgentRunStatus.RUNNING,
        phase_kind="coding-task",
        prompt="x",
        started_at=timezone.now(),
    )


def _yield_url(workspace, run_id):
    return f"/api/v1/workspaces/{workspace.slug}/agent-runs/{run_id}/yield/"


def _patch_url(workspace, issue):
    return f"/api/v1/workspaces/{workspace.slug}/projects/{issue.project_id}/work-items/{issue.id}/"


# ---------------------------------------------------------------------------
# run yield
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_yield_writes_the_outcome_on_the_run(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(
        _yield_url(workspace, active_run.id),
        {"outcome": "done", "note": "  approved  "},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(active_run.id),
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data["outcome"] == "done"
    assert resp.data["work_item_id"] == str(issue.id)
    active_run.refresh_from_db()
    assert active_run.done_payload["status"] == "done"
    assert active_run.done_payload["note"] == "approved"
    assert "yielded_at" in active_run.done_payload


@pytest.mark.unit
def test_yield_stores_stop_ticking(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(
        _yield_url(workspace, active_run.id),
        {"outcome": "done", "stop_ticking": True},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(active_run.id),
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data["stop_ticking"] is True
    active_run.refresh_from_db()
    assert active_run.done_payload["status"] == "done"
    assert active_run.done_payload["stop_ticking"] is True


@pytest.mark.unit
def test_yield_without_stop_ticking_stores_no_flag(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(_yield_url(workspace, active_run.id), {"outcome": "done"}, format="json")
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data["stop_ticking"] is False
    active_run.refresh_from_db()
    assert "stop_ticking" not in active_run.done_payload


@pytest.mark.unit
def test_yield_rejects_a_non_boolean_stop_ticking(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(
        _yield_url(workspace, active_run.id),
        {"outcome": "done", "stop_ticking": "yes"},
        format="json",
    )
    assert resp.status_code == http_status.HTTP_400_BAD_REQUEST
    active_run.refresh_from_db()
    assert not (active_run.done_payload or {})


@pytest.mark.unit
def test_yield_still_requires_an_outcome_with_stop_ticking(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(_yield_url(workspace, active_run.id), {"stop_ticking": True}, format="json")
    assert resp.status_code == http_status.HTTP_400_BAD_REQUEST
    assert "outcome" in resp.data["error"]


@pytest.mark.unit
def test_yield_accepts_every_vocabulary_word(api_key_client, workspace, issue, active_run):
    for outcome in ("progressed", "waiting_on_human", "waiting_on_external", "done", "blocked"):
        resp = api_key_client.post(_yield_url(workspace, active_run.id), {"outcome": outcome}, format="json")
        assert resp.status_code == http_status.HTTP_200_OK, (outcome, resp.data)
    active_run.refresh_from_db()
    assert active_run.done_payload["status"] == "blocked"


@pytest.mark.unit
def test_yield_rejects_an_unknown_outcome(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(_yield_url(workspace, active_run.id), {"outcome": "finished"}, format="json")
    assert resp.status_code == http_status.HTTP_400_BAD_REQUEST
    assert "done" in resp.data["allowed"]


@pytest.mark.unit
def test_yield_rejects_a_mismatched_header(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(
        _yield_url(workspace, active_run.id),
        {"outcome": "done"},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(uuid.uuid4()),
    )
    assert resp.status_code == http_status.HTTP_400_BAD_REQUEST


@pytest.mark.unit
def test_yield_404s_for_a_stale_or_foreign_run(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(_yield_url(workspace, uuid.uuid4()), {"outcome": "done"}, format="json")
    assert resp.status_code == http_status.HTTP_404_NOT_FOUND


@pytest.mark.unit
def test_yield_409s_when_the_run_is_no_longer_active(api_key_client, workspace, issue, active_run):
    AgentRun.objects.filter(pk=active_run.pk).update(status=AgentRunStatus.COMPLETED)
    resp = api_key_client.post(_yield_url(workspace, active_run.id), {"outcome": "done"}, format="json")
    assert resp.status_code == http_status.HTTP_409_CONFLICT


@pytest.mark.unit
def test_yield_requires_workspace_membership(api_client, workspace, issue, active_run):
    from pi_dash.db.models import User
    from pi_dash.db.models.api import APIToken

    outsider = User.objects.create(email="outsider@example.com", username="outsider")
    token = APIToken.objects.create(user=outsider, label="x", token="outsider-token-1")
    api_client.credentials(HTTP_X_API_KEY=token.token)
    resp = api_client.post(_yield_url(workspace, active_run.id), {"outcome": "done"}, format="json")
    assert resp.status_code == http_status.HTTP_404_NOT_FOUND


# ---------------------------------------------------------------------------
# PATCH with X-Pi-Dash-Run-Id — agent move vs human move
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_patch_with_run_id_header_is_an_agent_move(api_key_client, workspace, issue, states, active_run):
    """An agent moving the issue from inside its run queues the next stage's
    entry on the clock (it counts); no second run is created while the
    agent's own run is active."""
    IssueAgentTicker.objects.create(issue=issue, used=2, enabled=True, next_run_at=timezone.now())
    resp = api_key_client.patch(
        _patch_url(workspace, issue),
        {"state": str(states["in_review"].id)},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(active_run.id),
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    issue.refresh_from_db()
    assert issue.state_id == states["in_review"].id
    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.pending_entry is True
    assert ticker.pending_entry_free is False
    assert ticker.used == 2
    assert AgentRun.objects.filter(work_item=issue).count() == 1


@pytest.mark.unit
def test_patch_with_run_id_header_and_spent_pool_parks(api_key_client, workspace, issue, states, active_run):
    IssueAgentTicker.objects.create(issue=issue, used=10, enabled=True, next_run_at=timezone.now())
    resp = api_key_client.patch(
        _patch_url(workspace, issue),
        {"state": str(states["in_review"].id)},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(active_run.id),
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    issue.refresh_from_db()
    assert issue.state_id == states["in_review"].id  # the truthful state still lands
    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.enabled is False
    assert ticker.pending_entry is False
    assert ticker.disarm_reason == "pool_spent"


@pytest.mark.unit
def test_patch_without_header_is_a_human_move(api_key_client, workspace, issue, states):
    """No header and no active run of the caller's on this issue: a human
    move — free, and because someone else's run is active it is queued as
    a free entry."""
    from pi_dash.db.models import User, WorkspaceMember

    other = User.objects.create(email="member-d@example.com", username="member_d")
    WorkspaceMember.objects.create(workspace=workspace, member=other, role=15)
    AgentRun.objects.create(
        workspace=workspace, created_by=other, work_item=issue,
        status=AgentRunStatus.RUNNING, phase_kind="coding-task", prompt="x", started_at=timezone.now(),
    )
    IssueAgentTicker.objects.create(issue=issue, used=10, enabled=False, disarm_reason="pool_spent")
    resp = api_key_client.patch(
        _patch_url(workspace, issue), {"state": str(states["in_review"].id)}, format="json"
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.pending_entry is True
    assert ticker.pending_entry_free is True
    assert ticker.used == 10


@pytest.mark.unit
def test_patch_without_header_from_the_callers_own_active_run_is_an_agent_move(
    api_key_client, workspace, issue, states, active_run
):
    """An older ``pidash`` binary sends no header. Its moves must still be
    agent moves: the caller owns the run that is active on this issue, so
    the server infers it — a counted entry, and parking on a spent pool —
    rather than handing out free runs to a mixed fleet."""
    IssueAgentTicker.objects.create(issue=issue, used=2, enabled=True, next_run_at=timezone.now())
    resp = api_key_client.patch(
        _patch_url(workspace, issue), {"state": str(states["in_review"].id)}, format="json"
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.pending_entry is True
    assert ticker.pending_entry_free is False


@pytest.mark.unit
def test_agent_cannot_re_tick_its_own_issue(api_key_client, workspace, issue, active_run):
    IssueAgentTicker.objects.create(issue=issue, used=10, enabled=False, disarm_reason="pool_spent")
    url = f"/api/v1/workspaces/{workspace.slug}/projects/{issue.project_id}/work-items/{issue.id}/re-tick/"
    resp = api_key_client.post(url, {}, format="json", HTTP_X_PI_DASH_RUN_ID=str(active_run.id))
    assert resp.status_code == http_status.HTTP_403_FORBIDDEN
    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.granted == 0


@pytest.mark.unit
def test_patch_rejects_a_malformed_run_id(api_key_client, workspace, issue, states, active_run):
    resp = api_key_client.patch(
        _patch_url(workspace, issue),
        {"state": str(states["in_review"].id)},
        format="json",
        HTTP_X_PI_DASH_RUN_ID="not-a-uuid",
    )
    assert resp.status_code == http_status.HTTP_400_BAD_REQUEST
    issue.refresh_from_db()
    assert issue.state_id == states["in_progress"].id


@pytest.mark.unit
def test_patch_with_an_unknown_or_foreign_run_id_is_a_plain_request(
    api_key_client, workspace, issue, states, active_run, create_user
):
    """The CLI sends the header on every write for the life of the run —
    including patches to *other* issues. A run that is not active on this
    issue is not an agent move on it; the request goes through as a plain
    (human) one instead of failing."""
    other = Issue.objects.create(
        name="Other", workspace=workspace, project=issue.project, state=states["todo"], created_by=create_user
    )
    other_run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, work_item=other,
        status=AgentRunStatus.RUNNING, phase_kind="coding-task", prompt="x", started_at=timezone.now(),
    )
    IssueAgentTicker.objects.create(issue=issue, used=10, enabled=False, disarm_reason="pool_spent")
    for header in (str(uuid.uuid4()), str(other_run.id)):
        resp = api_key_client.patch(
            _patch_url(workspace, issue),
            {"priority": "high"},
            format="json",
            HTTP_X_PI_DASH_RUN_ID=header,
        )
        assert resp.status_code == http_status.HTTP_200_OK, (header, resp.data)
    # And a state move with a foreign id is a *human* move: free entry queued.
    resp = api_key_client.patch(
        _patch_url(workspace, issue),
        {"state": str(states["in_review"].id)},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(other_run.id),
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    ticker = IssueAgentTicker.objects.get(issue=issue)
    assert ticker.pending_entry_free is True


@pytest.mark.unit
def test_patch_with_a_finished_run_id_is_a_plain_request(api_key_client, workspace, issue, states, active_run):
    AgentRun.objects.filter(pk=active_run.pk).update(status=AgentRunStatus.COMPLETED)
    resp = api_key_client.patch(
        _patch_url(workspace, issue),
        {"state": str(states["in_review"].id)},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(active_run.id),
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data


@pytest.mark.unit
def test_patch_rejects_another_members_run_id(api_key_client, workspace, issue, states, create_user):
    """Membership is not authority over someone else's run: B cannot stamp
    their move as A's agent move."""
    from pi_dash.db.models import User, WorkspaceMember

    other = User.objects.create(email="member-b@example.com", username="member_b")
    WorkspaceMember.objects.create(workspace=workspace, member=other, role=15)
    theirs = AgentRun.objects.create(
        workspace=workspace, created_by=other, work_item=issue,
        status=AgentRunStatus.RUNNING, phase_kind="coding-task", prompt="x", started_at=timezone.now(),
    )
    resp = api_key_client.patch(
        _patch_url(workspace, issue),
        {"state": str(states["in_review"].id)},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(theirs.id),
    )
    assert resp.status_code == http_status.HTTP_400_BAD_REQUEST
    assert "not yours" in resp.data["error"]


@pytest.mark.unit
def test_yield_rejects_another_members_run(api_key_client, workspace, issue, create_user):
    from pi_dash.db.models import User, WorkspaceMember

    other = User.objects.create(email="member-c@example.com", username="member_c")
    WorkspaceMember.objects.create(workspace=workspace, member=other, role=15)
    theirs = AgentRun.objects.create(
        workspace=workspace, created_by=other, work_item=issue,
        status=AgentRunStatus.RUNNING, phase_kind="coding-task", prompt="x", started_at=timezone.now(),
    )
    resp = api_key_client.post(_yield_url(workspace, theirs.id), {"outcome": "done"}, format="json")
    assert resp.status_code == http_status.HTTP_404_NOT_FOUND
    theirs.refresh_from_db()
    assert theirs.done_payload is None


@pytest.mark.unit
def test_yield_allowed_for_the_runner_owner(api_key_client, workspace, issue, create_user):
    """Tick-started runs are created by the bot; the agent's CLI speaks as
    the runner owner, who must be able to yield on them."""
    from pi_dash.orchestration.workpad import get_agent_system_user
    from pi_dash.runner.models import Pod, Runner, RunnerStatus

    pod = Pod.default_for_project(issue.project)
    runner = Runner.objects.create(
        owner=create_user, workspace=workspace, pod=pod, name="r", status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )
    run = AgentRun.objects.create(
        workspace=workspace, created_by=get_agent_system_user(), work_item=issue, runner=runner, pod=pod,
        status=AgentRunStatus.RUNNING, phase_kind="coding-task", prompt="x", started_at=timezone.now(),
    )
    resp = api_key_client.post(_yield_url(workspace, run.id), {"outcome": "progressed"}, format="json")
    assert resp.status_code == http_status.HTTP_200_OK, resp.data


# ---------------------------------------------------------------------------
# run yield on a run with no work item (scheduler / direct) — PDASHOSS01-276
# ---------------------------------------------------------------------------


@pytest.fixture
def scheduler_run(db, workspace, project, create_user):
    """An active run fired by a scheduler binding: project-scoped, so
    ``work_item`` is NULL and ``scheduler_binding`` is set."""
    from datetime import timedelta

    from pi_dash.db.models import Scheduler, SchedulerBinding
    from pi_dash.runner.models import AgentRunTrigger, Pod

    with impersonate(create_user):
        scheduler = Scheduler.objects.create(
            workspace=workspace, slug="coordinator", name="Coordinator", description="x", prompt="Scan."
        )
        binding = SchedulerBinding.objects.create(
            scheduler=scheduler,
            project=project,
            workspace=workspace,
            dtstart=timezone.now() - timedelta(days=1),
            rrule="FREQ=HOURLY",
            tzid="UTC",
            enabled=True,
            actor=create_user,
        )
    return AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=Pod.default_for_project(project),
        work_item=None,
        scheduler_binding=binding,
        trigger=AgentRunTrigger.SCHEDULER,
        status=AgentRunStatus.RUNNING,
        prompt="x",
        started_at=timezone.now(),
    )


@pytest.mark.unit
def test_yield_records_the_outcome_on_a_scheduler_run(api_key_client, workspace, scheduler_run):
    resp = api_key_client.post(
        _yield_url(workspace, scheduler_run.id),
        {"outcome": "done", "note": "  nothing to file  "},
        format="json",
        HTTP_X_PI_DASH_RUN_ID=str(scheduler_run.id),
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data["ok"] is True
    assert resp.data["run_id"] == str(scheduler_run.id)
    assert resp.data["outcome"] == "done"
    assert resp.data["run_kind"] == "scheduler"
    assert resp.data["work_item_id"] is None
    assert resp.data["scheduler_binding_id"] == str(scheduler_run.scheduler_binding_id)
    assert resp.data["stop_ticking"] is False
    scheduler_run.refresh_from_db()
    assert scheduler_run.done_payload["status"] == "done"
    assert scheduler_run.done_payload["note"] == "nothing to file"
    assert "yielded_at" in scheduler_run.done_payload


@pytest.mark.unit
def test_yield_on_a_scheduler_run_ignores_stop_ticking(api_key_client, workspace, scheduler_run):
    """A scheduled run has no ticking clock. ``--stop-ticking`` is accepted
    (the agent's command still exits 0) but nothing is stored, and the
    response says so."""
    resp = api_key_client.post(
        _yield_url(workspace, scheduler_run.id),
        {"outcome": "blocked", "stop_ticking": True},
        format="json",
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data["stop_ticking"] is False
    assert "no ticking clock" in resp.data["detail"]
    scheduler_run.refresh_from_db()
    assert scheduler_run.done_payload["status"] == "blocked"
    assert "stop_ticking" not in scheduler_run.done_payload
    binding = scheduler_run.scheduler_binding
    binding.refresh_from_db()
    assert binding.enabled is True


@pytest.mark.unit
def test_scheduler_run_outcome_survives_the_terminal_payload(api_key_client, workspace, scheduler_run):
    """The runner's terminal ``done_payload`` arrives after the yield and
    must not erase it, and the binding's terminate hook still runs."""
    from pi_dash.runner.services.agent_run_finalization import finalize_agent_run

    binding = scheduler_run.scheduler_binding
    binding.last_error = "previous tick failed"
    binding.save(update_fields=["last_error"])
    resp = api_key_client.post(_yield_url(workspace, scheduler_run.id), {"outcome": "progressed"}, format="json")
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert finalize_agent_run(
        scheduler_run.id, AgentRunStatus.COMPLETED, updates={"done_payload": {"conclusion": "ok"}}
    )
    scheduler_run.refresh_from_db()
    assert scheduler_run.status == AgentRunStatus.COMPLETED
    assert scheduler_run.done_payload["status"] == "progressed"
    assert scheduler_run.done_payload["conclusion"] == "ok"
    binding.refresh_from_db()
    assert binding.last_error == ""


@pytest.mark.unit
def test_yield_rejects_another_members_scheduler_run(api_key_client, workspace, scheduler_run):
    from pi_dash.db.models import User, WorkspaceMember

    other = User.objects.create(email="member-d@example.com", username="member_d")
    WorkspaceMember.objects.create(workspace=workspace, member=other, role=15)
    AgentRun.objects.filter(pk=scheduler_run.pk).update(created_by=other)
    resp = api_key_client.post(_yield_url(workspace, scheduler_run.id), {"outcome": "done"}, format="json")
    assert resp.status_code == http_status.HTTP_404_NOT_FOUND
    scheduler_run.refresh_from_db()
    assert scheduler_run.done_payload is None


@pytest.mark.unit
def test_yield_409s_on_a_finished_scheduler_run(api_key_client, workspace, scheduler_run):
    AgentRun.objects.filter(pk=scheduler_run.pk).update(status=AgentRunStatus.COMPLETED)
    resp = api_key_client.post(_yield_url(workspace, scheduler_run.id), {"outcome": "done"}, format="json")
    assert resp.status_code == http_status.HTTP_409_CONFLICT
    scheduler_run.refresh_from_db()
    assert scheduler_run.done_payload is None


@pytest.mark.unit
def test_yield_records_the_outcome_on_a_direct_run(api_key_client, workspace, project, create_user):
    """A run with neither a work item nor a scheduler binding (a direct
    run) is still the caller's run — record the outcome, don't 404."""
    from pi_dash.runner.models import Pod

    run = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        work_item=None,
        pod=Pod.default_for_project(project),
        status=AgentRunStatus.RUNNING,
        prompt="x",
        started_at=timezone.now(),
    )
    resp = api_key_client.post(_yield_url(workspace, run.id), {"outcome": "done"}, format="json")
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data["run_kind"] == "direct"
    assert resp.data["work_item_id"] is None
    run.refresh_from_db()
    assert run.done_payload["status"] == "done"


@pytest.mark.unit
def test_issue_run_yield_response_is_unchanged_but_for_run_kind(api_key_client, workspace, issue, active_run):
    resp = api_key_client.post(
        _yield_url(workspace, active_run.id), {"outcome": "done", "stop_ticking": True}, format="json"
    )
    assert resp.status_code == http_status.HTTP_200_OK, resp.data
    assert resp.data == {
        "ok": True,
        "run_id": str(active_run.id),
        "work_item_id": str(issue.id),
        "outcome": "done",
        "stop_ticking": True,
        "run_kind": "issue",
    }
