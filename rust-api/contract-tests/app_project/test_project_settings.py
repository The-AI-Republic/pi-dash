"""Contract tests: projects.settings reads and PATCH (NEWPIDASH-278).

Full-serializer reads render the validated settings with defaults
filled in (DIFF-53: ``{"issue_run_policy": "creator_and_assignees"}``
on Rust, the raw stored ``{}`` on Django); the
compact ``.values()`` list projection has no settings key, like the
Django projection. PATCH accepts a nested ``settings`` object and
merges it atomically. Django's app serializers use
``fields = "__all__"``, so the bag is stored unvalidated; the strict
PATCH validation on Rust is the deliberate difference DIFF-51.
"""

from _harness.diffs import expect, is_rust


def test_retrieve_includes_settings_with_defaults(world):
    client, user, ws, project = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/")
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["settings"] == expect(
        "DIFF-53",
        django={},
        rust={
            "issue_run_policy": "creator_and_assignees",
            "agent_stages": {"mode": "auto", "review": True, "test": True},
            "repos": [],
        },
    )


def test_details_list_includes_settings_with_defaults(world):
    client, user, ws, project = world.full_stack()
    resp = client.get(f"/api/workspaces/{ws['slug']}/projects/details/")
    assert resp.status_code == 200, resp.text[:500]
    rows = resp.json()
    assert isinstance(rows, list) and len(rows) == 1
    assert rows[0]["settings"] == expect(
        "DIFF-53",
        django={},
        rust={
            "issue_run_policy": "creator_and_assignees",
            "agent_stages": {"mode": "auto", "review": True, "test": True},
            "repos": [],
        },
    )


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
    assert resp.json()["settings"] == expect(
        "DIFF-53",
        django={},
        rust={
            "issue_run_policy": "creator_and_assignees",
            "agent_stages": {"mode": "auto", "review": True, "test": True},
            "repos": [],
        },
    )


def test_patch_unknown_settings_key_is_400(world):
    # DIFF-51: Django `__all__` stores any blob; Rust validates.
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = client.patch(url, json={"settings": {"unknown_key": 1}})
    assert resp.status_code == expect("DIFF-51", django=200, rust=400)
    if is_rust():
        assert resp.json() == {"settings": ["unknown setting: unknown_key"]}
    else:
        assert client.get(url).json()["settings"] == {"unknown_key": 1}


def test_patch_non_object_settings_is_400(world):
    # DIFF-51: same divergence as above for non-object fragments.
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = client.patch(url, json={"settings": "x"})
    assert resp.status_code == expect("DIFF-51", django=200, rust=400)
    if is_rust():
        assert resp.json() == {"settings": ["settings must be a JSON object"]}
    else:
        assert client.get(url).json()["settings"] == "x"


def test_read_renders_validated_settings_only(world):
    # A blob written out-of-band stays in the column (unknown keys are
    # preserved for forward compatibility); reads render the validated
    # settings only, so `{}` until the first key lands. Django echoes
    # the raw blob (DIFF-51).
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
        "DIFF-51",
        django={"unknown_key": 1},
        rust={
            "issue_run_policy": "creator_and_assignees",
            "agent_stages": {"mode": "auto", "review": True, "test": True},
            "repos": [],
        },
    )


def test_patch_settings_requires_admin(world):
    client, user, ws, project = world.full_stack()
    member_user, member_client = world.add_member(ws, project, ws_role=15, proj_role=15)
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = member_client.patch(url, json={"settings": {}})
    assert resp.status_code == 403, resp.text[:500]


def test_issue_run_policy_admin_can_change(world):
    # NEWPIDASH-261 first key: project Admin (role 20) may change it.
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = client.patch(
        url, json={"settings": {"issue_run_policy": "project_members"}}
    )
    assert resp.status_code == expect("DIFF-51", django=200, rust=200)
    assert resp.json()["settings"] == expect(
        "DIFF-51",
        django={"issue_run_policy": "project_members"},
        rust={
            "issue_run_policy": "project_members",
            "agent_stages": {"mode": "auto", "review": True, "test": True},
            "repos": [],
        },
    )


def test_issue_run_policy_invalid_value_is_400(world):
    # DIFF-51: Django stores any blob; Rust validates the enum.
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = client.patch(
        url, json={"settings": {"issue_run_policy": "bogus"}}
    )
    assert resp.status_code == expect("DIFF-51", django=200, rust=400)
    if is_rust():
        assert resp.json() == {
            "settings": [
                'invalid value for issue_run_policy: must be '
                '"creator_and_assignees" or "project_members"'
            ]
        }
    else:
        assert client.get(url).json()["settings"] == {"issue_run_policy": "bogus"}


def test_issue_run_policy_non_string_is_400(world):
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = client.patch(
        url, json={"settings": {"issue_run_policy": 1}}
    )
    assert resp.status_code == expect("DIFF-51", django=200, rust=400)
    if is_rust():
        assert resp.json() == {
            "settings": [
                'invalid value for issue_run_policy: must be '
                '"creator_and_assignees" or "project_members"'
            ]
        }
    else:
        assert client.get(url).json()["settings"] == {"issue_run_policy": 1}


def test_issue_run_policy_member_cannot_change(world):
    # Only project Admin (20) or workspace Admin may change it; a
    # project Member gets 403 on both backends.
    client, user, ws, project = world.full_stack()
    member_user, member_client = world.add_member(ws, project, ws_role=15, proj_role=15)
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = member_client.patch(
        url, json={"settings": {"issue_run_policy": "project_members"}}
    )
    assert resp.status_code == 403, resp.text[:500]
    # The stored value is unchanged.
    assert client.get(url).json()["settings"] == expect(
        "DIFF-53",
        django={},
        rust={
            "issue_run_policy": "creator_and_assignees",
            "agent_stages": {"mode": "auto", "review": True, "test": True},
            "repos": [],
        },
    )


def test_issue_run_policy_roundtrip_default(world):
    # Switching back to the default restores strict mode.
    client, user, ws, project = world.full_stack()
    url = f"/api/workspaces/{ws['slug']}/projects/{project['id']}/"
    resp = client.patch(
        url, json={"settings": {"issue_run_policy": "project_members"}}
    )
    assert resp.status_code == 200, resp.text[:500]
    resp = client.patch(
        url, json={"settings": {"issue_run_policy": "creator_and_assignees"}}
    )
    assert resp.status_code == 200, resp.text[:500]
    assert resp.json()["settings"] == expect(
        "DIFF-53",
        django={"issue_run_policy": "creator_and_assignees"},
        rust={
            "issue_run_policy": "creator_and_assignees",
            "agent_stages": {"mode": "auto", "review": True, "test": True},
            "repos": [],
        },
    )
