# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Public catalog read: `GET /api/runners/model-catalog/` (NEWPIDASH-169, DIFF-8).

New-system-only feature: Django has no such path (resolver 404), Rust
serves the contract below. Statuses pin both arms with the DIFF-8
marker; bodies are Rust-only (Django's 404 page is not a contract).
"""

from _harness.diffs import expect, is_rust

CATALOG_PATH = "/api/runners/model-catalog/"
ADMIN_PATH = "/api/instances/runner-models/"


def test_anon_catalog_is_401(anon_api):
    res = anon_api.get(CATALOG_PATH)
    assert res.status_code == expect("DIFF-8", django=404, rust=401)
    if is_rust():
        assert res.json() == {"detail": "Authentication credentials were not provided."}


def test_member_catalog_returns_grouped_seed(member_api):
    res = member_api.get(CATALOG_PATH)
    assert res.status_code == expect("DIFF-8", django=404, rust=200)
    if not is_rust():
        return
    agents = res.json()["agents"]
    assert sorted(agents) == sorted(
        ["claude-code", "codex", "cursor-agent", "open-claw", "grok", "muse-code"]
    )
    assert len(agents["claude-code"]) == 6
    assert len(agents["codex"]) == 7
    assert len(agents["cursor-agent"]) == 12
    assert len(agents["grok"]) == 2
    assert agents["open-claw"] == []
    assert agents["muse-code"] == []


def test_member_catalog_preselect_is_opus(member_api):
    res = member_api.get(CATALOG_PATH)
    assert res.status_code == expect("DIFF-8", django=404, rust=200)
    if not is_rust():
        return
    rows = res.json()["agents"]["claude-code"]
    preselected = [row for row in rows if row["is_preselected"]]
    assert len(preselected) == 1
    assert preselected[0]["model"] == "claude-opus-4-8"
    assert preselected[0]["label"] == "Opus 4.8"


def test_member_catalog_rows_in_sort_order(member_api):
    res = member_api.get(CATALOG_PATH)
    assert res.status_code == expect("DIFF-8", django=404, rust=200)
    if not is_rust():
        return
    agents = res.json()["agents"]
    for agent, rows in agents.items():
        orders = [(row["sort_order"], row["model"]) for row in rows]
        assert orders == sorted(orders), agent
        # Codex rows carry the four efforts; the rest carry none.
        for row in rows:
            if agent == "codex":
                assert row["reasoning_efforts"] == ["low", "medium", "high", "xhigh"]
            else:
                assert row["reasoning_efforts"] == []
            assert row["is_enabled"] is True


def test_disabled_row_hidden_from_public(admin_api, member_api):
    created = admin_api.post(
        ADMIN_PATH,
        json={"agent": "grok", "model": "grok-new-1", "label": "Grok New 1"},
    )
    assert created.status_code == expect("DIFF-8", django=404, rust=201)
    if not is_rust():
        return
    row_id = created.json()["id"]

    disabled = admin_api.patch(
        f"{ADMIN_PATH}{row_id}/", json={"is_enabled": False}
    )
    assert disabled.status_code == 200
    assert disabled.json()["is_enabled"] is False

    res = member_api.get(CATALOG_PATH)
    assert res.status_code == 200
    models = [row["model"] for row in res.json()["agents"]["grok"]]
    assert "grok-new-1" not in models
    # Admin list still sees the disabled row.
    admin_rows = admin_api.get(ADMIN_PATH)
    assert admin_rows.status_code == 200
    grok = admin_rows.json()["agents"]["grok"]
    assert any(row["model"] == "grok-new-1" and row["is_enabled"] is False for row in grok)


def test_app_cookie_is_anonymous_on_admin_paths(world):
    """The app session cookie does not authorize admin paths: 401."""
    from _harness import user_client

    with user_client(world["member_key"]) as client:
        res = client.get(ADMIN_PATH)
    assert res.status_code == expect("DIFF-8", django=404, rust=401)
