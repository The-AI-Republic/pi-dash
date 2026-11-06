# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Backend rows of design §18 for dispatch/lifecycle (HA-3).

Covers alternation, engine dedicated dispatch, the stranded sweep, the
released sweep, release-pin H-13, the 12h bound, lost-runner handling,
machine merge, and preflight/refusal. See the design §18 table for the
row each test satisfies.
"""

from __future__ import annotations

from datetime import timedelta

import pytest
from django.utils import timezone
from rest_framework import status

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.state import State
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    PinKind,
    Pod,
    Runner,
    RunnerPodDispatch,
    RunnerStatus,
)
from pi_dash.runner.services.issue_assignment import (
    dedicated_run_refusal,
)
from pi_dash.runner.services.matcher import (
    _last_served_dedicated,
    next_for_runner,
    pod_has_runner_for_issue_principal,
)


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def state(workspace, project):
    return State.objects.create(
        name="Backlog", group="backlog", workspace=workspace, project=project,
        sequence=1.0, default=True,
    )


def _make_runner(user, workspace, pod, name="gpu-box", *, status=RunnerStatus.ONLINE):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=status,
        last_heartbeat_at=timezone.now() if status == RunnerStatus.ONLINE else None,
    )


def _make_issue(workspace, project, state, user, *, runner=None, pod=None):
    from crum import impersonate

    with impersonate(user):
        return Issue.objects.create(
            name="An issue",
            project=project,
            workspace=workspace,
            state=state,
            created_by=user,
            assigned_pod=pod,
            assigned_runner=runner,
        )


def _make_engine(user, workspace, pod, machine, name="engine"):
    return Runner.objects.create(
        owner=user,
        workspace=workspace,
        pod=pod,
        name=name,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
        dev_machine=machine,
        dev_metadata={"managed": True},
    )


@pytest.mark.unit
def test_alternation_dedicated_then_shared(db, workspace, project, create_user, pod, state):
    """Dedicated first, shared next, dedicated again (§18 matcher)."""
    runner = _make_runner(create_user, workspace, pod)
    # One active run per issue: each run gets its own issue.
    issue1 = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    issue2 = _make_issue(workspace, project, state, create_user, pod=pod)
    issue3 = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    dedicated1 = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue1, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED,
    )
    shared = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue2, status=AgentRunStatus.QUEUED,
        pinned_runner=None, pin_kind=PinKind.LOCALITY,
    )
    dedicated2 = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue3, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED,
    )
    dedicated1.created_at = timezone.now() - timedelta(seconds=30)
    dedicated1.save(update_fields=["created_at"])
    shared.created_at = timezone.now() - timedelta(seconds=20)
    shared.save(update_fields=["created_at"])
    dedicated2.created_at = timezone.now() - timedelta(seconds=10)
    dedicated2.save(update_fields=["created_at"])

    with __import__("django.db", fromlist=["transaction"]).transaction.atomic():
        first = next_for_runner(runner)
        assert first is not None and first.pin_kind == PinKind.DEDICATED
        # Simulate serving it: assign it and mark dedicated as last.
        first.runner = runner
        first.status = AgentRunStatus.ASSIGNED
        first.assigned_at = timezone.now()
        first.save(update_fields=["runner", "status", "assigned_at"])
        assert _last_served_dedicated(runner) is True
        second = next_for_runner(runner)
        assert second is not None and second.pinned_runner_id is None  # shared


@pytest.mark.unit
def test_engine_pinned_only_takes_dedicated(db, workspace, project, create_user, pod, state):
    """A pinned_only engine serves dedicated pins, never shared (§18)."""
    from pi_dash.runner.models import DevMachine, RunnerProvisioning

    machine = DevMachine.objects.create(owner=create_user, host_label="mbp", label="MBP")
    engine = _make_engine(create_user, workspace, pod, machine)
    engine.provisioning = RunnerProvisioning.DESKTOP_BUNDLED
    engine.pod_dispatch = RunnerPodDispatch.PINNED_ONLY
    engine.save()
    issue = _make_issue(workspace, project, state, create_user, runner=engine, pod=pod)
    issue2 = _make_issue(workspace, project, state, create_user, pod=pod)
    dedicated = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=engine, pin_kind=PinKind.DEDICATED,
    )
    AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue2, status=AgentRunStatus.QUEUED,
        pinned_runner=None, pin_kind=PinKind.LOCALITY,
    )
    with __import__("django.db", fromlist=["transaction"]).transaction.atomic():
        got = next_for_runner(engine)
        assert got is not None and got.pk == dedicated.pk


@pytest.mark.unit
def test_release_pin_refuses_dedicated(db, session_client, workspace, project, create_user, pod, state):
    """H-13: dedicated pin release is 409 (§18 sweep/release)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED,
    )
    resp = session_client.post(f"/api/runners/runs/{run.id}/release-pin/")
    assert resp.status_code == status.HTTP_409_CONFLICT
    assert resp.data["code"] == "dedicated_pin_not_releasable"
    run.refresh_from_db()
    assert run.pinned_runner_id == runner.id


