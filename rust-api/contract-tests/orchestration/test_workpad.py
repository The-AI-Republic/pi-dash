"""Workpad endpoint: byte-parity round-trip, the `body` wire-field rule,
clearing, and the timestamp-only PATCH response."""

from _harness import db, world
from _harness.db import get_database_url

SAMPLE = """# Plan

- [ ] 1. Parent task
  - [ ] 1.1 Child task — unicode: héllo wörld ✓ 日本語

```text
host:/abs/path@abc1234
```"""


def _db_workpad(issue):
    return db.fetchone(get_database_url(), "SELECT workpad FROM issues WHERE id=%s", (issue,))["workpad"]


def test_workpad_roundtrip_byte_parity(api_client):
    w, client = api_client
    issue = world.make_issue(w, "pad", "In Progress")

    r = client.patch_workpad(w.workspace_slug, w.project_id, issue, {"body": SAMPLE})
    assert r.status_code == 200, r.text
    assert _db_workpad(issue) == SAMPLE

    r = client.get_workpad(w.workspace_slug, w.project_id, issue)
    assert r.status_code == 200, r.text
    assert r.json()["body"] == SAMPLE


def test_workpad_patch_requires_body_field(api_client):
    """The wire field is `body`, not the model name `workpad`: sending the
    model name is a 400, never a silent no-op."""
    w, client = api_client
    issue = world.make_issue(w, "padbody", "In Progress")

    r = client.patch_workpad(w.workspace_slug, w.project_id, issue, {"workpad": "x"})
    assert r.status_code == 400, r.text
    assert _db_workpad(issue) == ""


def test_workpad_empty_body_clears(api_client):
    w, client = api_client
    issue = world.make_issue(w, "padclear", "In Progress")
    assert client.patch_workpad(w.workspace_slug, w.project_id, issue, {"body": SAMPLE}).status_code == 200

    r = client.patch_workpad(w.workspace_slug, w.project_id, issue, {"body": ""})
    assert r.status_code == 200, r.text
    assert _db_workpad(issue) == ""
    assert client.get_workpad(w.workspace_slug, w.project_id, issue).json()["body"] == ""


def test_workpad_write_trims_surrounding_whitespace(api_client):
    """Semantic trap: the write path is a DRF CharField, which strips
    leading/trailing whitespace on input. DB and GET agree on the trimmed
    value — the Rust port must reproduce the trim, not the raw bytes."""
    w, client = api_client
    issue = world.make_issue(w, "padtrim", "In Progress")

    r = client.patch_workpad(w.workspace_slug, w.project_id, issue, {"body": "  padded\n"})
    assert r.status_code == 200, r.text
    assert _db_workpad(issue) == "padded"
    assert client.get_workpad(w.workspace_slug, w.project_id, issue).json()["body"] == "padded"


def test_workpad_patch_returns_timestamp_only(api_client):
    """PATCH echoes only `updated_at` — the body just came from the caller
    and is not repeated."""
    w, client = api_client
    issue = world.make_issue(w, "padts", "In Progress")

    r = client.patch_workpad(w.workspace_slug, w.project_id, issue, {"body": SAMPLE})
    assert r.status_code == 200, r.text
    assert "updated_at" in r.json()
    assert "body" not in r.json()
