# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Token usage storage end to end (PDASHOSS01-188): runner frames through the
HTTP endpoints into ``AgentRun.usage``, the generated flat columns, and the
API responses that keep their flat keys."""

from __future__ import annotations

from unittest.mock import patch

import pytest
from django.db import DatabaseError, transaction
from django.utils import timezone

from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Pod,
    Runner,
    RunnerLiveState,
    RunnerStatus,
)
from pi_dash.runner.serializers import AgentRunSerializer, RunnerLiveStateSerializer
from pi_dash.runner.services import tokens

# What the runner sends as ``tokens`` for a Codex run: canonical counters
# (runner/src/daemon/observability.rs) plus the app-server usage object
# verbatim under ``raw`` — including a counter nobody models yet.
CODEX_WIRE_TOKENS = {
    "input": 21763,
    "output": 551,
    "total": 22314,
    "cache_read": 11008,
    "cache_write": 0,
    "reasoning": 133,
    "raw": {
        "last": {"inputTokens": 21763, "outputTokens": 551, "totalTokens": 22314},
        "modelContextWindow": 258400,
        "total": {
            "cacheWriteInputTokens": 0,
            "cachedInputTokens": 11008,
            "inputTokens": 21763,
            "outputTokens": 551,
            "reasoningOutputTokens": 133,
            "totalTokens": 22314,
            "brandNewCounter": 42,
        },
    },
}


@pytest.fixture
def pod(project):
    return Pod.default_for_project(project)


@pytest.fixture
def enrolled_runner(db, create_user, workspace, pod):
    return Runner.objects.create(
        owner=create_user,
        workspace=workspace,
        pod=pod,
        name="usageR",
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
def running_run(db, create_user, workspace, pod, enrolled_runner):
    return AgentRun.objects.create(
        owner=create_user,
        created_by=create_user,
        workspace=workspace,
        pod=pod,
        runner=enrolled_runner,
        prompt="x",
        status=AgentRunStatus.RUNNING,
        assigned_at=timezone.now(),
        started_at=timezone.now(),
    )


def _post(api_client, runner_token, path, body):
    return api_client.post(path, body, format="json", HTTP_AUTHORIZATION=f"Bearer {runner_token}")


# ---------------------------------------------------------------------------
# Generated flat columns
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_flat_columns_are_generated_from_usage(db, create_user, workspace, pod):
    run = AgentRun.objects.create(
        workspace=workspace,
        created_by=create_user,
        pod=pod,
        usage={"input": 100, "output": 40, "total": 140, "cache_read": 60},
    )
    # Read back via RETURNING on insert.
    assert (run.input_tokens, run.output_tokens, run.total_tokens) == (100, 40, 140)

    AgentRun.objects.filter(pk=run.pk).update(usage={"output": 7})
    run.refresh_from_db()
    assert (run.input_tokens, run.output_tokens, run.total_tokens) == (None, 7, None)

    # A full save() of a loaded row is fine — the columns are sent as DEFAULT.
    run.usage = {"input": 1, "output": 2, "total": 3}
    run.save()
    run.refresh_from_db()
    assert run.total_tokens == 3


@pytest.mark.unit
def test_flat_columns_cannot_be_written_directly(db, create_user, workspace, pod):
    run = AgentRun.objects.create(workspace=workspace, created_by=create_user, pod=pod)
    assert run.usage == {}
    assert run.total_tokens is None
    with pytest.raises(DatabaseError), transaction.atomic():
        AgentRun.objects.filter(pk=run.pk).update(total_tokens=5)


# ---------------------------------------------------------------------------
# Runner frame → stored row
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_complete_frame_tokens_survive_to_row_and_api(db, api_client, runner_token, running_run):
    resp = _post(
        api_client,
        runner_token,
        f"/api/v1/runner/runs/{running_run.id}/complete/",
        {"done_payload": {"summary": "done"}, "tokens": CODEX_WIRE_TOKENS, "model": "gpt-5.1-codex"},
    )
    assert resp.status_code == 200, resp.data

    running_run.refresh_from_db()
    assert running_run.usage == CODEX_WIRE_TOKENS
    assert running_run.usage["raw"]["total"]["brandNewCounter"] == 42
    assert (running_run.input_tokens, running_run.output_tokens, running_run.total_tokens) == (21763, 551, 22314)

    data = AgentRunSerializer(running_run).data
    assert (data["input_tokens"], data["output_tokens"], data["total_tokens"]) == (21763, 551, 22314)
    assert data["usage"]["cache_read"] == 11008
    assert data["usage"]["reasoning"] == 133


@pytest.mark.unit
def test_claude_done_payload_usage_is_normalised(db, api_client, runner_token, running_run):
    # Claude has no streaming usage; its final usage rides in the done payload.
    claude_usage = {
        "input_tokens": 2,
        "cache_creation_input_tokens": 15222,
        "cache_read_input_tokens": 15553,
        "output_tokens": 8,
        "service_tier": "standard",
    }
    resp = _post(
        api_client,
        runner_token,
        f"/api/v1/runner/runs/{running_run.id}/complete/",
        {"done_payload": {"conclusion": "success", "usage": claude_usage}},
    )
    assert resp.status_code == 200, resp.data

    running_run.refresh_from_db()
    usage = running_run.usage
    assert usage["cache_read"] == 15553
    assert usage["cache_write"] == 15222
    assert usage["input"] == 30777
    assert usage["total"] == 30785
    assert usage["raw"] == claude_usage
    assert running_run.total_tokens == 30785


@pytest.mark.unit
def test_crashed_run_takes_breakdown_from_live_state(db, api_client, runner_token, enrolled_runner, running_run):
    # A poll carries the streaming snapshot …
    with (
        patch("pi_dash.runner.views.sessions.outbox.is_pel_drained", return_value=True),
        patch("pi_dash.runner.views.sessions.outbox.aread_for_session", return_value=[]),
    ):
        open_resp = _post(
            api_client,
            runner_token,
            f"/api/v1/runner/runners/{enrolled_runner.id}/sessions/",
            {
                "version": "test",
                "os": "linux",
                "arch": "x86_64",
                "status": "busy",
                "in_flight_run": str(running_run.id),
            },
        )
        assert open_resp.status_code == 201, open_resp.data
        poll_resp = _post(
            api_client,
            runner_token,
            f"/api/v1/runner/runners/{enrolled_runner.id}/sessions/{open_resp.data['session_id']}/poll",
            {
                "ack": [],
                "status": {
                    "status": "busy",
                    "in_flight_run": str(running_run.id),
                    "observed_run_id": str(running_run.id),
                    "ts": timezone.now().isoformat(),
                    "tokens": CODEX_WIRE_TOKENS,
                },
            },
        )
    assert poll_resp.status_code == 200, poll_resp.data
    state = RunnerLiveState.objects.get(runner=enrolled_runner)
    assert state.usage == CODEX_WIRE_TOKENS
    live = RunnerLiveStateSerializer(state).data
    assert (live["input_tokens"], live["output_tokens"], live["total_tokens"]) == (21763, 551, 22314)

    # … then the agent dies without a usage report of its own.
    resp = _post(
        api_client,
        runner_token,
        f"/api/v1/runner/runs/{running_run.id}/fail/",
        {"reason": "agent_crash", "detail": "boom"},
    )
    assert resp.status_code == 200, resp.data
    running_run.refresh_from_db()
    assert running_run.status == AgentRunStatus.FAILED
    assert running_run.usage == CODEX_WIRE_TOKENS
    assert running_run.total_tokens == 22314


@pytest.mark.unit
def test_run_without_usage_keeps_empty_bag(db, api_client, runner_token, running_run):
    resp = _post(
        api_client,
        runner_token,
        f"/api/v1/runner/runs/{running_run.id}/complete/",
        {"done_payload": {"summary": "done"}},
    )
    assert resp.status_code == 200, resp.data
    running_run.refresh_from_db()
    assert running_run.usage == {}
    assert running_run.total_tokens is None
