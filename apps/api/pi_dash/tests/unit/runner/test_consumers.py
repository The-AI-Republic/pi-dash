# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for ``RunnerConsumer``'s sync helpers.

The consumer extends ``AsyncJsonWebsocketConsumer`` but most of the
state-machine logic is delegated to plain sync methods that we can call
directly without spinning up Channels. These tests cover:

- ``_handle_run_paused`` — pause path, including HTML escaping of
  agent-supplied question/summary that gets stored as an IssueComment.
- ``_handle_resume_unavailable`` — re-queue + pin-drop on the typed
  resume-failure path.
"""

from __future__ import annotations

from unittest.mock import patch

import pytest
from crum import impersonate
from django.utils import timezone

from pi_dash.db.models.issue import Issue, IssueComment
from pi_dash.db.models.project import Project
from pi_dash.db.models.state import State
from pi_dash.runner.consumers import RunnerConsumer
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerStatus,
)


@pytest.fixture
def pod(workspace):
    return Pod.default_for_project(project)


@pytest.fixture
def online_runner(db, create_user, workspace, pod):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="rX",
        credential_hash="h",
        credential_fingerprint="f" * 12,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
    )


@pytest.fixture
def issue_in_progress(db, workspace, create_user):
    with impersonate(create_user):
        project = Project.objects.create(
            name="P", identifier="P", workspace=workspace, created_by=create_user
        )
        in_progress = State.objects.create(
            name="In Progress", project=project, group="started"
        )
        return Issue.objects.create(
            name="task",
            workspace=workspace,
            project=project,
            state=in_progress,
            created_by=create_user,
        )


@pytest.fixture(autouse=True)
def _on_commit_immediate():
    with patch(
        "django.db.transaction.on_commit", side_effect=lambda fn, **kw: fn()
    ):
        yield


@pytest.fixture(autouse=True)
def _stub_send_to_runner():
    with patch("pi_dash.runner.services.pubsub.send_to_runner"):
        yield


def _consumer_for(runner):
    consumer = RunnerConsumer()
    consumer.runner = runner
    return consumer


# ---------------------------------------------------------------------------
# _handle_run_paused
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_paused_parks_run_without_ended_at(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        status=AgentRunStatus.RUNNING,
        prompt="working",
        started_at=timezone.now() - timezone.timedelta(minutes=2),
    )
    msg = {
        "run_id": str(run.id),
        "payload": {
            "status": "paused",
            "summary": "stuck",
            "autonomy": {"question_for_human": "which API surface?"},
        },
    }
    _consumer_for(online_runner)._handle_run_paused(online_runner, msg)
    run.refresh_from_db()
    assert run.status == AgentRunStatus.PAUSED_AWAITING_INPUT
    # Pause is non-terminal — ended_at must remain NULL.
    assert run.ended_at is None
    assert run.done_payload["autonomy"]["question_for_human"] == "which API surface?"


@pytest.mark.unit
def test_paused_escapes_agent_supplied_html_in_comment(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    """Agent payload is untrusted (upstream prompt can shape it). The
    comment we surface must escape HTML, never persist raw markup that
    could execute when rendered in the issue feed.
    """
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        status=AgentRunStatus.RUNNING,
        prompt="x",
        started_at=timezone.now(),
    )
    payload = {
        "summary": "<img src=x onerror=alert(1)>",
        "autonomy": {
            "question_for_human": "<script>alert('xss')</script>",
        },
    }
    _consumer_for(online_runner)._handle_run_paused(
        online_runner, {"run_id": str(run.id), "payload": payload}
    )

    comment = (
        IssueComment.objects.filter(issue=issue_in_progress)
        .order_by("-created_at")
        .first()
    )
    assert comment is not None
    html = comment.comment_html
    # No raw script/img tags reach storage — they only appear as escaped
    # text content. ``onerror=`` survives as a literal substring inside the
    # escaped form, which is harmless because it's no longer an attribute
    # on a real element.
    assert "<script>" not in html
    assert "<img" not in html
    # Escaped form is present — sanity check that escaping ran.
    assert "&lt;script&gt;" in html
    assert "&lt;img" in html


@pytest.mark.unit
def test_paused_skips_comment_when_no_question_or_summary(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        status=AgentRunStatus.RUNNING,
        prompt="x",
        started_at=timezone.now(),
    )
    _consumer_for(online_runner)._handle_run_paused(
        online_runner, {"run_id": str(run.id), "payload": {}}
    )
    assert IssueComment.objects.filter(issue=issue_in_progress).count() == 0


@pytest.mark.unit
def test_paused_sweep_creates_continuation_for_mid_run_comment(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    """Comments that arrived during RUNNING were skipped with
    'prior-run-active'. The pause transition is the symmetric recovery
    point — those comments must wake R_next.
    """
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        status=AgentRunStatus.RUNNING,
        prompt="x",
        started_at=timezone.now() - timezone.timedelta(minutes=5),
    )
    # Mid-run comment.
    with impersonate(create_user):
        IssueComment.objects.create(
            issue=issue_in_progress,
            project=issue_in_progress.project,
            workspace=workspace,
            actor=create_user,
            comment_html="<p>use option B</p>",
        )

    _consumer_for(online_runner)._handle_run_paused(
        online_runner,
        {
            "run_id": str(run.id),
            "payload": {"autonomy": {"question_for_human": "?"}},
        },
    )

    follow_up = (
        AgentRun.objects.filter(work_item=issue_in_progress, parent_run=run)
        .order_by("-created_at")
        .first()
    )
    assert follow_up is not None
    assert follow_up.status in (
        AgentRunStatus.QUEUED,
        AgentRunStatus.ASSIGNED,
    )


# ---------------------------------------------------------------------------
# _handle_resume_unavailable
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_resume_unavailable_drops_pin_and_requeues(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    parent = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        thread_id="sess_dead",
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
        prompt="prior",
        started_at=timezone.now() - timezone.timedelta(minutes=5),
    )
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        parent_run=parent,
        pinned_runner=online_runner,
        status=AgentRunStatus.ASSIGNED,
        prompt="continuation",
        assigned_at=timezone.now(),
    )
    _consumer_for(online_runner)._handle_resume_unavailable(
        online_runner, str(run.id)
    )
    run.refresh_from_db()
    parent.refresh_from_db()
    assert run.status == AgentRunStatus.QUEUED
    assert run.runner_id is None
    assert run.pinned_runner_id is None
    assert run.assigned_at is None
    # Parent's thread_id is cleared so the next dispatch doesn't hand the
    # same dead session id to a different runner.
    assert parent.thread_id == ""


@pytest.mark.unit
def test_resume_unavailable_noop_when_run_unknown(
    db, online_runner
):
    import uuid

    # Should not raise, should not touch any rows.
    _consumer_for(online_runner)._handle_resume_unavailable(
        online_runner, str(uuid.uuid4())
    )


# ---------------------------------------------------------------------------
# _rid_matches — envelope routing discriminator validation (design.md §4.2)
# ---------------------------------------------------------------------------
#
# These tests use a tiny stand-in for ``runner`` (just needs ``.id``) so they
# don't pay the test-DB setup cost — _rid_matches is a pure UUID-comparison
# static method.


class _StubRunner:
    def __init__(self, runner_id):
        self.id = runner_id


@pytest.mark.unit
def test_rid_matches_accepts_frame_without_rid():
    import uuid

    runner = _StubRunner(uuid.uuid4())
    # Connection-scoped frames (ping, bye) and legacy v1 traffic carry no
    # rid; the consumer must still accept them.
    assert RunnerConsumer._rid_matches(runner, {"type": "heartbeat"})


@pytest.mark.unit
def test_rid_matches_accepts_matching_rid():
    import uuid

    runner_id = uuid.uuid4()
    runner = _StubRunner(runner_id)
    msg = {"type": "heartbeat", "rid": str(runner_id)}
    assert RunnerConsumer._rid_matches(runner, msg)


@pytest.mark.unit
def test_rid_matches_rejects_mismatching_rid():
    import uuid

    runner = _StubRunner(uuid.uuid4())
    msg = {"type": "heartbeat", "rid": str(uuid.uuid4())}
    assert not RunnerConsumer._rid_matches(runner, msg)


@pytest.mark.unit
def test_rid_matches_rejects_garbage_rid():
    import uuid

    runner = _StubRunner(uuid.uuid4())
    msg = {"type": "heartbeat", "rid": "not-a-uuid"}
    assert not RunnerConsumer._rid_matches(runner, msg)


# ---------------------------------------------------------------------------
# _handle_token_hello — token-mode rejection paths (consumers.py)
# ---------------------------------------------------------------------------
#
# Token-mode brings runners online over a shared WS via per-runner Hello
# frames. The rejection branches all surface a connection-scoped
# `RemoveRunner` frame so the daemon's RunnerLoop can drop the instance
# instead of waiting for a Welcome that will never arrive. These tests
# exercise the branches that don't need a live channel layer.


def _token_consumer():
    """Build a RunnerConsumer wired up for token-mode unit tests.

    `_send_envelope` and `close` are replaced with AsyncMocks so test
    bodies can assert on what would have gone over the wire without
    standing up a Channels test client.
    """
    from unittest.mock import AsyncMock

    consumer = RunnerConsumer()
    consumer._send_envelope = AsyncMock()
    consumer.close = AsyncMock()
    return consumer


@pytest.mark.unit
def test_handle_token_hello_rejects_runner_not_owned(db, create_user, workspace):
    """Hello for a runner_id that's valid UUID but not in the token's
    owns-set must trigger a connection-scoped RemoveRunner so the
    daemon stops waiting for a Welcome — and must NOT add the runner
    to authorised_runners (security: prevents a leaked token from
    impersonating a runner under a different token).
    """
    import uuid

    from asgiref.sync import async_to_sync

    from pi_dash.runner.models import MachineToken
    from pi_dash.runner.services import tokens

    minted = tokens.mint_machine_token_secret()
    token = MachineToken.objects.create(
        workspace=workspace,
        created_by=create_user,
        title="t",
        secret_hash=minted.hashed,
        secret_fingerprint=minted.fingerprint,
    )
    consumer = _token_consumer()
    consumer.token = token

    fake_runner_id = uuid.uuid4()
    async_to_sync(consumer._handle_token_hello)(
        {
            "type": "hello",
            "runner_id": str(fake_runner_id),
            "rid": str(fake_runner_id),
        }
    )

    assert consumer.authorised_runners == {}
    payloads = [c.args[0] for c in consumer._send_envelope.call_args_list]
    assert any(
        p.get("type") == "remove_runner"
        and p.get("reason") == "not_owned_or_revoked"
        and p.get("runner_id") == str(fake_runner_id)
        for p in payloads
    ), payloads


@pytest.mark.unit
def test_handle_token_hello_rejects_envelope_rid_mismatch(db, create_user, workspace):
    """If envelope ``rid`` and Hello body ``runner_id`` disagree, the
    consumer must refuse — both because the daemon may have a routing
    bug and because the sane response is to ask it to drop that
    instance via RemoveRunner.
    """
    import uuid

    from asgiref.sync import async_to_sync

    from pi_dash.runner.models import MachineToken
    from pi_dash.runner.services import tokens

    minted = tokens.mint_machine_token_secret()
    token = MachineToken.objects.create(
        workspace=workspace,
        created_by=create_user,
        title="t",
        secret_hash=minted.hashed,
        secret_fingerprint=minted.fingerprint,
    )
    consumer = _token_consumer()
    consumer.token = token

    body_id = uuid.uuid4()
    envelope_id = uuid.uuid4()
    async_to_sync(consumer._handle_token_hello)(
        {
            "type": "hello",
            "runner_id": str(body_id),
            "rid": str(envelope_id),
        }
    )

    assert consumer.authorised_runners == {}
    payloads = [c.args[0] for c in consumer._send_envelope.call_args_list]
    assert any(
        p.get("type") == "remove_runner" and p.get("reason") == "hello_rid_mismatch"
        for p in payloads
    ), payloads


@pytest.mark.unit
def test_handle_token_hello_drops_silently_on_unparseable_runner_id(
    db, create_user, workspace
):
    """A Hello whose body runner_id is unparseable has no rid we can
    address an error frame to. The handler must drop without raising
    and without sending anything.
    """
    from asgiref.sync import async_to_sync

    from pi_dash.runner.models import MachineToken
    from pi_dash.runner.services import tokens

    minted = tokens.mint_machine_token_secret()
    token = MachineToken.objects.create(
        workspace=workspace,
        created_by=create_user,
        title="t",
        secret_hash=minted.hashed,
        secret_fingerprint=minted.fingerprint,
    )
    consumer = _token_consumer()
    consumer.token = token

    async_to_sync(consumer._handle_token_hello)(
        {"type": "hello", "runner_id": "not-a-uuid"}
    )
    assert consumer.authorised_runners == {}
    consumer._send_envelope.assert_not_called()


@pytest.mark.unit
def test_handle_token_hello_no_token_is_noop():
    """Defensive: _handle_token_hello called outside token mode must
    return immediately without touching state. The receive_json
    dispatcher only routes here when self.token is set, but the guard
    in the handler is the second line of defence.
    """
    import uuid

    from asgiref.sync import async_to_sync

    consumer = _token_consumer()
    assert consumer.token is None

    async_to_sync(consumer._handle_token_hello)(
        {"type": "hello", "runner_id": str(uuid.uuid4())}
    )
    consumer._send_envelope.assert_not_called()
    assert consumer.authorised_runners == {}


# ---------------------------------------------------------------------------
# runner_revoke — connection-scoped Revoke frame + close (consumers.py)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_runner_revoke_sends_revoke_frame_then_closes():
    """The cascade-revoke pubsub event must produce one connection-scoped
    Revoke frame (rid omitted) and then close the WebSocket. Without
    this, the daemon's supervisor never sees the wire-level signal and
    bounces into a reconnect-with-401 loop instead of exiting cleanly.
    """
    from asgiref.sync import async_to_sync

    from pi_dash.runner.consumers import CLOSE_CODE_ROTATED

    consumer = _token_consumer()
    async_to_sync(consumer.runner_revoke)({"reason": "token revoked"})

    consumer._send_envelope.assert_called_once()
    call = consumer._send_envelope.call_args
    payload = call.args[0]
    assert payload == {"type": "revoke", "reason": "token revoked"}
    assert call.kwargs.get("runner_scoped") is False
    consumer.close.assert_called_once_with(code=CLOSE_CODE_ROTATED)


@pytest.mark.unit
def test_runner_revoke_closes_even_when_send_raises():
    """The daemon may have already dropped the socket; the send may
    fail. The consumer must still call close() so pubsub bookkeeping
    eventually settles.
    """
    from asgiref.sync import async_to_sync

    from pi_dash.runner.consumers import CLOSE_CODE_ROTATED

    consumer = _token_consumer()
    consumer._send_envelope.side_effect = RuntimeError("socket already gone")
    async_to_sync(consumer.runner_revoke)({"reason": "token revoked"})

    consumer.close.assert_called_once_with(code=CLOSE_CODE_ROTATED)


# ---------------------------------------------------------------------------
# _apply_heartbeat — reap stale BUSY runs when runner reports it isn't
# actually working on them. Closes the zombie-run gap where a daemon restart
# leaves cloud thinking the runner is busy with an ASSIGNED run forever.
# ---------------------------------------------------------------------------


def _busy_run(create_user, workspace, pod, runner, issue, *, status, assigned_minutes_ago):
    return AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue,
        runner=runner,
        status=status,
        prompt="x",
        assigned_at=timezone.now() - timezone.timedelta(minutes=assigned_minutes_ago),
    )


@pytest.mark.unit
def test_heartbeat_reaps_zombie_run_when_runner_reports_idle(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    """Cloud has an ASSIGNED run; runner heartbeats with in_flight_run=None.

    Runner is the source of truth about its own work. The orphaned ASSIGNED
    row must be reaped so the runner can pick up new ticks.
    """
    zombie = _busy_run(
        create_user, workspace, pod, online_runner, issue_in_progress,
        status=AgentRunStatus.ASSIGNED, assigned_minutes_ago=10,
    )

    _consumer_for(online_runner)._apply_heartbeat(
        online_runner,
        {"ts": timezone.now().isoformat(), "in_flight_run": None, "status": "online"},
    )

    zombie.refresh_from_db()
    assert zombie.status == AgentRunStatus.FAILED
    assert zombie.ended_at is not None
    assert "heartbeat" in zombie.error.lower()


@pytest.mark.unit
def test_heartbeat_reaps_running_status_too(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    """RUNNING is also a BUSY status — same reaping applies."""
    zombie = _busy_run(
        create_user, workspace, pod, online_runner, issue_in_progress,
        status=AgentRunStatus.RUNNING, assigned_minutes_ago=5,
    )

    _consumer_for(online_runner)._apply_heartbeat(
        online_runner,
        {"ts": timezone.now().isoformat(), "in_flight_run": None, "status": "online"},
    )

    zombie.refresh_from_db()
    assert zombie.status == AgentRunStatus.FAILED


@pytest.mark.unit
def test_heartbeat_keeps_run_when_ids_match(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    """Runner reports it's working on this exact run — leave it alone."""
    run = _busy_run(
        create_user, workspace, pod, online_runner, issue_in_progress,
        status=AgentRunStatus.RUNNING, assigned_minutes_ago=5,
    )

    _consumer_for(online_runner)._apply_heartbeat(
        online_runner,
        {"ts": timezone.now().isoformat(), "in_flight_run": str(run.id), "status": "busy"},
    )

    run.refresh_from_db()
    assert run.status == AgentRunStatus.RUNNING


