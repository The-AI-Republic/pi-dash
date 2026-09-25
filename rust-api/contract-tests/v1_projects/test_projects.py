"""Contract tests: api-v1 project endpoints (urls/project.py, 4 URL entries).

Covers list/create, detail get/patch/delete, archive/unarchive and summary,
plus identifier-slug routing, one denied case and one tenant-isolation case.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402

KEY = "projects"

PAGE_KEYS = {
    "count", "extra_stats", "grouped_by", "next_cursor", "next_page_results",
    "prev_cursor", "prev_page_results", "results", "sub_grouped_by",
    "total_count", "total_pages", "total_results",
}


def base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects"


def test_list_shape(seed, conn):
    r = http.get(seed["keys"][KEY], base(seed) + "/")
    body = r.json()
    assert set(body.keys()) == PAGE_KEYS
    assert body["total_count"] >= 1
    ids = [p["id"] for p in body["results"]]
    assert seed["project"]["id"] in ids
    row = next(p for p in body["results"] if p["id"] == seed["project"]["id"])
    for k in ("id", "name", "identifier", "total_members", "total_cycles",
              "total_modules", "is_member", "member_role", "is_deployed"):
        assert k in row, f"project list item missing {k}"
    assert row["name"] == seed["project"]["name"]
    assert row["is_member"] is True
    assert row["member_role"] == db.ADMIN


def test_list_pagination(seed):
    r = http.get(seed["keys"][KEY], base(seed) + "/", params={"per_page": "1"})
    body = r.json()
    assert body["total_count"] >= 1
    assert len(body["results"]) <= 1


def test_create_roundtrip(seed, conn):
    tag = db.new_tag()
    name = f"CT Created {tag}"
    ident = f"C{tag.upper()}"[:12]
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": name, "identifier": ident}, expect=201)
    body = r.json()
    assert body["name"] == name
    assert body["identifier"] == ident
    saved = db.fetch_one(conn, "SELECT name, identifier FROM projects WHERE id = %s", (body["id"],))
    assert saved == {"name": name, "identifier": ident}
    # Creating default states is part of create: the project must be usable.
    states = http.get(seed["keys"]["states"],
                      f"{base(seed)}/{body['id']}/states/").json()
    assert states["total_count"] >= 1


def test_create_conflicts(seed):
    tag = db.new_tag()
    name = f"CT Conflict {tag}"
    ident = f"K{tag.upper()}"[:12]
    http.post(seed["keys"][KEY], base(seed) + "/",
              json={"name": name, "identifier": ident}, expect=201)
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": name, "identifier": f"Z{tag.upper()}"[:12]}, expect=409)
    assert r.json() == {"name": "The project name is already taken"}
    # Quirk pinned: a taken identifier surfaces as the *name* conflict body,
    # because the ProjectIdentifier pre-check lives only on the read serializer
    # path while create hits the projects unique index -> IntegrityError branch.
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": f"CT Other {tag}", "identifier": ident}, expect=409)
    assert r.json() == {"name": "The project name is already taken"}
    # An identifier taken by a seeded row hits the ProjectIdentifier pre-check
    # -> ValidationError branch -> 409 identifier body (distinct from the
    # IntegrityError name body above when the clash is only in projects).
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": f"CT Other2 {tag}",
                        "identifier": seed["project"]["identifier"]}, expect=409)
    assert r.json() == {"identifier": "The project identifier is already taken"}
    # A missing identifier fails serializer field validation -> 400.
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": f"CT NoIdent {tag}"}, expect=400)
    assert r.json() == {"identifier": ["This field is required."]}


def test_detail_uuid_and_slug(seed):
    pid = seed["project"]["id"]
    by_uuid = http.get(seed["keys"][KEY], f"{base(seed)}/{pid}/").json()
    by_slug = http.get(
        seed["keys"][KEY], f"{base(seed)}/{seed['project']['identifier']}/").json()
    assert by_uuid["id"] == pid
    assert by_slug["id"] == pid
    assert by_slug["identifier"] == seed["project"]["identifier"]
    for k in ("id", "name", "identifier", "description", "workspace",
              "created_at", "updated_at"):
        assert k in by_uuid, f"project detail missing {k}"


def test_detail_unknown_slug_404(seed):
    http.get(seed["keys"][KEY], f"{base(seed)}/ZZZZ/", expect=404)


def test_anonymous_get_401(seed):
    http.get(None, f"{base(seed)}/{seed['project']['id']}/", expect=401)


def test_patch(seed, conn):
    tag = db.new_tag()
    created = http.post(seed["keys"][KEY], base(seed) + "/",
                        json={"name": f"CT Patch {tag}",
                              "identifier": f"P{tag.upper()}"[:12]}, expect=201).json()
    r = http.patch(seed["keys"][KEY], f"{base(seed)}/{created['id']}/",
                   json={"description": "patched via contract suite"}).json()
    assert r["description"] == "patched via contract suite"
    assert r["id"] == created["id"]


def test_patch_archived_400(seed):
    tag = db.new_tag()
    created = http.post(seed["keys"][KEY], base(seed) + "/",
                        json={"name": f"CT Arch {tag}",
                              "identifier": f"A{tag.upper()}"[:12]}, expect=201).json()
    http.post(seed["keys"][KEY], f"{base(seed)}/{created['id']}/archive/", expect=204)
    r = http.patch(seed["keys"][KEY], f"{base(seed)}/{created['id']}/",
                   json={"description": "must not apply"}, expect=400)
    assert r.json() == {"error": "Archived project cannot be updated"}
    http.delete(seed["keys"][KEY], f"{base(seed)}/{created['id']}/archive/", expect=204)


def test_delete(seed, conn):
    tag = db.new_tag()
    created = http.post(seed["keys"][KEY], base(seed) + "/",
                        json={"name": f"CT Gone {tag}",
                              "identifier": f"G{tag.upper()}"[:12]}, expect=201).json()
    db.ensure_not_default(conn, created["id"])
    http.delete(seed["keys"][KEY], f"{base(seed)}/{created['id']}/")
    http.get(seed["keys"][KEY], f"{base(seed)}/{created['id']}/", expect=404)


def test_delete_default_400(seed, conn):
    tag = db.new_tag()
    created = http.post(seed["keys"][KEY], base(seed) + "/",
                        json={"name": f"CT Deflt {tag}",
                              "identifier": f"D{tag.upper()}"[:12]}, expect=201).json()
    # The first project per workspace is auto-defaulted on save(); clear the
    # workspace default first (partial unique index allows exactly one).
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET is_default = false WHERE workspace_id = %s",
            (seed["ws_a"]["id"],),
        )
        cur.execute("UPDATE projects SET is_default = true WHERE id = %s", (created["id"],))
    conn.commit()
    try:
        r = http.delete(seed["keys"][KEY], f"{base(seed)}/{created['id']}/", expect=400)
        assert r.json() == {"error": "Default project cannot be deleted"}
    finally:
        with conn.cursor() as cur:
            cur.execute("UPDATE projects SET is_default = false WHERE id = %s", (created["id"],))
        conn.commit()
    http.delete(seed["keys"][KEY], f"{base(seed)}/{created['id']}/")


def test_archive_unarchive(seed, conn):
    tag = db.new_tag()
    created = http.post(seed["keys"][KEY], base(seed) + "/",
                        json={"name": f"CT Arc2 {tag}",
                              "identifier": f"R{tag.upper()}"[:12]}, expect=201).json()
    pid = created["id"]
    http.post(seed["keys"][KEY], f"{base(seed)}/{pid}/archive/", expect=204)
    row = db.fetch_one(conn, "SELECT archived_at FROM projects WHERE id = %s", (pid,))
    assert row["archived_at"] is not None
    http.delete(seed["keys"][KEY], f"{base(seed)}/{pid}/archive/", expect=204)
    row = db.fetch_one(conn, "SELECT archived_at FROM projects WHERE id = %s", (pid,))
    assert row["archived_at"] is None
    db.ensure_not_default(conn, pid)
    http.delete(seed["keys"][KEY], f"{base(seed)}/{pid}/")


def test_summary(seed):
    pid = seed["project"]["id"]
    body = http.get(seed["keys"][KEY], f"{base(seed)}/{pid}/summary/").json()
    assert body["id"] == pid
    assert body["identifier"] == seed["project"]["identifier"]
    assert set(body["counts"].keys()) == {
        "members", "states", "labels", "cycles", "modules", "issues", "intakes", "pages",
    }
    assert body["counts"]["members"] >= 2
    narrowed = http.get(seed["keys"][KEY], f"{base(seed)}/{pid}/summary/",
                        params={"fields": "members,states"}).json()
    assert set(narrowed["counts"].keys()) == {"members", "states"}


def test_denied_outsider(seed):
    # No workspace membership at all: safe-method permission fails -> 403.
    http.get(seed["keys"]["outsider"], base(seed) + "/", expect=403)
    http.get(seed["keys"]["outsider"],
             f"{base(seed)}/{seed['project']['id']}/", expect=403)


def test_isolation_other_workspace(seed):
    # owner_b belongs to ws_b only: ws_b lists its own project, never ws_a's.
    body = http.get(seed["keys"]["owner_b"],
                    f"/api/v1/workspaces/{seed['ws_b']['slug']}/projects/").json()
    ids = [p["id"] for p in body["results"]]
    assert seed["project_b"]["id"] in ids
    assert seed["project"]["id"] not in ids
    # And ws_a's project is unreachable through ws_b's slug: permission runs
    # before lookup, so a caller outside ws_b gets 403, not 404.
    http.get(seed["keys"][KEY],
             f"/api/v1/workspaces/{seed['ws_b']['slug']}/projects/{seed['project']['id']}/",
             expect=403)
