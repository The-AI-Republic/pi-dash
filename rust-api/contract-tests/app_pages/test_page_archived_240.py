"""Contract: archived-project pages (DIFF-48/49/50, NEWPIDASH-240).

Old Django returns 0 rows for the archived-project list and 404 on detail reads; Rust returns the pages with project_status fields on the list and 409 project_archived on every other read/write. Move is new in Rust only.
"""
import uuid
import pytest
from _harness.auth import login_session
from _harness.http import api_client
from _harness.diffs import expect
from .conftest import page_url, pages_base, _project_world

pytestmark = pytest.mark.contract
ARCHIVED_AT = "2026-11-13T18:57:37+00:00"
LIST_ROW_KEYS = {"id","name","owned_by","access","color","parent","parent_id","sub_pages_count","is_favorite","is_locked","archived_at","workspace","created_at","updated_at","created_by","updated_by","view_props","logo_props","label_ids","project_ids"}
DETAIL_ROW_KEYS = LIST_ROW_KEYS | {"description_html","description_json"}

def _client_for(settings, world):
    client = api_client(settings.base_url)
    login_session(client, email=world["owner"]["email"], password=world["owner"]["password"])
    return client

@pytest.fixture()
def archived_world(seeder, db):
    world = _project_world(seeder, page_name="Archived host page")
    db.execute("UPDATE projects SET archived_at = %s WHERE id = %s", (ARCHIVED_AT, world["project"]["id"]))
    yield world
    db.execute("UPDATE projects SET archived_at = NULL WHERE id = %s", (world["project"]["id"],))

@pytest.fixture()
def active_world(seeder):
    return _project_world(seeder, page_name="Active host page")

@pytest.fixture()
def second_project(seeder, archived_world):
    project = seeder.create_project(archived_world["workspace"]["id"])
    seeder.create_project_member(archived_world["workspace"]["id"], project["id"], archived_world["owner"]["id"], role=20)
    return project

def _assert_archived_body(response, world):
    body = response.json()
    assert set(body) == {"error","code","project","project_identifier","archived_at"}, body
    assert body["code"] == "project_archived"
    assert body["project"] == world["project"]["identifier"]
    assert body["project_identifier"] == world["project"]["identifier"]
    assert body["archived_at"] == "2026-11-13T18:57:37Z"

def test_list_archived_returns_pages_with_status(settings, archived_world):
    client = _client_for(settings, archived_world)
    response = client.get(page_url(archived_world))
    assert response.status_code == expect("DIFF-48", django=200, rust=200)
    rows = response.json()
    from _harness.diffs import is_rust
    if is_rust():
        assert len(rows) == 1, rows
        row = rows[0]
        assert set(row) == LIST_ROW_KEYS | {"project_status","project_archived_at"}, sorted(row)
        assert row["project_status"] == "archived"
        assert row["project_archived_at"] == "2026-11-13T18:57:37Z"
        assert row["id"] == archived_world["page"]["id"]
    else:
        assert rows == [], rows
    client.close()

def test_list_active_gains_only_project_status(settings, active_world):
    client = _client_for(settings, active_world)
    response = client.get(page_url(active_world))
    assert response.status_code == 200
    rows = response.json()
    assert len(rows) == 1
    row = rows[0]
    from _harness.diffs import is_rust
    if is_rust():
        assert set(row) == LIST_ROW_KEYS | {"project_status","project_archived_at"}, sorted(row)
        assert row["project_status"] == "active"
        assert row["project_archived_at"] is None
    else:
        assert set(row) == LIST_ROW_KEYS, sorted(row)
    client.close()

def test_get_archived_409(settings, archived_world):
    client = _client_for(settings, archived_world)
    response = client.get(page_url(archived_world, archived_world["page"]["id"]))
    assert response.status_code == expect("DIFF-49", django=404, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_world)
    client.close()

