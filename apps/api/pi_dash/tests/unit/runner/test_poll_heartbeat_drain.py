# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regression tests for the heartbeat-recovery drain trigger.

The matcher rejects runners whose ``last_heartbeat_at`` is older than
``HEARTBEAT_GRACE`` (90 seconds). Prior to the fix, when a runner had
been silent past that window and then resumed polling on its existing
session, ``last_heartbeat_at`` was refreshed but **nothing triggered a
drain** — queued runs in the pod sat indefinitely until either a new
run was created or the runner opened a fresh session.

The fix captures the prior heartbeat before the update and, on a
stale→fresh transition, schedules ``drain_for_runner_by_id`` on commit.
These tests guard against:

  * the drain trigger being silently removed
  * the trigger firing on every poll (regression: per-poll churn)
"""

from __future__ import annotations

from contextlib import contextmanager
from datetime import timedelta
from unittest.mock import patch

import pytest
from django.utils import timezone

from pi_dash.runner.models import (
    Pod,
    Runner,
    RunnerSession,
    RunnerStatus,
)
from pi_dash.runner.services import tokens


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def enrolled_runner(db, create_user, workspace, pod):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="poll-runner",
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
        refresh_token_generation=1,
        enrolled_at=timezone.now(),
    )


@pytest.fixture
def runner_token(enrolled_runner):
    return tokens.mint_access_token(
        runner_id=str(enrolled_runner.id),
        user_id=str(enrolled_runner.owner_id),
        workspace_id=str(enrolled_runner.workspace_id),
        rtg=1,
    ).raw


@pytest.fixture
def runner_session(enrolled_runner):
    return RunnerSession.objects.create(
        runner=enrolled_runner,
        protocol_version=4,
        last_seen_at=timezone.now(),
    )


def _poll(api_client, runner_id, session_id, token, body=None):
    return api_client.post(
        f"/api/v1/runner/runners/{runner_id}/sessions/{session_id}/poll",
        body or {},
        format="json",
        HTTP_AUTHORIZATION=f"Bearer {token}",
    )


@contextmanager
def _patched_poll_dependencies(*, drain_side_effect=None):
    # Patch the matcher's drain + the Redis-touching parts of the poll
    # path so the tests run without Redis. Patching the symbol at its
    # source (matcher) because the view does a function-local import.
    with (
        patch(
            "pi_dash.runner.services.matcher.drain_for_runner_by_id",
            side_effect=drain_side_effect,
        ) as mock_drain,
        patch("pi_dash.runner.views.sessions.outbox.ack_for_session"),
        patch(
            "pi_dash.runner.views.sessions.outbox.is_pel_drained",
            return_value=True,
        ),
        patch(
            "pi_dash.runner.views.sessions.outbox.read_for_session",
            return_value=[],
        ),
        # Eviction-aware reader's pubsub fallback — short-circuit it.
        patch("pi_dash.settings.redis.redis_instance", return_value=None),
        # Fire on_commit callbacks inline so the drain assertion is synchronous.
        patch(
            "django.db.transaction.on_commit",
            side_effect=lambda fn, **_: fn(),
        ),
    ):
        yield mock_drain


@pytest.mark.unit
def test_drain_fires_when_runner_polls_after_stale_window(
    db, api_client, enrolled_runner, runner_token, runner_session
):
    """Stale heartbeat → poll → drain_for_runner_by_id called exactly once."""
    # Backdate the runner's heartbeat well past HEARTBEAT_GRACE (90s).
    stale_ts = timezone.now() - timedelta(seconds=600)
    Runner.objects.filter(pk=enrolled_runner.id).update(last_heartbeat_at=stale_ts)

    with _patched_poll_dependencies() as mock_drain:
        resp = _poll(
            api_client,
            enrolled_runner.id,
            runner_session.id,
            runner_token,
        )

    assert resp.status_code == 200, resp.data
    mock_drain.assert_called_once_with(enrolled_runner.id)


@pytest.mark.unit
def test_drain_does_not_fire_on_fresh_heartbeat_poll(
    db, api_client, enrolled_runner, runner_token, runner_session
):
    """Fresh heartbeat → poll → drain NOT called (no per-poll churn)."""
    # last_heartbeat_at is fresh from the fixture (now()).
    with _patched_poll_dependencies() as mock_drain:
        resp = _poll(
            api_client,
            enrolled_runner.id,
            runner_session.id,
            runner_token,
        )

    assert resp.status_code == 200, resp.data
    mock_drain.assert_not_called()


@pytest.mark.unit
def test_drain_fires_when_runner_has_no_prior_heartbeat(
    db, api_client, enrolled_runner, runner_token, runner_session
):
    """First-ever heartbeat (prior_hb is NULL) is treated as stale → drain.

    Covers the ``prior_hb is None`` branch of the staleness check, which
    matters for fresh runners whose initial poll arrives without any
    historical heartbeat row. Pinned runs from the session-open path may
    be waiting; the drain trigger is what picks them up.
    """
    Runner.objects.filter(pk=enrolled_runner.id).update(last_heartbeat_at=None)

    with _patched_poll_dependencies() as mock_drain:
        resp = _poll(
            api_client,
            enrolled_runner.id,
            runner_session.id,
            runner_token,
        )

    assert resp.status_code == 200, resp.data
    mock_drain.assert_called_once_with(enrolled_runner.id)


@pytest.mark.unit
def test_drain_failure_does_not_fail_poll_response(
    db, api_client, enrolled_runner, runner_token, runner_session
):
    """The recovery drain is best-effort; poll must still return messages."""
    stale_ts = timezone.now() - timedelta(seconds=600)
    Runner.objects.filter(pk=enrolled_runner.id).update(last_heartbeat_at=stale_ts)

    with _patched_poll_dependencies(
        drain_side_effect=RuntimeError("boom")
    ) as mock_drain:
        resp = _poll(
            api_client,
            enrolled_runner.id,
            runner_session.id,
            runner_token,
        )

    assert resp.status_code == 200, resp.data
    mock_drain.assert_called_once_with(enrolled_runner.id)