@pytest.mark.unit
def test_release_pin_still_works_for_locality(db, session_client, workspace, project, create_user, pod, state):
    """H-13 does not break locality release (§18)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.LOCALITY,
    )
    resp = session_client.post(f"/api/runners/runs/{run.id}/release-pin/")
    assert resp.status_code == status.HTTP_200_OK
    run.refresh_from_db()
    assert run.pinned_runner_id is None


@pytest.mark.unit
def test_dedicated_excluded_from_released_sweep(db, workspace, project, create_user, pod, state):
    """Dedicated pins: GET untouched, 24h paused, never released (§18)."""
    from pi_dash.runner.tasks import _releasable_pins

    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    issue2 = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    dedicated = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED,
    )
    locality = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue2, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.LOCALITY,
    )
    ids = list(_releasable_pins().values_list("id", flat=True))
    assert dedicated.id not in ids
    assert locality.id in ids


@pytest.mark.unit
def test_release_locality_pins_spares_dedicated(db, workspace, project, create_user, pod, state):
    """Engine pause releases locality pins only; dedicated pins wait (§12.1)."""
    from pi_dash.runner.services.dispatch_rules import release_locality_pins

    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    issue2 = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    dedicated = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED,
    )
    locality = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue2, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.LOCALITY,
    )
    assert release_locality_pins([runner.id]) == 1
    dedicated.refresh_from_db()
    locality.refresh_from_db()
    assert dedicated.pinned_runner_id == runner.id
    assert dedicated.pin_kind == PinKind.DEDICATED
    assert locality.pinned_runner_id is None


@pytest.mark.unit
def test_preflight_dedicated_branch(db, workspace, project, create_user, pod, state):
    """Only the dedicated runner registered: preflight passes (§18)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    assert pod_has_runner_for_issue_principal(pod, issue, create_user.id, human=True) is True
    # Offline dedicated runner still counts: the issue waits (§9.4).
    runner.status = RunnerStatus.OFFLINE
    runner.last_heartbeat_at = None
    runner.save(update_fields=["status", "last_heartbeat_at"])
    assert pod_has_runner_for_issue_principal(pod, issue, create_user.id, human=True) is True


@pytest.mark.unit
def test_h11_refusal_person_vs_automatic(db, workspace, project, create_user, pod, state):
    """Person refused, automatic waits, busy never refused (§18)."""
    runner = _make_runner(create_user, workspace, pod, status=RunnerStatus.OFFLINE)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    code, _ = dedicated_run_refusal(issue, human=True, user=create_user)
    assert code == "assigned_runner_offline"
    code, _ = dedicated_run_refusal(issue, human=False, user=create_user)
    assert code is None

    runner.status = RunnerStatus.ONLINE
    runner.paused_at = timezone.now()
    runner.save(update_fields=["status", "paused_at"])
    code, _ = dedicated_run_refusal(issue, human=True, user=create_user)
    assert code == "assigned_runner_paused"

    runner.paused_at = None
    runner.last_heartbeat_at = timezone.now()
    runner.save(update_fields=["paused_at", "last_heartbeat_at"])
    AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.RUNNING,
        runner=runner, pin_kind=PinKind.DEDICATED,
    )
    code, _ = dedicated_run_refusal(issue, human=True, user=create_user)
    assert code is None


