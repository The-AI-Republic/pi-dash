"""Contract tests: api-v1 project settings reads and PATCH (NEWPIDASH-278).

Detail reads render the validated settings with defaults (``{}``
until the first key lands with NEWPIDASH-261). PATCH merges a nested
``settings`` object. Django's v1 update serializer has no settings
field (the key is ignored) and reads echo the raw stored blob, so the
strict PATCH validation on Rust is the deliberate difference DIFF-41.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import http  # noqa: E402
from _harness.diffs import expect, is_rust  # noqa: E402

KEY = "settings"


def base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects"


def test_detail_read_includes_settings_with_defaults(seed):
    url = base(seed) + f"/{seed['project']['id']}/"
    r = http.get(seed["keys"][KEY], url)
    assert r.json()["settings"] == {}


def test_patch_empty_settings_is_noop(seed):
    url = base(seed) + f"/{seed['project']['id']}/"
    r = http.patch(seed["keys"][KEY], url, json={"settings": {}}, expect=200)
    assert r.json()["settings"] == {}


def test_patch_unknown_settings_key_is_400(seed):
    # DIFF-41: Django's v1 update serializer ignores the key; Rust
    # validates.
    url = base(seed) + f"/{seed['project']['id']}/"
    with http.client(seed["keys"][KEY]) as c:
        r = c.patch(url, json={"settings": {"unknown_key": 1}})
    assert r.status_code == expect("DIFF-41", django=200, rust=400)
    if is_rust():
        assert r.json() == {"settings": ["unknown setting: unknown_key"]}


def test_patch_non_object_settings_is_400(seed):
    # DIFF-41: same divergence as above for non-object fragments.
    url = base(seed) + f"/{seed['project']['id']}/"
    with http.client(seed["keys"][KEY]) as c:
        r = c.patch(url, json={"settings": "x"})
    assert r.status_code == expect("DIFF-41", django=200, rust=400)
    if is_rust():
        assert r.json() == {"settings": ["settings must be a JSON object"]}


def test_patch_settings_requires_project_access(seed):
    # `guest` is a workspace member with no project membership: the
    # PATCH gate denies on both backends.
    url = base(seed) + f"/{seed['project']['id']}/"
    with http.client(seed["keys"]["guest"]) as c:
        r = c.patch(url, json={"settings": {}})
    assert r.status_code == 403, r.text[:500]