@pytest.mark.unit
def test_heartbeat_reaps_others_but_keeps_named_in_flight(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    """Cloud has 2 BUSY runs on this runner; runner says it's only on one
    of them. Reap the other; keep the named one.
    """
    keeper = _busy_run(
        create_user, workspace, pod, online_runner, issue_in_progress,
        status=AgentRunStatus.RUNNING, assigned_minutes_ago=5,
    )
    zombie = _busy_run(
        create_user, workspace, pod, online_runner, issue_in_progress,
        status=AgentRunStatus.ASSIGNED, assigned_minutes_ago=5,
    )

    _consumer_for(online_runner)._apply_heartbeat(
        online_runner,
        {"ts": timezone.now().isoformat(), "in_flight_run": str(keeper.id), "status": "busy"},
    )

    keeper.refresh_from_db()
    zombie.refresh_from_db()
    assert keeper.status == AgentRunStatus.RUNNING
    assert zombie.status == AgentRunStatus.FAILED


@pytest.mark.unit
def test_heartbeat_does_not_reap_run_assigned_after_heartbeat_ts(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    """Race guard: a heartbeat in flight before the assignment was written
    must not reap the freshly-assigned run.
    """
    fresh = _busy_run(
        create_user, workspace, pod, online_runner, issue_in_progress,
        status=AgentRunStatus.ASSIGNED, assigned_minutes_ago=0,
    )
    # heartbeat sent 5 minutes ago, before the assignment existed
    old_ts = (timezone.now() - timezone.timedelta(minutes=5)).isoformat()

    _consumer_for(online_runner)._apply_heartbeat(
        online_runner,
        {"ts": old_ts, "in_flight_run": None, "status": "online"},
    )

    fresh.refresh_from_db()
    assert fresh.status == AgentRunStatus.ASSIGNED


@pytest.mark.unit
def test_heartbeat_does_not_touch_paused_awaiting_input(
    db, create_user, workspace, pod, online_runner, issue_in_progress
):
    """PAUSED_AWAITING_INPUT is intentionally NOT in BUSY_STATUSES — the
    runner is free to take other work while a paused run waits for human
    reply. Heartbeat must not reap paused runs.
    """
    paused = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        work_item=issue_in_progress,
        runner=online_runner,
        status=AgentRunStatus.PAUSED_AWAITING_INPUT,
        prompt="x",
        assigned_at=timezone.now() - timezone.timedelta(minutes=10),
        started_at=timezone.now() - timezone.timedelta(minutes=8),
    )

    _consumer_for(online_runner)._apply_heartbeat(
        online_runner,
        {"ts": timezone.now().isoformat(), "in_flight_run": None, "status": "online"},
    )

    paused.refresh_from_db()
    assert paused.status == AgentRunStatus.PAUSED_AWAITING_INPUT


@pytest.mark.unit
def test_heartbeat_updates_last_heartbeat_at_unconditionally(
    db, online_runner
):
    """Even with no zombies, the existing mark_heartbeat behavior must run."""
    before = online_runner.last_heartbeat_at
    # Move the clock-on-row backwards so we can detect an update.
    Runner.objects.filter(pk=online_runner.pk).update(
        last_heartbeat_at=timezone.now() - timezone.timedelta(minutes=10),
    )
    online_runner.refresh_from_db()

    _consumer_for(online_runner)._apply_heartbeat(
        online_runner,
        {"ts": timezone.now().isoformat(), "in_flight_run": None, "status": "online"},
    )

    online_runner.refresh_from_db()
    assert online_runner.last_heartbeat_at > before
