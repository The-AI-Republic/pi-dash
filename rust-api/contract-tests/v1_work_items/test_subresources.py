"""Contract: work-item sub-resources — links, comments, activities, attachments,
relations, workpad, PR / code-review links.

Covers ``api/urls/work_item.py`` below the issue-detail level. Activity rows
are seeded via SQL (the contract environment runs no Celery worker, so the
enqueued activity tasks never execute); everything else goes through the API.
"""

import pytest

from .conftest import ENVELOPE_KEYS, issue_url

pytestmark = pytest.mark.contract

LINK_KEYS = {
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "title",
    "url",
    "metadata",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "issue",
}

COMMENT_KEYS = {
    "id",
    "url",
    "created_at",
    "updated_at",
    "deleted_at",
    "comment_html",
    "attachments",
    "labels",
    "access",
    "external_source",
    "external_id",
    "speaker_type",
    "speaker_label",
    "speaker_agent_run_id",
    "edited_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "description",
    "issue",
    "actor",
    "parent",
}

PR_LINK_KEYS = {
    "id",
    "issue",
    "repo_owner",
    "repo_name",
    "pr_number",
    "url",
    "title",
    "state",
    "merged",
    "draft",
    "pr_updated_at",
    "created_at",
    "updated_at",
    "created_by",
}

REVIEW_LINK_KEYS = {
    "id",
    "issue",
    "provider",
    "host_url",
    "namespace",
    "repo_name",
    "repo_external_id",
    "external_id",
    "external_iid",
    "url",
    "title",
    "state",
    "merged",
    "draft",
    "remote_updated_at",
    "metadata",
    "created_at",
    "updated_at",
    "created_by",
}

RELATION_KEYS = {
    "blocked_by",
    "blocking",
    "duplicate",
    "relates_to",
    "start_after",
    "start_before",
    "finish_after",
    "finish_before",
}


def sub_url(world, issue_id, *parts):
    return issue_url(world, issue_id) + "/".join(parts) + "/"


def test_link_create_and_list_shape(api, world):
    issue_id = world["issue"]["id"]
    created = api.post(
        sub_url(world, issue_id, "links"),
        json={"title": "Spec", "url": "https://example.com/spec"},
    )
    assert created.status_code == 201, created.text
    assert set(created.json()) == LINK_KEYS
    listed = api.get(sub_url(world, issue_id, "links")).json()
    assert set(listed) == ENVELOPE_KEYS
    assert listed["total_results"] == 1
    assert set(listed["results"][0]) == LINK_KEYS


def test_link_detail_patch_delete(api, world):
    issue_id = world["issue"]["id"]
    link_id = api.post(
        sub_url(world, issue_id, "links"),
        json={"title": "Old", "url": "https://example.com/old"},
    ).json()["id"]
    patched = api.patch(sub_url(world, issue_id, "links", link_id), json={"title": "New"})
    assert patched.status_code == 200
    assert patched.json()["title"] == "New"
    assert api.get(sub_url(world, issue_id, "links", link_id)).status_code == 200
    assert api.delete(sub_url(world, issue_id, "links", link_id)).status_code == 204
    assert api.get(sub_url(world, issue_id, "links", link_id)).status_code == 404


def test_comment_create_and_list_shape(api, world):
    issue_id = world["issue"]["id"]
    created = api.post(
        sub_url(world, issue_id, "comments"),
        json={"comment_html": "<p>Hello</p>", "access": "EXTERNAL"},
    )
    assert created.status_code == 201, created.text
    assert set(created.json()) == COMMENT_KEYS
    listed = api.get(sub_url(world, issue_id, "comments")).json()
    assert set(listed) == ENVELOPE_KEYS
    assert listed["total_results"] == 1
    assert listed["results"][0]["comment_html"] == "<p>Hello</p>"


def test_comment_detail_patch_delete(api, world):
    issue_id = world["issue"]["id"]
    comment_id = api.post(
        sub_url(world, issue_id, "comments"),
        json={"comment_html": "<p>v1</p>", "access": "EXTERNAL"},
    ).json()["id"]
    patched = api.patch(
        sub_url(world, issue_id, "comments", comment_id),
        json={"comment_html": "<p>v2</p>"},
    )
    assert patched.status_code == 200
    assert patched.json()["comment_html"] == "<p>v2</p>"
    assert api.delete(sub_url(world, issue_id, "comments", comment_id)).status_code == 204


def test_activity_list_shape(api, world, seeder):
    issue_id = world["issue"]["id"]
    seeder.create_issue_activity(
        world["workspace"]["id"], world["project"]["id"], issue_id, verb="created"
    )
    response = api.get(sub_url(world, issue_id, "activities"))
    assert response.status_code == 200
    body = response.json()
    assert set(body) == ENVELOPE_KEYS
    assert body["total_results"] == 1
    row = body["results"][0]
    assert row["verb"] == "created"
    assert row["issue"] == issue_id


def test_activity_detail_shape(api, world, seeder):
    issue_id = world["issue"]["id"]
    activity = seeder.create_issue_activity(
        world["workspace"]["id"], world["project"]["id"], issue_id, verb="updated"
    )
    response = api.get(sub_url(world, issue_id, "activities", activity["id"]))
    assert response.status_code == 200
    assert response.json()["id"] == activity["id"]