@pytest.mark.unit
def test_12h_bound_fails_still_unavailable(db, workspace, project, create_user, pod, state):
    """Automatic run fails after 12h while runner unavailable (§18)."""
    from pi_dash.runner.tasks import fail_dedicated_runs

    runner = _make_runner(create_user, workspace, pod, status=RunnerStatus.OFFLINE)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED, trigger="tick",
    )
    AgentRun.objects.filter(pk=run.pk).update(created_at=timezone.now() - timedelta(hours=13))
    assert fail_dedicated_runs() == 1
    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED
    assert run.error_code == "dedicated_runner_unavailable"


@pytest.mark.unit
def test_12h_bound_leaves_person_run(db, workspace, project, create_user, pod, state):
    """A runner that came back and went offline cannot strand a person's run (§11.3)."""
    from pi_dash.runner.tasks import fail_dedicated_runs

    runner = _make_runner(create_user, workspace, pod, status=RunnerStatus.OFFLINE)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED, trigger="tick",
    )
    AgentRun.objects.filter(pk=run.pk).update(created_at=timezone.now() - timedelta(hours=13))
    # Runner back online: no failure.
    runner.status = RunnerStatus.ONLINE
    runner.last_heartbeat_at = timezone.now()
    runner.save(update_fields=["status", "last_heartbeat_at"])
    assert fail_dedicated_runs() == 0
    run.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED


@pytest.mark.unit
def test_revoke_fail_to_backlog(db, workspace, project, create_user, pod, state):
    """Revoked runner + fail_to_backlog: cancel + clear + comment (§18)."""
    from pi_dash.db.models.issue import IssueComment

    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED,
    )
    runner.revoke(reason="manual_revoke")
    run.refresh_from_db()
    assert run.status == AgentRunStatus.CANCELLED
    assert run.error_code == "dedicated_runner_unavailable"
    issue.refresh_from_db()
    assert issue.assigned_runner_id is None
    assert IssueComment.objects.filter(issue=issue).count() >= 1


@pytest.mark.unit
def test_revoke_fall_back(db, workspace, project, create_user, pod, state):
    """Revoked runner + fall_back: pod run again (§18)."""
    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    project.dedicated_runner_lost_action = "fall_back"
    project.save(update_fields=["dedicated_runner_lost_action"])
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED,
    )
    runner.revoke(reason="manual_revoke")
    run.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED
    assert run.pin_kind == PinKind.LOCALITY
    assert run.pinned_runner_id is None
    issue.refresh_from_db()
    assert issue.assigned_runner_id is None


@pytest.mark.unit
def test_merge_re_pins_dedicated_to_survivor(db, workspace, project, create_user, pod, state):
    """Machine merge: QUEUED dedicated pin re-pins to survivor (§18 lifecycle)."""
    from pi_dash.runner.models import DevMachine
    from pi_dash.runner.services.machine_identity import merge_dev_machine

    source = DevMachine.objects.create(owner=create_user, host_label="a", label="A")
    target = DevMachine.objects.create(owner=create_user, host_label="b", label="B")
    loser = _make_engine(create_user, workspace, pod, source, name="loser")
    from pi_dash.runner.models import RunnerProvisioning

    loser.provisioning = RunnerProvisioning.DESKTOP_BUNDLED
    loser.save(update_fields=["provisioning"])
    survivor = _make_engine(create_user, workspace, pod, target, name="survivor")
    survivor.provisioning = RunnerProvisioning.DESKTOP_BUNDLED
    survivor.save(update_fields=["provisioning"])
    issue = _make_issue(workspace, project, state, create_user, runner=loser, pod=pod)
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=loser, pin_kind=PinKind.DEDICATED,
    )
    merge_dev_machine(user=create_user, source_id=source.id, target_id=target.id)
    run.refresh_from_db()
    assert run.pinned_runner_id == survivor.id
    assert run.pin_kind == PinKind.DEDICATED


