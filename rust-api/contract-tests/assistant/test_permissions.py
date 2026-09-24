# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Cross-cutting permission ladder and desktop-only endpoints.

The assistant surface requires workspace role >= MEMBER everywhere it is
workspace-scoped, and an authenticated user everywhere else; the
agent-profile/token pair additionally requires a desktop session, which a
browser login never has. If any ``require_member`` check or the
``IsDesktopSession`` permission is removed, the denial cases below fail —
that is the deliberate-permission-removal tripwire for this domain.
"""

import uuid

from .conftest import threads_url

PROFILE_URL = "/api/users/me/ai-assistant/agent-profile/"
TOKEN_URL = "/api/users/me/ai-assistant/agent-token/"


def test_agent_profile_shape_for_browser_session(world, member):
    # A browser session is authenticated but not a desktop session: the
    # endpoint answers with the desktop-only denial, never a credential.
    res = member.get(PROFILE_URL)
    assert res.status_code == 403
    body = res.json()
    assert body["error"] == "desktop_session_required"
    assert "managed_runner_enabled" not in body


def test_agent_token_shape_for_browser_session(world, member):
    res = member.post(TOKEN_URL)
    assert res.status_code == 403
    body = res.json()
    assert body["error"] == "desktop_session_required"
    assert "token" not in body


def test_agent_endpoints_reject_anonymous(world, anon):
    assert anon.get(PROFILE_URL).status_code in (401, 403)
    assert anon.post(TOKEN_URL).status_code in (401, 403)


def test_workspace_role_ladder_on_threads(world, admin, project_outsider):
    base = threads_url(world)
    assert project_outsider.get(f"{base}/threads/").status_code == 200
    assert admin.get(f"{base}/threads/").status_code == 200


def test_cross_workspace_isolation(world, member):
    # A member of workspace A sees nothing of workspace B's threads: the
    # other workspace's slug resolves to "not a member" before any row is
    # touched.
    other_base = f"/api/workspaces/{world.other_ws.slug}/ai-assistant"
    assert member.get(f"{other_base}/threads/").status_code == 403
    assert (
        member.post(f"{other_base}/threads/", json={"title": "x"}).status_code == 403
    )
    assert (
        member.post(
            f"{other_base}/threads/{uuid.uuid4()}/messages/", json={"content": "x"}
        ).status_code
        == 403
    )
