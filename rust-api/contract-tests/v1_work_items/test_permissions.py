"""Contract: permission and tenancy floor for the work-items domain.

- Every endpoint requires authentication: no ``X-Api-Key`` → 401.
- A GUEST-role (5) project member may read but not write: unsafe methods → 403
  on ``ProjectEntityPermission`` endpoints.
- Cross-tenant objects resolve to 404, never to another tenant's rows.

Deliberate-removal tripwire: the write-denied tests below (guest 403s and
anonymous 401s on POST/PATCH/DELETE) fail if the view's
``permission_classes`` are removed — removing them turns the 401/403s into
2xx. Demonstrated in the PR with a one-line local patch (reverted).
"""

import pytest

from .conftest import issue_url

pytestmark = pytest.mark.contract


def _issue_paths(world):
    issue_id = world["issue"]["id"]
    slug = world["workspace"]["slug"]
    project_id = world["project"]["id"]
    base = f"/api/v1/workspaces/{slug}/projects/{project_id}"
    return [
        ("GET", f"{base}/work-items/"),
        ("POST", f"{base}/work-items/"),
        ("GET", f"{base}/work-items/{issue_id}/"),
        ("PATCH", f"{base}/work-items/{issue_id}/"),
        ("DELETE", f"{base}/work-items/{issue_id}/"),
        ("POST", f"{base}/work-items/{issue_id}/move/"),
        ("POST", f"{base}/work-items/{issue_id}/re-tick/"),
        ("POST", f"{base}/work-items/{issue_id}/wait/"),
        ("POST", f"{base}/work-items/{issue_id}/run-ai/"),
        ("GET", f"{base}/work-items/{issue_id}/links/"),
        ("POST", f"{base}/work-items/{issue_id}/links/"),
        ("GET", f"{base}/work-items/{issue_id}/comments/"),
        ("POST", f"{base}/work-items/{issue_id}/comments/"),
        ("GET", f"{base}/work-items/{issue_id}/activities/"),
        ("GET", f"{base}/work-items/{issue_id}/attachments/"),
        ("GET", f"/api/v1/workspaces/{slug}/work-items/search/"),
        ("GET", f"{base}/labels/"),
        ("POST", f"{base}/labels/"),
        ("GET", f"{base}/pages/"),
    ]


def _request(client, method, path, **kwargs):
    kwargs.setdefault("json", {"name": "x"} if method in ("POST", "PATCH") else None)
    if method in ("GET", "DELETE") and "json" in kwargs:
        del kwargs["json"]
    return client.request(method, path, **kwargs)


def test_anonymous_is_401_everywhere(anon, world):
    failures = []
    for method, path in _issue_paths(world):
        response = _request(anon, method, path)
        if response.status_code != 401:
            failures.append(f"{method} {path} -> {response.status_code}")
    assert not failures, f"non-401 anonymous responses: {failures}"


def test_guest_cannot_write_issue(guest_client, world):
    issue_id = world["issue"]["id"]
    assert guest_client.get(issue_url(world)).status_code == 200
    assert guest_client.get(issue_url(world, issue_id)).status_code == 200
    for method, path in [
        ("POST", issue_url(world)),
        ("PATCH", issue_url(world, issue_id)),
        ("DELETE", issue_url(world, issue_id)),
        ("POST", issue_url(world, issue_id) + "move/"),
        ("POST", issue_url(world, issue_id) + "run-ai/"),
    ]:
        response = _request(guest_client, method, path, json={"name": "guest-write"})
        assert response.status_code == 403, f"{method} {path} -> {response.status_code}"


def test_guest_cannot_write_labels(guest_client, world):
    base = (
        f"/api/v1/workspaces/{world['workspace']['slug']}"
        f"/projects/{world['project']['id']}/labels/"
    )
    assert guest_client.get(base).status_code == 200
    assert guest_client.post(base, json={"name": "g", "color": "#000"}).status_code == 403


def test_cross_tenant_issue_is_404(api, world, other_world):
    """Tenant isolation: another workspace's issue id is not visible here."""
    slug = world["workspace"]["slug"]
    project_id = world["project"]["id"]
    foreign_id = other_world["issue"]["id"]
    response = api.get(f"/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{foreign_id}/")
    assert response.status_code == 404


def test_cross_tenant_label_list_is_empty(api, world, other_api, other_world):
    other_api.post(
        f"/api/v1/workspaces/{other_world['workspace']['slug']}"
        f"/projects/{other_world['project']['id']}/labels/",
        json={"name": "Foreign", "color": "#000"},
    )
    own = api.get(
        f"/api/v1/workspaces/{world['workspace']['slug']}"
        f"/projects/{world['project']['id']}/labels/"
    ).json()
    assert own["total_results"] == 0


def test_member_of_nothing_is_denied(seeder, settings, world):
    """A valid token with no project membership sees 403, not data."""
    import httpx

    outsider = seeder.create_user()
    token = seeder.create_api_token(outsider["id"], world["workspace"]["id"])
    with httpx.Client(
        base_url=settings.base_url, timeout=30, headers={"X-Api-Key": token["token"]}
    ) as client:
        assert client.get(issue_url(world)).status_code == 403
