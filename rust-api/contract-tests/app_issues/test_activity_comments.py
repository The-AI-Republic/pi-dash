"""Activity + comments + reactions + subscribers shapes.

Seed: one activity row and comment C1 (admin, with a member +1 reaction)
on I1; member issue-reaction (rocket) and subscription on I1.

Order matters in this file: all C1-dependent reads run before the
delete tests at the bottom. The member-created comment in
``test_member_cannot_delete_comment`` cannot be removed through the API
(delete needs ADMIN + creator) and lingers; list assertions below it use
superset checks.
"""

ACTIVITY_KEYS = {
    "id", "actor_detail", "issue_detail", "project_detail",
    "workspace_detail", "source_data", "created_at", "updated_at",
    "deleted_at", "verb", "field", "old_value", "new_value", "comment",
    "attachments", "old_identifier", "new_identifier", "epoch",
    "created_by", "updated_by", "project", "workspace",
    "issue", "issue_comment", "actor",
}

# List + retrieve annotate ``is_member``; create and the history
# issue-comment branch serialize without it.
COMMENT_WRITE_KEYS = {
    "id", "actor_detail", "issue_detail", "project_detail",
    "workspace_detail", "comment_reactions", "is_synced", "created_at",
    "updated_at", "deleted_at", "comment_stripped", "comment_json",
    "comment_html", "attachments", "labels", "access",
    "external_source", "external_id", "speaker_type", "speaker_label",
    "speaker_agent_run_id", "edited_at", "created_by", "updated_by",
    "project", "workspace", "issue",
    "description", "parent", "actor",
}

COMMENT_KEYS = COMMENT_WRITE_KEYS | {"is_member"}

FORBIDDEN = {"error": "You don't have the required permissions."}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _issue(ws, pid, iid):
    return f"{_base(ws, pid)}/issues/{iid}"


def test_history_default_500s(clients, seed):
    # Ported bug: the unfiltered history branch sorts raw model
    # instances with ``instance["created_at"]`` (``activity.py``), which
    # raises ``TypeError`` on every issue that has any activity or
    # comment. The filtered branches below work.
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(f"{_issue(ws, pid, i1)}/history/")
    assert resp.status_code == 500
    assert resp.json() == {"error": "Something went wrong please try again later"}


def test_history_issue_property_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(f"{_issue(ws, pid, i1)}/history/?activity_type=issue-property")
    assert resp.status_code == 200
    rows = resp.json()
    assert len(rows) == 1
    assert set(rows[0]) == ACTIVITY_KEYS
    assert rows[0]["verb"] == "updated"
    assert rows[0]["field"] == "priority"
    assert rows[0]["old_value"] == "low"
    assert rows[0]["new_value"] == "high"
    assert rows[0]["actor_detail"]["display_name"] == "is_admin"
    assert rows[0]["issue_detail"]["name"] == "I1 parent"


def test_history_issue_comment_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, c1 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["comment1"])
    resp = admin.get(f"{_issue(ws, pid, i1)}/history/?activity_type=issue-comment")
    assert resp.status_code == 200
    rows = resp.json()
    assert len(rows) == 1
    assert set(rows[0]) == COMMENT_WRITE_KEYS
    assert rows[0]["id"] == c1
    assert rows[0]["comment_html"] == "<p>seed comment</p>"
    assert rows[0]["speaker_type"] == "human"
    reactions = rows[0]["comment_reactions"]
    assert len(reactions) == 1
    assert reactions[0]["reaction"] == "+1"
    assert reactions[0]["display_name"] == "is_member"


def test_comment_list_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, c1 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["comment1"])
    resp = admin.get(f"{_issue(ws, pid, i1)}/comments/")
    assert resp.status_code == 200
    rows = resp.json()
    assert len(rows) == 1
    assert set(rows[0]) == COMMENT_KEYS
    assert rows[0]["id"] == c1


def test_comment_retrieve_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, c1 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["comment1"])
    resp = admin.get(f"{_issue(ws, pid, i1)}/comments/{c1}/")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body) == COMMENT_KEYS
    assert body["comment_stripped"] == "seed comment"
    assert body["is_member"] is True


