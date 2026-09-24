"""Contract: work-item list / create / detail / update / delete / search.

Covers ``api/urls/work_item.py`` issue endpoints (new ``work-items/*`` paths
plus the deprecated ``issues/*`` twins, the by-identifier route, and both
search routes). Shapes below are the live Django wire format; the Rust port
must reproduce them byte for byte.
"""

import pytest

from .conftest import ENVELOPE_KEYS, issue_url

pytestmark = pytest.mark.contract

ISSUE_ROW_KEYS = {
    "id",
    "type_id",
    "url",
    "created_at",
    "updated_at",
    "deleted_at",
    "point",
    "name",
    "description_html",
    "description_binary",
    "priority",
    "complexity_score",
    "start_date",
    "target_date",
    "sequence_id",
    "sort_order",
    "completed_at",
    "archived_at",
    "is_draft",
    "external_source",
    "external_id",
    "git_work_branch",
    "created_via",
    "agent_executor",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "parent",
    "state",
    "estimate_point",
    "type",
    "assigned_pod",
    "assignees",
    "labels",
}

ISSUE_WRITE_KEYS = ISSUE_ROW_KEYS | {"relations_summary", "has_open_blockers"}

ISSUE_DETAIL_KEYS = ISSUE_WRITE_KEYS | {"relations"}


def test_issue_list_shape(api, world):
    response = api.get(issue_url(world))
    assert response.status_code == 200
    body = response.json()
    assert set(body) == ENVELOPE_KEYS, f"envelope changed: {sorted(set(body) ^ ENVELOPE_KEYS)}"
    assert body["total_results"] == 1
    row = body["results"][0]
    assert set(row) == ISSUE_ROW_KEYS, f"row keys changed: {sorted(set(row) ^ ISSUE_ROW_KEYS)}"
    assert row["name"] == "Contract issue"
    assert row["sequence_id"] == 1


def test_issue_create_shape(api, world):
    response = api.post(
        issue_url(world),
        json={"name": "Second issue", "description_html": "<p>body</p>", "priority": "high"},
    )
    assert response.status_code == 201
    body = response.json()
    assert set(body) == ISSUE_WRITE_KEYS, f"keys changed: {sorted(set(body) ^ ISSUE_WRITE_KEYS)}"
    assert body["name"] == "Second issue"
    assert body["priority"] == "high"
    assert body["sequence_id"] == 2
    assert "/browse/" in body["url"]
    assert body["url"].endswith(f"-{body['sequence_id']}")


def test_issue_detail_shape(api, world):
    response = api.get(issue_url(world, world["issue"]["id"]))
    assert response.status_code == 200
    body = response.json()
    assert set(body) == ISSUE_DETAIL_KEYS, f"keys changed: {sorted(set(body) ^ ISSUE_DETAIL_KEYS)}"
    assert body["id"] == world["issue"]["id"]
    assert body["has_open_blockers"] is False


def test_issue_patch_shape(api, world):
    response = api.patch(
        issue_url(world, world["issue"]["id"]),
        json={"name": "Renamed", "priority": "low"},
    )
    assert response.status_code == 200
    body = response.json()
    assert set(body) == ISSUE_WRITE_KEYS
    assert body["name"] == "Renamed"
    assert body["priority"] == "low"


def test_issue_delete(api, world):
    created = api.post(issue_url(world), json={"name": "Doomed"}).json()
    response = api.delete(issue_url(world, created["id"]))
    assert response.status_code == 204
    assert api.get(issue_url(world, created["id"])).status_code == 404


def test_issue_detail_unknown_is_404(api, world):
    response = api.get(issue_url(world, "00000000-0000-0000-0000-000000000000"))
    assert response.status_code == 404


def test_deprecated_issues_prefix_parity(api, world):
    """The old ``issues/*`` routes serve the same views: spot-check parity."""
    slug = world["workspace"]["slug"]
    project_id = world["project"]["id"]
    issue_id = world["issue"]["id"]
    listed = api.get(f"/api/v1/workspaces/{slug}/projects/{project_id}/issues/").json()
    assert set(listed) == ENVELOPE_KEYS
    assert listed["total_results"] == 1
    assert set(listed["results"][0]) == ISSUE_ROW_KEYS
    detail = api.get(f"/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/")
    assert detail.status_code == 200
    assert set(detail.json()) == ISSUE_DETAIL_KEYS


def test_issue_by_identifier(api, world):
    project = world["project"]
    identifier = f"{project_identifier(world)}-{world['issue']['sequence_id']}"
    response = api.get(f"/api/v1/workspaces/{world['workspace']['slug']}/work-items/{identifier}/")
    assert response.status_code == 200
    assert set(response.json()) == ISSUE_DETAIL_KEYS
    assert response.json()["id"] == world["issue"]["id"]


def project_identifier(world):
    return world["issue"]["url"].rsplit("/", 1)[-1].rsplit("-", 1)[0]


SEARCH_ROW_KEYS = {
    "name",
    "id",
    "sequence_id",
    "project__identifier",
    "project_id",
    "workspace__slug",
}


def test_search_shape(api, world):
    response = api.get(
        f"/api/v1/workspaces/{world['workspace']['slug']}/work-items/search/",
        params={"search": "Contract"},
    )
    assert response.status_code == 200
    body = response.json()
    assert set(body) == {"issues"}
    assert len(body["issues"]) == 1
    assert set(body["issues"][0]) == SEARCH_ROW_KEYS
    assert body["issues"][0]["id"] == world["issue"]["id"]


def test_search_empty_without_query(api, world):
    response = api.get(
        f"/api/v1/workspaces/{world['workspace']['slug']}/work-items/search/"
    )
    assert response.status_code == 200
    assert response.json() == {"issues": []}


def test_deprecated_search_parity(api, world):
    response = api.get(
        f"/api/v1/workspaces/{world['workspace']['slug']}/issues/search/",
        params={"search": "Contract"},
    )
    assert response.status_code == 200
    assert set(response.json()) == {"issues"}
    assert len(response.json()["issues"]) == 1


def test_advanced_search_shape(api, world):
    response = api.get(
        f"/api/v1/workspaces/{world['workspace']['slug']}/work-items/search/advanced/",
        params={"q": "Contract"},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert set(body) == {"query", "count", "results"}
    assert body["count"] >= 1
