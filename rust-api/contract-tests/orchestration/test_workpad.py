"""Workpad endpoint: byte-parity round-trip, the `body` wire-field rule,
clearing, the timestamp-only PATCH response, and the `body_sha256`
stale-write guard (NEWPIDASH-320 §6.1, DIFF-60)."""

import hashlib

from _harness import db, world
from _harness.db import get_database_url
from _harness.diffs import expect, is_rust

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


def _sha(body):
    return hashlib.sha256(body.encode()).hexdigest()


def test_workpad_get_carries_body_sha256(api_client):
    """GET answers `body_sha256` — the stored body's hex digest — on Rust
    (DIFF-60); Django answers `{body, updated_at}` only."""
    w, client = api_client
    issue = world.make_issue(w, "padsha", "In Progress")
    assert client.patch_workpad(w.workspace_slug, w.project_id, issue, {"body": SAMPLE}).status_code == 200

    r = client.get_workpad(w.workspace_slug, w.project_id, issue)
    assert r.status_code == 200, r.text
    if is_rust():
        assert r.json()["body_sha256"] == _sha(SAMPLE), r.text
    else:
        assert "body_sha256" not in r.json(), r.text


def test_workpad_patch_returns_body_sha256(api_client):
    """PATCH answers the new body's hash on Rust (DIFF-60) alongside
    `updated_at`; Django answers the timestamp only."""
    w, client = api_client
    issue = world.make_issue(w, "padshap", "In Progress")

    r = client.patch_workpad(w.workspace_slug, w.project_id, issue, {"body": SAMPLE})
    assert r.status_code == 200, r.text
    if is_rust():
        assert r.json()["body_sha256"] == _sha(SAMPLE), r.text
    else:
        assert "body_sha256" not in r.json(), r.text


def test_workpad_patch_stale_guard_returns_412(api_client):
    """A PATCH whose `if_body_sha256` no longer matches the stored body is
    refused on Rust (DIFF-60): 412 with the current hash, and nothing is
    written. Django ignores the unknown field and writes unconditionally."""
    w, client = api_client
    issue = world.make_issue(w, "padstale", "In Progress")
    assert client.patch_workpad(w.workspace_slug, w.project_id, issue, {"body": SAMPLE}).status_code == 200
    current = client.get_workpad(w.workspace_slug, w.project_id, issue).json()
    if is_rust():
        assert current["body_sha256"] == _sha(SAMPLE)

    r = client.patch_workpad(
        w.workspace_slug, w.project_id, issue, {"body": "new notes", "if_body_sha256": "0" * 64}
    )
    assert r.status_code == expect("DIFF-60", django=200, rust=412), r.text
    if is_rust():
        assert r.json() == {
            "error": "workpad changed since you read it",
            "body_sha256": _sha(SAMPLE),
        }, r.text
        assert _db_workpad(issue) == SAMPLE
    else:
        assert _db_workpad(issue) == "new notes"

    # A matching hash writes through on Rust, unconditionally elsewhere.
    r = client.patch_workpad(
        w.workspace_slug, w.project_id, issue, {"body": "new notes", "if_body_sha256": current.get("body_sha256", "") or _sha(SAMPLE)}
    )
    assert r.status_code == 200, r.text
    assert _db_workpad(issue) == "new notes"


def test_workpad_guard_survives_state_move(api_client):
    """The guard hashes the body, not the issue row's `updated_at`: an
    intervening state move (which re-stamps the row) must not turn a fresh
    hash stale. Passes on both backends — Django ignores the field, Rust
    compares the unchanged body."""
    w, client = api_client
    issue = world.make_issue(w, "padmove", "In Progress")

    r = client.get_workpad(w.workspace_slug, w.project_id, issue)
    assert r.status_code == 200, r.text
    fresh = r.json().get("body_sha256") or _sha("")

    states = client.list_states(w.workspace_slug, w.project_id).json()["results"]
    target = next(s["id"] for s in states if s["id"] != w.states["In Progress"]["id"])
    r = client.patch_issue(w.workspace_slug, w.project_id, issue, {"state": target})
    assert r.status_code == 200, r.text

    r = client.patch_workpad(
        w.workspace_slug, w.project_id, issue, {"body": SAMPLE, "if_body_sha256": fresh}
    )
    assert r.status_code == 200, r.text
    assert _db_workpad(issue) == SAMPLE


def test_workpad_patch_over_the_size_cap_is_refused(api_client):
    """Unified workpad U7/§8.5: the workpad is inlined into every issue-run
    prompt, so the Rust API refuses a body over 16,000 characters with a
    400 and leaves the stored body as it was. Django has no cap."""
    w, client = api_client
    issue = world.make_issue(w, "padcap", "In Progress")
    assert client.patch_workpad(w.workspace_slug, w.project_id, issue, {"body": SAMPLE}).status_code == 200

    # Characters, not bytes: a multi-byte body exactly at the cap fits.
    at_cap = "é" * 16_000
    r = client.patch_workpad(w.workspace_slug, w.project_id, issue, {"body": at_cap})
    assert r.status_code == 200, r.text

    r = client.patch_workpad(w.workspace_slug, w.project_id, issue, {"body": "x" * 16_001})
    if is_rust():
        assert r.status_code == 400, r.text
        assert "longer than 16000 characters" in r.json()["error"], r.text
        assert _db_workpad(issue) == at_cap
    else:
        assert r.status_code == 200, r.text
