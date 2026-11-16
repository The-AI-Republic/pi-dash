# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Non-canonical ``<uuid:>`` spellings on app_workspace detail routes (PIDASHCONV-1713).

Django declares every segment below with ``<uuid:pk>`` / ``<uuid:user_id>``
(``apps/api/pi_dash/app/urls/workspace.py``: ``workspace-themes/<uuid:pk>/``,
``user-stats/`` / ``user-activity/`` / ``user-profile/`` / ``user-issues/``
with ``<uuid:user_id>``, ``user-activity/<uuid:user_id>/export/``,
``quick-links/<uuid:pk>/``, ``stickies/<uuid:pk>/``), whose converter
matches only the canonical lowercase hyphenated form — every other
spelling is a resolver 404 before auth or the view.

Rust parsed the segment with ``Uuid::parse``, which also accepts
uppercase, simple (no hyphens), braced and ``urn:uuid:`` spellings, so it
served — and PATCH/DELETE wrote through — URLs Django never routes.

Fix: classify each segment with ``classify_uuid_segment`` before the
session check; canonical serves, anything else answers the resolver 404.

Status-only assertions: the harness serves Django with
``pi_dash.settings.test`` (``DEBUG = True``), so its resolver 404 is the
technical HTML page, while Rust answers the prod JSON resolver bytes
(``{"error": "Page not found."}``, pinned by unit tests).
"""

from __future__ import annotations

import pytest

pytestmark = pytest.mark.contract

SPELLINGS = ["uppercase", "simple"]


def _spellings(row_id):
    """Spellings ``Uuid::parse`` accepts but Django's ``<uuid:>`` does not."""
    return {
        "uppercase": row_id.upper(),
        "simple": row_id.replace("-", ""),
    }


def _assert_404(response):
    # Resolution precedes auth and the view: never a 401, a 403, the 400
    # ``{"error": "Please provide valid detail"}`` body, or 200/204.
    assert response.status_code == 404, response.text[:500]


@pytest.fixture
def space(world):
    db = world["db"]
    ws = db.make_workspace("Acme", "acme", world["user"]["id"])
    db.make_workspace_member(ws["id"], world["user"]["id"], role=20)
    return ws


def _sticky(user_api, space):
    res = user_api.post(f"/api/workspaces/{space['slug']}/stickies/", json={"name": "S1"})
    assert res.status_code == 201, res.text
    return res.json()["id"]


def _qlink(user_api, space):
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/quick-links/",
        json={"title": "Q1", "url": "https://example.com/x"},
    )
    assert res.status_code == 201, res.text
    return res.json()["id"]


def _theme(user_api, space):
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/workspace-themes/",
        json={"name": "T1", "deleted_at": None},
    )
    assert res.status_code == 201, res.text
    return res.json()["id"]


# --- stickies ---


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_sticky_get_noncanonical_404s(user_api, space, spelling):
    sid = _sticky(user_api, space)
    control = user_api.get(f"/api/workspaces/{space['slug']}/stickies/{sid}/")
    assert control.status_code == 200, control.text
    _assert_404(
        user_api.get(
            f"/api/workspaces/{space['slug']}/stickies/{_spellings(sid)[spelling]}/"
        )
    )


def test_sticky_get_garbage_404s(user_api, space):
    sid = _sticky(user_api, space)
    _assert_404(user_api.get(f"/api/workspaces/{space['slug']}/stickies/not-a-uuid/"))
    control = user_api.get(f"/api/workspaces/{space['slug']}/stickies/{sid}/")
    assert control.status_code == 200, control.text


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_sticky_patch_noncanonical_404s_and_unchanged(user_api, space, spelling):
    sid = _sticky(user_api, space)
    _assert_404(
        user_api.patch(
            f"/api/workspaces/{space['slug']}/stickies/{_spellings(sid)[spelling]}/",
            json={"name": "mutated"},
        )
    )
    control = user_api.get(f"/api/workspaces/{space['slug']}/stickies/{sid}/")
    assert control.status_code == 200, control.text
    assert control.json()["name"] == "S1"


def test_sticky_patch_garbage_404s_and_unchanged(user_api, space):
    sid = _sticky(user_api, space)
    _assert_404(
        user_api.patch(
            f"/api/workspaces/{space['slug']}/stickies/not-a-uuid/",
            json={"name": "mutated"},
        )
    )
    control = user_api.get(f"/api/workspaces/{space['slug']}/stickies/{sid}/")
    assert control.status_code == 200, control.text
    assert control.json()["name"] == "S1"


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_sticky_delete_noncanonical_404s_and_keeps_row(user_api, space, spelling):
    sid = _sticky(user_api, space)
    _assert_404(
        user_api.delete(
            f"/api/workspaces/{space['slug']}/stickies/{_spellings(sid)[spelling]}/"
        )
    )
    control = user_api.get(f"/api/workspaces/{space['slug']}/stickies/{sid}/")
    assert control.status_code == 200, control.text
    assert control.json()["name"] == "S1"