def test_comment_reaction_roundtrip(clients, seed):
    member = clients["member"]
    ws, pid2, c1 = seed["ws_slug"], seed["project"], seed["comment1"]
    url = f"{_base(ws, pid2)}/comments/{c1}/reactions/"
    rows = member.get(url).json()
    assert [r["reaction"] for r in rows] == ["+1"]
    assert member.delete(f"{url}+1/").status_code == 204
    assert member.get(url).json() == []
    resp = member.post(url, json={"reaction": "+1"})
    assert resp.status_code == 201
    assert resp.json()["reaction"] == "+1"
    assert resp.json()["comment"] == c1
    resp = member.post(url, json={"reaction": "+1"})
    assert resp.status_code == 400
    assert resp.json() == {"error": "Reaction already exists for the user"}
    assert [r["reaction"] for r in member.get(url).json()] == ["+1"]


def test_issue_reaction_roundtrip(clients, seed):
    member = clients["member"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    url = f"{_issue(ws, pid, i1)}/reactions/"
    rows = member.get(url).json()
    assert [r["reaction"] for r in rows] == ["rocket"]
    assert member.delete(f"{url}rocket/").status_code == 204
    assert member.get(url).json() == []
    resp = member.post(url, json={"reaction": "rocket"})
    assert resp.status_code == 201
    assert resp.json()["reaction"] == "rocket"
    # Duplicate issue reactions surface the generic integrity body
    # (unlike comment reactions, this view does not catch it).
    resp = member.post(url, json={"reaction": "rocket"})
    assert resp.status_code == 400
    assert resp.json() == {"error": "The payload is not valid"}
    assert [r["reaction"] for r in member.get(url).json()] == ["rocket"]


def test_comment_create_delete_cycle(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.post(
        f"{_issue(ws, pid, i1)}/comments/",
        json={"comment_html": "<p>cycle note</p>",
              "comment_stripped": "cycle note"},
    )
    assert resp.status_code == 201
    body = resp.json()
    assert set(body) == COMMENT_WRITE_KEYS
    assert body["comment_html"] == "<p>cycle note</p>"
    assert body["actor_detail"]["display_name"] == "is_admin"
    comment_id = body["id"]
    try:
        assert admin.get(
            f"{_issue(ws, pid, i1)}/comments/{comment_id}/").status_code == 200
    finally:
        assert admin.delete(
            f"{_issue(ws, pid, i1)}/comments/{comment_id}/").status_code == 204
    assert comment_id not in {
        row["id"] for row in
        admin.get(f"{_issue(ws, pid, i1)}/comments/").json()
    }


def test_comment_delete_semantics(clients, seed):
    # The creator bypasses the role gate, so a member can delete their
    # own comment but not someone else's; an admin (project ADMIN role)
    # can delete either.
    admin, member = clients["admin"], clients["member"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    member_comment = member.post(
        f"{_issue(ws, pid, i1)}/comments/",
        json={"comment_html": "<p>member note</p>",
              "comment_stripped": "member note"},
    ).json()["id"]
    admin_comment = admin.post(
        f"{_issue(ws, pid, i1)}/comments/",
        json={"comment_html": "<p>admin note</p>",
              "comment_stripped": "admin note"},
    ).json()["id"]
    try:
        resp = member.delete(
            f"{_issue(ws, pid, i1)}/comments/{admin_comment}/")
        assert resp.status_code == 403
        assert resp.json() == FORBIDDEN
        assert member.delete(
            f"{_issue(ws, pid, i1)}/comments/{member_comment}/"
        ).status_code == 204
        assert admin.delete(
            f"{_issue(ws, pid, i1)}/comments/{admin_comment}/"
        ).status_code == 204
    finally:
        for comment_id in (member_comment, admin_comment):
            admin.request(
                "DELETE", f"{_issue(ws, pid, i1)}/comments/{comment_id}/")
    rows = admin.get(f"{_issue(ws, pid, i1)}/comments/").json()
    assert {member_comment, admin_comment} - {row["id"] for row in rows} == {
        member_comment, admin_comment}


def test_guest_comment_on_foreign_issue_is_400(clients, seed):
    guest = clients["guest"]
    ws, p2, j1 = seed["ws_slug"], seed["project2"], seed["j1"]
    resp = guest.post(
        f"{_issue(ws, p2, j1)}/comments/",
        json={"comment_html": "<p>hi</p>", "comment_stripped": "hi"},
    )
    assert resp.status_code == 400
    assert resp.json() == {"error": "You are not allowed to comment on the issue"}


def test_comment_put_and_patch(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, c1 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["comment1"])
    try:
        resp = admin.put(
            f"{_issue(ws, pid, i1)}/comments/{c1}/",
            json={"comment_html": "<p>put</p>", "comment_stripped": "put"},
        )
        assert resp.status_code == 200
        assert set(resp.json()) == COMMENT_KEYS
        assert resp.json()["comment_html"] == "<p>put</p>"
        resp = admin.patch(
            f"{_issue(ws, pid, i1)}/comments/{c1}/",
            json={"comment_html": "<p>seed comment</p>",
                  "comment_stripped": "seed comment"},
        )
        assert resp.status_code == 200
        assert resp.json()["comment_html"] == "<p>seed comment</p>"
    finally:
        admin.patch(
            f"{_issue(ws, pid, i1)}/comments/{c1}/",
            json={"comment_html": "<p>seed comment</p>",
                  "comment_stripped": "seed comment"},
        )


def test_comment_patch_member_denied(clients, seed):
    member = clients["member"]
    ws, pid, i1, c1 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["comment1"])
    resp = member.patch(
        f"{_issue(ws, pid, i1)}/comments/{c1}/",
        json={"comment_html": "<p>hijack</p>"},
    )
    assert resp.status_code == 403
    assert resp.json() == FORBIDDEN


def test_subscriber_create_validation(clients, seed):
    # The subscriber serializer takes every model field, so a bare
    # subscriber id is rejected; explicit nulls create the row.
    admin = clients["admin"]
    ws, pid, i2 = seed["ws_slug"], seed["project"], seed["issue2"]
    resp = admin.post(
        f"{_issue(ws, pid, i2)}/issue-subscribers/",
        json={"subscriber": str(seed["guest"])},
    )
    assert resp.status_code == 400
    assert resp.json() == {"deleted_at": ["This field is required."]}
    resp = admin.post(
        f"{_issue(ws, pid, i2)}/issue-subscribers/",
        json={"subscriber": str(seed["guest"]), "deleted_at": None},
    )
    assert resp.status_code == 201
    assert resp.json()["subscriber"] == seed["guest"]
    assert admin.request(
        "DELETE",
        f"{_issue(ws, pid, i2)}/issue-subscribers/{seed['guest']}/"
    ).status_code == 204


def test_comment_delete_by_creator(clients, seed):
    # Runs last among the C1 readers: removes the seeded comment.
    admin = clients["admin"]
    ws, pid, i1, c1 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["comment1"])
    assert admin.delete(
        f"{_issue(ws, pid, i1)}/comments/{c1}/").status_code == 204
    assert c1 not in {
        row["id"] for row in
        admin.get(f"{_issue(ws, pid, i1)}/comments/").json()
    }


def test_subscribe_roundtrip(clients, seed):
    admin, member = clients["admin"], clients["member"]
    ws, pid, i2 = seed["ws_slug"], seed["project"], seed["issue2"]
    resp = member.post(f"{_issue(ws, pid, i2)}/subscribe/")
    assert resp.status_code == 201
    assert set(resp.json()) == {
        "id", "created_at", "updated_at", "deleted_at", "created_by",
        "updated_by", "project", "workspace", "issue", "subscriber",
    }
    assert resp.json()["subscriber"] == seed["member"]
    resp = member.post(f"{_issue(ws, pid, i2)}/subscribe/")
    assert resp.status_code == 400
    assert resp.json() == {"message": "User already subscribed to the issue."}
    assert member.get(
        f"{_issue(ws, pid, i2)}/subscribe/").json() == {"subscribed": True}
    # The subscribers list carries project members, not subscriptions.
    members = admin.get(f"{_issue(ws, pid, i2)}/issue-subscribers/").json()
    assert {m["member"]["display_name"] for m in members} >= {
        "is_admin", "is_member", "is_guest"}
    assert member.delete(f"{_issue(ws, pid, i2)}/subscribe/").status_code == 204
    assert member.get(
        f"{_issue(ws, pid, i2)}/subscribe/").json() == {"subscribed": False}


def test_subscriber_destroy_by_id(clients, seed):
    admin = clients["admin"]
    ws, pid, i2 = seed["ws_slug"], seed["project"], seed["issue2"]
    assert admin.post(
        f"{_issue(ws, pid, i2)}/subscribe/").status_code == 201
    assert admin.request(
        "DELETE",
        f"{_issue(ws, pid, i2)}/issue-subscribers/{seed['admin']}/"
    ).status_code == 204
    assert admin.get(
        f"{_issue(ws, pid, i2)}/subscribe/").json() == {"subscribed": False}
