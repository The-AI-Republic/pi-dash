"""Relations + links shapes (relations, links, PRs, code reviews).

Seed: I1 blocked_by I3, I1 relates_to I2; one link, one PR link and one
code-review link on I1. List-shape tests run before the destroy tests at
the bottom of this file; creations use throwaway issues and are removed.
"""

BUCKETS = {
    "blocking", "blocked_by", "duplicate", "relates_to", "start_after",
    "start_before", "finish_after", "finish_before",
}

REL_ROW_KEYS = {
    "id", "name", "state_id", "sort_order", "priority", "sequence_id",
    "project_id", "label_ids", "assignee_ids", "created_at", "updated_at",
    "created_by", "updated_by", "relation_type",
}

LINK_KEYS = {
    "id", "created_by_detail", "created_at", "updated_at", "deleted_at",
    "title", "url", "metadata", "created_by", "updated_by", "project",
    "workspace", "issue",
}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def test_relation_list_buckets(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, i2, i3 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["issue2"],
        seed["issue3"])
    resp = admin.get(f"{_base(ws, pid)}/issues/{i1}/issue-relation/")
    assert resp.status_code == 200
    body = resp.json()
    assert set(body) == BUCKETS
    assert body["blocking"] == []
    assert body["duplicate"] == []
    assert body["start_after"] == []
    blocked = body["blocked_by"]
    assert len(blocked) == 1
    assert set(blocked[0]) == REL_ROW_KEYS
    assert blocked[0]["id"] == i3
    assert blocked[0]["relation_type"] == "blocked_by"
    related = body["relates_to"]
    assert len(related) == 1
    assert related[0]["id"] == i2
    assert related[0]["relation_type"] == "relates_to"


def test_relation_create_requires_type(clients, seed):
    admin = clients["admin"]
    ws, pid, i1, i2 = (
        seed["ws_slug"], seed["project"], seed["issue1"], seed["issue2"])
    resp = admin.post(
        f"{_base(ws, pid)}/issues/{i1}/issue-relation/",
        json={"issues": [i2]},
    )
    assert resp.status_code == 400
    assert resp.json() == {"message": "Issue relation type is required"}


