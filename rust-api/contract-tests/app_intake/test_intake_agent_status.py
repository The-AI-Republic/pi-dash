"""Contract tests for the intake-issue detail agent blocks (PIDASHCONV-1435).

`GET intake-issues/<pk>/` nests the full `IssueDetailSerializer`
(`app/serializers/intake.py:93-117`), whose `agent_ticker` / `agent_status`
blocks must match the app-issue rendering byte for byte: token counts read
from the `usage` `input` / `output` / `total` keys, `live_state` kept when
`observed_run_id` is NULL, datetimes as plain UTC `isoformat` (`+00:00`),
and the ticker interval resolved through the registered ticking-state
names (`orchestration/agent_phases.py:191-200`).

Uses fresh tenants (`make_tenant`): each test sets its actor's timezone to
Asia/Kolkata plus custom states and project intervals.
"""

import json
import uuid

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract


def _as_kolkata(tenant):
    tenant["_database"].execute(
        "UPDATE users SET user_timezone = 'Asia/Kolkata' WHERE id = %s",
        (tenant["user"]["id"],),
    )


def _accept_on_default_state(tenant, issue_id):
    """Accept the intake issue onto a default Backlog state (cf.
    test_patch_accept_moves_to_default_state)."""
    tenant["_database"].execute(
        'UPDATE states SET "default" = true WHERE project_id = %s',
        (tenant["project"]["id"],),
    )
    urls = intake_urls(tenant)
    response = tenant["client"].patch(
        f"{urls['intake_issues']}{issue_id}/", json={"status": 1}
    )
    assert response.status_code == 200, response.text


def _finish_run(tenant, run_id, *, usage):
    tenant["_database"].execute(
        "UPDATE agent_run SET usage = %s,"
        " started_at = now() - interval '2 hours',"
        " ended_at = now() - interval '1 hour',"
        " created_at = now() WHERE id = %s",
        (json.dumps(usage), run_id),
    )


def _attach_runner(tenant, run_id, *, usage):
    """Seed an online runner with a NULL-observed live state, attach it to
    the run, and backdate the run so it is active but not latest."""
    database = tenant["_database"]
    runner_id = uuid.uuid4().hex
    database.execute(
        """INSERT INTO runner
           (id, owner_id, workspace_id, pod_id, name, host_label,
            provisioning, visibility, status, capabilities,
            os, arch, runner_version,
            refresh_token_hash, refresh_token_fingerprint,
            refresh_token_generation, previous_refresh_token_hash,
            access_token_signing_key_version,
            enrollment_token_hash, enrollment_token_fingerprint,
            enrolled_at, revoked_at, revoked_reason,
            dev_machine_id, dev_metadata, protocol_version,
            created_at, updated_at)
           VALUES (%s,%s,%s,%s,%s,'', 'manual',0,'online','[]',
                   '','','',
                   '','',0,'',1,
                   '','',
                   NULL,NULL,'',
                   NULL,'{}',1,
                   now(),now())""",
        (
            runner_id,
            tenant["user"]["id"],
            tenant["workspace"]["id"],
            tenant["pod"]["id"],
            "agent-status-runner",
        ),
    )
    database.execute(
        """INSERT INTO runner_live_state
           (runner_id, observed_run_id, last_event_at, last_event_kind,
            last_event_summary, agent_pid, agent_subprocess_alive,
            approvals_pending, usage, llm_model, turn_count, updated_at)
           VALUES (%s, NULL, now() - interval '5 minutes', 'tick',
                   'working', 4242, true, 0,
                   %s, 'contract-model', 7, now())""",
        (runner_id, json.dumps(usage)),
    )
    database.execute(
        "UPDATE agent_run SET runner_id = %s, usage = %s,"
        " assigned_at = now() - interval '50 minutes',"
        " started_at = now() - interval '40 minutes',"
        " created_at = now() - interval '1 hour' WHERE id = %s",
        (runner_id, json.dumps({"input": 1, "output": 2, "total": 3}), run_id),
    )
    return runner_id


def _seed_ticker(tenant, issue_id):
    tenant["_database"].execute(
        """INSERT INTO issue_agent_ticker
           (id, issue_id, user_disabled, used, enabled, next_run_at,
            last_tick_at, disarm_reason, granted, pending_entry,
            pending_entry_free, pending_entry_actor_id, pending_entry_trigger,
            resume_parent_run_id, waited, created_at, updated_at)
           VALUES (%s,%s,false,3,true, now() + interval '1 hour',
                   now() - interval '30 minutes', '', 0, false,
                   false, NULL, '', NULL, 0, now(), now())""",
        (uuid.uuid4().hex, issue_id),
    )


