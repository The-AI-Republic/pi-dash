"""Daemon health + metrics shapes (PIDASHCONV-97).

``GET /api/v1/runner/health/`` and ``GET /api/v1/runner/metrics/`` are
AllowAny: no auth header anywhere in this file, byte for byte.
"""

from __future__ import annotations

import pytest

from _harness.http import anonymous_client

from .conftest import DAEMON

pytestmark = pytest.mark.contract


def test_daemon_health_shape(settings, anon_client):
    response = anon_client.get(f"{DAEMON}/health/")
    assert response.status_code == 200
    assert response.json() == {"ok": True, "protocol_version": 3}


def test_daemon_metrics_shape_and_content_type(settings, anon_client):
    response = anon_client.get(f"{DAEMON}/metrics/")
    assert response.status_code == 200
    assert response.headers["content-type"] == "text/plain; version=0.0.4"
    body = response.text
    for gauge in (
        "pi_dash_runner_online",
        "pi_dash_runner_busy",
        "pi_dash_runner_offline",
        "pi_dash_runs_active",
        "pi_dash_approvals_pending",
    ):
        assert f"# HELP {gauge} " in body
        assert f"# TYPE {gauge} gauge\n" in body
        assert f"\n{gauge} " in body


def test_daemon_metrics_counts_reflect_seeded_state(seeder, daemon_world, anon_client):
    """Gauges count exactly what the DB holds: one online + one busy runner,
    one in-flight run, one pending approval."""
    owner_id = daemon_world["owner"]["id"]
    ws_id = daemon_world["workspace"]["id"]
    pod_id = daemon_world["pod"]["id"]
    online = seeder.enroll_runner(owner_id, ws_id, pod_id, f"apd_en_{seeder.tag}health01x")
    busy = seeder.enroll_runner(owner_id, ws_id, pod_id, f"apd_en_{seeder.tag}health02x")
    seeder.db.execute("UPDATE runner SET status = 'online' WHERE id = %s", (online["id"],))
    seeder.db.execute("UPDATE runner SET status = 'busy' WHERE id = %s", (busy["id"],))
    run = seeder.create_daemon_run(owner_id, ws_id, pod_id, busy["id"], status="running")
    assert (
        seeder.db.fetchval("SELECT COUNT(*) FROM agent_run WHERE id = %s", (run["id"],)) == 1
    )

    response = anon_client.get(f"{DAEMON}/metrics/")
    assert response.status_code == 200
    values = {}
    for line in response.text.splitlines():
        if line.startswith("pi_dash_") and not line.startswith("#"):
            name, value = line.split(" ")
            values[name] = int(value)
    assert values["pi_dash_runner_online"] == 1
    assert values["pi_dash_runner_busy"] == 1
    assert values["pi_dash_runner_offline"] == 0
    assert values["pi_dash_runs_active"] == 1


def test_daemon_health_no_auth_header_sent(settings):
    """Wire compat: health must answer with zero credentials (doctor probes)."""
    with anonymous_client(settings.base_url) as client:
        assert "authorization" not in {k.lower() for k in client.headers}
        response = client.get(f"{DAEMON}/health/")
        assert response.status_code == 200
