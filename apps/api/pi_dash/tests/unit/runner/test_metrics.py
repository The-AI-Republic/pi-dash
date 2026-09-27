# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The Prometheus ``metrics/`` endpoint, including the managed-runner series.

The endpoint is a snapshot over DB queries, so the tests assert the exposed
text directly: a scraper reading these lines is exactly what these assertions
stand in for.
"""

from __future__ import annotations

from datetime import timedelta

import pytest
from django.utils import timezone

from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.runner.models import (
    AgentRun,
    AgentRunStatus,
    Runner,
    RunnerProvisioning,
    RunnerStatus,
)

pytestmark = pytest.mark.unit

METRICS_URL = "/api/v1/runner/metrics/"


def _bundled_runner(project, owner, status=RunnerStatus.ONLINE):
    return Runner.objects.create(
        owner=owner,
        workspace=project.workspace,
        pod=project.pods.get(is_default=True),
        name=f"desktop-{Runner.objects.count()}",
        host_label="rich-laptop",
        provisioning=RunnerProvisioning.DESKTOP_BUNDLED,
        status=status,
        last_heartbeat_at=timezone.now() if status == RunnerStatus.ONLINE else None,
        enrolled_at=timezone.now(),
    )


def _queued_managed_run(project, owner, *, age_seconds):
    run = AgentRun.objects.create(
        workspace=project.workspace,
        created_by=owner,
        pod=project.pods.get(is_default=True),
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    # ``created_at`` is auto_now_add, so backdate it explicitly to simulate a
    # run that has been waiting for a closed desktop.
    AgentRun.objects.filter(id=run.id).update(
        created_at=timezone.now() - timedelta(seconds=age_seconds)
    )
    return run


def _series_value(body: str, line_prefix: str) -> str:
    for line in body.splitlines():
        if line.startswith(line_prefix) and not line.startswith("#"):
            return line[len(line_prefix):].strip()
    raise AssertionError(f"no series line starting with {line_prefix!r} in:\n{body}")


def test_endpoint_is_public_and_plain_text(api_client, db):
    resp = api_client.get(METRICS_URL)
    assert resp.status_code == 200
    assert resp["Content-Type"].startswith("text/plain")
    # The pre-existing gauges must still be present.
    assert "pi_dash_runner_online" in resp.content.decode()
    assert "pi_dash_approvals_pending" in resp.content.decode()


def test_managed_runner_online_gauge_counts_only_bundled_online(project, create_user, api_client):
    _bundled_runner(project, create_user, status=RunnerStatus.ONLINE)
    _bundled_runner(project, create_user, status=RunnerStatus.OFFLINE)
    # A manual online runner must NOT be counted by the managed gauge.
    Runner.objects.create(
        owner=create_user,
        workspace=project.workspace,
        pod=project.pods.get(is_default=True),
        name="manual",
        host_label="rich-laptop",
        provisioning=RunnerProvisioning.MANUAL,
        status=RunnerStatus.ONLINE,
        last_heartbeat_at=timezone.now(),
        enrolled_at=timezone.now(),
    )
    body = api_client.get(METRICS_URL).content.decode()
    assert _series_value(body, "pi_dash_managed_runner_online ") == "1"
    # HELP/TYPE lines are present so the scrape parses cleanly.
    assert "# TYPE pi_dash_managed_runner_online gauge" in body


def test_runs_total_counter_labels_every_executor_kind(project, create_user, api_client):
    AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=project.pods.get(is_default=True),
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=project.pods.get(is_default=True),
        executor_kind=AgentExecutorKind.LOCAL_RUNNER,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    body = api_client.get(METRICS_URL).content.decode()
    assert "# TYPE pi_dash_runs_total counter" in body
    assert 'pi_dash_runs_total{executor_kind="managed_runner"} 1' in body
    assert 'pi_dash_runs_total{executor_kind="local_runner"} 1' in body
    # Every known kind is emitted zero-filled, even ones with no rows.
    for kind in AgentExecutorKind.values:
        assert f'pi_dash_runs_total{{executor_kind="{kind}"}}' in body


def test_queued_wait_histogram_buckets_managed_runs(project, create_user, api_client):
    # One run waiting ~2 minutes, one waiting ~2 hours.
    _queued_managed_run(project, create_user, age_seconds=120)
    _queued_managed_run(project, create_user, age_seconds=7200)
    body = api_client.get(METRICS_URL).content.decode()
    assert "# TYPE pi_dash_managed_runner_queued_wait_seconds histogram" in body
    # Cumulative buckets: le=60 catches neither, le=300 catches the 120s run,
    # le=10800 (3h) catches both.
    assert 'pi_dash_managed_runner_queued_wait_seconds_bucket{le="60"} 0' in body
    assert 'pi_dash_managed_runner_queued_wait_seconds_bucket{le="300"} 1' in body
    assert 'pi_dash_managed_runner_queued_wait_seconds_bucket{le="10800"} 2' in body
    assert 'pi_dash_managed_runner_queued_wait_seconds_bucket{le="+Inf"} 2' in body
    assert _series_value(body, "pi_dash_managed_runner_queued_wait_seconds_count ") == "2"


def test_queued_wait_histogram_ignores_non_managed_and_non_queued(project, create_user, api_client):
    # A running managed run and a queued local run both fall outside the series.
    AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=project.pods.get(is_default=True),
        executor_kind=AgentExecutorKind.MANAGED_RUNNER,
        status=AgentRunStatus.RUNNING,
        prompt="",
    )
    AgentRun.objects.create(
        workspace=project.workspace,
        created_by=create_user,
        pod=project.pods.get(is_default=True),
        executor_kind=AgentExecutorKind.LOCAL_RUNNER,
        status=AgentRunStatus.QUEUED,
        prompt="",
    )
    body = api_client.get(METRICS_URL).content.decode()
    assert _series_value(body, "pi_dash_managed_runner_queued_wait_seconds_count ") == "0"
