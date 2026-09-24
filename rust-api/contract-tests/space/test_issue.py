"""Contract: public issue, comment, reaction and vote endpoints.

Covers ``space/urls/issue.py`` (8 paths):
retrieve, comment list/create/retrieve/patch/delete, issue-reaction
list/create/delete, comment-reaction list/create/delete, vote list/create/delete.

Permission split under test: retrieve and comment list/retrieve are ``AllowAny``;
every write goes through ``IsAuthenticated`` (session cookie), enforced twice:
the global ``DEFAULT_PERMISSION_CLASSES`` and ``BaseViewSet.permission_classes``
(which is redundant with it). The unauthenticated-write cases
(``test_list_requires_auth``, ``test_create_requires_auth``,
``test_issue_reaction_list_requires_auth``) are the suite's
denied-permission tripwire: remove both ``IsAuthenticated`` layers and they
flip from 401/403 to 200. Removing only the ``BaseViewSet`` line is a no-op —
the global default still denies — so the tripwire demo must cover both.
"""

import pytest

pytestmark = pytest.mark.contract

BASE = "/api/public"

ISSUE_RETRIEVE_KEYS = {
    "id",
    "name",
    "state_id",
    "sort_order",
    "description_json",
    "description_html",
    "description_stripped",
    "description_binary",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "created_by",
    "state__group",
    "vote_items",
    "reaction_items",
}


def _comments_url(world, comment_id=None):
    url = f"{BASE}/anchor/{world['anchor']}/issues/{world['issue']['id']}/comments/"
    return url if comment_id is None else f"{url}{comment_id}/"


def test_issue_retrieve_shape(anon_client, world):
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/issues/{world['issue']['id']}/")
    assert response.status_code == 200
    body = response.json()
    assert set(body) == ISSUE_RETRIEVE_KEYS, f"keys changed: {sorted(set(body) ^ ISSUE_RETRIEVE_KEYS)}"
    assert body["name"] == "Contract issue"
    assert body["state__group"] == "backlog"
    assert body["label_ids"] == []
    assert body["assignee_ids"] == []
    assert isinstance(body["vote_items"], list) and len(body["vote_items"]) == 1
    assert isinstance(body["reaction_items"], list) and len(body["reaction_items"]) == 1


def test_issue_retrieve_other_tenant_is_not_found(anon_client, world, seeder):
    owner2 = seeder.create_user()
    workspace2 = seeder.create_workspace(owner2["id"])
    project2 = seeder.create_project(workspace2["id"])
    board2 = seeder.create_board(workspace2["id"], project2["id"])
    state2 = seeder.create_state(workspace2["id"], project2["id"])
    issue2 = seeder.create_issue(workspace2["id"], project2["id"], state2["id"])
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/issues/{issue2['id']}/")
    assert response.status_code == 200
    # Scoped to the anchor's board: another tenant's issue renders as an
    # empty body — the view returns ``Response(None)``, which DRF renders
    # with no content rather than JSON null — never as their data.
    assert response.content == b""
    own = anon_client.get(f"{BASE}/anchor/{board2['anchor']}/issues/{issue2['id']}/")
    assert own.status_code == 200
    assert own.json()["id"] == issue2["id"]


def test_comment_list_shape_public(anon_client, world):
    response = anon_client.get(_comments_url(world))
    assert response.status_code == 200
    rows = response.json()
    assert isinstance(rows, list) and len(rows) == 1
    row = rows[0]
    for key in ("id", "comment_html", "issue", "actor", "access", "actor_detail", "is_member"):
        assert key in row, f"comment row missing {key}"
    assert row["access"] == "EXTERNAL"


def test_comment_internal_not_listed(anon_client, world, seeder):
    seeder.create_comment(
        world["workspace"]["id"],
        world["project"]["id"],
        world["issue"]["id"],
        world["owner"]["id"],
        access="INTERNAL",
    )
    rows = anon_client.get(_comments_url(world)).json()
    assert len(rows) == 1
    assert all(row["access"] == "EXTERNAL" for row in rows)


def test_comment_create_requires_auth(anon_client, world):
    response = anon_client.post(_comments_url(world), json={"comment_html": "<p>hi</p>"})
    assert response.status_code in (401, 403)


def test_comment_create_shape(user_client, world):
    response = user_client.post(_comments_url(world), json={"comment_html": "<p>public note</p>"})
    assert response.status_code == 201
    body = response.json()
    assert body["comment_html"] == "<p>public note</p>"
    assert body["access"] == "EXTERNAL"


