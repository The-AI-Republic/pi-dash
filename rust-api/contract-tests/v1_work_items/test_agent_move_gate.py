# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""NEWPIDASH-338 item 3: the agent-move gate covers headerless attribution.

An issue run may not move its own issue into Done/Cancelled or a stage
the project switched off. The gate applies whenever the move is
attributed to a run (``moved_by_run`` is set) — including the headerless
inference over the caller's active run, which a run-token patch without
``X-Pi-Dash-Run-Id`` still stamps.

New-system-only rule (lean workflow §6.5/§7): Django has no such gate
and moves the issue (200); Rust refuses (400).
"""

from __future__ import annotations

import uuid

import pytest

from _harness.diffs import is_rust

from .conftest import issue_url

pytestmark = pytest.mark.contract


def _make_state(seeder, world, name, group):
    state_id = str(uuid.uuid4())
    seeder.db.execute(
        """INSERT INTO states
               (id, workspace_id, project_id, name, description, color, slug,
                sequence, "group", is_triage, "default", created_at, updated_at)
           VALUES (%s,%s,%s,%s,'','#808080',%s,65535,%s,false,false,now(),now())""",
        (
            state_id,
            world["workspace"]["id"],
            world["project"]["id"],
            name,
            name.lower().replace(" ", "-"),
            group,
        ),
    )
    seeder._put("states", state_id)
    return state_id


@pytest.fixture()
def gated_world(seeder, world):
    """Issue in In Progress, a completed-group Done, and the owner's active run."""
    progress = _make_state(seeder, world, "In Progress", "started")
    done = _make_state(seeder, world, "Done", "completed")
    seeder.db.execute("UPDATE issues SET state_id = %s WHERE id = %s", (progress, world["issue"]["id"]))
    pod = seeder.create_pod(world["workspace"]["id"], world["project"]["id"])
    run = seeder.create_agent_run(
        world["workspace"]["id"],
        world["owner"]["id"],
        pod["id"],
        world["issue"]["id"],
        status="running",
    )
    return {"progress": progress, "done": done, "run": run}


def _patch(api, world, state_id, headers=None):
    return api.patch(
        issue_url(world, world["issue"]["id"]),
        json={"state": state_id},
        headers=headers or {},
    )


def test_headerless_run_patch_to_done_is_refused(api, world, seeder, gated_world):
    """A run-token patch without the run-id header is still an agent move."""
    resp = _patch(api, world, gated_world["done"])
    if not is_rust():
        assert resp.status_code == 200, resp.text[:500]
        return
    assert resp.status_code == 400, resp.text[:500]
    body = resp.json()
    assert "is for a person to set" in body["error"], body
    assert "AI Completed" not in body["error"], body
    assert "waiting_on_human" in body["error"], body


def test_header_run_patch_to_done_is_refused(api, world, seeder, gated_world):
    """Explicit-header attribution was already gated; pin it."""
    resp = _patch(
        api,
        world,
        gated_world["done"],
        headers={"X-Pi-Dash-Run-Id": gated_world["run"]["id"]},
    )
    if not is_rust():
        assert resp.status_code == 200, resp.text[:500]
        return
    assert resp.status_code == 400, resp.text[:500]
    assert "is for a person to set" in resp.json()["error"]
