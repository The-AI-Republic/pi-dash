# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Bulk sidebar-preferences PATCH stays on the requesting user's rows
(NEWPIDASH-267, DIFF-33).

Old bug fix: Django's ``WorkspaceUserPreferenceViewSet.patch``
(``app/views/workspace/user_preference.py:88``) looks each entry up with
``filter(key=key, workspace__slug=slug).first()`` — no user scope — over
``Meta.ordering = ("-created_at",)``, so the write lands on whichever
workspace member most recently auto-created that key (rows are
auto-created on first GET), while the endpoint still answers 200
``{"message": "Successfully updated"}``. The Rust port copied this
verbatim (PB-0771, PB-0860).

The new system scopes the lookup to the requesting user
(``user=request.user``), like the neighboring workspace user-properties
endpoint does:

* user A PATCHes ``drafts`` AFTER user B auto-created the rows (B's row
  is newest): on Rust only A's row flips (B's row byte-identical); on
  Django A's row is unchanged and B's row flips.
* The same holds for a ``sort_order`` reorder write.
* Unknown keys stay silently skipped with 200 on both backends.
"""

import pytest

from _harness.diffs import expect

# Autocreate defaults for `drafts` (empty-table trace,
# queries_extras::sidebar_autocreate_plan): pinned, sort 95535.
DRAFTS_SORT = 95535.0


@pytest.fixture
def space(world):
    db = world["db"]
    ws = db.make_workspace("Acme", "acme", world["user"]["id"])
    db.make_workspace_member(ws["id"], world["user"]["id"], role=20)
    return ws


@pytest.fixture
def member_api(world, space):
    """other@example.com as a role-15 member of the space workspace."""
    from _harness import user_client

    world["db"].make_workspace_member(space["id"], world["other"]["id"], role=15)
    with user_client(world["other_key"]) as client:
        yield client


def _get_prefs(client, slug):
    res = client.get(f"/api/workspaces/{slug}/sidebar-preferences/")
    assert res.status_code == 200
    return res.json()


def _patch(client, slug, items):
    res = client.patch(f"/api/workspaces/{slug}/sidebar-preferences/", json=items)
    assert res.status_code == 200
    assert res.json() == {"message": "Successfully updated"}


def test_sidebar_patch_toggle_stays_on_requesting_user(user_api, member_api, space):
    slug = space["slug"]
    # A auto-creates first, then B: B's `drafts` row is the newest across
    # users, so the buggy lookup matches B's row on A's PATCH.
    assert _get_prefs(user_api, slug)["drafts"] == {
        "is_pinned": True,
        "sort_order": DRAFTS_SORT,
    }
    assert _get_prefs(member_api, slug)["drafts"] == {
        "is_pinned": True,
        "sort_order": DRAFTS_SORT,
    }

    _patch(user_api, slug, [{"key": "drafts", "is_pinned": False}])

    assert _get_prefs(user_api, slug)["drafts"]["is_pinned"] == expect(
        "DIFF-33", django=True, rust=False
    )
    assert _get_prefs(member_api, slug)["drafts"]["is_pinned"] == expect(
        "DIFF-33", django=False, rust=True
    )


def test_sidebar_patch_reorder_stays_on_requesting_user(user_api, member_api, space):
    slug = space["slug"]
    _get_prefs(user_api, slug)
    _get_prefs(member_api, slug)

    _patch(user_api, slug, [{"key": "drafts", "sort_order": 55555.0}])

    assert _get_prefs(user_api, slug)["drafts"]["sort_order"] == expect(
        "DIFF-33", django=DRAFTS_SORT, rust=55555.0
    )
    assert _get_prefs(member_api, slug)["drafts"]["sort_order"] == expect(
        "DIFF-33", django=55555.0, rust=DRAFTS_SORT
    )


def test_sidebar_patch_unknown_key_still_skipped(user_api, space):
    slug = space["slug"]
    _get_prefs(user_api, slug)
    before = _get_prefs(user_api, slug)
    _patch(user_api, slug, [{"key": "nope", "is_pinned": False}])
    assert _get_prefs(user_api, slug) == before