def test_comment_retrieve_patch_delete_owned(user_client, world):
    comment_id = world["comment"]["id"]
    got = user_client.get(_comments_url(world, comment_id))
    assert got.status_code == 200
    assert got.json()["id"] == comment_id

    patched = user_client.patch(_comments_url(world, comment_id), json={"comment_html": "<p>edited</p>"})
    assert patched.status_code == 200
    assert patched.json()["comment_html"] == "<p>edited</p>"

    deleted = user_client.delete(_comments_url(world, comment_id))
    assert deleted.status_code == 204


def test_comment_patch_by_other_user_is_not_found(other_client, world):
    response = other_client.patch(
        _comments_url(world, world["comment"]["id"]), json={"comment_html": "<p>hijack</p>"}
    )
    # Update/delete are scoped to ``actor=request.user``: someone else's
    # comment is invisible, not forbidden.
    assert response.status_code == 404


def test_comment_write_when_disabled_is_400(user_client, world, seeder):
    # One live project board per project (unique constraint on
    # deploy_boards(entity_name, entity_identifier) where deleted_at is null),
    # so the comments-disabled board lives on a fresh project with its own
    # state and issue — the 400 fires before the issue is even read.
    project = seeder.create_project(world["workspace"]["id"])
    board = seeder.create_board(world["workspace"]["id"], project["id"], comments=False)
    state = seeder.create_state(world["workspace"]["id"], project["id"])
    issue = seeder.create_issue(world["workspace"]["id"], project["id"], state["id"])
    url = f"{BASE}/anchor/{board['anchor']}/issues/{issue['id']}/comments/"
    response = user_client.post(url, json={"comment_html": "<p>x</p>"})
    assert response.status_code == 400
    assert response.json() == {"error": "Comments are not enabled for this project"}


def test_issue_reaction_create_and_delete(user_client, world):
    url = f"{BASE}/anchor/{world['anchor']}/issues/{world['issue']['id']}/reactions/"
    created = user_client.post(url, json={"reaction": "rocket"})
    assert created.status_code == 201
    assert created.json()["reaction"] == "rocket"

    deleted = user_client.delete(url + "rocket/")
    assert deleted.status_code == 204


def test_issue_reaction_list_requires_auth(anon_client, world):
    response = anon_client.get(f"{BASE}/anchor/{world['anchor']}/issues/{world['issue']['id']}/reactions/")
    assert response.status_code in (401, 403)


def test_issue_reaction_list_shape(user_client, world):
    # NOTE (ported bug): the list queryset filters on ``slug``/``project_id``
    # kwargs the URL never provides, so it always returns []. Pinned as-is.
    response = user_client.get(
        f"{BASE}/anchor/{world['anchor']}/issues/{world['issue']['id']}/reactions/"
    )
    assert response.status_code == 200
    assert response.json() == []


def test_comment_reaction_create_and_delete(user_client, world):
    url = f"{BASE}/anchor/{world['anchor']}/comments/{world['comment']['id']}/reactions/"
    created = user_client.post(url, json={"reaction": "eyes"})
    assert created.status_code == 201
    assert created.json()["reaction"] == "eyes"

    listed = user_client.get(url)
    assert listed.status_code == 200
    assert {row["reaction"] for row in listed.json()} >= {"heart", "eyes"}

    deleted = user_client.delete(url + "eyes/")
    assert deleted.status_code == 204


def test_vote_upsert_and_delete(user_client, world):
    url = f"{BASE}/anchor/{world['anchor']}/issues/{world['issue']['id']}/votes/"
    created = user_client.post(url, json={"vote": 1})
    assert created.status_code == 201
    body = created.json()
    for key in ("issue", "vote", "workspace", "project", "actor", "actor_detail"):
        assert key in body, f"vote missing {key}"
    assert body["vote"] == 1

    deleted = user_client.delete(url)
    assert deleted.status_code == 204


def test_vote_list_shape(user_client, world):
    # NOTE (ported bug): like issue reactions, the vote list filters on a
    # ``slug`` kwarg the URL never provides, so it always returns []. Pinned.
    response = user_client.get(f"{BASE}/anchor/{world['anchor']}/issues/{world['issue']['id']}/votes/")
    assert response.status_code == 200
    assert response.json() == []
