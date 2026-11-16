"""Denied-permission cases for every restricted surface.

Workspace v2 mint/fetch/confirm/delete and the v1 legacy surface carry no
membership gate (``IsAuthenticated`` only): the denied case there is an
unauthenticated caller (401), and ``X-Api-Key`` is ignored on app routes.
Project, bulk, check, restore, duplicate, and download routes gate on
workspace/project membership: a validly-authenticated outsider gets 403.

The ``*_permission_enforced`` tests are the deliberate-removal tripwires:
removing the view's ``@allow_permission`` gate flips them from 403 to 2xx
and the suite goes red. That removal is demonstrated once (locally,
reverted) before the PR.
"""

import uuid

from _harness import seed

ANON = {"detail": "Authentication credentials were not provided."}
DENIED = {"error": "You don't have the required permissions."}


def ws_paths(org, asset_id):
    slug = org.workspace["slug"]
    return {
        "ws-post": ("POST", f"/api/assets/v2/workspaces/{slug}/"),
        "ws-get": ("GET", f"/api/assets/v2/workspaces/{slug}/{asset_id}/"),
        "ws-patch": ("PATCH", f"/api/assets/v2/workspaces/{slug}/{asset_id}/"),
        "ws-delete": ("DELETE", f"/api/assets/v2/workspaces/{slug}/{asset_id}/"),
        "ws-download": ("GET", f"/api/assets/v2/workspaces/{slug}/download/{asset_id}/"),
        "check": ("GET", f"/api/assets/v2/workspaces/{slug}/check/{asset_id}/"),
        "restore": ("POST", f"/api/assets/v2/workspaces/{slug}/restore/{asset_id}/"),
        "duplicate": (
            "POST",
            f"/api/assets/v2/workspaces/{slug}/duplicate-assets/{asset_id}/",
        ),
    }


def proj_paths(org, asset_id):
    base = (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}"
    )
    return {
        "proj-post": ("POST", f"{base}/"),
        "proj-get": ("GET", f"{base}/{asset_id}/"),
        "proj-patch": ("PATCH", f"{base}/{asset_id}/"),
        "proj-delete": ("DELETE", f"{base}/{asset_id}/"),
        "proj-download": ("GET", f"{base}/download/{asset_id}/"),
        "bulk": ("POST", f"{base}/{uuid.uuid4()}/bulk/"),
    }


def v1_paths(org):
    slug = org.workspace["slug"]
    wid = org.workspace["id"]
    return {
        "v1-ws-post": ("POST", f"/api/workspaces/{slug}/file-assets/"),
        "v1-ws-get": ("GET", f"/api/workspaces/file-assets/{wid}/k/"),
        "v1-ws-delete": ("DELETE", f"/api/workspaces/file-assets/{wid}/k/"),
        "v1-ws-restore": ("POST", f"/api/workspaces/file-assets/{wid}/k/restore/"),
        "v1-user-post": ("POST", "/api/users/file-assets/"),
        "v1-user-get": ("GET", "/api/users/file-assets/k/"),
        "v1-user-delete": ("DELETE", "/api/users/file-assets/k/"),
        "v2-user-post": ("POST", "/api/assets/v2/user-assets/"),
        "v2-user-patch": (
            "PATCH",
            "/api/assets/v2/user-assets/00000000-0000-0000-0000-000000000000/",
        ),
        "v2-user-delete": (
            "DELETE",
            "/api/assets/v2/user-assets/00000000-0000-0000-0000-000000000000/",
        ),
        "static-unknown": (
            "GET",
            "/api/assets/v2/static/00000000-0000-0000-0000-000000000000/",
        ),
    }


def test_anonymous_denied_everywhere(org):
    asset_id = "00000000-0000-0000-0000-000000000000"
    paths = {**ws_paths(org, asset_id), **proj_paths(org, asset_id),
             **v1_paths(org)}
    # Static serves anyone; it is asserted separately.
    paths.pop("static-unknown")
    for name, (method, path) in paths.items():
        r = org.request(method, path, None, json={})
        assert r.status_code == 401, name
        assert r.json() == ANON, name


def test_api_key_is_ignored_on_app_routes(org):
    # Unlike /api/v1/, the app tree authenticates sessions only: a caller
    # presenting even a REAL api token is anonymous here.
    token = seed.create_api_token(org.conn, user_id=org.admin["id"])
    asset_id = "00000000-0000-0000-0000-000000000000"
    _, path = ws_paths(org, asset_id)["check"]
    r = org.client(None).get(path, headers={"X-Api-Key": token})
    assert r.status_code == 401
    assert r.json() == ANON


def test_outsider_denied_on_gated_routes(org):
    asset_id = org.request(
        "POST",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/",
        "admin",
        json={"name": "t.png", "type": "image/png", "size": 10,
              "entity_type": "WORKSPACE_LOGO",
              "entity_identifier": org.workspace["id"]},
    ).json()["asset_id"]
    gated = {**{k: v for k, v in ws_paths(org, asset_id).items()
                if k in {"check", "restore", "duplicate", "ws-download"}},
             **proj_paths(org, asset_id)}
    bulk_method, bulk_path = gated.pop("bulk")
    for name, (method, path) in gated.items():
        payload = (
            {"name": "x.png", "entity_type": "ISSUE_DESCRIPTION"}
            if method == "POST" else {}
        )
        r = org.request(method, path, "outsider", json=payload)
        assert r.status_code == 403, name
        assert r.json() == DENIED, name
    r = org.request(
        bulk_method, bulk_path, "outsider", json={"asset_ids": [asset_id]}
    )
    assert r.status_code == 403
    assert r.json() == DENIED


def test_detector_check_permission_enforced(org):
    """Tripwire: fails if ``AssetCheckEndpoint`` loses its WORKSPACE gate."""
    asset_id = org.request(
        "POST",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/",
        "admin",
        json={"name": "t.png", "type": "image/png", "size": 10,
              "entity_type": "WORKSPACE_LOGO",
              "entity_identifier": org.workspace["id"]},
    ).json()["asset_id"]
    r = org.request(
        "GET",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}/check/{asset_id}/",
        "outsider",
    )
    assert r.status_code == 403, (
        "outsider reached the check endpoint: the WORKSPACE gate is not enforced"
    )
    assert r.json() == DENIED


def test_detector_project_permission_enforced(org):
    """Tripwire: fails if ``ProjectAssetEndpoint`` loses its PROJECT gate."""
    r = org.request(
        "POST",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/",
        "outsider",
        json={"name": "x.png", "type": "image/png", "size": 10,
              "entity_type": "ISSUE_DESCRIPTION", "entity_identifier": None},
    )
    assert r.status_code == 403, (
        "outsider minted a project asset: the PROJECT gate is not enforced"
    )
    assert r.json() == DENIED