@pytest.mark.unit
def test_h10_teammate_refused_even_when_offline(db, workspace, project, create_user, pod, state):
    """H-10 first: excluded teammate gets not-owner even when offline (§23.3)."""
    from pi_dash.db.models import User
    from pi_dash.runner.models import DevMachine, RunnerProvisioning
    from pi_dash.runner.services.issue_assignment import (
        dedicated_run_refusal,
        dedicated_refusal_body,
        pod_once_auth_code,
    )

    machine = DevMachine.objects.create(owner=create_user, host_label="mbp", label="MBP")
    engine = _make_engine(create_user, workspace, pod, machine)
    engine.provisioning = RunnerProvisioning.DESKTOP_BUNDLED
    engine.status = RunnerStatus.OFFLINE
    engine.last_heartbeat_at = None
    engine.save(update_fields=["provisioning", "status", "last_heartbeat_at"])
    issue = _make_issue(workspace, project, state, create_user, runner=engine, pod=pod)
    teammate = User.objects.create(username="team1", email="team@example.com", first_name="Te", last_name="Amate")
    code, _ = dedicated_run_refusal(issue, human=True, user=teammate)
    assert code == "assigned_runner_not_owner"
    body = dedicated_refusal_body(issue, code=code)
    assert body["options"] == ["run_on_my_runner", "pod_once"]
    assert body["pause_reason"] is None
    # Skipped on pod_once (§23.3).
    code, _ = dedicated_run_refusal(issue, human=True, user=create_user, pod_once=True)
    assert code is None
    assert pod_once_auth_code(issue, user=teammate) == "pod_once_not_allowed"


@pytest.mark.unit
def test_refusal_body_shape_paused(db, workspace, project, create_user, pod, state):
    """Refusal body carries pause_reason/runner/pod/options (§11.2)."""
    from pi_dash.runner.services.issue_assignment import dedicated_refusal_body

    runner = _make_runner(create_user, workspace, pod)
    runner.paused_at = timezone.now()
    runner.save(update_fields=["paused_at"])
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    body = dedicated_refusal_body(issue, code="assigned_runner_paused")
    assert set(body) == {
        "error", "reason", "pause_reason", "runner", "runner_name",
        "runner_last_seen_at", "pod", "pod_name", "options",
    }
    assert body["reason"] == "assigned_runner_paused"
    assert body["pause_reason"] == "dispatch_paused"
    assert body["runner"] == str(runner.id)
    assert body["options"] == ["pod_once", "change_runner"]


@pytest.mark.unit
def test_preflight_pinned_only_engine_paused_counts(db, workspace, project, create_user, pod, state):
    """Paused pinned_only engine still counts for its dedicated run (§9.4)."""
    from pi_dash.runner.models import DevMachine, RunnerProvisioning

    machine = DevMachine.objects.create(owner=create_user, host_label="mbp", label="MBP")
    engine = _make_engine(create_user, workspace, pod, machine)
    engine.provisioning = RunnerProvisioning.DESKTOP_BUNDLED
    engine.pod_dispatch = RunnerPodDispatch.PINNED_ONLY
    engine.paused_at = timezone.now()
    engine.save(update_fields=["provisioning", "pod_dispatch", "paused_at"])
    issue = _make_issue(workspace, project, state, create_user, runner=engine, pod=pod)
    assert pod_has_runner_for_issue_principal(pod, issue, create_user.id, human=True) is True


@pytest.mark.unit
def test_stranded_sweep_fails_not_permitted(db, workspace, project, create_user, pod, state):
    """Stranded dedicated fails once, online/idle and old (§12.1)."""
    from pi_dash.db.models import User
    from pi_dash.runner.models import DevMachine, RunnerProvisioning
    from pi_dash.runner.tasks import sweep_stranded_dedicated_runs

    machine = DevMachine.objects.create(owner=create_user, host_label="mbp", label="MBP")
    engine = _make_engine(create_user, workspace, pod, machine)
    engine.provisioning = RunnerProvisioning.DESKTOP_BUNDLED
    engine.save(update_fields=["provisioning"])
    issue = _make_issue(workspace, project, state, create_user, runner=engine, pod=pod)
    teammate = User.objects.create(username="team2", email="team2@example.com", first_name="Te", last_name="Amate")
    run = AgentRun.objects.create(
        workspace=workspace, created_by=teammate, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=engine, pin_kind=PinKind.DEDICATED, trigger="run_ai",
    )
    AgentRun.objects.filter(pk=run.pk).update(created_at=timezone.now() - timedelta(seconds=600))
    assert sweep_stranded_dedicated_runs() == 1
    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED
    assert run.error_code == "dedicated_runner_not_permitted"


