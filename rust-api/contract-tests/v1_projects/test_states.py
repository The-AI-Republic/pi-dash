"""Contract tests: api-v1 state endpoints (urls/state.py, 2 URL entries).

Covers state list/create and detail get/patch/delete, plus identifier-slug
routing, one denied case and one tenant-isolation case. Note create answers
200 (not 201). Non-canonical UUID detail spellings 404 (PIDASHCONV-1393).
"""

import sys
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402

KEY = "states"

PAGE_KEYS = {
    "count", "extra_stats", "grouped_by", "next_cursor", "next_page_results",
    "prev_cursor", "prev_page_results", "results", "sub_grouped_by",
    "total_count", "total_pages", "total_results",
}


def base(seed):
    return (f"/api/v1/workspaces/{seed['ws_a']['slug']}"
            f"/projects/{seed['project']['id']}/states")


def test_list_shape(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    body = http.get(seed["keys"][KEY], base(seed) + "/").json()
    assert set(body.keys()) == PAGE_KEYS
    assert body["total_count"] >= 1
    by_id = {s["id"]: s for s in body["results"]}
    assert st["id"] in by_id
    row = by_id[st["id"]]
    for k in ("id", "name", "color", "group", "sequence", "default",
              "project", "workspace", "created_at", "updated_at"):
        assert k in row, f"state list item missing {k}"
    assert row["is_triage"] is False


def test_list_by_slug(seed):
    body = http.get(
        seed["keys"][KEY],
        f"/api/v1/workspaces/{seed['ws_a']['slug']}"
        f"/projects/{seed['project']['identifier']}/states/").json()
    assert body["total_count"] >= 1


def test_create(seed):
    tag = db.new_tag()
    name = f"CT State {tag}"
    # Create answers 200, not 201.
    body = http.post(seed["keys"][KEY], base(seed) + "/",
                     json={"name": name, "color": "#00ff00", "group": "backlog",
                           "sequence": 1000.0},
                     expect=200).json()
    assert body["name"] == name
    assert body["group"] == "backlog"
    assert body["default"] is False


def test_create_conflicts(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    # Same name -> 409 with the surviving row's id.
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": st["name"], "color": "#00ff00", "group": "backlog"},
                  expect=409)
    assert r.json() == {
        "error": "State with the same name already exists in the project",
        "id": st["id"],
    }
    # Triage group can never be created through the API.
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": f"CT Tri {tag}", "color": "#00ff00", "group": "triage"},
                  expect=400)
    assert r.json() == {"non_field_errors": ["Cannot create triage state"]}
    # Same external id + source -> 409 with the surviving row's id.
    ext = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag + "e")
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE states SET external_id = 'EXT-1', external_source = 'GH' WHERE id = %s",
            (ext["id"],),
        )
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": f"CT Ext {tag}", "color": "#00ff00", "group": "backlog",
                        "external_id": "EXT-1", "external_source": "GH"},
                  expect=409)
    assert r.json() == {
        "error": "State with the same external id and external source already exists",
        "id": ext["id"],
    }


def test_retrieve(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    body = http.get(seed["keys"][KEY], f"{base(seed)}/{st['id']}/").json()
    assert body["id"] == st["id"]
    assert body["name"] == st["name"]


def test_patch(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    body = http.patch(seed["keys"][KEY], f"{base(seed)}/{st['id']}/",
                      json={"color": "#0000ff"}).json()
    assert body["color"] == "#0000ff"
    assert body["id"] == st["id"]
    # External-id clash on update -> 409.
    other = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag + "o")
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE states SET external_id = 'EXT-2', external_source = 'GH' WHERE id = %s",
            (other["id"],),
        )
    r = http.patch(seed["keys"][KEY], f"{base(seed)}/{st['id']}/",
                   json={"external_id": "EXT-2", "external_source": "GH"},
                   expect=409)
    assert r.json()["error"] == (
        "State with the same external id and external source already exists")


