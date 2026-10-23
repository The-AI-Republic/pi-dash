"""Contract tests: project `context` field (PDASHOSS01-294).

`Project.context` holds the agent-facing instructions that used to live in
`description`. It is a plain `TextField(blank=True, default="")` on the
`__all__` serializers, so create, PATCH and detail round-trip it next to
`description`, and DRF rejects `null`.
"""

import uuid

from psycopg.rows import dict_row

CONTEXT = "Never push to main.\nPrefer additive migrations — always."


def _create(client, slug, **extra):
    tag = uuid.uuid4().hex[:8].upper()
    body = {"name": "Ctx Project %s" % tag, "identifier": ("C%s" % tag)[:10]}
    body.update(extra)
    resp = client.post(f"/api/workspaces/{slug}/projects/", json=body)
    assert resp.status_code == 201, resp.text[:500]
    return resp.json()


def _stored(world, project_id):
    with world.conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT description, context FROM projects WHERE id = %s",
                    (str(project_id),))
        return cur.fetchone()


def test_create_round_trips_context(world):
    client, _, ws, _ = world.full_stack()
    created = _create(client, ws["slug"], description="Summary", context=CONTEXT)
    assert created["description"] == "Summary"
    assert created["context"] == CONTEXT
    keys = list(created)
    assert keys.index("context") == keys.index("description") + 1
    detail = client.get(f"/api/workspaces/{ws['slug']}/projects/{created['id']}/")
    assert detail.status_code == 200
    assert detail.json()["context"] == CONTEXT
    assert _stored(world, created["id"]) == {"description": "Summary", "context": CONTEXT}


def test_create_without_context_defaults_to_empty(world):
    client, _, ws, project = world.full_stack()
    assert project["context"] == ""
    assert _stored(world, project["id"])["context"] == ""


def test_patch_context_leaves_description(world):
    client, _, ws, _ = world.full_stack()
    created = _create(client, ws["slug"], description="Summary")
    url = f"/api/workspaces/{ws['slug']}/projects/{created['id']}/"
    resp = client.patch(url, json={"context": CONTEXT})
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["context"] == CONTEXT
    assert resp.json()["description"] == "Summary"
    resp = client.patch(url, json={"description": "Edited"})
    assert resp.status_code == 200, resp.text[:500]
    assert _stored(world, created["id"]) == {"description": "Edited", "context": CONTEXT}
    resp = client.patch(url, json={"context": ""})
    assert resp.status_code == 200, resp.text[:500]
    assert _stored(world, created["id"]) == {"description": "Edited", "context": ""}


def test_long_context_round_trips(world):
    client, _, ws, _ = world.full_stack()
    long_text = ("Rule %d: keep the port byte-identical.\n" * 300) % tuple(range(300))
    created = _create(client, ws["slug"], context=long_text)
    # DRF `CharField(trim_whitespace=True)`: the trailing newline is dropped.
    assert created["context"] == long_text.strip()
    assert _stored(world, created["id"])["context"] == long_text.strip()


def test_null_context_400(world):
    client, _, ws, project = world.full_stack()
    resp = client.patch(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/", json={"context": None})
    assert resp.status_code == 400
    assert resp.json() == {"context": ["This field may not be null."]}
