"""Contract tests: projects.settings reads and PATCH (NEWPIDASH-278).

Full-serializer reads render the validated settings with defaults
filled in (``{}`` until the first key lands with NEWPIDASH-261); the
compact ``.values()`` list projection has no settings key, like the
Django projection. PATCH accepts a nested ``settings`` object and
merges it atomically. Django's app serializers use
``fields = "__all__"``, so the bag is stored unvalidated; the strict
PATCH validation on Rust is the deliberate difference DIFF-41.
"""

from _harness.diffs import expect, is_rust


def test_retrieve_includes_settings_with_defaults(world):
    client, user, ws, project = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/")
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["settings"] == {}


def test_details_list_includes_settings_with_defaults(world):
    client, user, ws, project = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/details/")
    assert resp.status_code == 200, resp.text[:500]
    rows = resp.json()
    assert isinstance(rows, list) and len(rows) == 1
    assert rows[0]["settings"] == {}


def test_compact_list_has_no_settings_key(world):
    client, user, ws, project = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/")
    assert resp.status_code == 200, resp.text[:500]
    assert "settings" not in resp.json()[0]


def test_patch_empty_settings_is_noop(world):
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = client.patch(url, json={"settings": {}})
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["settings"] == {}


def test_patch_unknown_settings_key_is_400(world):
    # DIFF-41: Django `__all__` stores any blob; Rust validates.
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = client.patch(url, json={"settings": {"unknown_key": 1}})
    assert resp.status_code == expect("DIFF-41", django=200, rust=400)
    if is_rust():
        assert resp.json() == {"settings": ["unknown setting: unknown_key"]}
    else:
        assert client.get(url).json()["settings"] == {"unknown_key": 1}


def test_patch_non_object_settings_is_400(world):
    # DIFF-41: same divergence as above for non-object fragments.
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = client.patch(url, json={"settings": "x"})
    assert resp.status_code == expect("DIFF-41", django=200, rust=400)
    if is_rust():
        assert resp.json() == {"settings": ["settings must be a JSON object"]}
    else:
        assert client.get(url).json()["settings"] == "x"


def test_read_renders_validated_settings_only(world):
    # A blob written out-of-band stays in the column (unknown keys are
    # preserved for forward compatibility); reads render the validated
    # settings only, so `{}` until the first key lands. Django echoes
    # the raw blob (DIFF-41).
    client, user, ws, project = world.full_stack()
    with world.conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET settings = %s WHERE id = %s",
            ('{"unknown_key": 1}', str(project["id"])),
        )
    world.conn.commit()
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/")
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["settings"] == expect(
        "DIFF-41", django={"unknown_key": 1}, rust={}
    )


def test_patch_settings_requires_admin(world):
    client, user, ws, project = world.full_stack()
    member_user, member_client = world.add_member(ws, project, ws_role=15, proj_role=15)
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = member_client.patch(url, json={"settings": {}})
    assert resp.status_code == 403, resp.text[:500]
