# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Instance workspaces: slug check, paginated list, create + validation."""

import hashlib


def test_slug_check_requires_slug(admin_api):
    res = admin_api.get("/api/instances/workspace-slug-check/")
    assert res.status_code == 400
    assert res.json() == {"error": "Workspace Slug is required"}


def test_slug_check_free_slug(admin_api):
    assert admin_api.get("/api/instances/workspace-slug-check/", params={"slug": "fresh-slug"}).json() == {
        "status": True
    }


def test_slug_check_taken_slug_is_case_insensitive(admin_api, world):
    world["db"].make_workspace("Taken", "Taken-Slug", world["admin"]["id"])
    assert admin_api.get(
        "/api/instances/workspace-slug-check/", params={"slug": "taken-slug"}
    ).json() == {"status": False}


def test_slug_check_restricted_slug(admin_api):
    assert admin_api.get("/api/instances/workspace-slug-check/", params={"slug": "api"}).json() == {
        "status": False
    }


def test_list_workspaces_shape(admin_api, world):
    ws = world["db"].make_workspace("Acme", "acme", world["admin"]["id"])
    world["db"].make_workspace_member(ws["id"], world["admin"]["id"])
    res = admin_api.get("/api/instances/workspaces/")
    assert res.status_code == 200
    body = res.json()
    assert set(body.keys()) == {
        "grouped_by",
        "sub_grouped_by",
        "total_count",
        "next_cursor",
        "prev_cursor",
        "next_page_results",
        "prev_page_results",
        "count",
        "total_pages",
        "total_results",
        "extra_stats",
        "results",
    }
    (entry,) = [w for w in body["results"] if w["slug"] == "acme"]
    assert entry["name"] == "Acme"
    assert entry["total_members"] == 1
    assert entry["total_projects"] == 0
    assert entry["owner"]["id"] == world["admin"]["id"]


def test_list_workspaces_search(admin_api, world):
    world["db"].make_workspace("Acme Corp", "acme", world["admin"]["id"])
    world["db"].make_workspace("Other", "other", world["admin"]["id"])
    body = admin_api.get("/api/instances/workspaces/", params={"search": "acme"}).json()
    assert {w["slug"] for w in body["results"]} == {"acme"}


def test_create_workspace(admin_api, world):
    res = admin_api.post(
        "/api/instances/workspaces/", json={"name": "New Space", "slug": "new-space"}
    )
    assert res.status_code == 201
    assert res.json()["slug"] == "new-space"
    with world["db"].connect() as conn:
        ws = conn.execute("select id, owner_id from workspaces where slug = 'new-space';").fetchone()
        members = conn.execute(
            "select count(*) from workspace_members where workspace_id = %s and member_id = %s;",
            (ws[0], world["admin"]["id"]),
        ).fetchone()[0]
    assert str(ws[1]) == world["admin"]["id"]
    assert members == 1


def test_create_workspace_seeds_builtin_schedulers(admin_api, world):
    # NEWPIDASH-256: builtin catalog empty → no rows seeded.
    res = admin_api.post(
        "/api/instances/workspaces/", json={"name": "Seeded", "slug": "seeded"}
    )
    assert res.status_code == 201
    with world["db"].connect() as conn:
        rows = conn.execute(
            "select slug, name, description, prompt, source, is_enabled, color,"
            " created_by_id, updated_by_id, deleted_at"
            " from schedulers where workspace_id = %s order by slug;",
            (res.json()["id"],),
        ).fetchall()
    assert rows == []


def test_create_workspace_requires_name_and_slug(admin_api):
    res = admin_api.post("/api/instances/workspaces/", json={"name": "Only Name"})
    assert res.status_code == 400
    assert res.json() == {"error": "Both name and slug are required"}


def test_create_workspace_rejects_overlong_fields(admin_api):
    res = admin_api.post(
        "/api/instances/workspaces/", json={"name": "n" * 81, "slug": "s" * 49}
    )
    assert res.status_code == 400
    assert res.json() == {"error": "The maximum length for name is 80 and for slug is 48"}


def test_create_workspace_rejects_restricted_slug(admin_api):
    res = admin_api.post("/api/instances/workspaces/", json={"name": "Api", "slug": "api"})
    assert res.status_code == 400


def test_create_workspace_rejects_duplicate_slug(admin_api, world):
    world["db"].make_workspace("Acme", "acme", world["admin"]["id"])
    res = admin_api.post("/api/instances/workspaces/", json={"name": "Acme 2", "slug": "ACME"})
    assert res.status_code == 400


def test_create_workspace_slug_held_by_soft_deleted_row_returns_409(admin_api, world):
    # PIDASHCONV-1100: a queryset-style soft delete keeps the slug, so the
    # live-row-only validators pass and the INSERT collides on UNIQUE(slug).
    # Django's str(IntegrityError) carries the Postgres DETAIL line
    # ("Key (slug)=(acme) already exists."), so the view answers 409.
    world["db"].make_workspace("Acme", "acme", world["admin"]["id"])
    with world["db"].connect() as conn:
        with conn.cursor() as cur:
            cur.execute("UPDATE workspaces SET deleted_at = now() WHERE slug = 'acme';")
        conn.commit()
    res = admin_api.post("/api/instances/workspaces/", json={"name": "x", "slug": "acme"})
    assert res.status_code == 409
    assert res.json() == {"slug": "The workspace with the slug already exists"}