def test_update_archived_409(settings, archived_world):
    client = _client_for(settings, archived_world)
    response = client.patch(page_url(archived_world, archived_world["page"]["id"]), json={"name":"Nope"})
    # Django arm: patch on archived project still answers 200 (the view's
    # scoped fetch bypasses the archived filter for writes); Rust is 409.
    assert response.status_code == expect("DIFF-49", django=200, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_world)
    client.close()

def test_create_archived_409(settings, archived_world):
    client = _client_for(settings, archived_world)
    response = client.post(page_url(archived_world), json={"name":"Nope"})
    assert response.status_code == expect("DIFF-49", django=404, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_world)
    client.close()

def test_favorite_archived_409(settings, archived_world):
    client = _client_for(settings, archived_world)
    url = f"{pages_base(archived_world)}/favorite-pages/{archived_world['page']['id']}/"
    response = client.post(url)
    assert response.status_code == expect("DIFF-49", django=204, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_world)
    client.close()

def test_versions_archived_409(settings, archived_world):
    client = _client_for(settings, archived_world)
    url = page_url(archived_world, archived_world["page"]["id"]) + "versions/"
    response = client.get(url)
    assert response.status_code == expect("DIFF-49", django=200, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_world)
    client.close()

def test_lock_archived_409(settings, archived_world):
    client = _client_for(settings, archived_world)
    url = page_url(archived_world, archived_world["page"]["id"]) + "lock/"
    response = client.post(url)
    assert response.status_code == expect("DIFF-49", django=204, rust=409)
    if response.status_code == 409:
        _assert_archived_body(response, archived_world)
    client.close()

def test_move_archived_to_active_succeeds(settings, archived_world, second_project):
    client = _client_for(settings, archived_world)
    move_url = page_url(archived_world, archived_world["page"]["id"]) + "move/"
    response = client.post(move_url, json={"target_project_id": second_project["id"]})
    from _harness.diffs import is_rust
    if is_rust():
        assert response.status_code == 200, response.text
        target_base = f"/api/workspaces/{archived_world['workspace']['slug']}/projects/{second_project['id']}/pages/{archived_world['page']['id']}/"
        detail = client.get(target_base)
        assert detail.status_code == 200, detail.text
        assert detail.json()["name"] == "Archived host page"
        src_list = client.get(page_url(archived_world))
        assert src_list.status_code == 200
        ids = {r["id"] for r in src_list.json()}
        assert archived_world["page"]["id"] not in ids
        tgt_list_url = f"/api/workspaces/{archived_world['workspace']['slug']}/projects/{second_project['id']}/pages/"
        tgt_list = client.get(tgt_list_url)
        assert tgt_list.status_code == 200
        rows = tgt_list.json()
        assert any(r["id"] == archived_world["page"]["id"] for r in rows)
        assert all(r.get("project_status") == "active" for r in rows)
    else:
        assert response.status_code in (404, 405)
    client.close()

def test_move_to_archived_refused(settings, active_world, seeder, db):
    client = _client_for(settings, active_world)
    # Create archived target in same workspace
    target = seeder.create_project(active_world["workspace"]["id"])
    seeder.create_project_member(active_world["workspace"]["id"], target["id"], active_world["owner"]["id"], role=20)
    db.execute("UPDATE projects SET archived_at = %s WHERE id = %s", (ARCHIVED_AT, target["id"]))
    move_url = page_url(active_world, active_world["page"]["id"]) + "move/"
    response = client.post(move_url, json={"target_project_id": target["id"]})
    from _harness.diffs import is_rust
    if is_rust():
        assert response.status_code == 409, response.text
        assert response.json()["code"] == "project_archived"
    else:
        assert response.status_code in (404, 405)
    db.execute("UPDATE projects SET archived_at = NULL WHERE id = %s", (target["id"],))
    client.close()