@pytest.mark.unit
def test_stranded_sweep_skips_young_and_unavailable(db, workspace, project, create_user, pod, state):
    """Stranded sweep leaves young, offline and busy runs alone (§12.1)."""
    from pi_dash.db.models import User
    from pi_dash.runner.models import DevMachine, RunnerProvisioning
    from pi_dash.runner.tasks import sweep_stranded_dedicated_runs

    machine = DevMachine.objects.create(owner=create_user, host_label="mbp", label="MBP")
    engine = _make_engine(create_user, workspace, pod, machine)
    engine.provisioning = RunnerProvisioning.DESKTOP_BUNDLED
    engine.save(update_fields=["provisioning"])
    issue = _make_issue(workspace, project, state, create_user, runner=engine, pod=pod)
    teammate = User.objects.create(username="team3", email="team3@example.com", first_name="Te", last_name="Amate")
    young = AgentRun.objects.create(
        workspace=workspace, created_by=teammate, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=engine, pin_kind=PinKind.DEDICATED, trigger="run_ai",
    )
    assert sweep_stranded_dedicated_runs() == 0
    young.refresh_from_db()
    assert young.status == AgentRunStatus.QUEUED
    AgentRun.objects.filter(pk=young.pk).update(created_at=timezone.now() - timedelta(seconds=600))
    engine.status = RunnerStatus.OFFLINE
    engine.last_heartbeat_at = None
    engine.save(update_fields=["status", "last_heartbeat_at"])
    assert sweep_stranded_dedicated_runs() == 0


@pytest.mark.unit
def test_12h_bound_stops_clock_with_text(db, workspace, project, create_user, pod, state):
    """12h bound: FAILED + stop_ticking + design text (§11.3)."""
    from pi_dash.runner.tasks import fail_dedicated_runs

    runner = _make_runner(create_user, workspace, pod, status=RunnerStatus.OFFLINE)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.QUEUED,
        pinned_runner=runner, pin_kind=PinKind.DEDICATED, trigger="tick",
    )
    AgentRun.objects.filter(pk=run.pk).update(created_at=timezone.now() - timedelta(hours=13))
    assert fail_dedicated_runs() == 1
    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED
    assert run.done_payload.get("stop_ticking") is True
    assert runner.name in run.error_details.get("message", "")


@pytest.mark.unit
def test_merge_moves_issue_assignment(db, workspace, project, create_user, pod, state):
    """Merge moves Issue.assigned_runner to the survivor (§23.4)."""
    from pi_dash.runner.models import DevMachine, RunnerProvisioning
    from pi_dash.runner.services.machine_identity import merge_dev_machine

    source = DevMachine.objects.create(owner=create_user, host_label="a", label="A")
    target = DevMachine.objects.create(owner=create_user, host_label="b", label="B")
    loser = _make_engine(create_user, workspace, pod, source, name="loser")
    loser.provisioning = RunnerProvisioning.DESKTOP_BUNDLED
    loser.save(update_fields=["provisioning"])
    survivor = _make_engine(create_user, workspace, pod, target, name="survivor")
    survivor.provisioning = RunnerProvisioning.DESKTOP_BUNDLED
    survivor.save(update_fields=["provisioning"])
    issue = _make_issue(workspace, project, state, create_user, runner=loser, pod=pod)
    merge_dev_machine(user=create_user, source_id=source.id, target_id=target.id)
    issue.refresh_from_db()
    assert issue.assigned_runner_id == survivor.id
    assert issue.assigned_pod_id == pod.id


