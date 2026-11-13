"""Contract tests: api-v1 ``Project.default_work_type`` (PDASHOSS01-234).

Covers the create-time resolution in ``Project.save()`` (blank → ``software``
when a repo is bound, else ``general``), the ``_validate_default_work_type``
registry check on create and PATCH, the key's position in the
``ProjectSerializer`` output (``fields="__all__"``: right after
``default_agent_executor``), and the PATCH lock that refuses an agent run
(non-blank ``X-Pi-Dash-Run-Id``) changing the value.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402

KEY = "projects"
RUN_ID_HEADER = "X-Pi-Dash-Run-Id"
UNKNOWN_BODY = {
    "default_work_type": ["unknown work type; expected one of ['general', 'software']"]
}
LOCKED_BODY = {
    "error": "default_work_type is locked for agent runs; only a human can change it"
}


def base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects"


def _create(seed, **extra):
    tag = db.new_tag()
    body = {"name": f"CT WorkType {tag}", "identifier": f"W{tag.upper()}"[:12], **extra}
    with http.client(seed["keys"][KEY]) as c:
        return c.post(base(seed) + "/", json=body)


def _patch(seed, project_id, body, headers=None):
    with http.client(seed["keys"][KEY]) as c:
        return c.patch(f"{base(seed)}/{project_id}/", json=body, headers=headers or {})


def _stored(conn, project_id):
    row = db.fetch_one(
        conn, "SELECT default_work_type FROM projects WHERE id = %s", (project_id,)
    )
    return row["default_work_type"]


def _assert_after_executor(item):
    keys = list(item.keys())
    assert "default_work_type" in keys
    assert keys.index("default_work_type") == keys.index("default_agent_executor") + 1, keys


def test_create_without_repo_resolves_general(seed, conn):
    r = _create(seed)
    assert r.status_code == 201, r.text
    body = r.json()
    assert body["default_work_type"] == "general"
    _assert_after_executor(body)
    assert _stored(conn, body["id"]) == "general"


def test_create_with_repo_resolves_software(seed, conn):
    r = _create(seed, repo_url="https://github.com/example/repo.git")
    assert r.status_code == 201, r.text
    assert r.json()["default_work_type"] == "software"
    assert _stored(conn, r.json()["id"]) == "software"


def test_create_blank_value_still_resolves(seed, conn):
    r = _create(seed, default_work_type="", repo_url="https://github.com/example/b.git")
    assert r.status_code == 201, r.text
    assert r.json()["default_work_type"] == "software"


def test_create_explicit_value_kept(seed, conn):
    r = _create(seed, default_work_type="software")
    assert r.status_code == 201, r.text
    assert r.json()["default_work_type"] == "software"
    assert _stored(conn, r.json()["id"]) == "software"
    r = _create(seed, default_work_type="general", repo_url="https://github.com/example/c.git")
    assert r.status_code == 201, r.text
    assert r.json()["default_work_type"] == "general"


def test_create_unknown_value_400(seed):
    r = _create(seed, default_work_type="bogus")
    assert r.status_code == 400, r.text
    assert r.json() == UNKNOWN_BODY


def test_create_too_long_value_400(seed):
    r = _create(seed, default_work_type="x" * 33)
    assert r.status_code == 400, r.text
    assert r.json() == {"default_work_type": ["Ensure this field has no more than 32 characters."]}


def test_list_and_detail_key_position(seed):
    r = http.get(seed["keys"][KEY], base(seed) + "/")
    row = next(p for p in r.json()["results"] if p["id"] == seed["project"]["id"])
    _assert_after_executor(row)
    assert row["default_work_type"] == "software"
    r = http.get(seed["keys"][KEY], f"{base(seed)}/{seed['project']['id']}/")
    _assert_after_executor(r.json())
    assert r.json()["default_work_type"] == "software"


def test_patch_unknown_value_400(seed, conn):
    pid = _create(seed).json()["id"]
    r = _patch(seed, pid, {"default_work_type": "bogus"})
    assert r.status_code == 400, r.text
    assert r.json() == UNKNOWN_BODY
    assert _stored(conn, pid) == "general"


def test_human_patch_changes_value(seed, conn):
    pid = _create(seed).json()["id"]
    r = _patch(seed, pid, {"default_work_type": "software"})
    assert r.status_code == 200, r.text
    assert r.json()["default_work_type"] == "software"
    _assert_after_executor(r.json())
    assert _stored(conn, pid) == "software"


def test_blank_run_id_header_is_human(seed, conn):
    pid = _create(seed).json()["id"]
    r = _patch(seed, pid, {"default_work_type": "software"}, {RUN_ID_HEADER: ""})
    assert r.status_code == 200, r.text
    assert _stored(conn, pid) == "software"


def test_agent_patch_change_locked_403(seed, conn):
    pid = _create(seed).json()["id"]
    r = _patch(seed, pid, {"default_work_type": "software"}, {RUN_ID_HEADER: "run-123"})
    assert r.status_code == 403, r.text
    assert r.json() == LOCKED_BODY
    assert _stored(conn, pid) == "general"


def test_agent_patch_lock_precedes_validation(seed, conn):
    pid = _create(seed).json()["id"]
    # An unknown value differs from the stored one: the lock answers first.
    r = _patch(seed, pid, {"default_work_type": "bogus"}, {RUN_ID_HEADER: "run-123"})
    assert r.status_code == 403, r.text
    assert r.json() == LOCKED_BODY
    # Clearing (falsy → "") also differs from the stored value.
    r = _patch(seed, pid, {"default_work_type": None}, {RUN_ID_HEADER: "run-123"})
    assert r.status_code == 403, r.text
    assert r.json() == LOCKED_BODY
    assert _stored(conn, pid) == "general"


def test_agent_patch_same_value_or_other_fields_allowed(seed, conn):
    created = _create(seed).json()
    pid = created["id"]
    r = _patch(seed, pid, {"default_work_type": "general"}, {RUN_ID_HEADER: "run-123"})
    assert r.status_code == 200, r.text
    assert r.json()["default_work_type"] == "general"
    r = _patch(seed, pid, {"description": "agent edit"}, {RUN_ID_HEADER: "run-123"})
    assert r.status_code == 200, r.text
    assert r.json()["description"] == "agent edit"


def test_archived_check_precedes_lock(seed, conn):
    pid = _create(seed).json()["id"]
    with conn.cursor() as cur:
        cur.execute("UPDATE projects SET archived_at = now() WHERE id = %s", (pid,))
    conn.commit()
    r = _patch(seed, pid, {"default_work_type": "software"}, {RUN_ID_HEADER: "run-123"})
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "Archived project cannot be updated"}