def test_move_cross_workspace_refused(settings, active_world, seeder):
    client = _client_for(settings, active_world)
    # Second workspace/project via seeder
    from .conftest import _project_world
    world2 = _project_world(seeder, page_name="Other ws page")
    # Grant membership in ws2 to the caller so the workspace check (not gate) fires
    # Actually the move target gate resolves via source slug, so a UUID from another workspace will 404 or 400.
    # We try moving to world2 project.
    move_url = page_url(active_world, active_world["page"]["id"]) + "move/"
    response = client.post(move_url, json={"target_project_id": world2["project"]["id"]})
    from _harness.diffs import is_rust
    if is_rust():
        # Cross-workspace is 400; if the target gate fails first (403) for non-member, that also refuses.
        # Our impl checks workspace after target gate, but target gate is 403 for non-member of ws2.
        # So we grant membership to prove 400.
        # Since world2 is a new workspace, active_world owner is not a member of world2 project.
        # The test should grant membership before asserting 400.
        # For now, allow either 400 or 403 as refusal, but ideally 400 after granting.
        assert response.status_code in (400, 403), response.text
        if response.status_code == 400:
            pass
        else:
            # If 403, it's still a refusal; grant and retry to check 400 path
            seeder.create_project_member(world2["workspace"]["id"], world2["project"]["id"], active_world["owner"]["id"], role=20)
            seeder.create_workspace_member(world2["workspace"]["id"], active_world["owner"]["id"], role=20)
            # Need a fresh workspace_id check: source ws vs target ws different, so next attempt should be 400
            # Re-try with same client (client is for active_world owner, now member of ws2)
            response2 = client.post(move_url, json={"target_project_id": world2["project"]["id"]})
            assert response2.status_code == 400, response2.text
    else:
        assert response.status_code in (404, 405)
    client.close()

SWEEP = [
    ("desc-get", "get", "/description/", 200, {}),
    ("desc-patch", "patch", "/description/", 200, {"json": {"description_html": "<p>x</p>"}}),
    ("access", "post", "/access/", 204, {"json": {"access": 0}}),
    ("fav-del", "delete", "/favorite-pages:{pid}", 404, {}),
    ("archive", "post", "/archive/", 200, {}),
    ("unarchive", "delete", "/archive/", 204, {}),
    ("unlock", "delete", "/lock/", 204, {}),
    ("destroy", "delete", "/", 400, {}),
    ("duplicate", "post", "/duplicate/", 201, {}),
    ("summary", "get", "/pages-summary/", 200, {}),
    ("version-detail", "get", "/versions:{ver}/", 404, {}),
]

@pytest.mark.parametrize("name,method,suffix,django_status,kwargs", SWEEP)
def test_archived_409_sweep(settings, archived_world, name, method, suffix, django_status, kwargs):
    client = _client_for(settings, archived_world)
    pid = archived_world["page"]["id"]
    if suffix == "/favorite-pages:{pid}":
        url = f"{pages_base(archived_world)}/favorite-pages/{pid}/"
    elif suffix.startswith("/versions:"):
        url = page_url(archived_world, pid) + f"versions/{uuid.uuid4()}/"
    elif suffix == "/pages-summary/":
        url = f"{pages_base(archived_world)}/pages-summary/"
    elif suffix in ("/description/", "/access/", "/archive/", "/lock/", "/duplicate/"):
        url = page_url(archived_world, pid) + suffix.lstrip("/")
    else:
        url = page_url(archived_world, pid)
    response = getattr(client, method)(url, **kwargs)
    assert response.status_code == expect("DIFF-49", django=django_status, rust=409), name
    if response.status_code == 409:
        _assert_archived_body(response, archived_world)
    client.close()

def test_move_active_to_active_succeeds(settings, active_world, seeder):
    target = seeder.create_project(active_world["workspace"]["id"])
    seeder.create_project_member(active_world["workspace"]["id"], target["id"], active_world["owner"]["id"], role=20)
    client = _client_for(settings, active_world)
    move_url = page_url(active_world, active_world["page"]["id"]) + "move/"
    response = client.post(move_url, json={"target_project_id": target["id"]})
    from _harness.diffs import is_rust
    if is_rust():
        assert response.status_code == 200, response.text
        detail = client.get(f"/api/workspaces/{active_world['workspace']['slug']}/projects/{target['id']}/pages/{active_world['page']['id']}/")
        assert detail.status_code == 200, detail.text
        src_list = client.get(page_url(active_world))
        assert active_world["page"]["id"] not in {r["id"] for r in src_list.json()}
    else:
        assert response.status_code in (404, 405)
    client.close()

