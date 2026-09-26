# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Workspace extras: themes, profile stats/activity/issues/export, labels,
states, estimates, modules, cycles, favorites, drafts, quick links, home
prefs, recent visits, stickies, sidebar prefs, user properties.

Covers the remaining routes of app/urls/workspace.py not in
test_workspace_core.py. Frozen quirks: favorites POST returns 200 on create
(and 200 with the existing row on duplicate); draft PATCH returns 204 with an
empty body; home/sidebar PATCH is a no-op until the first GET auto-creates
the rows (unknown home keys 400, unknown sidebar keys are silently skipped);
GET user-stats has no membership check (non-members get 200); GET
user-profile for a non-member is 404; quick-link retrieve 404s with {"error"}
while partial_update 404s with {"detail"}.
"""

import uuid

import pytest

THEME_KEYS = {
    "actor",
    "colors",
    "created_at",
    "created_by",
    "deleted_at",
    "id",
    "name",
    "updated_at",
    "updated_by",
    "workspace",
}

STATS_KEYS = {
    "assigned_issues",
    "completed_issues",
    "created_issues",
    "pending_issues",
    "present_cycles",
    "priority_distribution",
    "state_distribution",
    "subscribed_issues",
    "upcoming_cycles",
}

PAGINATED_KEYS = {
    "count",
    "extra_stats",
    "grouped_by",
    "next_cursor",
    "next_page_results",
    "prev_cursor",
    "prev_page_results",
    "results",
    "sub_grouped_by",
    "total_count",
    "total_pages",
    "total_results",
}

USER_DATA_KEYS = {
    "email",
    "first_name",
    "last_name",
    "avatar_url",
    "cover_image_url",
    "date_joined",
    "user_timezone",
    "display_name",
}

USER_PROPS_KEYS = {
    "created_at",
    "created_by",
    "deleted_at",
    "display_filters",
    "display_properties",
    "filters",
    "id",
    "navigation_control_preference",
    "navigation_project_limit",
    "rich_filters",
    "updated_at",
    "updated_by",
    "user",
    "workspace",
}

FAV_KEYS = {
    "entity_data",
    "entity_identifier",
    "entity_type",
    "id",
    "is_folder",
    "name",
    "parent",
    "project_id",
    "sequence",
    "workspace_id",
}

DRAFT_KEYS = {
    "assignee_ids",
    "completed_at",
    "created_at",
    "created_by",
    "cycle_id",
    "description_html",
    "estimate_point",
    "id",
    "label_ids",
    "module_ids",
    "name",
    "parent_id",
    "priority",
    "project_id",
    "sort_order",
    "start_date",
    "state_id",
    "target_date",
    "type_id",
    "updated_at",
    "updated_by",
}

QUICKLINK_KEYS = {
    "created_at",
    "created_by",
    "deleted_at",
    "id",
    "metadata",
    "owner",
    "project",
    "title",
    "updated_at",
    "updated_by",
    "url",
    "workspace",
}

STICKY_KEYS = {
    "background_color",
    "color",
    "created_at",
    "created_by",
    "deleted_at",
    "description",
    "description_binary",
    "description_html",
    "description_stripped",
    "id",
    "logo_props",
    "name",
    "owner",
    "sort_order",
    "updated_at",
    "updated_by",
    "workspace",
}

SIDEBAR_KEYS = {
    "active_cycles",
    "analytics",
    "archives",
    "drafts",
    "stickies",
    "views",
    "your_work",
}


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


# --- workspace themes ---


def test_themes_list_empty(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/workspace-themes/")
    assert res.status_code == 200
    assert res.json() == []


def test_theme_create(user_api, space, world):
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/workspace-themes/",
        json={"name": "T1", "deleted_at": None},
    )
    assert res.status_code == 201
    body = res.json()
    assert set(body.keys()) == THEME_KEYS
    assert body["name"] == "T1"
    assert body["workspace"] == space["id"]
    assert body["actor"] == world["user"]["id"]


def test_theme_create_missing_fields(user_api, space):
    res = user_api.post(f"/api/workspaces/{space['slug']}/workspace-themes/", json={})
    assert res.status_code == 400
    assert res.json() == {
        "deleted_at": ["This field is required."],
        "name": ["This field is required."],
    }


def test_theme_crud(user_api, space):
    tid = user_api.post(
        f"/api/workspaces/{space['slug']}/workspace-themes/",
        json={"name": "T1", "deleted_at": None},
    ).json()["id"]
    res = user_api.get(f"/api/workspaces/{space['slug']}/workspace-themes/{tid}/")
    assert res.status_code == 200
    assert res.json()["name"] == "T1"
    res = user_api.patch(
        f"/api/workspaces/{space['slug']}/workspace-themes/{tid}/", json={"name": "T2"}
    )
    assert res.status_code == 200
    assert res.json()["name"] == "T2"
    res = user_api.delete(f"/api/workspaces/{space['slug']}/workspace-themes/{tid}/")
    assert res.status_code == 204
    assert (
        user_api.get(f"/api/workspaces/{space['slug']}/workspace-themes/{tid}/").status_code
        == 404
    )


def test_themes_member_allowed(member_api, space):
    # WorkSpaceAdminPermission admits Admin+Member despite the name.
    res = member_api.post(
        f"/api/workspaces/{space['slug']}/workspace-themes/",
        json={"name": "M", "deleted_at": None},
    )
    assert res.status_code == 201
    assert member_api.get(f"/api/workspaces/{space['slug']}/workspace-themes/").status_code == 200


def test_themes_non_member_denied(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/workspace-themes/")
    assert res.status_code == 403
    assert res.json() == {"detail": "You do not have permission to perform this action."}


def test_themes_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/workspace-themes/")
    assert res.status_code == 401


def test_themes_tenant_isolation(user_api, world, space):
    ws2 = world["db"].make_workspace("Other", "other-ws", world["user"]["id"])
    world["db"].make_workspace_member(ws2["id"], world["user"]["id"], role=20)
    tid = user_api.post(
        f"/api/workspaces/{ws2['slug']}/workspace-themes/",
        json={"name": "Elsewhere", "deleted_at": None},
    ).json()["id"]
    mine = user_api.get(f"/api/workspaces/{space['slug']}/workspace-themes/").json()
    assert mine == []
    assert user_api.get(f"/api/workspaces/{space['slug']}/workspace-themes/{tid}/").status_code == 404


# --- user stats / activity / profile / issues / export ---


def test_user_stats_shape(user_api, space, world):
    res = user_api.get(f"/api/workspaces/{space['slug']}/user-stats/{world['user']['id']}/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == STATS_KEYS
    assert body["created_issues"] == 0
    assert body["state_distribution"] == []


def test_user_stats_non_member_open(other_api, space, world):
    # Frozen quirk: no membership check on this endpoint.
    res = other_api.get(f"/api/workspaces/{space['slug']}/user-stats/{world['user']['id']}/")
    assert res.status_code == 200
    assert set(res.json().keys()) == STATS_KEYS


def test_user_stats_anon(anon_api, space, world):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/user-stats/{world['user']['id']}/")
    assert res.status_code == 401


def test_user_activity_shape(user_api, space, world):
    res = user_api.get(f"/api/workspaces/{space['slug']}/user-activity/{world['user']['id']}/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == PAGINATED_KEYS
    assert body["results"] == []
    assert body["total_count"] == 0


def test_user_activity_non_member_denied(other_api, space, world):
    res = other_api.get(f"/api/workspaces/{space['slug']}/user-activity/{world['user']['id']}/")
    assert res.status_code == 403


def test_user_activity_anon(anon_api, space, world):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/user-activity/{world['user']['id']}/")
    assert res.status_code == 401


def test_export_activity_get_not_allowed(user_api, space, world):
    res = user_api.get(f"/api/workspaces/{space['slug']}/user-activity/{world['user']['id']}/export/")
    assert res.status_code == 405
    assert res.json() == {"detail": 'Method "GET" not allowed.'}


def test_export_activity_missing_date(user_api, space, world):
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/user-activity/{world['user']['id']}/export/", json={}
    )
    assert res.status_code == 400
    assert res.json() == {"error": "Date is required"}


def test_export_activity_csv(user_api, space, world):
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/user-activity/{world['user']['id']}/export/",
        json={"date": "2026-09-24"},
    )
    assert res.status_code == 200
    assert res.headers["content-type"].startswith("text/csv")
    assert "Actor name" in res.text


def test_user_profile_shape(user_api, space, world):
    res = user_api.get(f"/api/workspaces/{space['slug']}/user-profile/{world['user']['id']}/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == {"project_data", "user_data"}
    assert set(body["user_data"].keys()) == USER_DATA_KEYS
    assert body["user_data"]["email"] == "user@example.com"
    assert body["project_data"] == []


def test_user_profile_other_member(member_api, space, world):
    res = member_api.get(f"/api/workspaces/{space['slug']}/user-profile/{world['user']['id']}/")
    assert res.status_code == 200
    assert res.json()["user_data"]["email"] == "user@example.com"


def test_user_profile_non_member_404(other_api, space, world):
    res = other_api.get(f"/api/workspaces/{space['slug']}/user-profile/{world['user']['id']}/")
    assert res.status_code == 404
    assert res.json() == {"error": "The required object does not exist."}


def test_user_profile_anon(anon_api, space, world):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/user-profile/{world['user']['id']}/")
    assert res.status_code == 401


def test_user_issues_shape(user_api, space, world):
    res = user_api.get(f"/api/workspaces/{space['slug']}/user-issues/{world['user']['id']}/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == PAGINATED_KEYS
    assert body["results"] == []


def test_user_issues_non_member_denied(other_api, space, world):
    res = other_api.get(f"/api/workspaces/{space['slug']}/user-issues/{world['user']['id']}/")
    assert res.status_code == 403


def test_user_issues_anon(anon_api, space, world):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/user-issues/{world['user']['id']}/")
    assert res.status_code == 401


# --- labels / states / estimates / modules / cycles ---


def test_labels_empty(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/labels/")
    assert res.status_code == 200
    assert res.json() == []


def test_labels_non_member_denied(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/labels/")
    assert res.status_code == 403
    assert res.json() == {"detail": "You do not have permission to perform this action."}


def test_labels_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/labels/")
    assert res.status_code == 401


def test_states_empty(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/states/")
    assert res.status_code == 200
    assert res.json() == []


def test_states_non_member_denied(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/states/")
    assert res.status_code == 403


def test_states_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/states/")
    assert res.status_code == 401


def test_estimates_empty(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/estimates/")
    assert res.status_code == 200
    assert res.json() == []


def test_estimates_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/estimates/")
    assert res.status_code == 401


def test_modules_empty(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/modules/")
    assert res.status_code == 200
    assert res.json() == []


def test_modules_non_member_denied(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/modules/")
    assert res.status_code == 403


def test_modules_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/modules/")
    assert res.status_code == 401


def test_cycles_empty(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/cycles/")
    assert res.status_code == 200
    assert res.json() == []


def test_cycles_non_member_denied(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/cycles/")
    assert res.status_code == 403


def test_cycles_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/cycles/")
    assert res.status_code == 401


def test_user_properties_shape(user_api, space, world):
    res = user_api.get(f"/api/workspaces/{space['slug']}/user-properties/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == USER_PROPS_KEYS
    assert body["user"] == world["user"]["id"]
    assert body["workspace"] == space["id"]


def test_user_properties_patch(user_api, space):
    res = user_api.patch(
        f"/api/workspaces/{space['slug']}/user-properties/",
        json={"navigation_project_limit": 7},
    )
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == USER_PROPS_KEYS
    assert body["navigation_project_limit"] == 7
    assert user_api.get(f"/api/workspaces/{space['slug']}/user-properties/").json()[
        "navigation_project_limit"
    ] == 7


def test_user_properties_non_member_denied(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/user-properties/")
    assert res.status_code == 403


def test_user_properties_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/user-properties/")
    assert res.status_code == 401


# --- favorites ---


def test_favorites_list_empty(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/user-favorites/")
    assert res.status_code == 200
    assert res.json() == []


def test_favorite_create_folder(user_api, space):
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/user-favorites/",
        json={"entity_type": "folder", "name": "F1", "is_folder": True},
    )
    # Frozen quirk: creation answers 200, not 201.
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == FAV_KEYS
    assert body["name"] == "F1"
    assert body["workspace_id"] == space["id"]
    assert body["sequence"] == 65535.0


def test_favorite_create_missing_type(user_api, space):
    res = user_api.post(f"/api/workspaces/{space['slug']}/user-favorites/", json={})
    assert res.status_code == 400
    assert res.json() == {"entity_type": ["This field is required."]}


def test_favorite_create_bad_identifier(user_api, space):
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/user-favorites/",
        json={"entity_type": "project", "entity_identifier": "x-1", "name": "F1"},
    )
    assert res.status_code == 400
    assert res.json() == {"error": "Please provide valid detail"}


def test_favorite_duplicate_returns_existing(user_api, space):
    ident = str(uuid.uuid4())
    first = user_api.post(
        f"/api/workspaces/{space['slug']}/user-favorites/",
        json={"entity_type": "project", "entity_identifier": ident, "name": "F2"},
    )
    assert first.status_code == 200
    second = user_api.post(
        f"/api/workspaces/{space['slug']}/user-favorites/",
        json={"entity_type": "project", "entity_identifier": ident, "name": "F2"},
    )
    assert second.status_code == 200
    assert second.json()["id"] == first.json()["id"]
    assert len(user_api.get(f"/api/workspaces/{space['slug']}/user-favorites/").json()) == 1


def test_favorite_patch_group_delete(user_api, space):
    fid = user_api.post(
        f"/api/workspaces/{space['slug']}/user-favorites/",
        json={"entity_type": "folder", "name": "F1", "is_folder": True},
    ).json()["id"]
    res = user_api.patch(
        f"/api/workspaces/{space['slug']}/user-favorites/{fid}/", json={"name": "F2"}
    )
    assert res.status_code == 200
    assert res.json()["name"] == "F2"
    res = user_api.get(f"/api/workspaces/{space['slug']}/user-favorites/{fid}/group/")
    assert res.status_code == 200
    assert res.json() == []
    assert user_api.delete(f"/api/workspaces/{space['slug']}/user-favorites/{fid}/").status_code == 204
    assert user_api.get(f"/api/workspaces/{space['slug']}/user-favorites/").json() == []


def test_favorites_non_member_denied(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/user-favorites/")
    assert res.status_code == 403
    assert res.json() == {"error": "You don't have the required permissions."}


def test_favorites_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/user-favorites/")
    assert res.status_code == 401


# --- drafts ---


def test_drafts_list_empty(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/draft-issues/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == PAGINATED_KEYS
    assert body["results"] == []


def test_draft_create(user_api, space, world):
    res = user_api.post(f"/api/workspaces/{space['slug']}/draft-issues/", json={"name": "D1"})
    assert res.status_code == 201
    body = res.json()
    assert set(body.keys()) == DRAFT_KEYS
    assert body["name"] == "D1"
    assert body["created_by"] == world["user"]["id"]
    assert body["priority"] == "none"
    assert body["label_ids"] == []


def test_draft_retrieve_patch_delete(user_api, space):
    did = user_api.post(
        f"/api/workspaces/{space['slug']}/draft-issues/", json={"name": "D1"}
    ).json()["id"]
    res = user_api.get(f"/api/workspaces/{space['slug']}/draft-issues/{did}/")
    assert res.status_code == 200
    assert set(res.json().keys()) == DRAFT_KEYS
    assert res.json()["name"] == "D1"
    # Frozen quirk: update answers 204 with an empty body.
    res = user_api.patch(f"/api/workspaces/{space['slug']}/draft-issues/{did}/", json={"name": "D2"})
    assert res.status_code == 204
    assert user_api.get(f"/api/workspaces/{space['slug']}/draft-issues/{did}/").json()["name"] == "D2"
    assert user_api.delete(f"/api/workspaces/{space['slug']}/draft-issues/{did}/").status_code == 204
    res = user_api.get(f"/api/workspaces/{space['slug']}/draft-issues/{did}/")
    assert res.status_code == 404
    assert res.json() == {"error": "The required object does not exist."}


def test_draft_to_issue_requires_project(user_api, space):
    did = user_api.post(
        f"/api/workspaces/{space['slug']}/draft-issues/", json={"name": "D1"}
    ).json()["id"]
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/draft-to-issue/{did}/", json={"name": "I1"}
    )
    assert res.status_code == 400
    assert res.json() == {"error": "Project is required to create an issue."}


def test_drafts_only_own_visible(user_api, member_api, space):
    member_api.post(f"/api/workspaces/{space['slug']}/draft-issues/", json={"name": "Other"})
    assert user_api.get(f"/api/workspaces/{space['slug']}/draft-issues/").json()["results"] == []


def test_drafts_non_member_denied(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/draft-issues/")
    assert res.status_code == 403
    assert res.json() == {"error": "You don't have the required permissions."}


def test_drafts_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/draft-issues/")
    assert res.status_code == 401


# --- quick links ---


def test_quicklinks_list_empty(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/quick-links/")
    assert res.status_code == 200
    assert res.json() == []


def test_quicklink_crud(user_api, space, world):
    res = user_api.post(
        f"/api/workspaces/{space['slug']}/quick-links/",
        json={"title": "Q1", "url": "https://example.com/x"},
    )
    assert res.status_code == 201
    body = res.json()
    assert set(body.keys()) == QUICKLINK_KEYS
    assert body["owner"] == world["user"]["id"]
    qid = body["id"]
    assert user_api.get(f"/api/workspaces/{space['slug']}/quick-links/{qid}/").json()["title"] == "Q1"
    res = user_api.patch(
        f"/api/workspaces/{space['slug']}/quick-links/{qid}/", json={"title": "Q2"}
    )
    assert res.status_code == 200
    assert res.json()["title"] == "Q2"
    assert user_api.delete(f"/api/workspaces/{space['slug']}/quick-links/{qid}/").status_code == 204
    res = user_api.get(f"/api/workspaces/{space['slug']}/quick-links/{qid}/")
    assert res.status_code == 404
    assert res.json() == {"error": "Quick link not found."}


def test_quicklink_create_missing_url(user_api, space):
    res = user_api.post(f"/api/workspaces/{space['slug']}/quick-links/", json={})
    assert res.status_code == 400
    assert res.json() == {"url": ["This field is required."]}


def test_quicklinks_owner_scoped(user_api, member_api, space):
    qid = member_api.post(
        f"/api/workspaces/{space['slug']}/quick-links/",
        json={"title": "Other", "url": "https://example.com/o"},
    ).json()["id"]
    assert user_api.get(f"/api/workspaces/{space['slug']}/quick-links/").json() == []
    res = user_api.get(f"/api/workspaces/{space['slug']}/quick-links/{qid}/")
    assert res.status_code == 404
    assert res.json() == {"error": "Quick link not found."}


def test_quicklinks_non_member_denied(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/quick-links/")
    assert res.status_code == 403


def test_quicklinks_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/quick-links/")
    assert res.status_code == 401


# --- home prefs / recent visits / stickies / sidebar ---


def test_home_prefs_autocreate(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/home-preferences/")
    assert res.status_code == 200
    body = res.json()
    assert {p["key"] for p in body} == {"my_stickies", "recents", "quick_links"}
    assert set(body[0].keys()) == {"config", "is_enabled", "key", "sort_order"}


def test_home_pref_patch(user_api, space):
    user_api.get(f"/api/workspaces/{space['slug']}/home-preferences/")
    res = user_api.patch(
        f"/api/workspaces/{space['slug']}/home-preferences/recents/", json={"is_enabled": False}
    )
    assert res.status_code == 200
    assert set(res.json().keys()) == {"is_enabled", "key", "sort_order"}
    assert res.json()["is_enabled"] is False
    after = user_api.get(f"/api/workspaces/{space['slug']}/home-preferences/").json()
    assert [p for p in after if p["key"] == "recents"][0]["is_enabled"] is False


def test_home_pref_patch_unknown_key(user_api, space):
    user_api.get(f"/api/workspaces/{space['slug']}/home-preferences/")
    res = user_api.patch(
        f"/api/workspaces/{space['slug']}/home-preferences/nope/", json={"is_enabled": False}
    )
    assert res.status_code == 400
    assert res.json() == {"detail": "Preference not found"}


def test_home_prefs_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/home-preferences/")
    assert res.status_code == 401


def test_recent_visits_empty(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/recent-visits/")
    assert res.status_code == 200
    assert res.json() == []


def test_recent_visits_filter(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/recent-visits/", params={"entity_name": "issue"})
    assert res.status_code == 200
    assert res.json() == []


def test_recent_visits_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/recent-visits/")
    assert res.status_code == 401


def test_stickies_list_empty(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/stickies/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == PAGINATED_KEYS
    assert body["results"] == []


def test_sticky_crud(user_api, space, world):
    res = user_api.post(f"/api/workspaces/{space['slug']}/stickies/", json={"name": "S1"})
    assert res.status_code == 201
    body = res.json()
    assert set(body.keys()) == STICKY_KEYS
    assert body["owner"] == world["user"]["id"]
    assert body["workspace"] == space["id"]
    sid = body["id"]
    assert user_api.get(f"/api/workspaces/{space['slug']}/stickies/{sid}/").json()["name"] == "S1"
    res = user_api.patch(f"/api/workspaces/{space['slug']}/stickies/{sid}/", json={"name": "S2"})
    assert res.status_code == 200
    assert res.json()["name"] == "S2"
    assert user_api.delete(f"/api/workspaces/{space['slug']}/stickies/{sid}/").status_code == 204


def test_stickies_owner_scoped(user_api, member_api, space):
    member_api.post(f"/api/workspaces/{space['slug']}/stickies/", json={"name": "Other"})
    assert user_api.get(f"/api/workspaces/{space['slug']}/stickies/").json()["results"] == []


def test_stickies_non_member_denied(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/stickies/")
    assert res.status_code == 403


def test_stickies_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/stickies/")
    assert res.status_code == 401


def test_sidebar_prefs_shape(user_api, space):
    res = user_api.get(f"/api/workspaces/{space['slug']}/sidebar-preferences/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == SIDEBAR_KEYS
    assert set(body["views"].keys()) == {"is_pinned", "sort_order"}


def test_sidebar_pref_patch(user_api, space):
    user_api.get(f"/api/workspaces/{space['slug']}/sidebar-preferences/")
    res = user_api.patch(
        f"/api/workspaces/{space['slug']}/sidebar-preferences/",
        json=[{"key": "views", "is_pinned": True}],
    )
    assert res.status_code == 200
    assert res.json() == {"message": "Successfully updated"}
    after = user_api.get(f"/api/workspaces/{space['slug']}/sidebar-preferences/").json()
    assert after["views"]["is_pinned"] is True


def test_sidebar_non_member_denied(other_api, space):
    res = other_api.get(f"/api/workspaces/{space['slug']}/sidebar-preferences/")
    assert res.status_code == 403
    assert res.json() == {"error": "You don't have the required permissions."}


def test_sidebar_anon(anon_api, space):
    res = anon_api.get(f"/api/workspaces/{space['slug']}/sidebar-preferences/")
    assert res.status_code == 401
