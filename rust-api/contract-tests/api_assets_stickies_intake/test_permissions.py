"""Denied-permission cases for every restricted surface.

Assets carry no membership gate (``IsAuthenticated`` only): the denied case
is an unauthenticated or badly-tokened caller (401). Stickies
(``WorkspaceUserPermission``) and intake (``ProjectLitePermission``) deny a
validly-authenticated non-member with 403.

The two ``*_permission_enforced`` tests are the deliberate-removal tripwires:
commenting out the view's ``permission_classes`` flips them from 403 to
2xx and the suite goes red. That removal is demonstrated once (locally,
reverted) before the PR.
"""

from . import seed_d21


def _sticky_base(org):
    return f"/api/v1/workspaces/{org.workspace['slug']}/stickies/"


def _intake_base(org):
    return (
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/intake-issues/"
    )


def test_user_asset_anonymous_denied(org):
    for method, path in (
        ("POST", "/api/v1/assets/user-assets/"),
        ("PATCH", "/api/v1/assets/user-assets/123e4567-e89b-12d3-a456-426614174000/"),
        ("DELETE", "/api/v1/assets/user-assets/123e4567-e89b-12d3-a456-426614174000/"),
        ("POST", "/api/v1/assets/user-assets/server/"),
    ):
        r = org.request(method, path, None, json={})
        assert r.status_code == 401, (method, path, r.status_code)


def test_generic_asset_anonymous_denied(org):
    base = f"/api/v1/workspaces/{org.workspace['slug']}/assets/"
    for method, path in (
        ("POST", base),
        ("GET", f"{base}123e4567-e89b-12d3-a456-426614174000/"),
        ("PATCH", f"{base}123e4567-e89b-12d3-a456-426614174000/"),
    ):
        r = org.request(method, path, None, json={})
        assert r.status_code == 401, (method, path, r.status_code)


def test_unknown_api_token_403(org):
    # A bad token fails authentication, but the backend answers 403 (not the
    # usual DRF 401) with the AuthenticationFailed message pinned below.
    r = org.client().request(
        "GET", _sticky_base(org), headers={"X-Api-Key": "pi_dash_api_nope"}
    )
    assert r.status_code == 403
    assert r.json() == {"detail": "Given API token is not valid"}


def test_sticky_outsider_denied(org):
    r = org.request("POST", _sticky_base(org), "outsider", json={"name": "x"})
    assert r.status_code == 403
    r = org.request("GET", _sticky_base(org), "outsider")
    assert r.status_code == 403
    # Detail actions need an existing row; create one as admin first.
    created = org.request(
        "POST", _sticky_base(org), "admin", json={"name": "y"}
    ).json()
    sticky_id = created["id"]
    for method, path, kw in (
        ("GET", f"{_sticky_base(org)}{sticky_id}/", {}),
        ("PATCH", f"{_sticky_base(org)}{sticky_id}/", {"json": {"name": "z"}}),
        ("DELETE", f"{_sticky_base(org)}{sticky_id}/", {}),
    ):
        r = org.request(method, path, "outsider", **kw)
        assert r.status_code == 403, (method, r.status_code)


def test_sticky_anonymous_denied(org):
    r = org.request("GET", _sticky_base(org), None)
    assert r.status_code == 401


def test_intake_outsider_denied(org):
    created = org.create_intake_issue(name="private")
    for method, path, kw in (
        ("GET", _intake_base(org), {}),
        ("POST", _intake_base(org), {"json": {"issue": {"name": "n"}}}),
        ("GET", f"{_intake_base(org)}{created['issue']}/", {}),
        ("PATCH", f"{_intake_base(org)}{created['issue']}/", {"json": {"status": -1}}),
        ("DELETE", f"{_intake_base(org)}{created['issue']}/", {}),
    ):
        r = org.request(method, path, "outsider", **kw)
        assert r.status_code == 403, (method, path, r.status_code)


def test_intake_anonymous_denied(org):
    r = org.request("GET", _intake_base(org), None)
    assert r.status_code == 401


def test_detector_sticky_permission_enforced(org):
    """Tripwire: fails if ``StickyViewSet.permission_classes`` is removed."""
    r = org.request("POST", _sticky_base(org), "outsider", json={"name": "intruder"})
    assert r.status_code == 403, (
        "outsider created a sticky: WorkspaceUserPermission is not enforced"
    )


def test_detector_intake_permission_enforced(org):
    """Tripwire: fails if the intake ``ProjectLitePermission`` gate is removed."""
    r = org.request(
        "POST", _intake_base(org), "outsider", json={"issue": {"name": "intruder"}}
    )
    assert r.status_code == 403, (
        "outsider created an intake issue: ProjectLitePermission is not enforced"
    )


def test_machine_token_runner_download(org):
    """Installed-runner wire compat: an ``mt_`` machine token bound to a
    workspace member may fetch the generic-asset download shape."""
    r = org.request(
        "POST", f"/api/v1/workspaces/{org.workspace['slug']}/assets/", "admin",
        json={"name": "bundle.bin", "type": "application/pdf", "size": 512},
    )
    assert r.status_code == 200, r.text[:500]
    asset_id = r.json()["asset_id"]
    org.conn.execute("UPDATE file_assets SET is_uploaded = true WHERE id = %s", (asset_id,))
    raw = seed_d21.create_machine_token(
        org.conn, user_id=org.admin["id"], workspace_id=org.workspace["id"]
    )
    assert raw.startswith("mt_")
    r = org.client().request(
        "GET",
        f"/api/v1/workspaces/{org.workspace['slug']}/assets/{asset_id}/",
        headers={"X-Api-Key": raw},
    )
    assert r.status_code == 200
    assert set(r.json()) == {"asset_id", "asset_url", "asset_name", "asset_type"}