def test_attachment_list_shape(api, world, seeder):
    """The attachment list is a bare JSON list (not the paginated envelope)."""
    issue_id = world["issue"]["id"]
    attachment = seeder.create_issue_attachment(
        world["workspace"]["id"], world["project"]["id"], issue_id
    )
    response = api.get(sub_url(world, issue_id, "attachments"))
    assert response.status_code == 200
    rows = response.json()
    assert isinstance(rows, list) and len(rows) == 1
    assert rows[0]["id"] == attachment["id"]
    assert rows[0]["issue"] == issue_id


def test_attachment_delete(api, world, seeder):
    """Detail GET redirects to a presigned S3 URL (needs storage creds), so
    the offline contract covers list + delete only."""
    issue_id = world["issue"]["id"]
    attachment = seeder.create_issue_attachment(
        world["workspace"]["id"], world["project"]["id"], issue_id
    )
    assert api.delete(sub_url(world, issue_id, "attachments", attachment["id"])).status_code == 204
    assert api.get(sub_url(world, issue_id, "attachments")).json() == []


def test_relations_list_shape(api, world, seeder):
    sibling = api.post(issue_url(world), json={"name": "Sibling"}).json()
    seeder.create_issue_relation(
        world["workspace"]["id"],
        world["project"]["id"],
        world["issue"]["id"],
        sibling["id"],
    )
    response = api.get(sub_url(world, world["issue"]["id"], "relations"))
    assert response.status_code == 200
    assert set(response.json()) == RELATION_KEYS


def test_relate_unrelate_roundtrip(api, world):
    """`created` / `removed` carry issue identifiers (PROJ-123), not UUIDs."""
    issue_id = world["issue"]["id"]
    sibling = api.post(issue_url(world), json={"name": "Sibling"}).json()
    sibling_identifier = sibling["url"].rsplit("/", 1)[-1]
    related = api.post(
        sub_url(world, issue_id, "relations", "relate"),
        json={"relation_type": "relates_to", "issues": [sibling["id"]]},
    )
    assert related.status_code == 200, related.text
    assert related.json()["created"] == [sibling_identifier]
    grouped = api.get(sub_url(world, issue_id, "relations", "grouped")).json()
    assert set(grouped) == {"issue", "relations"}
    unrelate = api.post(
        sub_url(world, issue_id, "relations", "unrelate"),
        json={"relation_type": "relates_to", "issues": [sibling["id"]]},
    )
    assert unrelate.status_code == 200
    assert unrelate.json()["removed"] == [sibling_identifier]


def test_workpad_get_patch(api, world):
    issue_id = world["issue"]["id"]
    gotten = api.get(sub_url(world, issue_id, "workpad"))
    assert gotten.status_code == 200
    assert set(gotten.json()) == {"body", "updated_at"}
    patched = api.patch(sub_url(world, issue_id, "workpad"), json={"body": "# notes"})
    assert patched.status_code == 200, patched.text
    assert set(patched.json()) == {"updated_at"}
    assert api.get(sub_url(world, issue_id, "workpad")).json()["body"] == "# notes"


def test_workpad_patch_requires_body(api, world):
    response = api.patch(
        sub_url(world, world["issue"]["id"], "workpad"), json={"workpad": "wrong key"}
    )
    assert response.status_code == 400


def test_pr_link_create_list_delete(api, world, seeder):
    import random

    issue_id = world["issue"]["id"]
    pr_number = random.randint(100000, 999999)
    payload = {
        "repo_owner": "acme",
        "repo_name": "web",
        "pr_number": pr_number,
        "url": f"https://github.com/acme/web/pull/{pr_number}",
        "title": "Fix it",
        "state": "open",
        "merged": False,
        "draft": False,
    }
    created = api.post(sub_url(world, issue_id, "github", "pull-requests"), json=payload)
    assert created.status_code == 201, created.text
    assert set(created.json()) == PR_LINK_KEYS
    link_id = created.json()["id"]
    payload["title"] = "Fix it (again)"
    relink = api.post(sub_url(world, issue_id, "github", "pull-requests"), json=payload)
    assert relink.status_code == 200
    assert relink.json()["id"] == link_id
    listed = api.get(sub_url(world, issue_id, "github", "pull-requests")).json()
    assert set(listed) == ENVELOPE_KEYS
    assert listed["total_results"] == 1
    assert api.delete(sub_url(world, issue_id, "github", "pull-requests", link_id)).status_code == 204


def test_review_link_create_list_delete(api, world):
    """Fresh attach returns 201; re-attaching the same review is an idempotent 200."""
    import random

    issue_id = world["issue"]["id"]
    external_iid = str(random.randint(100000, 999999))
    payload = {
        "provider": "github",
        "host_url": "https://github.com",
        "namespace": "acme",
        "repo_name": "web",
        "repo_external_id": "",
        "external_id": f"mr-{external_iid}",
        "external_iid": external_iid,
        "url": f"https://github.com/acme/web/pull/{external_iid}",
        "title": "Review me",
        "state": "open",
        "merged": False,
        "draft": False,
    }
    created = api.post(sub_url(world, issue_id, "code-reviews"), json=payload)
    assert created.status_code == 201, created.text
    assert set(created.json()) == REVIEW_LINK_KEYS
    link_id = created.json()["id"]
    # Re-attaching the same review to the same issue is idempotent (200).
    relink = api.post(sub_url(world, issue_id, "code-reviews"), json=payload)
    assert relink.status_code == 200
    assert relink.json()["id"] == link_id
    listed = api.get(sub_url(world, issue_id, "code-reviews")).json()
    assert set(listed) == ENVELOPE_KEYS
    assert listed["total_results"] == 1
    assert api.delete(sub_url(world, issue_id, "code-reviews", link_id)).status_code == 204
