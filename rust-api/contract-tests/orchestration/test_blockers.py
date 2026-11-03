"""Blocker semantics (blockers.py): `has_open_blockers` on the issue detail
view follows the blocked_by edges — true while a blocker is open, false once
every blocker is Done. The closing side fires nothing: relations are inert
data the dependent re-reads."""

from _harness import db, world
from _harness.db import get_database_url


def _detail(client, w, issue):
    r = client.get_issue(w.workspace_slug, w.project_id, issue)
    assert r.status_code == 200, r.text
    return r.json()


def test_open_blocker_marks_dependent(api_client):
    w, client = api_client
    a = world.make_issue(w, "blocked", "In Progress")
    b = world.make_issue(w, "blocker", "In Progress")

    assert _detail(client, w, a).get("has_open_blockers") is False

    r = client.relate(w.workspace_slug, w.project_id, a, "blocked_by", [b])
    assert r.status_code == 200, r.text
    assert _detail(client, w, a).get("has_open_blockers") is True

    grouped = client.grouped_relations(w.workspace_slug, w.project_id, a).json()
    blocked_by = grouped["relations"].get("blocked_by", [])
    assert any(item["id"] == b for item in blocked_by)
    entry = next(item for item in blocked_by if item["id"] == b)
    assert entry["state_group"] in ("started",)


def test_resolved_blocker_clears_flag_without_side_effects(api_client):
    """Closing the blocker flips the dependent's flag; nothing fires on the
    dependent itself (no run, no ticker write — it picks the change up on its
    next tick)."""
    w, client = api_client
    a = world.make_issue(w, "dep", "In Progress")
    b = world.make_issue(w, "blk", "Backlog")
    assert client.relate(w.workspace_slug, w.project_id, a, "blocked_by", [b]).status_code == 200
    assert _detail(client, w, a).get("has_open_blockers") is True

    runs_before = db.fetchone(get_database_url(), "SELECT count(*) AS n FROM agent_run")["n"]
    tickers_before = db.fetchone(get_database_url(), "SELECT count(*) AS n FROM issue_agent_ticker")["n"]

    r = client.patch_issue(w.workspace_slug, w.project_id, b,
                           {"state": w.states["Done"]["id"]})
    assert r.status_code == 200, r.text

    assert _detail(client, w, a).get("has_open_blockers") is False
    assert db.fetchone(get_database_url(), "SELECT count(*) AS n FROM agent_run")["n"] == runs_before
    assert db.fetchone(get_database_url(), "SELECT count(*) AS n FROM issue_agent_ticker")["n"] == tickers_before


def test_relate_is_idempotent(api_client):
    w, client = api_client
    a = world.make_issue(w, "idem-a", "Backlog")
    b = world.make_issue(w, "idem-b", "Backlog")

    first = client.relate(w.workspace_slug, w.project_id, a, "blocked_by", [b]).json()
    assert first["created"]
    second = client.relate(w.workspace_slug, w.project_id, a, "blocked_by", [b]).json()
    assert second["unchanged"]
    assert not second["created"]
