"""Asset downloads: presigned attachment redirects, workspace and project scoped."""

from urllib.parse import parse_qs, urlparse

from . import seed_assets as seed_a


def mint(org, **kw):
    payload = {
        "name": "file.png",
        "type": "image/png",
        "size": 300,
        "entity_type": "WORKSPACE_LOGO",
        "entity_identifier": org.workspace["id"],
    }
    payload.update(kw)
    r = org.request(
        "POST", f"/api/assets/v2/workspaces/{org.workspace['slug']}/",
        "admin", json=payload,
    )
    assert r.status_code == 200, r.text
    return r.json()["asset_id"]


def ws_download(org, asset_id):
    return f"/api/assets/v2/workspaces/{org.workspace['slug']}/download/{asset_id}/"


def proj_download(org, asset_id):
    return (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/download/{asset_id}/"
    )


def test_workspace_download_redirect(org):
    asset_id = mint(org)
    seed_a.mark_uploaded(org.conn, asset_id)
    r = org.request("GET", ws_download(org, asset_id), "admin")
    assert r.status_code == 302, r.text
    location = r.headers["location"]
    assert seed_a.fetch(org.conn, asset_id)["asset"] in location
    assert "response-content-disposition=attachment" in location
    assert "file.png" in location


def test_slashed_name_disposition_keeps_slash(org):
    # `_get_content_disposition` (storage.py:115-124) quotes the stored
    # name with `urllib.parse.quote` (default `safe='/'`), so a name that
    # contains `/` (a pasted path, a `YYYY/MM/file` convention) keeps the
    # slash and every signed redirect carries `filename*=UTF-8''a/b.png`.
    # The strict SigV4 encoder would sign `a%2Fb.png` instead, so the raw
    # bytes of the three project-side redirects diverge per backend.
    r = org.request(
        "POST",
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/",
        "admin",
        json={"name": "a/b.png", "type": "image/png", "size": 300,
              "entity_type": "ISSUE_DESCRIPTION", "entity_identifier": None},
    )
    assert r.status_code == 200, r.text
    asset_id = r.json()["asset_id"]
    seed_a.mark_uploaded(org.conn, asset_id)

    expected = "attachment; filename*=UTF-8''a/b.png"
    project_fetch = (
        f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/{asset_id}/"
    )
    for path in (
        project_fetch,
        ws_download(org, asset_id),
        proj_download(org, asset_id),
    ):
        r = org.request("GET", path, "member")
        assert r.status_code == 302, (path, r.text)
        location = r.headers["location"]
        decoded = parse_qs(urlparse(location).query)[
            "response-content-disposition"
        ][0]
        assert decoded == expected, (path, decoded)


def test_workspace_download_before_upload_404(org):
    asset_id = mint(org)
    r = org.request("GET", ws_download(org, asset_id), "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "The requested asset could not be found."}


def test_workspace_download_roles(org):
    asset_id = mint(org)
    seed_a.mark_uploaded(org.conn, asset_id)
    denied = {"error": "You don't have the required permissions."}
    r = org.request("GET", ws_download(org, asset_id), "guest")
    assert r.status_code == 302
    r = org.request("GET", ws_download(org, asset_id), "outsider")
    assert r.status_code == 403
    assert r.json() == denied
    r = org.request("GET", ws_download(org, asset_id), None)
    assert r.status_code == 401


def test_project_download_round_trip(org):
    r = org.request(
        "POST", f"/api/assets/v2/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/",
        "admin",
        json={"name": "p.png", "type": "image/png", "size": 70,
              "entity_type": "ISSUE_DESCRIPTION", "entity_identifier": None},
    )
    asset_id = r.json()["asset_id"]
    seed_a.mark_uploaded(org.conn, asset_id)
    r = org.request("GET", proj_download(org, asset_id), "member")
    assert r.status_code == 302, r.text
    assert seed_a.fetch(org.conn, asset_id)["asset"] in r.headers["location"]
    denied = {"error": "You don't have the required permissions."}
    r = org.request("GET", proj_download(org, asset_id), "outsider")
    assert r.status_code == 403
    assert r.json() == denied
    r = org.request("GET", proj_download(org, asset_id), None)
    assert r.status_code == 401


def test_project_download_asset_without_project_404(org):
    asset_id = mint(org)
    seed_a.mark_uploaded(org.conn, asset_id)
    r = org.request("GET", proj_download(org, asset_id), "admin")
    assert r.status_code == 404
    assert r.json() == {"error": "The requested asset could not be found."}