def test_move_unknown_page_404(settings, active_world, seeder):
    target = seeder.create_project(active_world["workspace"]["id"])
    seeder.create_project_member(active_world["workspace"]["id"], target["id"], active_world["owner"]["id"], role=20)
    client = _client_for(settings, active_world)
    move_url = page_url(active_world, str(uuid.uuid4())) + "move/"
    response = client.post(move_url, json={"target_project_id": target["id"]})
    from _harness.diffs import is_rust
    if is_rust():
        assert response.status_code == 404, response.text
    else:
        assert response.status_code in (404, 405)
    client.close()

def test_move_unknown_target_404(settings, active_world):
    client = _client_for(settings, active_world)
    move_url = page_url(active_world, active_world["page"]["id"]) + "move/"
    response = client.post(move_url, json={"target_project_id": str(uuid.uuid4())})
    from _harness.diffs import is_rust
    if is_rust():
        assert response.status_code == 404, response.text
    else:
        assert response.status_code in (404, 405)
    client.close()

def test_move_new_project_id_alias(settings, archived_world, second_project):
    client = _client_for(settings, archived_world)
    move_url = page_url(archived_world, archived_world["page"]["id"]) + "move/"
    response = client.post(move_url, json={"new_project_id": second_project["id"]})
    from _harness.diffs import is_rust
    if is_rust():
        assert response.status_code == 200, response.text
    else:
        assert response.status_code in (404, 405)
    client.close()

def test_move_self_is_noop(settings, active_world):
    client = _client_for(settings, active_world)
    move_url = page_url(active_world, active_world["page"]["id"]) + "move/"
    response = client.post(move_url, json={"target_project_id": active_world["project"]["id"]})
    from _harness.diffs import is_rust
    if is_rust():
        # Self-move is a no-op 200: the page stays linked and readable.
        assert response.status_code == 200, response.text
        assert response.json().get("project_status") == "active"
        detail = client.get(page_url(active_world, active_world["page"]["id"]))
        assert detail.status_code == 200, detail.text
        listing = client.get(page_url(active_world))
        assert listing.status_code == 200
        assert active_world["page"]["id"] in {r["id"] for r in listing.json()}
    else:
        assert response.status_code in (404, 405)
    client.close()

def test_move_self_archived_refused(settings, archived_world):
    client = _client_for(settings, archived_world)
    move_url = page_url(archived_world, archived_world["page"]["id"]) + "move/"
    response = client.post(move_url, json={"target_project_id": archived_world["project"]["id"]})
    from _harness.diffs import is_rust
    if is_rust():
        # Target is archived (source), so the move is refused with 409.
        assert response.status_code == 409, response.text
        _assert_archived_body(response, archived_world)
    else:
        assert response.status_code in (404, 405)
    client.close()

def test_move_ordering_403_before_409(settings, active_world, seeder, db):
    # Caller is a member of the source but NOT of the archived target:
    # must get 403 (gate), not the 409 that would name the archive.
    target = seeder.create_project(active_world["workspace"]["id"])
    db.execute("UPDATE projects SET archived_at = %s WHERE id = %s", (ARCHIVED_AT, target["id"]))
    client = _client_for(settings, active_world)
    move_url = page_url(active_world, active_world["page"]["id"]) + "move/"
    response = client.post(move_url, json={"target_project_id": target["id"]})
    from _harness.diffs import is_rust
    if is_rust():
        assert response.status_code == 403, response.text
    else:
        assert response.status_code in (404, 405)
    db.execute("UPDATE projects SET archived_at = NULL WHERE id = %s", (target["id"],))
    client.close()