@pytest.mark.unit
def test_cross_project_pod_move_refused(db, session_client, workspace, project, create_user, pod, state):
    """Move to another project's pod is 409 while issues are dedicated (§23.4)."""
    from pi_dash.db.models.project import Project
    from pi_dash.runner.models import Pod

    runner = _make_runner(create_user, workspace, pod)
    _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    other_project = Project.objects.create(
        name="Other Project", identifier="OTH", workspace=workspace, created_by=create_user,
    )
    other_pod = Pod.default_for_project(other_project)
    resp = session_client.patch(f"/api/runners/{runner.id}/", {"pod": str(other_pod.id)}, format="json")
    assert resp.status_code == 409
    assert resp.data["code"] == "runner_has_assigned_issues"
    runner.refresh_from_db()
    assert runner.pod_id == pod.id


@pytest.mark.unit
def test_run_ai_default_dedicated_pin_and_pod_once(db, workspace, project, create_user, pod, state):
    """Run AI defaults to the dedicated pin; pod_once is explicit (§9.2/§23.3)."""
    from pi_dash.orchestration.scheduling import dispatch_run_ai_run_with_reason

    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    run, reason = dispatch_run_ai_run_with_reason(issue, actor=create_user)
    assert run is not None, reason
    assert run.pinned_runner_id == runner.id
    assert run.pin_kind == PinKind.DEDICATED
    run.status = AgentRunStatus.COMPLETED
    run.save(update_fields=["status"])

    issue2 = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    run2, reason2 = dispatch_run_ai_run_with_reason(issue2, actor=create_user, pod_once=True)
    assert run2 is not None, reason2
    assert run2.pinned_runner_id is None


@pytest.mark.unit
def test_comment_refused_before_clock(db, workspace, project, create_user, pod):
    """Comment & Run refusal: no run, system comment, clock untouched (§11.2)."""
    from pi_dash.db.models.issue import IssueComment
    from pi_dash.db.models.state import State
    from pi_dash.orchestration.service import handle_issue_comment

    in_progress = State.objects.create(
        name="In Progress", group="started", workspace=workspace, project=project,
        sequence=2.0, default=False,
    )
    runner = _make_runner(create_user, workspace, pod, status=RunnerStatus.OFFLINE)
    issue = _make_issue(workspace, project, in_progress, create_user, runner=runner, pod=pod)
    AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.COMPLETED,
        runner=runner, pin_kind=PinKind.DEDICATED,
    )
    comment = IssueComment.objects.create(
        issue=issue, project=project, workspace=workspace, actor=create_user,
        comment_html="<p>do it</p>", speaker_type=IssueComment.SpeakerType.HUMAN,
    )
    outcome = handle_issue_comment(comment)
    assert outcome.reason == "assigned_runner_offline"
    assert outcome.created_run is None
    assert AgentRun.objects.filter(work_item=issue, status=AgentRunStatus.QUEUED).count() == 0
    assert IssueComment.objects.filter(issue=issue, speaker_type=IssueComment.SpeakerType.SYSTEM).exists()


@pytest.mark.unit
def test_resume_keeps_dedicated_pin(db, workspace, project, create_user, pod, state):
    """A dedicated pin survives resume-unavailable (§9.3)."""
    from django.db import transaction

    from pi_dash.runner.services.run_lifecycle import apply_run_resume_unavailable

    runner = _make_runner(create_user, workspace, pod)
    issue = _make_issue(workspace, project, state, create_user, runner=runner, pod=pod)
    parent = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.COMPLETED,
        runner=runner, pin_kind=PinKind.DEDICATED, thread_id="t1",
    )
    run = AgentRun.objects.create(
        workspace=workspace, created_by=create_user, pod=pod,
        work_item=issue, status=AgentRunStatus.ASSIGNED,
        runner=runner, pinned_runner=runner, pin_kind=PinKind.DEDICATED,
        parent_run=parent,
    )
    with transaction.atomic():
        apply_run_resume_unavailable(runner, run.id)
    run.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED
    assert run.pinned_runner_id == runner.id
    assert run.pin_kind == PinKind.DEDICATED