def _seed_agent_state(tenant, issue_id):
    """The PIDASHCONV-1435 seed: a finished latest run with real token
    usage, an active run on a NULL-observed live state, and a ticker."""
    seeder = tenant["seeder"]
    finished = seeder.create_agent_run(
        tenant["workspace"]["id"],
        tenant["user"]["id"],
        tenant["pod"]["id"],
        issue_id,
        status="completed",
    )
    _finish_run(
        tenant, finished["id"], usage={"input": 12000, "output": 800, "total": 12800}
    )
    active = seeder.create_agent_run(
        tenant["workspace"]["id"],
        tenant["user"]["id"],
        tenant["pod"]["id"],
        issue_id,
        status="running",
    )
    _attach_runner(tenant, active["id"], usage={"input": 10, "output": 20, "total": 30})
    _seed_ticker(tenant, issue_id)


def test_agent_status_tokens_live_state_datetimes(make_tenant):
    tenant = make_tenant()
    _as_kolkata(tenant)
    tenant["pod"] = tenant["seeder"].create_pod(
        tenant["workspace"]["id"], tenant["project"]["id"],
        name="Default pod", is_default=True,
    )
    urls = intake_urls(tenant)
    created = create_intake_issue(
        tenant["client"], urls["intake_issues"], name="Agent status probe"
    )
    issue_id = created["issue"]["id"]
    _accept_on_default_state(tenant, issue_id)
    _seed_agent_state(tenant, issue_id)

    response = tenant["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert response.status_code == 200, response.text
    nested = response.json()["issue"]

    status = nested["agent_status"]
    assert status["run_count"] == 2

    latest = status["latest_run"]
    assert latest["status"] == "completed"
    assert latest["input_tokens"] == 12000
    assert latest["output_tokens"] == 800
    assert latest["total_tokens"] == 12800
    assert latest["created_at"].endswith("+00:00")
    assert latest["assigned_at"] is None
    assert latest["started_at"].endswith("+00:00")
    assert latest["ended_at"].endswith("+00:00")

    active = status["active_run"]
    assert active["status"] == "running"
    assert active["input_tokens"] == 1
    assert active["output_tokens"] == 2
    assert active["total_tokens"] == 3
    assert active["created_at"].endswith("+00:00")
    assert active["assigned_at"].endswith("+00:00")
    assert active["started_at"].endswith("+00:00")
    assert active["ended_at"] is None

    # A NULL observed_run_id keeps the block (issue.py:1343-1350): only a
    # non-null id differing from the run drops it.
    live = active["live_state"]
    assert isinstance(live, dict)
    assert live["observed_run_id"] is None
    assert live["input_tokens"] == 10
    assert live["output_tokens"] == 20
    assert live["total_tokens"] == 30
    assert live["last_event_at"].endswith("+00:00")
    assert live["updated_at"].endswith("+00:00")

    # The ticker block duplicates the top-level one (issue.py:1383+1408).
    assert status["ticker"] == nested["agent_ticker"]
    assert nested["agent_ticker"]["next_run_at"].endswith("+00:00")
    assert nested["agent_ticker"]["last_tick_at"].endswith("+00:00")


def test_agent_ticker_interval_follows_registered_states(make_tenant):
    """The ticker interval resolves through `cadence_fields_for`: only the
    registered ticking-state names (In Progress / In Review / In Test)
    take their stage column; any other state — even inside a ticking
    group — falls back to the impl pair (agent_phases.py:191-200)."""
    tenant = make_tenant()
    _as_kolkata(tenant)
    database = tenant["_database"]
    database.execute(
        "UPDATE projects SET agent_default_interval_seconds = 7200,"
        " agent_review_default_interval_seconds = 3600,"
        " agent_test_default_interval_seconds = 5400 WHERE id = %s",
        (tenant["project"]["id"],),
    )
    custom = tenant["seeder"].create_state(
        tenant["workspace"]["id"], tenant["project"]["id"],
        name="Custom Review", group="review",
    )
    registered = tenant["seeder"].create_state(
        tenant["workspace"]["id"], tenant["project"]["id"],
        name="In Review", group="review",
    )
    urls = intake_urls(tenant)
    created = create_intake_issue(
        tenant["client"], urls["intake_issues"], name="Ticker interval probe"
    )
    issue_id = created["issue"]["id"]
    _seed_ticker(tenant, issue_id)
    detail_url = f"{urls['intake_issues']}{issue_id}/"

    database.execute(
        "UPDATE issues SET state_id = %s WHERE id = %s", (custom["id"], issue_id)
    )
    response = tenant["client"].get(detail_url)
    assert response.status_code == 200, response.text
    assert response.json()["issue"]["agent_ticker"]["interval_seconds"] == 7200

    database.execute(
        "UPDATE issues SET state_id = %s WHERE id = %s", (registered["id"], issue_id)
    )
    response = tenant["client"].get(detail_url)
    assert response.status_code == 200, response.text
    assert response.json()["issue"]["agent_ticker"]["interval_seconds"] == 3600
