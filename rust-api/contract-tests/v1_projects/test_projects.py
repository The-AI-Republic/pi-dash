"""Contract tests: api-v1 project endpoints (urls/project.py, 4 URL entries).

Covers list/create, detail get/patch/delete, archive/unarchive and summary,
plus identifier-slug routing, one denied case and one tenant-isolation case.
"""

import sys
from pathlib import Path

import pytest
from psycopg.types.json import Json

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
    # Default pod (runner post_save signal): one active row, named
    # {identifier}_pod_1, carrying the signal's description verbatim.
    pod = db.fetch_one(
        conn,
        "SELECT name, description, is_default, created_by_id FROM pod"
        " WHERE project_id = %s AND deleted_at IS NULL", (body["id"],))
    assert pod == {"name": "%s_pod_1" % ident,
                   "description": "Auto-created default pod. Add tier pods anytime.",
                   "is_default": True, "created_by_id": None}
    # bulk_create bypasses State.save(), so the 8 seeded default states
    # keep the slug field default '' (db/models/state.py:97,132).
    with conn.cursor() as cur:
        cur.execute(
            "SELECT name, slug FROM states WHERE project_id = %s ORDER BY sequence",
            (body["id"],),
        )
        seeded = cur.fetchall()
    assert [name for name, _ in seeded] == [
        "Backlog", "Todo", "In Progress", "In Review",
        "In Test", "Done", "Cancelled", "Triage",
    ]
    assert [slug for _, slug in seeded] == [""] * 8
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


def _partial_create_rows(conn, workspace_id, identifier):
    """Rows the project_lead create path leaves behind (PB-2023).

    Returns (project_id, members, state_count, pod_name). ``members`` is a
    sorted list of ``(member_id, role)``.
    """
    proj = db.fetch_one(
        conn,
        "SELECT id, project_lead_id FROM projects"
        " WHERE identifier = %s AND workspace_id = %s AND deleted_at IS NULL",
        (identifier, workspace_id),
    )
    assert proj is not None, "project row was not committed"
    with conn.cursor() as cur:
        cur.execute(
            "SELECT member_id, role FROM project_members WHERE project_id = %s",
            (proj["id"],),
        )
        members = sorted((str(m), r) for m, r in cur.fetchall())
        cur.execute(
            "SELECT count(*) FROM states WHERE project_id = %s", (proj["id"],))
        state_count = cur.fetchone()[0]
    pod = db.fetch_one(
        conn,
        "SELECT name FROM pod WHERE project_id = %s AND deleted_at IS NULL",
        (proj["id"],),
    )
    return proj, members, state_count, pod


def test_create_with_project_lead_of_member(seed, conn):
    """Ported Django quirk (PB-2023, views/project.py:230-238): a create that
    names any workspace member as project_lead answers 400
    {"error": "Please provide valid detail"} after committing the project row
    plus the creator's membership — no lead membership, no states."""
    tag = db.new_tag()
    ident = f"L{tag.upper()}"[:12]
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": f"CT Lead {tag}", "identifier": ident,
                        "project_lead": seed["member"]["id"]}, expect=400)
    assert r.status_code == 400
    assert r.json() == {"error": "Please provide valid detail"}
    proj, members, state_count, pod = _partial_create_rows(
        conn, seed["ws_a"]["id"], ident)
    assert str(proj["project_lead_id"]) == seed["member"]["id"]
    assert members == [(seed["owner"]["id"], db.ADMIN)]
    assert state_count == 0
    # The post_save signal still ran before the raise.
    assert pod == {"name": f"{ident}_pod_1"}


def test_create_with_self_project_lead(seed, conn):
    """Same quirk for the creator's own UUID: the guard compares a User
    string against a UUID string, so the self case 400s too."""
    tag = db.new_tag()
    ident = f"S{tag.upper()}"[:12]
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": f"CT Self {tag}", "identifier": ident,
                        "project_lead": seed["owner"]["id"]}, expect=400)
    assert r.status_code == 400
    assert r.json() == {"error": "Please provide valid detail"}
    proj, members, state_count, pod = _partial_create_rows(
        conn, seed["ws_a"]["id"], ident)
    assert str(proj["project_lead_id"]) == seed["owner"]["id"]
    assert members == [(seed["owner"]["id"], db.ADMIN)]
    assert state_count == 0
    assert pod == {"name": f"{ident}_pod_1"}


