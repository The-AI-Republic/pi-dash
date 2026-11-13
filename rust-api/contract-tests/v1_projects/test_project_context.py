"""Public API: project `context` field (PDASHOSS01-294).

`ProjectCreateSerializer` / `ProjectUpdateSerializer` accept `context`, and
the `__all__` `ProjectSerializer` returns it right after `description`.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _harness import db, http  # noqa: E402

KEY = "projects"
CONTEXT = "Never push to main.\nPrefer additive migrations."


def base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects"


def _stored(conn, project_id):
    with conn.cursor() as cur:
        cur.execute("SELECT description, context FROM projects WHERE id = %s", (project_id,))
        return cur.fetchone()


def test_create_round_trips_context(seed, conn):
    tag = db.new_tag()
    created = http.post(seed["keys"][KEY], base(seed) + "/",
                        json={"name": f"CT Ctx {tag}", "identifier": f"X{tag.upper()}"[:12],
                              "description": "Summary", "context": CONTEXT}).json()
    assert created["description"] == "Summary"
    assert created["context"] == CONTEXT
    keys = list(created)
    assert keys.index("context") == keys.index("description") + 1
    detail = http.get(seed["keys"][KEY], f"{base(seed)}/{created['id']}/").json()
    assert detail["context"] == CONTEXT
    assert tuple(_stored(conn, created["id"])) == ("Summary", CONTEXT)


def test_create_without_context_defaults_to_empty(seed, conn):
    tag = db.new_tag()
    created = http.post(seed["keys"][KEY], base(seed) + "/",
                        json={"name": f"CT NoCtx {tag}",
                              "identifier": f"N{tag.upper()}"[:12]}).json()
    assert created["context"] == ""
    assert tuple(_stored(conn, created["id"])) == ("", "")


def test_patch_context_leaves_description(seed, conn):
    tag = db.new_tag()
    created = http.post(seed["keys"][KEY], base(seed) + "/",
                        json={"name": f"CT PCtx {tag}", "identifier": f"Q{tag.upper()}"[:12],
                              "description": "Summary"}).json()
    r = http.patch(seed["keys"][KEY], f"{base(seed)}/{created['id']}/",
                   json={"context": CONTEXT}).json()
    assert r["context"] == CONTEXT
    assert r["description"] == "Summary"
    assert tuple(_stored(conn, created["id"])) == ("Summary", CONTEXT)


def test_null_context_400(seed):
    tag = db.new_tag()
    created = http.post(seed["keys"][KEY], base(seed) + "/",
                        json={"name": f"CT Null {tag}",
                              "identifier": f"Z{tag.upper()}"[:12]}).json()
    r = http.patch(seed["keys"][KEY], f"{base(seed)}/{created['id']}/",
                   json={"context": None}, expect=400)
    assert r.json() == {"context": ["This field may not be null."]}
