"""Contract tests: app ``Project.default_work_type`` (PDASHOSS01-234).

``ProjectSerializer`` (create / PATCH) runs ``validate_default_work_type``
(a registered key or blank) after the ``CharField(max_length=32)`` checks;
``Project.save()`` resolves a blank value at creation (``software`` with a
repo, else ``general``). The read serializers (``fields="__all__"``) render
the key right after ``default_agent_executor``. PUT goes through
``ProjectListSerializer``, which has no ``validate_default_work_type``.
"""

import uuid

from psycopg.rows import dict_row

UNKNOWN = ["unknown work type; expected one of ['general', 'software']"]


def _url(ws, pid=None):
    base = f"/api/workspaces/{ws['slug']}/projects/"
    return base if pid is None else f"{base}{pid}/"


def _create(client, ws, **extra):
    tag = uuid.uuid4().hex[:8].upper()
    body = {"name": "WorkType %s" % tag, "identifier": "T%s" % tag[:9], **extra}
    return client.post(_url(ws), json=body)


def _stored(db, pid):
    with db.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT default_work_type FROM projects WHERE id=%s", (str(pid),))
        return cur.fetchone()["default_work_type"]


def _assert_after_executor(item):
    keys = list(item.keys())
    assert keys.index("default_work_type") == keys.index("default_agent_executor") + 1, keys


def test_create_resolves_from_repo(world, db):
    client, _, ws, project = world.full_stack()
    # The world project was created through the API without a repo.
    assert project["default_work_type"] == "general"
    _assert_after_executor(project)
    resp = _create(client, ws, repo_url="https://github.com/example/repo.git")
    assert resp.status_code == 201, resp.text
    assert resp.json()["default_work_type"] == "software"
    assert _stored(db, resp.json()["id"]) == "software"
    resp = _create(client, ws, default_work_type="")
    assert resp.status_code == 201, resp.text
    assert resp.json()["default_work_type"] == "general"


def test_create_explicit_value_kept(world, db):
    client, _, ws, _ = world.full_stack()
    resp = _create(client, ws, default_work_type="software")
    assert resp.status_code == 201, resp.text
    assert resp.json()["default_work_type"] == "software"
    assert _stored(db, resp.json()["id"]) == "software"


def test_create_unknown_value_400(world):
    client, _, ws, _ = world.full_stack()
    resp = _create(client, ws, default_work_type="bogus")
    assert resp.status_code == 400, resp.text
    assert resp.json() == {"default_work_type": UNKNOWN}


def test_create_too_long_value_400(world):
    client, _, ws, _ = world.full_stack()
    resp = _create(client, ws, default_work_type="x" * 33)
    assert resp.status_code == 400, resp.text
    assert resp.json() == {
        "default_work_type": ["Ensure this field has no more than 32 characters."]
    }


def test_list_detail_and_retrieve_key_position(world):
    client, _, ws, project = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/details/")
    assert resp.status_code == 200
    _assert_after_executor(resp.json()[0])
    assert resp.json()[0]["default_work_type"] == "general"
    resp = client.get(_url(ws, project["id"]))
    assert resp.status_code == 200
    _assert_after_executor(resp.json())
    # The lite list serializer does not carry the field.
    resp = client.get(_url(ws))
    assert resp.status_code == 200
    assert "default_work_type" not in resp.json()[0]


def test_patch_validates_and_stores(world, db):
    client, _, ws, project = world.full_stack()
    resp = client.patch(_url(ws, project["id"]), json={"default_work_type": "bogus"})
    assert resp.status_code == 400, resp.text
    assert resp.json() == {"default_work_type": UNKNOWN}
    resp = client.patch(_url(ws, project["id"]), json={"default_work_type": "software"})
    assert resp.status_code == 200, resp.text
    assert resp.json()["default_work_type"] == "software"
    _assert_after_executor(resp.json())
    assert _stored(db, project["id"]) == "software"
    # Blank is accepted on update and stored as-is (no create-time resolve).
    resp = client.patch(_url(ws, project["id"]), json={"default_work_type": ""})
    assert resp.status_code == 200, resp.text
    assert _stored(db, project["id"]) == ""


def test_put_skips_work_type_validation(world):
    client, _, ws, project = world.full_stack()
    resp = client.put(
        _url(ws, project["id"]),
        json={"name": "Full Put", "identifier": project["identifier"],
              "default_work_type": "bogus"},
    )
    assert resp.status_code == 400
    assert sorted(resp.json().keys()) == ["deleted_at", "workspace"]
