"""Contract tests: api-v1 project settings reads and PATCH (NEWPIDASH-278).

Detail reads render the validated settings with defaults (DIFF-53:
``issue_run_policy`` defaults to ``creator_and_assignees`` on Rust, the
raw stored ``{}`` on Django; DIFF-51: Rust fills every other known key,
such as ``agent_stages``, with its default). PATCH merges a nested
``settings`` object. Django's v1 update serializer has no settings
field (the key is ignored) and reads echo the raw stored blob, so the
strict PATCH validation on Rust is the deliberate difference DIFF-51.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import http  # noqa: E402
from _harness.diffs import expect, is_rust  # noqa: E402

KEY = "settings"

# Rust reads fill every known settings key with its default
# (`project_settings::KNOWN_KEYS`); `agent_stages` defaults to Auto with
# both optional stages on.
AGENT_STAGES_DEFAULT = {"mode": "auto", "review": True, "test": True}
RUST_DEFAULT_SETTINGS = {
    "issue_run_policy": "creator_and_assignees",
    "agent_stages": AGENT_STAGES_DEFAULT,
}


def base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects"


def test_detail_read_includes_settings_with_defaults(seed):
    url = base(seed) + f"/{seed['project']['id']}/"
    r = http.get(seed["keys"][KEY], url)
    assert r.json()["settings"] == expect(
        "DIFF-53",
        django={},
        rust=RUST_DEFAULT_SETTINGS,
    )


def test_patch_empty_settings_is_noop(seed):
    url = base(seed) + f"/{seed['project']['id']}/"
    r = http.patch(seed["keys"][KEY], url, json={"settings": {}}, expect=200)
    assert r.json()["settings"] == expect(
        "DIFF-53",
        django={},
        rust=RUST_DEFAULT_SETTINGS,
    )


def test_patch_unknown_settings_key_is_400(seed):
    # DIFF-51: Django's v1 update serializer ignores the key; Rust
    # validates.
    url = base(seed) + f"/{seed['project']['id']}/"
    with http.client(seed["keys"][KEY]) as c:
        r = c.patch(url, json={"settings": {"unknown_key": 1}})
    assert r.status_code == expect("DIFF-51", django=200, rust=400)
    if is_rust():
        assert r.json() == {"settings": ["unknown setting: unknown_key"]}


def test_patch_non_object_settings_is_400(seed):
    # DIFF-51: same divergence as above for non-object fragments.
    url = base(seed) + f"/{seed['project']['id']}/"
    with http.client(seed["keys"][KEY]) as c:
        r = c.patch(url, json={"settings": "x"})
    assert r.status_code == expect("DIFF-51", django=200, rust=400)
    if is_rust():
        assert r.json() == {"settings": ["settings must be a JSON object"]}


def test_patch_settings_requires_project_access(seed):
    # `guest` is a workspace member with no project membership: the
    # PATCH gate denies on both backends.
    url = base(seed) + f"/{seed['project']['id']}/"
    with http.client(seed["keys"]["guest"]) as c:
        r = c.patch(url, json={"settings": {}})
    assert r.status_code == 403, r.text[:500]


def test_issue_run_policy_admin_can_change(seed):
    # NEWPIDASH-261 first key (v1 PROJECT PATCH): owner (project ADMIN)
    # may change it; Django ignores the key (200, settings unchanged).
    url = base(seed) + f"/{seed['project']['id']}/"
    with http.client(seed["keys"][KEY]) as c:
        r = c.patch(url, json={"settings": {"issue_run_policy": "project_members"}})
    assert r.status_code == 200, r.text[:500]
    assert r.json()["settings"] == expect(
        "DIFF-51",
        django={},
        rust={**RUST_DEFAULT_SETTINGS, "issue_run_policy": "project_members"},
    )


def test_issue_run_policy_invalid_value_is_400(seed):
    # DIFF-51: Django's v1 update serializer ignores the key; Rust
    # validates the enum.
    url = base(seed) + f"/{seed['project']['id']}/"
    with http.client(seed["keys"][KEY]) as c:
        r = c.patch(url, json={"settings": {"issue_run_policy": "bogus"}})
    assert r.status_code == expect("DIFF-51", django=200, rust=400)
    if is_rust():
        assert r.json() == {
            "settings": [
                'invalid value for issue_run_policy: must be '
                '"creator_and_assignees" or "project_members"'
            ]
        }


def test_issue_run_policy_member_cannot_change(seed):
    # `member_self` is a project ADMIN on `project` (see conftest);
    # use `guest` (workspace member, no project membership): the PATCH
    # gate denies on both backends before the settings check.
    url = base(seed) + f"/{seed['project']['id']}/"
    with http.client(seed["keys"]["guest"]) as c:
        r = c.patch(url, json={"settings": {"issue_run_policy": "project_members"}})
    assert r.status_code == 403, r.text[:500]