def test_patch_reslugifies_without_name(seed, conn):
    # Django State.save() slugifies unconditionally (db/models/state.py:132),
    # so a PATCH that omits name still rewrites the slug. Default states are
    # bulk-created with slug "" (bulk_create skips save()), so a color-only
    # PATCH on Backlog must return and store "backlog". PIDASHCONV-1390.
    tag = db.new_tag()
    ident = f"C{tag.upper()}"[:12]
    created = http.post(
        seed["keys"][KEY],
        f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects/",
        json={"name": f"CT Reslug {tag}", "identifier": ident},
        expect=201,
    ).json()
    pid = created["id"]
    pbase = (f"/api/v1/workspaces/{seed['ws_a']['slug']}"
             f"/projects/{pid}/states")
    states = http.get(seed["keys"][KEY], pbase + "/").json()
    backlog = [s for s in states["results"] if s["name"] == "Backlog"][0]
    assert backlog["slug"] == ""
    body = http.patch(seed["keys"][KEY], f"{pbase}/{backlog['id']}/",
                      json={"color": "#0000ff"}).json()
    assert body["color"] == "#0000ff"
    assert body["slug"] == "backlog"
    row = db.fetch_one(
        conn, "SELECT slug FROM states WHERE id = %s", (backlog["id"],))
    assert row["slug"] == "backlog"