def _deactivated_member(conn, seed, tag):
    """A workspace MEMBER whose users.is_active is false (PIDASHCONV-1394).

    Mirrors account deactivation (app/views/user/base.py:279-346), which flips
    users.is_active but leaves the membership row in place: Django's lead /
    assignee FK querysets (stock UserManager) and the validate() membership
    check filter on neither, so the member stays assignable.
    """
    cand = db.create_user(conn, tag + "d", first_name="CtDeact")
    db.add_workspace_member(conn, seed["ws_a"]["id"], cand["id"], db.MEMBER)
    with conn.cursor() as cur:
        cur.execute("UPDATE users SET is_active = false WHERE id = %s", (cand["id"],))
    conn.commit()
    return cand


def test_patch_deactivated_member_as_lead_and_assignee(seed, conn):
    """A deactivated workspace member stays assignable (PIDASHCONV-1394).

    PATCHing project_lead / default_assignee to a member with
    users.is_active=false answers 200 with the id echoed back, and the FK is
    written — on both backends. A client echoing a project whose lead has
    since deactivated must not get a 400.
    """
    tag = db.new_tag()
    cand = _deactivated_member(conn, seed, tag)
    pid = db.create_project(conn, seed["ws_a"]["id"], tag + "p",
                            created_by_id=seed["owner"]["id"])["id"]
    db.add_project_member(conn, seed["ws_a"]["id"], pid, seed["owner"]["id"], db.ADMIN)
    r = http.patch(seed["keys"][KEY], f"{base(seed)}/{pid}/",
                   json={"project_lead": cand["id"]})
    assert r.json()["project_lead"] == cand["id"]
    saved = db.fetch_one(conn, "SELECT project_lead_id FROM projects WHERE id = %s", (pid,))
    assert str(saved["project_lead_id"]) == cand["id"]
    r = http.patch(seed["keys"][KEY], f"{base(seed)}/{pid}/",
                   json={"default_assignee": cand["id"]})
    assert r.json()["default_assignee"] == cand["id"]
    saved = db.fetch_one(conn, "SELECT default_assignee_id FROM projects WHERE id = %s", (pid,))
    assert str(saved["default_assignee_id"]) == cand["id"]


def test_create_with_deactivated_project_lead(seed, conn):
    """Create with a deactivated lead follows the PB-2023 path (PIDASHCONV-1394).

    The FK validates (no is_active filter), so the create commits the project
    row plus the creator's membership and then answers 400
    {"error": "Please provide valid detail"} — same rows as an active lead.
    """
    tag = db.new_tag()
    cand = _deactivated_member(conn, seed, tag)
    ident = f"X{tag.upper()}"[:12]
    r = http.post(seed["keys"][KEY], base(seed) + "/",
                  json={"name": f"CT DeactLead {tag}", "identifier": ident,
                        "project_lead": cand["id"]}, expect=400)
    assert r.status_code == 400
    assert r.json() == {"error": "Please provide valid detail"}
    proj, members, state_count, pod = _partial_create_rows(
        conn, seed["ws_a"]["id"], ident)
    assert str(proj["project_lead_id"]) == cand["id"]
    assert members == [(seed["owner"]["id"], db.ADMIN)]
    assert state_count == 0
    assert pod == {"name": f"{ident}_pod_1"}


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


def test_expand_user_null_email(seed, conn):
    # users.email is nullable (db/models/user.py:61); expanding a NULL-email
    # user on any project user FK must render "email": null, not 500
    # (PIDASHCONV-514).
    tag = db.new_tag()
    user = db.create_user(conn, tag, first_name="CtNull", email=None)
    assert user["email"] is None
    pid = db.create_project(conn, seed["ws_a"]["id"], tag + "e",
                            created_by_id=user["id"])["id"]
    db.add_project_member(conn, seed["ws_a"]["id"], pid, seed["owner"]["id"], db.ADMIN)
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET default_assignee_id = %s, project_lead_id = %s,"
            " updated_by_id = %s WHERE id = %s",
            (user["id"], user["id"], user["id"], pid),
        )
    conn.commit()
    key = seed["keys"][KEY]
    expands = ("default_assignee", "project_lead", "created_by", "updated_by")
    for expand in expands:
        body = http.get(key, f"{base(seed)}/{pid}/", params={"expand": expand}).json()
        assert body[expand]["id"] == user["id"]
        assert body[expand]["email"] is None
    body = http.get(key, base(seed) + "/",
                    params={"expand": ",".join(expands)}).json()
    row = next(p for p in body["results"] if p["id"] == pid)
    for expand in expands:
        assert row[expand]["id"] == user["id"]
        assert row[expand]["email"] is None


