# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""NEWPIDASH-218: profile user-issues relation filters.

The profile list (``GET /api/workspaces/{slug}/user-issues/{user_id}/``)
runs both the rich ``filters=`` tree and the legacy query keys. Django
applies them as two ``.filter()`` calls (two joins), so an issue linked
to both values matches. The pre-fix Rust profile compiled both stacks
over one join alias per relation, so a rich leaf and a legacy key on
the same relation required one link row to equal both values and listed
nothing (NEWPIDASH-195 twin). A negated relation leaf must use
``NOT EXISTS`` (Django ``split_exclude``, NEWPIDASH-194 twin) so
unlabelled issues survive.

Fixture (debugger probe): bot assigned to A (L1), B (L1+L2 + other
assignee), H (bare). All three are in bot's profile scope via the
assignee link. Queries mirror the old app's Assigned tab (legacy
``assignees=`` + saved rich filters).
"""

import json

import pytest

from _harness import env
from _harness.seed import Seed


@pytest.fixture
def space(world):
    db = world["db"]
    ws = db.make_workspace("Acme", "acme", world["user"]["id"])
    db.make_workspace_member(ws["id"], world["user"]["id"], role=20)
    return ws


def _seed_profile_abc(world, space):
    # Target users must be workspace members for the viewer-gated list.
    world["db"].make_workspace_member(
        space["id"], world["bot"]["id"], role=15,
    )
    world["db"].make_workspace_member(
        space["id"], world["other"]["id"], role=15,
    )
    conn = env.connect()
    try:
        seed = Seed(conn)
        uid, ws_id = world["user"]["id"], space["id"]
        bot_id, other_id = world["bot"]["id"], world["other"]["id"]
        project_id = seed.project(ws_id)
        seed.project_member(project_id, ws_id, uid, role=20)
        label_l1 = seed.label(ws_id, project_id, name="218 L1")
        label_l2 = seed.label(ws_id, project_id, name="218 L2")
        issue_a = seed.issue(ws_id, project_id, name="218 A")
        issue_b = seed.issue(ws_id, project_id, name="218 B")
        issue_h = seed.issue(ws_id, project_id, name="218 H")
        # Bot assigned to all three → all in bot's profile scope.
        seed.issue_assignee(ws_id, project_id, issue_a, bot_id)
        seed.issue_assignee(ws_id, project_id, issue_b, bot_id)
        seed.issue_assignee(ws_id, project_id, issue_h, bot_id)
        # B also assigned to other → rich+legacy assignee mix hits B only
        # (A and H lack the other link; the rich leaf alone would also
        # hit only B, but the legacy leg list alone hits A+B+H).
        seed.issue_assignee(ws_id, project_id, issue_b, other_id)
        seed.issue_label(ws_id, project_id, issue_a, label_l1)
        seed.issue_label(ws_id, project_id, issue_b, label_l1)
        seed.issue_label(ws_id, project_id, issue_b, label_l2)
    finally:
        conn.close()
    return issue_a, issue_b, issue_h, label_l1, label_l2


def test_profile_rich_plus_legacy_assignee(user_api, space, world):
    issue_a, issue_b, issue_h, _, _ = _seed_profile_abc(world, space)
    bot_id, other_id = world["bot"]["id"], world["other"]["id"]
    url = f"/api/workspaces/{space['slug']}/user-issues/{bot_id}/"
    res = user_api.get(
        url,
        params={
            "filters": json.dumps({"and": [{"assignee_id__in": other_id}]}),
            "assignees": bot_id,
        },
    )
    assert res.status_code == 200, res.text
    assert {row["id"] for row in res.json()["results"]} == {issue_b}


def test_profile_rich_plus_legacy_label(user_api, space, world):
    issue_a, issue_b, issue_h, label_l1, label_l2 = _seed_profile_abc(world, space)
    bot_id = world["bot"]["id"]
    url = f"/api/workspaces/{space['slug']}/user-issues/{bot_id}/"
    res = user_api.get(
        url,
        params={
            "filters": json.dumps({"and": [{"label_id__in": label_l2}]}),
            "labels": label_l1,
        },
    )
    assert res.status_code == 200, res.text
    assert {row["id"] for row in res.json()["results"]} == {issue_b}


def test_profile_not_label_returns_unlabelled(user_api, space, world):
    issue_a, issue_b, issue_h, label_l1, _ = _seed_profile_abc(world, space)
    bot_id = world["bot"]["id"]
    url = f"/api/workspaces/{space['slug']}/user-issues/{bot_id}/"
    res = user_api.get(
        url,
        params={"filters": json.dumps({"not": {"label_id__in": label_l1}})},
    )
    assert res.status_code == 200, res.text
    assert {row["id"] for row in res.json()["results"]} == {issue_h}