def test_sticky_delete_garbage_404s_and_keeps_row(user_api, space):
    sid = _sticky(user_api, space)
    _assert_404(user_api.delete(f"/api/workspaces/{space['slug']}/stickies/not-a-uuid/"))
    control = user_api.get(f"/api/workspaces/{space['slug']}/stickies/{sid}/")
    assert control.status_code == 200, control.text


def test_sticky_garbage_anon_404s_before_auth(anon_api, space):
    # Resolution precedes authentication: never 401.
    _assert_404(anon_api.get(f"/api/workspaces/{space['slug']}/stickies/not-a-uuid/"))


# --- quick links ---


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_quicklink_patch_noncanonical_404s_and_unchanged(user_api, space, spelling):
    qid = _qlink(user_api, space)
    _assert_404(
        user_api.patch(
            f"/api/workspaces/{space['slug']}/quick-links/{_spellings(qid)[spelling]}/",
            json={"title": "mutated"},
        )
    )
    control = user_api.get(f"/api/workspaces/{space['slug']}/quick-links/{qid}/")
    assert control.status_code == 200, control.text
    assert control.json()["title"] == "Q1"


def test_quicklink_patch_garbage_404s_and_unchanged(user_api, space):
    qid = _qlink(user_api, space)
    _assert_404(
        user_api.patch(
            f"/api/workspaces/{space['slug']}/quick-links/not-a-uuid/",
            json={"title": "mutated"},
        )
    )
    control = user_api.get(f"/api/workspaces/{space['slug']}/quick-links/{qid}/")
    assert control.status_code == 200, control.text
    assert control.json()["title"] == "Q1"


# --- themes ---


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_theme_delete_noncanonical_404s_and_keeps_row(user_api, space, spelling):
    tid = _theme(user_api, space)
    _assert_404(
        user_api.delete(
            f"/api/workspaces/{space['slug']}/workspace-themes/{_spellings(tid)[spelling]}/"
        )
    )
    control = user_api.get(f"/api/workspaces/{space['slug']}/workspace-themes/{tid}/")
    assert control.status_code == 200, control.text
    assert control.json()["name"] == "T1"


def test_theme_delete_garbage_404s_and_keeps_row(user_api, space):
    tid = _theme(user_api, space)
    _assert_404(
        user_api.delete(
            f"/api/workspaces/{space['slug']}/workspace-themes/not-a-uuid/"
        )
    )
    control = user_api.get(f"/api/workspaces/{space['slug']}/workspace-themes/{tid}/")
    assert control.status_code == 200, control.text


# --- user profile / issues / export ---


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_user_profile_get_noncanonical_404s(user_api, space, world, spelling):
    uid = world["user"]["id"]
    control = user_api.get(f"/api/workspaces/{space['slug']}/user-profile/{uid}/")
    assert control.status_code == 200, control.text
    _assert_404(
        user_api.get(
            f"/api/workspaces/{space['slug']}/user-profile/{_spellings(uid)[spelling]}/"
        )
    )


def test_user_profile_get_garbage_404s(user_api, space, world):
    uid = world["user"]["id"]
    _assert_404(
        user_api.get(f"/api/workspaces/{space['slug']}/user-profile/not-a-uuid/")
    )
    control = user_api.get(f"/api/workspaces/{space['slug']}/user-profile/{uid}/")
    assert control.status_code == 200, control.text


def test_user_issues_garbage_non_member_404s_before_gate(other_api, space):
    # Resolution precedes the class gate: never 403.
    _assert_404(other_api.get(f"/api/workspaces/{space['slug']}/user-issues/xyz/"))


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_export_noncanonical_404s(user_api, space, world, spelling):
    uid = world["user"]["id"]
    control = user_api.post(
        f"/api/workspaces/{space['slug']}/user-activity/{uid}/export/",
        json={"date": "2024-01-01"},
    )
    assert control.status_code == 200, control.text
    _assert_404(
        user_api.post(
            f"/api/workspaces/{space['slug']}/user-activity/{_spellings(uid)[spelling]}/export/",
            json={"date": "2024-01-01"},
        )
    )


def test_export_garbage_404s(user_api, space, world):
    uid = world["user"]["id"]
    _assert_404(
        user_api.post(
            f"/api/workspaces/{space['slug']}/user-activity/not-a-uuid/export/",
            json={"date": "2024-01-01"},
        )
    )
    control = user_api.post(
        f"/api/workspaces/{space['slug']}/user-activity/{uid}/export/",
        json={"date": "2024-01-01"},
    )
    assert control.status_code == 200, control.text