def test_expand_user_avatar_asset_unresolvable_renders_null(seed, conn):
    # avatar_url returns the asset URL as-is when an asset is set
    # (db/models/user.py:142-151): an unmapped entity type renders null even
    # with non-empty avatar text — no fall-through (PIDASHCONV-514).
    tag = db.new_tag()
    user = db.create_user(conn, tag, first_name="CtAv")
    aid = db.create_file_asset(conn, "DRAFT_ISSUE_ATTACHMENT")
    db.set_user_avatar(conn, user["id"], asset_id=aid, avatar="https://cdn.example/legacy.png")
    pid = db.create_project(conn, seed["ws_a"]["id"], tag + "v",
                            created_by_id=user["id"])["id"]
    db.add_project_member(conn, seed["ws_a"]["id"], pid, seed["owner"]["id"], db.ADMIN)
    key = seed["keys"][KEY]
    body = http.get(key, f"{base(seed)}/{pid}/", params={"expand": "created_by"}).json()
    assert body["created_by"]["avatar"] == "https://cdn.example/legacy.png"
    assert body["created_by"]["avatar_url"] is None
    # Sanity: a mapped type on the same row resolves, proving the seed shape.
    aid2 = db.create_file_asset(conn, "USER_AVATAR")
    db.set_user_avatar(conn, user["id"], asset_id=aid2, avatar="https://cdn.example/legacy.png")
    body = http.get(key, f"{base(seed)}/{pid}/", params={"expand": "created_by"}).json()
    assert body["created_by"]["avatar_url"] == f"/api/assets/v2/static/{aid2}/"


def test_expand_null_fk_renders_empty_object(seed, conn):
    # A NULL user FK expands to {} (UserLiteSerializer(None).data), not null
    # (serializers/base.py:108-113, probed). Unlike
    # test_expand_user_null_email (which sets all four FKs), this test leaves
    # every FK NULL (PIDASHCONV-515).
    tag = db.new_tag()
    pid = db.create_project(conn, seed["ws_a"]["id"], tag + "n",
                            created_by_id=None)["id"]
    db.add_project_member(conn, seed["ws_a"]["id"], pid, seed["owner"]["id"], db.ADMIN)
    row = db.fetch_one(
        conn,
        "SELECT default_assignee_id, project_lead_id, created_by_id,"
        " updated_by_id FROM projects WHERE id = %s",
        (pid,),
    )
    assert row == {
        "default_assignee_id": None,
        "project_lead_id": None,
        "created_by_id": None,
        "updated_by_id": None,
    }
    key = seed["keys"][KEY]
    expands = ("default_assignee", "project_lead", "created_by", "updated_by")
    # Sanity: unexpanded, the FKs render null.
    body = http.get(key, f"{base(seed)}/{pid}/").json()
    for expand in expands:
        assert body[expand] is None
    # Detail: each FK individually, then all four together.
    for expand in expands:
        body = http.get(key, f"{base(seed)}/{pid}/", params={"expand": expand}).json()
        assert body[expand] == {}
    body = http.get(key, f"{base(seed)}/{pid}/",
                    params={"expand": ",".join(expands)}).json()
    for expand in expands:
        assert body[expand] == {}
    # List: each FK individually, then all four together.
    for expand in expands:
        body = http.get(key, base(seed) + "/", params={"expand": expand}).json()
        row = next(p for p in body["results"] if p["id"] == pid)
        assert row[expand] == {}
    body = http.get(key, base(seed) + "/",
                    params={"expand": ",".join(expands)}).json()
    row = next(p for p in body["results"] if p["id"] == pid)
    for expand in expands:
        assert row[expand] == {}


# --- Unknown workspace slug vs project-ref rewrite order (PIDASHCONV-1147) ---
# Django's `_rewrite_project_kwarg` (api/views/base.py:51-98) runs in
# `BaseAPIView.initial` *before* `check_permissions`, so a non-UUID project
# ref under an unknown workspace slug resolves through
# `Project.resolve(workspace_slug, raw)` (db/models/project.py:192-219) and
# 404s `{"detail":"Project not found"}`. A UUID-form ref passes through
# unverified, so it reaches the permission gate and 403s.

