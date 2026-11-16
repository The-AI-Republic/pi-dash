"""Contract tests: app_project write content types (PIDASHCONV-1517).

``parse_body`` (``handlers_invites.rs``) serves six write handlers (invite
create, user invite create, join, favorite create, board create, board
PATCH). DRF's ``DefaultContentNegotiation.select_parser`` matches any
content type the JSON media type satisfies — ``application/json`` in any
case with any params, plus the ``*/*`` and ``application/*`` request-side
wildcards — to the JSON parser, so those bodies parse as JSON. A media
type no parser accepts 415s naming the content type verbatim; a missing
``Content-Type`` with a non-empty body 415s naming ``""`` under prod
Django (uvicorn/ASGI). Runserver (wsgiref) renders ``text/plain`` there,
so the missing-CT tests assert the status plus the PIDASHCONV-1391
prefix/suffix, and the exact ``""`` naming is pinned by uvicorn probe
evidence, not by this suite.

Same suite runs against Django and against Rust with identical
expectations.
"""

from psycopg.rows import dict_row

from .conftest import assert_keys
from .test_project import BOARD_KEYS

CT_415_PREFIX = '{"detail":"Unsupported media type \\"'
CT_415_SUFFIX = '\\" in request."}'


def _send_no_content_type(client, method, path, body):
    request = client.build_request(method, path, content=body)
    assert "content-type" not in request.headers
    return client.send(request)


def _board_base(ws, project):
    return (f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
            f"project-deploy-boards/")


def test_wildcard_star_star_board_patch_parses_json(world):
    client, _, ws, project = world.full_stack()
    base = _board_base(ws, project)
    resp = client.post(base, json={"is_comments_enabled": True})
    assert resp.status_code == 200, resp.text[:300]
    board_id = resp.json()["id"]
    request = client.build_request(
        "PATCH", f"{base}{board_id}/",
        content=b'{"is_reactions_enabled": true}',
        headers={"Content-Type": "*/*"},
    )
    resp = client.send(request)
    assert resp.status_code == 200, resp.text[:300]
    body = resp.json()
    assert_keys(body, BOARD_KEYS, "board-patch-wildcard")
    assert body["is_reactions_enabled"] is True
    assert body["entity_identifier"] == str(project["id"])


def test_wildcard_application_star_board_patch_parses_json(world):
    client, _, ws, project = world.full_stack()
    base = _board_base(ws, project)
    resp = client.post(base, json={"is_comments_enabled": True})
    assert resp.status_code == 200, resp.text[:300]
    board_id = resp.json()["id"]
    request = client.build_request(
        "PATCH", f"{base}{board_id}/",
        content=b'{"is_votes_enabled": true}',
        headers={"Content-Type": "application/*"},
    )
    resp = client.send(request)
    assert resp.status_code == 200, resp.text[:300]
    body = resp.json()
    assert_keys(body, BOARD_KEYS, "board-patch-app-wildcard")
    assert body["is_votes_enabled"] is True


def test_wildcard_star_star_favorite_create_parses_json(world, db):
    # Second parse_body consumer: the wildcard fix lives in the shared
    # function, so it applies to every write handler, not just boards.
    client, _, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/user-favorite-projects/"
    request = client.build_request(
        "POST", url,
        content=('{"project": "%s"}' % project["id"]).encode(),
        headers={"Content-Type": "*/*"},
    )
    resp = client.send(request)
    assert resp.status_code == 204, resp.text[:300]
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM user_favorites WHERE project_id=%s",
            (str(project["id"]),))
        assert cur.fetchone()["n"] == 1


def test_missing_content_type_board_patch_415(world):
    client, _, ws, project = world.full_stack()
    base = _board_base(ws, project)
    resp = client.post(base, json={"is_comments_enabled": True})
    assert resp.status_code == 200, resp.text[:300]
    board_id = resp.json()["id"]
    resp = _send_no_content_type(
        client, "PATCH", f"{base}{board_id}/", b'{"is_votes_enabled": true}')
    assert resp.status_code == 415, resp.text[:300]
    assert resp.text.startswith(CT_415_PREFIX), resp.text[:300]
    assert resp.text.endswith(CT_415_SUFFIX), resp.text[:300]
    # The 415 lands before handler logic: nothing is written.
    resp = client.get(f"{base}{board_id}/")
    assert resp.status_code == 200
    assert resp.json()["is_votes_enabled"] is False


def test_missing_content_type_favorite_create_415(world, db):
    client, _, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/user-favorite-projects/"
    resp = _send_no_content_type(
        client, "POST", url, ('{"project": "%s"}' % project["id"]).encode())
    assert resp.status_code == 415, resp.text[:300]
    assert resp.text.startswith(CT_415_PREFIX), resp.text[:300]
    assert resp.text.endswith(CT_415_SUFFIX), resp.text[:300]
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT COUNT(*) AS n FROM user_favorites WHERE project_id=%s",
            (str(project["id"]),))
        assert cur.fetchone()["n"] == 0
