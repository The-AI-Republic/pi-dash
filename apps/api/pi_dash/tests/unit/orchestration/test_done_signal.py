# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.orchestration.done_signal import (
    DoneSignalError,
    ingest_into_run,
    parse,
)
from pi_dash.runner.models import AgentRun, AgentRunStatus


VALID_BODY = """
Some preamble chatter that the agent printed before the fence.

```pi-dash-done
{
  "status": "completed",
  "summary": "made the button blue",
  "state_transition": {"requested_group": "completed", "reason": "implementation done"},
  "changes": {
    "branch": "feat/blue-button",
    "commits": ["abc123"],
    "files_touched": ["web/button.tsx"],
    "pr_url": null
  },
  "validation": {"acceptance_all_met": true, "ran": ["pnpm test"], "notes": null},
  "progress": {
    "phase": "implementing",
    "checkpoints": {
      "investigation_complete": true,
      "design_choice_recorded": true,
      "implementation_complete": true,
      "validation_complete": true,
      "pr_opened": "n/a",
      "review_feedback_addressed": "n/a"
    }
  },
  "autonomy": {
    "score": 1,
    "type": "none",
    "reason": "clear local choice",
    "question_for_human": null,
    "safe_to_continue": true
  },
  "blockers": []
}
```
"""


@pytest.mark.unit
def test_parse_valid_fence():
    signal = parse(VALID_BODY)
    assert signal.status == "completed"
    assert signal.payload["changes"]["branch"] == "feat/blue-button"
    assert signal.payload["progress"]["phase"] == "implementing"


@pytest.mark.unit
def test_parse_missing_fence_raises():
    with pytest.raises(DoneSignalError):
        parse("no fence here")


@pytest.mark.unit
def test_parse_invalid_json_raises():
    body = "```pi-dash-done\n{not-json}\n```"
    with pytest.raises(DoneSignalError):
        parse(body)


@pytest.mark.unit
def test_parse_rejects_unknown_status():
    body = '```pi-dash-done\n{"status": "bogus"}\n```'
    with pytest.raises(DoneSignalError):
        parse(body)


@pytest.mark.unit
def test_ingest_into_run_completed(db, workspace, create_user):
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="",
        status=AgentRunStatus.RUNNING,
    )
    signal = ingest_into_run(run, VALID_BODY)
    run.refresh_from_db()
    assert signal is not None
    assert run.status == AgentRunStatus.COMPLETED
    assert run.done_payload["status"] == "completed"


@pytest.mark.unit
def test_ingest_into_run_blocked(db, workspace, create_user):
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="",
        status=AgentRunStatus.RUNNING,
    )
    body = (
        "```pi-dash-done\n"
        '{"status": "blocked", "blockers": ["missing key"]}\n'
        "```"
    )
    ingest_into_run(run, body)
    run.refresh_from_db()
    assert run.status == AgentRunStatus.BLOCKED
    assert run.done_payload["blockers"] == ["missing key"]


@pytest.mark.unit
def test_ingest_malformed_marks_failed(db, workspace, create_user):
    run = AgentRun.objects.create(
        owner=create_user,
        workspace=workspace,
        prompt="",
        status=AgentRunStatus.RUNNING,
    )
    ingest_into_run(run, "agent forgot the fence entirely")
    run.refresh_from_db()
    assert run.status == AgentRunStatus.FAILED
    assert "done-signal parse error" in run.error