PROJECT_NOT_FOUND = {"detail": "Project not found"}
PERMISSION_DENIED = {
    "detail": "You do not have permission to perform this action."
}


def unknown_base(seed):
    return f"/api/v1/workspaces/ct-unknown-{seed['tag']}/projects"


@pytest.mark.parametrize("ref", ["ENG", "not-a-project"])
def test_unknown_workspace_non_uuid_ref_404(seed, ref):
    key = seed["keys"][KEY]
    b = f"{unknown_base(seed)}/{ref}"
    responses = (
        http.get(key, b + "/", expect=404),
        http.patch(key, b + "/", json={"description": "x"}, expect=404),
        http.delete(key, b + "/", expect=404),
        http.post(key, b + "/archive/", expect=404),
        http.delete(key, b + "/archive/", expect=404),
        http.get(key, b + "/summary/", expect=404),
    )
    for r in responses:
        assert r.json() == PROJECT_NOT_FOUND


def test_unknown_workspace_uuid_ref_403(seed):
    key = seed["keys"][KEY]
    b = f"{unknown_base(seed)}/{seed['project']['id']}"
    responses = (
        http.get(key, b + "/", expect=403),
        http.patch(key, b + "/", json={"description": "x"}, expect=403),
        http.delete(key, b + "/", expect=403),
        http.post(key, b + "/archive/", expect=403),
        http.delete(key, b + "/archive/", expect=403),
        http.get(key, b + "/summary/", expect=403),
    )
    for r in responses:
        assert r.json() == PERMISSION_DENIED
# --- cover image removed (PDASHOSS01-237) -----------------------------------
#
# The project cover columns are gone (db migration 0169): no read shape carries
# `cover_image`, `cover_image_asset` or `cover_image_url`, and
# `ProjectCreateSerializer.Meta.fields` no longer lists `cover_image`, so a
# `cover_image` key in a create/update body is an unknown field that
# `ModelSerializer` ignores.

COVER_KEYS = {"cover_image", "cover_image_asset", "cover_image_url"}


def test_project_reads_carry_no_cover_keys(seed):
    key = seed["keys"][KEY]
    pid = seed["project"]["id"]
    detail = http.get(key, f"{base(seed)}/{pid}/").json()
    assert not COVER_KEYS & set(detail)
    body = http.get(key, base(seed) + "/", params={"per_page": "100"}).json()
    row = next(p for p in body["results"] if p["id"] == pid)
    assert not COVER_KEYS & set(row)


def test_cover_image_body_key_is_ignored(seed):
    key = seed["keys"][KEY]
    tag = db.new_tag()
    created = http.post(
        key,
        base(seed) + "/",
        json={"name": f"CT Cover {tag}", "identifier": f"V{tag.upper()}"[:12],
              "cover_image": "https://example.com/cover.png"},
        expect=201,
    ).json()
    assert not COVER_KEYS & set(created)
    patched = http.patch(
        key,
        f"{base(seed)}/{created['id']}/",
        json={"cover_image": "https://example.com/other.png"},
    ).json()
    assert not COVER_KEYS & set(patched)
    assert patched["name"] == created["name"]


def test_detail_renders_stored_network_and_description_json(seed, conn):
    """Detail echoes stored network + JSON descriptions (PIDASHCONV-1388 twin).

    HTTP-side twin of the tasks_webhooks project-chain tests: the worker
    fan-out must decode these same columns (INT2 network, jsonb
    descriptions). Seeded via SQL because create ignores the description
    fields (ProjectCreateSerializer.Meta.fields) — the read path is what
    this pins.
    """
    pid = db.create_project(conn, seed["ws_a"]["id"], db.new_tag(),
                            created_by_id=seed["owner"]["id"])["id"]
    db.add_project_member(conn, seed["ws_a"]["id"], pid, seed["owner"]["id"], db.ADMIN)
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET network = 2, description_text = %s,"
            " description_html = %s WHERE id = %s",
            (Json({"ops": "text"}), Json({"a": 1}), pid),
        )
    conn.commit()
    body = http.get(seed["keys"][KEY], f"{base(seed)}/{pid}/").json()
    assert body["network"] == 2
    assert body["description_text"] == {"ops": "text"}
    assert body["description_html"] == {"a": 1}
