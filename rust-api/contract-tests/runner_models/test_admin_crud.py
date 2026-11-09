# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Instance-admin CRUD: `/api/instances/runner-models/` (NEWPIDASH-169, DIFF-8).

Instance admins only: anonymous is 401, a signed-in non-admin is 403.
Validation (known agent, non-empty model/label, no duplicate model per
agent, efforts in the allowed set) answers 400. Setting
`is_preselected` clears it on the agent's other rows. Deleting a model
leaves runners alone (their model is stored in their own config, which
these tests never touch). New-system-only: Django answers 404; Rust
pins the contract below.
"""

from _harness.diffs import expect, is_rust

ADMIN_PATH = "/api/instances/runner-models/"


def test_anon_admin_list_is_401(anon_api):
    res = anon_api.get(ADMIN_PATH)
    assert res.status_code == expect("DIFF-8", django=404, rust=401)
    if is_rust():
        assert res.json() == {"detail": "Authentication credentials were not provided."}


def test_member_admin_list_is_403(member_admin_api):
    res = member_admin_api.get(ADMIN_PATH)
    assert res.status_code == expect("DIFF-8", django=404, rust=403)
    if is_rust():
        assert res.json() == {"detail": "You do not have permission to perform this action."}


def test_member_admin_create_is_403(member_admin_api):
    res = member_admin_api.post(
        ADMIN_PATH,
        json={"agent": "grok", "model": "grok-x", "label": "Grok X"},
    )
    assert res.status_code == expect("DIFF-8", django=404, rust=403)


def test_admin_list_returns_all_rows(admin_api):
    res = admin_api.get(ADMIN_PATH)
    assert res.status_code == expect("DIFF-8", django=404, rust=200)
    if not is_rust():
        return
    agents = res.json()["agents"]
    total = sum(len(rows) for rows in agents.values())
    assert total == 27


def test_create_then_read_back(admin_api):
    res = admin_api.post(
        ADMIN_PATH,
        json={
            "agent": "codex",
            "model": "gpt-9-test",
            "label": "GPT-9 Test",
            "reasoning_efforts": ["low", "high"],
            "sort_order": 99,
        },
    )
    assert res.status_code == expect("DIFF-8", django=404, rust=201)
    if not is_rust():
        return
    created = res.json()
    assert created["agent"] == "codex"
    assert created["model"] == "gpt-9-test"
    assert created["reasoning_efforts"] == ["low", "high"]
    assert created["is_enabled"] is True
    assert created["is_preselected"] is False

    res = admin_api.get(ADMIN_PATH)
    codex = res.json()["agents"]["codex"]
    assert any(row["model"] == "gpt-9-test" for row in codex)


def test_create_validation(admin_api):
    res = admin_api.post(
        ADMIN_PATH, json={"agent": "nope", "model": "m", "label": "M"}
    )
    assert res.status_code == expect("DIFF-8", django=404, rust=400)
    if not is_rust():
        return
    assert res.json() == {"error": "unknown_agent"}

    res = admin_api.post(
        ADMIN_PATH, json={"agent": "codex", "model": "  ", "label": "M"}
    )
    assert res.status_code == 400
    assert res.json() == {"error": "empty_model"}

    res = admin_api.post(
        ADMIN_PATH, json={"agent": "codex", "model": "m", "label": ""}
    )
    assert res.status_code == 400
    assert res.json() == {"error": "empty_label"}

    res = admin_api.post(
        ADMIN_PATH,
        json={
            "agent": "codex",
            "model": "m",
            "label": "M",
            "reasoning_efforts": ["low", "ultra"],
        },
    )
    assert res.status_code == 400
    assert res.json() == {"error": "invalid_effort"}

    res = admin_api.post(
        ADMIN_PATH,
        json={"agent": "codex", "model": "gpt-5.5", "label": "Dupe"},
    )
    assert res.status_code == 400
    assert res.json() == {"error": "duplicate_model"}


def test_patch_reorder_and_rename(admin_api):
    created = admin_api.post(
        ADMIN_PATH,
        json={"agent": "grok", "model": "grok-p", "label": "Grok P"},
    )
    assert created.status_code == expect("DIFF-8", django=404, rust=201)
    if not is_rust():
        return
    row_id = created.json()["id"]

    res = admin_api.patch(
        f"{ADMIN_PATH}{row_id}/", json={"label": "Grok P Prime", "sort_order": 50}
    )
    assert res.status_code == 200
    patched = res.json()
    assert patched["label"] == "Grok P Prime"
    assert patched["sort_order"] == 50

    # Renaming onto another row's model in the same agent fails.
    res = admin_api.patch(f"{ADMIN_PATH}{row_id}/", json={"model": "grok-4.3"})
    assert res.status_code == 400
    assert res.json() == {"error": "duplicate_model"}


def test_patch_unknown_id_is_404(admin_api):
    res = admin_api.patch(
        f"{ADMIN_PATH}00000000-0000-4000-8000-000000000000/",
        json={"label": "Nope"},
    )
    # 404 on both backends (resolver 404 on Django, not_found on Rust).
    assert res.status_code == 404
    if is_rust():
        assert res.json() == {"error": "not_found"}


def test_preselect_clears_others(admin_api):
    # Cursor starts with no preselected row.
    res = admin_api.get(ADMIN_PATH)
    assert res.status_code == expect("DIFF-8", django=404, rust=200)
    if not is_rust():
        return
    cursor = res.json()["agents"]["cursor-agent"]
    assert not any(row["is_preselected"] for row in cursor)
    first = next(row for row in cursor if row["model"] == "auto")

    res = admin_api.patch(
        f"{ADMIN_PATH}{first['id']}/", json={"is_preselected": True}
    )
    assert res.status_code == 200
    assert res.json()["is_preselected"] is True

    # Preselecting a second row clears the first.
    second = next(row for row in cursor if row["model"] == "composer-2.5")
    res = admin_api.patch(
        f"{ADMIN_PATH}{second['id']}/", json={"is_preselected": True}
    )
    assert res.status_code == 200

    res = admin_api.get(ADMIN_PATH)
    cursor = res.json()["agents"]["cursor-agent"]
    preselected = [row["model"] for row in cursor if row["is_preselected"]]
    assert preselected == ["composer-2.5"]


def test_delete_removes_row(admin_api):
    created = admin_api.post(
        ADMIN_PATH,
        json={"agent": "grok", "model": "grok-d", "label": "Grok D"},
    )
    assert created.status_code == expect("DIFF-8", django=404, rust=201)
    if not is_rust():
        return
    row_id = created.json()["id"]

    res = admin_api.delete(f"{ADMIN_PATH}{row_id}/")
    assert res.status_code == 204

    res = admin_api.get(ADMIN_PATH)
    models = [row["model"] for row in res.json()["agents"]["grok"]]
    assert "grok-d" not in models

    # Deleting again is 404.
    res = admin_api.delete(f"{ADMIN_PATH}{row_id}/")
    assert res.status_code == 404
