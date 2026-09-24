"""Contract tests: state URL module (4 routes)."""

from conftest import assert_keys

STATE_KEYS = [
    "color", "default", "description", "group", "id", "name",
    "project_id", "sequence", "workspace_id",
]

LIST_EXTRA = ["order"]


def _states(client, ws, project):
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/")
    assert resp.status_code == 200
    return resp.json()


def test_list_states_shape(world):
    client, _, ws, project = world.full_stack()
    body = _states(client, ws, project)
    assert isinstance(body, list) and len(body) == 7
    for item in body:
        assert_keys(item, STATE_KEYS + LIST_EXTRA, "states-list")
        assert 0 < item["order"] <= 1
    by_group = {}
    for item in body:
        by_group.setdefault(item["group"], []).append(item["name"])
    assert by_group["backlog"] == ["Backlog"]
    assert "triage" not in by_group


def test_list_states_grouped(world):
    client, _, ws, project = world.full_stack()
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/",
        params={"grouped": "true"})
    assert resp.status_code == 200
    body = resp.json()
    assert isinstance(body, dict)
    assert body["backlog"][0]["name"] == "Backlog"


def test_retrieve_state_shape(world):
    client, _, ws, project = world.full_stack()
    first = _states(client, ws, project)[0]
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/{first['id']}/")
    assert resp.status_code == 200
    assert_keys(resp.json(), STATE_KEYS, "state-retrieve")


def test_retrieve_missing_state_404(world):
    client, _, ws, project = world.full_stack()
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/"
        "00000000-0000-0000-0000-000000000000/")
    assert resp.status_code == 404


def test_create_state_shape(world, db):
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/",
        json={"name": "Contract New", "color": "#FF0000", "group": "unstarted",
              "sequence": 45000})
    assert resp.status_code == 200
    assert_keys(resp.json(), STATE_KEYS, "state-create")
    assert resp.json()["name"] == "Contract New"
    with db.cursor() as cur:
        cur.execute('SELECT "group" FROM states WHERE id=%s', (resp.json()["id"],))
        assert cur.fetchone()["group"] == "unstarted"


def test_create_duplicate_state_400(world):
    client, _, ws, project = world.full_stack()
    body = {"name": "Todo", "color": "#000000", "group": "unstarted", "sequence": 1}
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/", json=body)
    assert resp.status_code == 400
    assert resp.json() == {"name": "The state name is already taken"}


def test_create_triage_state_400(world):
    client, _, ws, project = world.full_stack()
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/",
        json={"name": "Sneaky", "color": "#000000", "group": "triage", "sequence": 1})
    assert resp.status_code == 400


def test_partial_update_state(world):
    client, _, ws, project = world.full_stack()
    first = _states(client, ws, project)[0]
    resp = client.patch(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/{first['id']}/",
        json={"color": "#123456", "description": "updated"})
    assert resp.status_code == 200
    assert_keys(resp.json(), STATE_KEYS, "state-partial")
    assert resp.json()["color"] == "#123456"


def test_mark_default_flips(world, db):
    client, _, ws, project = world.full_stack()
    states = _states(client, ws, project)
    current = [s for s in states if s["default"]][0]
    target = [s for s in states if not s["default"]][0]
    resp = client.post(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/{target['id']}/"
        "mark-default/")
    assert resp.status_code == 204
    with db.cursor() as cur:
        cur.execute('SELECT "default" FROM states WHERE id=%s', (target["id"],))
        assert cur.fetchone()["default"] is True
        cur.execute('SELECT "default" FROM states WHERE id=%s', (current["id"],))
        assert cur.fetchone()["default"] is False


def test_destroy_state(world, db):
    from _harness import seed as _seed
    client, _, ws, project = world.full_stack()
    extra = _seed.state(world.conn, project["id"], ws["id"], name="Doomed")
    resp = client.delete(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/{extra['id']}/")
    assert resp.status_code == 204
    # Deletes are soft: the row stays with deleted_at set.
    with db.cursor() as cur:
        cur.execute("SELECT deleted_at FROM states WHERE id=%s", (str(extra["id"]),))
        assert cur.fetchone()["deleted_at"] is not None


def test_destroy_default_state_400(world):
    client, _, ws, project = world.full_stack()
    default = [s for s in _states(client, ws, project) if s["default"]][0]
    resp = client.delete(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/{default['id']}/")
    assert resp.status_code == 400
    assert resp.json() == {"error": "Default state cannot be deleted"}


def test_destroy_nonempty_state_400(world, db):
    client, _, ws, project = world.full_stack()
    extra = _seed_state(world, ws, project)
    import uuid as _uuid
    with db.cursor() as cur:
        cur.execute(
            'INSERT INTO issues (id, name, description_json, priority, sequence_id,'
            ' project_id, workspace_id, description_html, sort_order, is_draft,'
            ' git_work_branch, workpad, complexity_score, state_id,'
            ' created_at, updated_at)'
            ' VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s,'
            ' now(), now())',
            (str(_uuid.uuid4()), "Blocking issue", "{}", "none", 1,
             str(project["id"]), str(ws["id"]),
             "", 0.0, False, "", "", 0, str(extra["id"])))
    db.commit()
    resp = client.delete(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/states/{extra['id']}/")
    assert resp.status_code == 400
    assert resp.json() == {"error": "The state is not empty, only empty states can be deleted"}


def _seed_state(world, ws, project):
    from _harness import seed as _seed
    return _seed.state(world.conn, project["id"], ws["id"], name="Occupied")


def test_intake_state_shape(world):
    client, _, ws, project = world.full_stack()
    resp = client.get(
        f"/api/workspaces/{ws['slug']}/projects/{project['id']}/intake-state/")
    assert resp.status_code == 200
    assert_keys(resp.json(), STATE_KEYS, "intake-state")
    assert resp.json()["group"] == "triage"