def test_relation_create_and_remove_roundtrip(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    a = admin.post(
        f"{_base(ws, pid)}/issues/", json={"name": "Rel A"}).json()
    b = admin.post(
        f"{_base(ws, pid)}/issues/", json={"name": "Rel B"}).json()
    try:
        resp = admin.post(
            f"{_base(ws, pid)}/issues/{a['id']}/issue-relation/",
            json={"relation_type": "relates_to", "issues": [b["id"]]},
        )
        assert resp.status_code == 201
        created = resp.json()
        assert isinstance(created, list) and len(created) == 1
        assert set(created[0]) == {
            "id", "project_id", "sequence_id", "relation_type", "name",
            "state_id", "priority", "created_by", "created_at",
            "updated_at", "updated_by",
        }
        assert created[0]["relation_type"] == "relates_to"
        listing = admin.get(
            f"{_base(ws, pid)}/issues/{a['id']}/issue-relation/").json()
        assert {row["id"] for row in listing["relates_to"]} == {b["id"]}
        resp = admin.post(
            f"{_base(ws, pid)}/issues/{a['id']}/remove-relation/",
            json={"related_issue": b["id"]},
        )
        assert resp.status_code == 204
        listing = admin.get(
            f"{_base(ws, pid)}/issues/{a['id']}/issue-relation/").json()
        assert listing["relates_to"] == []
    finally:
        for issue_id in (a["id"], b["id"]):
            assert admin.delete(
                f"{_base(ws, pid)}/issues/{issue_id}/").status_code == 204


def test_relation_blocking_branch(clients, seed):
    # The blocking/start_after/finish_after branch stores the mapped
    # relation type with swapped sides and serializes related issues.
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    a = admin.post(
        f"{_base(ws, pid)}/issues/", json={"name": "Blk A"}).json()
    b = admin.post(
        f"{_base(ws, pid)}/issues/", json={"name": "Blk B"}).json()
    try:
        resp = admin.post(
            f"{_base(ws, pid)}/issues/{a['id']}/issue-relation/",
            json={"relation_type": "blocking", "issues": [b["id"]]},
        )
        assert resp.status_code == 201
        created = resp.json()
        assert len(created) == 1
        assert created[0]["relation_type"] == "blocked_by"
        listing = admin.get(
            f"{_base(ws, pid)}/issues/{a['id']}/issue-relation/").json()
        assert [row["id"] for row in listing["blocking"]] == [b["id"]]
        assert listing["blocking"][0]["relation_type"] == "blocking"
        assert admin.post(
            f"{_base(ws, pid)}/issues/{a['id']}/remove-relation/",
            json={"related_issue": b["id"]},
        ).status_code == 204
    finally:
        for issue_id in (a["id"], b["id"]):
            admin.delete(f"{_base(ws, pid)}/issues/{issue_id}/")


def test_link_crud_roundtrip(clients, seed):
    admin = clients["admin"]
    ws, pid, i2 = seed["ws_slug"], seed["project"], seed["issue2"]
    resp = admin.post(
        f"{_base(ws, pid)}/issues/{i2}/issue-links/",
        json={"title": "docs", "url": "https://example.com/docs"},
    )
    assert resp.status_code == 201
    body = resp.json()
    assert set(body) == LINK_KEYS
    assert body["title"] == "docs"
    assert body["url"] == "https://example.com/docs"
    link_id = body["id"]
    try:
        resp = admin.get(f"{_base(ws, pid)}/issues/{i2}/issue-links/")
        assert resp.status_code == 200
        assert link_id in {row["id"] for row in resp.json()}
        resp = admin.patch(
            f"{_base(ws, pid)}/issues/{i2}/issue-links/{link_id}/",
            json={"title": "docs v2"},
        )
        assert resp.status_code == 200
        assert resp.json()["title"] == "docs v2"
    finally:
        assert admin.delete(
            f"{_base(ws, pid)}/issues/{i2}/issue-links/{link_id}/"
        ).status_code == 204
    assert link_id not in {
        row["id"] for row in
        admin.get(f"{_base(ws, pid)}/issues/{i2}/issue-links/").json()
    }


def test_link_list_seed_shape(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(f"{_base(ws, pid)}/issues/{i1}/issue-links/")
    assert resp.status_code == 200
    rows = resp.json()
    assert len(rows) == 1
    assert set(rows[0]) == LINK_KEYS
    assert rows[0]["url"] == "https://example.com/seed-spec"
    assert rows[0]["created_by_detail"]["display_name"] == "is_admin"


def test_pr_link_list_and_invalid_url(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(
        f"{_base(ws, pid)}/issues/{i1}/github-pull-requests/")
    assert resp.status_code == 200
    rows = resp.json()
    assert len(rows) == 1
    assert rows[0]["id"] == seed["pr_link"]
    assert rows[0]["url"] == "https://github.com/octo/demo/pull/7"
    assert rows[0]["pr_number"] == 7
    resp = admin.post(
        f"{_base(ws, pid)}/issues/{i1}/github-pull-requests/",
        json={"url": "not-a-url"},
    )
    assert resp.status_code == 400
    assert resp.json() == {
        "error": "A valid github.com pull request URL is required."}


def test_code_review_list_and_invalid_url(clients, seed):
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.get(f"{_base(ws, pid)}/issues/{i1}/code-reviews/")
    assert resp.status_code == 200
    rows = resp.json()
    assert len(rows) == 2
    assert {row["id"] for row in rows} == {
        seed["review_link"], seed["review_link2"]}
    assert {row["provider"] for row in rows} == {"github"}
    resp = admin.post(
        f"{_base(ws, pid)}/issues/{i1}/code-reviews/",
        json={"url": "not-a-url"},
    )
    assert resp.status_code == 400
    assert resp.json() == {
        "error": "A supported GitHub pull request or GitLab merge "
        "request URL is required."}


def test_code_review_destroy_without_cascade(clients, seed):
    # Attaching needs the live GitHub API, so destroy is covered against
    # the seeded rows. Deleting review #8 touches no legacy PR link
    # (cascade matches on namespace/repo/number), so the PR row survives.
    # Runs after the list-shape tests above.
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.delete(
        f"{_base(ws, pid)}/issues/{i1}/code-reviews/"
        f"{seed['review_link2']}/")
    assert resp.status_code == 204
    remaining = admin.get(
        f"{_base(ws, pid)}/issues/{i1}/code-reviews/").json()
    assert [row["id"] for row in remaining] == [seed["review_link"]]
    assert len(admin.get(
        f"{_base(ws, pid)}/issues/{i1}/github-pull-requests/").json()) == 1


def test_pr_link_destroy_cascades_to_review(clients, seed):
    # ``detach_pull_request_link`` also removes the provider-neutral
    # code-review link for the same repo/number — deleting the seeded
    # PR #7 takes review #7 with it. Both lists end empty.
    admin = clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    resp = admin.delete(
        f"{_base(ws, pid)}/issues/{i1}/github-pull-requests/"
        f"{seed['pr_link']}/")
    assert resp.status_code == 204
    assert admin.get(
        f"{_base(ws, pid)}/issues/{i1}/github-pull-requests/").json() == []
    assert admin.get(
        f"{_base(ws, pid)}/issues/{i1}/code-reviews/").json() == []
