# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""User module: profile, session, settings, email, accounts, activity, workspaces.

Covers the 16 routes of app/urls/user.py. Auth is the user `session-id`
cookie; anonymous callers get 401 everywhere except `users/session/`, which
is AllowAny and reports `is_authenticated: false`.
"""

import json
import uuid

ME_KEYS = {
    "avatar",
    "avatar_url",
    "cover_image",
    "cover_image_url",
    "date_joined",
    "display_name",
    "email",
    "first_name",
    "id",
    "is_active",
    "is_bot",
    "is_email_verified",
    "is_password_autoset",
    "last_login_medium",
    "last_login_time",
    "last_name",
    "user_timezone",
    "username",
}

# PATCH users/me/ goes through the full UserSerializer, not UserMeSerializer.
ME_UPDATE_KEYS = {
    "avatar",
    "avatar_asset",
    "bot_type",
    "cover_image",
    "cover_image_asset",
    "created_at",
    "created_location",
    "date_joined",
    "display_name",
    "email",
    "first_name",
    "id",
    "is_active",
    "is_bot",
    "is_email_valid",
    "is_email_verified",
    "is_managed",
    "is_password_autoset",
    "is_password_expired",
    "is_password_reset_required",
    "is_staff",
    "is_superuser",
    "last_active",
    "last_location",
    "last_login",
    "last_login_ip",
    "last_login_medium",
    "last_login_time",
    "last_login_uagent",
    "last_logout_ip",
    "last_logout_time",
    "last_name",
    "masked_at",
    "mobile_number",
    "token",
    "token_updated_at",
    "updated_at",
    "user_timezone",
    "username",
}

PROFILE_KEYS = {
    "background_color",
    "billing_address",
    "billing_address_country",
    "company_name",
    "created_at",
    "goals",
    "has_billing_address",
    "has_marketing_email_consent",
    "id",
    "is_app_rail_docked",
    "is_mobile_onboarded",
    "is_navigation_tour_completed",
    "is_onboarded",
    "is_smooth_cursor_enabled",
    "is_subscribed_to_changelog",
    "is_tour_completed",
    "language",
    "last_workspace_id",
    "mobile_onboarding_step",
    "mobile_timezone_auto_set",
    "notification_view_mode",
    "onboarding_step",
    "product_tour",
    "role",
    "settings",
    "start_of_the_week",
    "theme",
    "updated_at",
    "use_case",
    "user",
}

ACCOUNT_KEYS = {
    "access_token",
    "access_token_expired_at",
    "created_at",
    "id",
    "id_token",
    "last_connected_at",
    "metadata",
    "provider",
    "provider_account_id",
    "refresh_token",
    "refresh_token_expired_at",
    "updated_at",
    "user",
}

ACTIVITIES_KEYS = {
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

WORKSPACE_ITEM_KEYS = {
    "background_color",
    "created_at",
    "created_by",
    "deleted_at",
    "id",
    "logo",
    "logo_asset",
    "logo_url",
    "name",
    "organization_size",
    "owner",
    "role",
    "slug",
    "timezone",
    "total_members",
    "updated_at",
    "updated_by",
}

DASHBOARD_KEYS = {
    "assigned_issues_count",
    "completed_issues",
    "completed_issues_count",
    "issue_activities",
    "issues_due_week_count",
    "overdue_issues",
    "pending_issues_count",
    "state_distribution",
    "upcoming_issues",
}

DENIED = {"detail": "Authentication credentials were not provided."}
MISSING = {"error": "The required object does not exist."}


def test_me_retrieve_shape(user_api, world):
    res = user_api.get("/api/users/me/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == ME_KEYS
    assert body["id"] == world["user"]["id"]
    assert body["email"] == world["user"]["email"]
    assert body["first_name"] == "Contract"
    assert body["is_active"] is True


def test_me_retrieve_returns_caller_not_other(other_api, world):
    body = other_api.get("/api/users/me/").json()
    assert body["id"] == world["other"]["id"]
    assert body["email"] == "other@example.com"


def test_me_anon_denied(anon_api):
    for method, url in [
        ("get", "/api/users/me/"),
        ("patch", "/api/users/me/"),
        ("delete", "/api/users/me/"),
        ("get", "/api/users/me/settings/"),
        ("get", "/api/users/me/profile/"),
        ("get", "/api/users/me/accounts/"),
        ("get", "/api/users/me/instance-admin/"),
        ("get", "/api/users/me/activities/"),
        ("get", "/api/users/me/workspaces/"),
    ]:
        res = getattr(anon_api, method)(url)
        assert res.status_code == 401, (method, url)
        assert res.json() == DENIED


def test_me_partial_update_shape(user_api, world):
    res = user_api.patch("/api/users/me/", json={"first_name": "Ada", "last_name": "Lovelace"})
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == ME_UPDATE_KEYS
    assert body["first_name"] == "Ada"
    assert body["last_name"] == "Lovelace"
    with world["db"].connect() as conn:
        row = conn.execute(
            "select first_name, last_name from users where id = %s;", (world["user"]["id"],)
        ).fetchone()
    assert (row[0], row[1]) == ("Ada", "Lovelace")


def test_me_partial_update_rejects_url_name(user_api):
    res = user_api.patch("/api/users/me/", json={"first_name": "https://evil.example/x"})
    assert res.status_code == 400
    assert res.json() == {"first_name": ["First name cannot contain a URL."]}


def test_me_partial_update_email_is_read_only(user_api, world):
    res = user_api.patch("/api/users/me/", json={"email": "hacked@example.com"})
    assert res.status_code == 200
    assert res.json()["email"] == world["user"]["email"]


def test_session_authed_shape(user_api, world):
    res = user_api.get("/api/users/session/")
    assert res.status_code == 200
    body = res.json()
    assert body["is_authenticated"] is True
    assert set(body["user"].keys()) == ME_KEYS
    assert body["user"]["id"] == world["user"]["id"]


def test_session_anon_is_public(anon_api):
    # UserSessionEndpoint is AllowAny: no denied-permission case exists for
    # this one route; the denied floor is covered by test_me_anon_denied.
    res = anon_api.get("/api/users/session/")
    assert res.status_code == 200
    assert res.json() == {"is_authenticated": False}


def test_settings_shape_without_membership(user_api, world):
    res = user_api.get("/api/users/me/settings/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == {"email", "id", "workspace"}
    assert body["id"] == world["user"]["id"]
    assert body["workspace"] == {
        "last_workspace_id": None,
        "last_workspace_slug": None,
        "fallback_workspace_id": None,
        "fallback_workspace_slug": None,
        "invites": 0,
    }


def test_settings_shape_last_workspace(user_api, world):
    ws = world["db"].make_workspace("Ws One", "ws-one", world["user"]["id"])
    world["db"].make_workspace_member(ws["id"], world["user"]["id"], role=20)
    with world["db"].connect() as conn:
        conn.execute(
            "update profiles set last_workspace_id = %s where user_id = %s;",
            (ws["id"], world["user"]["id"]),
        )
        conn.commit()
    ws_block = user_api.get("/api/users/me/settings/").json()["workspace"]
    assert ws_block == {
        "last_workspace_id": ws["id"],
        "last_workspace_slug": "ws-one",
        "last_workspace_name": "Ws One",
        "last_workspace_logo": "",
        "fallback_workspace_id": ws["id"],
        "fallback_workspace_slug": "ws-one",
        "invites": 0,
    }


def test_settings_shape_fallback_workspace(user_api, world):
    ws = world["db"].make_workspace("Ws One", "ws-one", world["user"]["id"])
    world["db"].make_workspace_member(ws["id"], world["user"]["id"], role=20)
    ws_block = user_api.get("/api/users/me/settings/").json()["workspace"]
    assert ws_block["last_workspace_id"] is None
    assert ws_block["fallback_workspace_id"] == ws["id"]
    assert ws_block["fallback_workspace_slug"] == "ws-one"


def test_generate_code_rejects_bad_email(user_api):
    res = user_api.post("/api/users/me/email/generate-code/", json={"email": "not-an-email"})
    assert res.status_code == 400
    assert res.json() == {"error": "Invalid email format"}


def test_generate_code_rejects_current_email(user_api, world):
    res = user_api.post(
        "/api/users/me/email/generate-code/", json={"email": world["user"]["email"]}
    )
    assert res.status_code == 400
    assert res.json() == {"error": "New email must be different from current email"}


def test_generate_code_rejects_taken_email(user_api, world):
    res = user_api.post(
        "/api/users/me/email/generate-code/", json={"email": world["other"]["email"]}
    )
    assert res.status_code == 400
    assert res.json() == {"error": "An account with this email already exists"}


def test_generate_code_valid_email(user_api):
    res = user_api.post(
        "/api/users/me/email/generate-code/", json={"email": "brandnew@example.com"}
    )
    assert res.status_code == 200
    assert res.json() == {"message": "Verification code sent to email"}


def test_update_email_requires_code(user_api):
    res = user_api.patch("/api/users/me/email/", json={"email": "newaddr@example.com"})
    assert res.status_code == 400
    assert res.json() == {"error": "Verification code is required"}


def test_update_email_rejects_stale_code(user_api):
    res = user_api.patch(
        "/api/users/me/email/", json={"email": "newaddr@example.com", "code": "000000"}
    )
    assert res.status_code == 400
    assert res.json() == {"error": "Verification code has expired or is invalid"}


def test_profile_get_shape(user_api, world):
    res = user_api.get("/api/users/me/profile/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == PROFILE_KEYS
    assert body["user"] == world["user"]["id"]
    assert body["is_onboarded"] is False
    assert body["is_tour_completed"] is False
    assert body["onboarding_step"] == {
        "profile_complete": False,
        "workspace_create": False,
        "workspace_invite": False,
        "workspace_join": False,
    }


def test_profile_get_missing_row_is_404(user_api, world):
    # Raw-seeded users bypass the signup signal that creates the profile row;
    # freezing the 404 keeps the Rust port byte-compatible for that state.
    with world["db"].connect() as conn:
        conn.execute("delete from profiles where user_id = %s;", (world["user"]["id"],))
        conn.commit()
    res = user_api.get("/api/users/me/profile/")
    assert res.status_code == 404
    assert res.json() == MISSING


def test_profile_patch_theme(user_api, world):
    res = user_api.patch("/api/users/me/profile/", json={"theme": {"mode": "dark"}})
    assert res.status_code == 200
    assert res.json()["theme"] == {"mode": "dark"}
    with world["db"].connect() as conn:
        row = conn.execute(
            "select theme from profiles where user_id = %s;", (world["user"]["id"],)
        ).fetchone()
    theme = row[0] if isinstance(row[0], dict) else json.loads(row[0])
    assert theme == {"mode": "dark"}


def test_profile_patch_unknown_settings_namespace_rejected(user_api):
    res = user_api.patch("/api/users/me/profile/", json={"settings": {"nope": {"k": 1}}})
    assert res.status_code == 400
    assert res.json() == {"settings": ["unknown settings namespace: nope"]}


def test_accounts_list_empty(user_api):
    res = user_api.get("/api/users/me/accounts/")
    assert res.status_code == 200
    assert res.json() == []


def test_account_detail_shape(user_api, world):
    seed = world["db"].make_account(world["user"]["id"])
    res = user_api.get(f"/api/users/me/accounts/{seed['id']}/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == ACCOUNT_KEYS
    assert body["provider"] == "google"
    assert body["user"] == world["user"]["id"]


def test_account_detail_missing_is_404(user_api):
    res = user_api.get(f"/api/users/me/accounts/{uuid.uuid4()}/")
    assert res.status_code == 404
    assert res.json() == MISSING


def test_account_isolated_per_user(user_api, world):
    seed = world["db"].make_account(world["other"]["id"], provider_account_id="other-acc")
    assert user_api.get(f"/api/users/me/accounts/{seed['id']}/").status_code == 404
    assert user_api.delete(f"/api/users/me/accounts/{seed['id']}/").status_code == 404


def test_account_delete(user_api, world):
    seed = world["db"].make_account(world["user"]["id"])
    res = user_api.delete(f"/api/users/me/accounts/{seed['id']}/")
    assert res.status_code == 204
    with world["db"].connect() as conn:
        n = conn.execute("select count(*) from accounts where id = %s;", (seed["id"],)).fetchone()[0]
    assert n == 0


def test_instance_admin_false_by_default(user_api):
    res = user_api.get("/api/users/me/instance-admin/")
    assert res.status_code == 200
    assert res.json() == {"is_instance_admin": False}


def test_instance_admin_true_when_granted(user_api, world):
    inst = world["db"].make_instance()
    world["db"].make_admin(inst["id"], world["user"]["id"])
    res = user_api.get("/api/users/me/instance-admin/")
    assert res.status_code == 200
    assert res.json() == {"is_instance_admin": True}


def test_onboard_patch(user_api, world):
    res = user_api.patch("/api/users/me/onboard/", json={"is_onboarded": True})
    assert res.status_code == 200
    assert res.json() == {"message": "Updated successfully"}
    with world["db"].connect() as conn:
        flag = conn.execute(
            "select is_onboarded from profiles where user_id = %s;", (world["user"]["id"],)
        ).fetchone()[0]
    assert flag is True


def test_tour_patch(user_api, world):
    res = user_api.patch("/api/users/me/tour-completed/", json={"is_tour_completed": True})
    assert res.status_code == 200
    assert res.json() == {"message": "Updated successfully"}
    with world["db"].connect() as conn:
        flag = conn.execute(
            "select is_tour_completed from profiles where user_id = %s;",
            (world["user"]["id"],),
        ).fetchone()[0]
    assert flag is True


def test_activities_empty_shape(user_api):
    res = user_api.get("/api/users/me/activities/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == ACTIVITIES_KEYS
    assert body["count"] == 0
    assert body["results"] == []


def test_my_workspaces_empty(user_api):
    res = user_api.get("/api/users/me/workspaces/")
    assert res.status_code == 200
    assert res.json() == []


def test_my_workspaces_shape(user_api, world):
    ws = world["db"].make_workspace("Ws One", "ws-one", world["user"]["id"])
    world["db"].make_workspace_member(ws["id"], world["user"]["id"], role=20)
    res = user_api.get("/api/users/me/workspaces/")
    assert res.status_code == 200
    body = res.json()
    assert len(body) == 1
    assert set(body[0].keys()) == WORKSPACE_ITEM_KEYS
    assert body[0]["slug"] == "ws-one"
    assert body[0]["role"] == 20
    assert body[0]["total_members"] == 1


def test_my_workspaces_isolated(other_api, world):
    ws = world["db"].make_workspace("Ws One", "ws-one", world["user"]["id"])
    world["db"].make_workspace_member(ws["id"], world["user"]["id"], role=20)
    assert other_api.get("/api/users/me/workspaces/").json() == []


def test_graphs_empty_for_member(user_api, world):
    world["db"].make_workspace_member(
        world["db"].make_workspace("Ws One", "ws-one", world["user"]["id"])["id"],
        world["user"]["id"],
        role=20,
    )
    assert user_api.get("/api/users/me/workspaces/ws-one/activity-graph/").json() == []
    assert user_api.get("/api/users/me/workspaces/ws-one/issues-completed-graph/").json() == []


def test_dashboard_shape(user_api, world):
    world["db"].make_workspace_member(
        world["db"].make_workspace("Ws One", "ws-one", world["user"]["id"])["id"],
        world["user"]["id"],
        role=20,
    )
    res = user_api.get("/api/users/me/workspaces/ws-one/dashboard/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == DASHBOARD_KEYS
    assert body["assigned_issues_count"] == 0
    assert body["pending_issues_count"] == 0
    assert body["completed_issues_count"] == 0
    assert body["issues_due_week_count"] == 0
    assert body["issue_activities"] == []
    assert body["overdue_issues"] == []
    assert body["upcoming_issues"] == []


def test_dashboard_unknown_slug_is_zeros(user_api):
    # No membership check on these graph endpoints: an unknown slug returns
    # zeros rather than 404. Frozen so the Rust port matches byte for byte.
    res = user_api.get("/api/users/me/workspaces/nosuchslug/dashboard/")
    assert res.status_code == 200
    assert res.json()["assigned_issues_count"] == 0
    assert user_api.get("/api/users/me/workspaces/nosuchslug/activity-graph/").json() == []


def test_dashboard_cross_user_shows_caller_zeros(other_api, world):
    ws = world["db"].make_workspace("Ws One", "ws-one", world["user"]["id"])
    world["db"].make_workspace_member(ws["id"], world["user"]["id"], role=20)
    body = other_api.get("/api/users/me/workspaces/ws-one/dashboard/").json()
    assert body["assigned_issues_count"] == 0
    assert body["overdue_issues"] == []


def test_deactivate(user_api, world):
    res = user_api.delete("/api/users/me/")
    assert res.status_code == 204
    with world["db"].connect() as conn:
        row = conn.execute(
            "select is_active from users where id = %s;", (world["user"]["id"],)
        ).fetchone()
        assert row[0] is False
        n_sessions = conn.execute(
            "select count(*) from sessions where user_id = %s;", (world["user"]["id"],)
        ).fetchone()[0]
        assert n_sessions == 0
        prof = conn.execute(
            "select is_onboarded, is_tour_completed from profiles where user_id = %s;",
            (world["user"]["id"],),
        ).fetchone()
        assert (prof[0], prof[1]) == (False, False)


def test_deactivate_instance_admin_blocked(user_api, world):
    inst = world["db"].make_instance()
    world["db"].make_admin(inst["id"], world["user"]["id"])
    res = user_api.delete("/api/users/me/")
    assert res.status_code == 400
    assert res.json() == {
        "error": "You cannot deactivate your account since you are an instance admin"
    }