def test_delete_empty(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    http.delete(seed["keys"][KEY], f"{base(seed)}/{st['id']}/")
    http.get(seed["keys"][KEY], f"{base(seed)}/{st['id']}/", expect=404)


def test_delete_default_400(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    with conn.cursor() as cur:
        cur.execute("UPDATE states SET \"default\" = true WHERE id = %s", (st["id"],))
    r = http.delete(seed["keys"][KEY], f"{base(seed)}/{st['id']}/", expect=400)
    assert r.json() == {"error": "Default state cannot be deleted"}


def test_denied_outsider(seed):
    http.get(seed["keys"]["outsider"], base(seed) + "/", expect=403)
    http.post(seed["keys"]["outsider"], base(seed) + "/",
              json={"name": "CT Nope", "color": "#fff", "group": "backlog"},
              expect=403)


def test_isolation_other_workspace(seed, conn):
    tag = db.new_tag()
    st_b = db.create_state(conn, seed["ws_b"]["id"], seed["project_b"]["id"], tag)
    db.add_project_member(conn, seed["ws_b"]["id"], seed["project_b"]["id"],
                          seed["owner"]["id"], db.ADMIN)
    try:
        # owner (admin in both workspaces now) lists ws_b: sees only ws_b states.
        body = http.get(
            seed["keys"][KEY],
            f"/api/v1/workspaces/{seed['ws_b']['slug']}"
            f"/projects/{seed['project_b']['id']}/states/").json()
        ids = {s["id"] for s in body["results"]}
        assert st_b["id"] in ids
        # ... and ws_a's states never leak into ws_b's list.
        body_a = http.get(seed["keys"][KEY], base(seed) + "/").json()
        assert st_b["id"] not in {s["id"] for s in body_a["results"]}
    finally:
        with conn.cursor() as cur:
            cur.execute(
                "DELETE FROM project_members WHERE project_id = %s AND member_id = %s",
                (seed["project_b"]["id"], seed["owner"]["id"]),
            )


def _spellings(canonical):
    # Every spelling `uuid.UUID()` (and Rust's `Uuid::parse_str`) accepts
    # but Django's `<uuid:...>` converter (`[0-9a-f]{8}-...`, matched
    # case-sensitively) does not route (PIDASHCONV-1393).
    return {
        "uppercase": canonical.upper(),
        "simple": canonical.replace("-", ""),
        "braced": "{" + canonical + "}",
    }


def _assert_resolver_404(resp):
    # Routing precedes auth and the view entirely: the answer is the
    # resolver's 404, never the view's body or a 401. The harness serves
    # Django with `pi_dash.settings.test` (`DEBUG = True`), so the
    # resolver miss is Django's technical HTML page on both backends
    # (PIDASHCONV-1193 note); the production JSON handler404 only
    # appears with `DEBUG = False` and is unreachable here.
    assert resp.status_code == 404, resp.text[:400]
    assert resp.headers.get("content-type", "").startswith("text/html"), \
        resp.headers.get("content-type")


def test_detail_non_canonical_spelling_404s(seed, conn):
    # `states/<uuid:state_id>/` (`state.py:19`): uppercase, plain-hex and
    # braced spellings of a real state id resolver-404 on GET, PATCH and
    # DELETE — for anonymous callers too — and change nothing.
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    before = db.fetch_one(
        conn,
        "SELECT name, color, deleted_at FROM states WHERE id = %s",
        (st["id"],),
    )
    for segment in _spellings(st["id"]).values():
        url = f"{base(seed)}/{segment}/"
        _assert_resolver_404(http.get(None, url, expect=404))
        _assert_resolver_404(http.get(seed["keys"][KEY], url, expect=404))
        _assert_resolver_404(http.patch(
            seed["keys"][KEY], url, json={"color": "#0000ff"}, expect=404))
        _assert_resolver_404(http.delete(seed["keys"][KEY], url, expect=404))
    after = db.fetch_one(
        conn,
        "SELECT name, color, deleted_at FROM states WHERE id = %s",
        (st["id"],),
    )
    assert after == before
    # Canonical control: the gate must not reject the spelling Django routes.
    body = http.get(seed["keys"][KEY], f"{base(seed)}/{st['id']}/").json()
    assert body["id"] == st["id"]
    assert body["name"] == st["name"]


# --- expand asset URLs: odd cover/avatar assets (PIDASHCONV-1396) -----------
#
# State `?expand=project` / `?expand=created_by` render `cover_image_url` /
# `avatar_url` through the same `FileAsset.asset_url` semantics PIDASHCONV-1148
# fixed on the project route (`db/models/asset.py:80-100`): a NULL/unmapped
# `entity_type` renders null; an attachment/description asset with a NULL
# workspace raises a plain AttributeError inside the property, which DRF's
# `Field.get_attribute` turns into `SkipField` for the read-only lite field
# (the key is left out, 200); NULL project/issue ids render as "None" text.
# Seeds mirror the 1148 helpers in test_projects.py.

EXPAND_DESCRIPTION_TYPES = (
    "ISSUE_DESCRIPTION",
    "COMMENT_DESCRIPTION",
    "PAGE_DESCRIPTION",
    "DRAFT_ISSUE_DESCRIPTION",
)


def _add_expand_asset(conn, entity_type, *, workspace_id=None):
    aid = str(uuid.uuid4())
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO file_assets (id, attributes, asset, entity_type,
                workspace_id, is_deleted, is_archived,
                size, is_uploaded, created_at, updated_at)
            VALUES (%s, '{}', 'ct/expand.png', %s, %s,
                false, false, 0, false, now(), now())
            """,
            (aid, entity_type, workspace_id),
        )
    conn.commit()
    return aid


def _expand_state(seed, conn, entity_type, *, workspace_id=None):
    # One project (cover asset + text) + one creator (avatar asset + text)
    # + one state by that creator, all sharing the same asset shape.
    tag = db.new_tag()
    pid = db.create_project(conn, seed["ws_a"]["id"], tag,
                            created_by_id=seed["owner"]["id"])["id"]
    db.add_project_member(conn, seed["ws_a"]["id"], pid, seed["owner"]["id"],
                          db.ADMIN)
    cover_aid = _add_expand_asset(conn, entity_type, workspace_id=workspace_id)
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET cover_image_asset_id = %s, cover_image = %s"
            " WHERE id = %s",
            (cover_aid, "COVER_TEXT_XYZ", pid),
        )
    creator = db.create_user(conn, tag + "c", first_name="CtExp")
    avatar_aid = _add_expand_asset(conn, entity_type, workspace_id=workspace_id)
    db.set_user_avatar(conn, creator["id"], asset_id=avatar_aid,
                       avatar="AVATAR_TEXT_XYZ")
    st = db.create_state(conn, seed["ws_a"]["id"], pid, tag + "s",
                         created_by_id=creator["id"])
    return pid, st["id"], cover_aid, avatar_aid


def _expanded_row(seed, pid, sid, expand):
    pbase = (f"/api/v1/workspaces/{seed['ws_a']['slug']}"
             f"/projects/{pid}/states")
    body = http.get(seed["keys"][KEY], pbase + "/",
                    params={"expand": expand, "per_page": "100"}).json()
    return next(s for s in body["results"] if s["id"] == sid)


def test_expand_project_cover_null_entity_renders_null(seed, conn):
    # (a) NULL / unmapped entity_type: the expanded project renders
    # "cover_image_url": null even with non-empty cover_image text.
    for entity in (None, "DRAFT_ISSUE_ATTACHMENT"):
        pid, sid, _, _ = _expand_state(seed, conn, entity)
        row = _expanded_row(seed, pid, sid, "project")
        assert "cover_image_url" in row["project"]
        assert row["project"]["cover_image_url"] is None


def test_expand_created_by_avatar_null_entity_renders_null(seed, conn):
    # (a) NULL / unmapped entity_type: the expanded creator renders
    # "avatar_url": null even with non-empty avatar text.
    for entity in (None, "DRAFT_ISSUE_ATTACHMENT"):
        pid, sid, _, _ = _expand_state(seed, conn, entity)
        row = _expanded_row(seed, pid, sid, "created_by")
        assert "avatar_url" in row["created_by"]
        assert row["created_by"]["avatar_url"] is None


def test_expand_project_cover_null_workspace_omits_key(seed, conn):
    # (c) attachment/description asset with NULL workspace_id: DRF leaves
    # the cover_image_url key out of the expanded project (200).
    for entity in ("ISSUE_ATTACHMENT",) + EXPAND_DESCRIPTION_TYPES:
        pid, sid, _, _ = _expand_state(seed, conn, entity)
        row = _expanded_row(seed, pid, sid, "project")
        assert "cover_image_url" not in row["project"], entity


def test_expand_created_by_avatar_null_workspace_omits_key(seed, conn):
    # (c) attachment/description asset with NULL workspace_id: DRF leaves
    # the avatar_url key out of the expanded creator (200).
    for entity in ("ISSUE_ATTACHMENT",) + EXPAND_DESCRIPTION_TYPES:
        pid, sid, _, _ = _expand_state(seed, conn, entity)
        row = _expanded_row(seed, pid, sid, "created_by")
        assert "avatar_url" not in row["created_by"], entity


def test_expand_cover_null_project_renders_none_text(seed, conn):
    # (b) ISSUE_ATTACHMENT with a workspace but NULL project/issue: the
    # Python f-string renders str(None) == "None" into the URL path, on
    # both the expanded project and the expanded creator.
    ws_id = seed["ws_a"]["id"]
    pid, sid, cover_aid, avatar_aid = _expand_state(
        seed, conn, "ISSUE_ATTACHMENT", workspace_id=ws_id)
    slug = seed["ws_a"]["slug"]
    row = _expanded_row(seed, pid, sid, "project")
    assert row["project"]["cover_image_url"] == (
        f"/api/assets/v2/workspaces/{slug}/projects/None"
        f"/issues/None/attachments/{cover_aid}/"
    )
    row = _expanded_row(seed, pid, sid, "created_by")
    assert row["created_by"]["avatar_url"] == (
        f"/api/assets/v2/workspaces/{slug}/projects/None"
        f"/issues/None/attachments/{avatar_aid}/"
    )


def test_expand_cover_static_asset_renders_url(seed, conn):
    # Control: mapped static assets render their static URL verbatim on
    # both expands.
    pid, sid, cover_aid, _ = _expand_state(
        seed, conn, "PROJECT_COVER")
    row = _expanded_row(seed, pid, sid, "project")
    assert row["project"]["cover_image_url"] == (
        f"/api/assets/v2/static/{cover_aid}/")
    pid2, sid2, _, avatar_aid2 = _expand_state(seed, conn, "USER_AVATAR")
    row2 = _expanded_row(seed, pid2, sid2, "created_by")
    assert row2["created_by"]["avatar_url"] == (
        f"/api/assets/v2/static/{avatar_aid2}/")
