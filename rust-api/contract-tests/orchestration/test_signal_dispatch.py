"""Dispatch on signal: entering / moving between / leaving ticking states.

Phase registry pinned here: started→"In Progress", review→"In Review",
test→"In Test" (literal names only); pool default 10, cadence 10800 s.
"""

from _harness import broker_redis as broker, db, world
from _harness.db import get_database_url


def _issue_state(issue_id):
    # Trunk db returns raw UUIDs (no str-ify): normalize for str comparisons.
    return str(db.fetchone(get_database_url(), "SELECT state_id FROM issues WHERE id=%s", (issue_id,))["state_id"])


def _ticker(issue_id):
    return db.fetchone(get_database_url(), "SELECT * FROM issue_agent_ticker WHERE issue_id=%s", (issue_id,))


def test_enter_bucket_arms_ticker_without_runner_dispatch(api_client):
    """Backlog → In Progress (human): clock arms, no run without a pod.

    No pod exists, so dispatch stops at `no-pod-available`: the issue stays
    In Progress, the ticker is enabled with a fresh `next_run_at`, and no
    AgentRun row appears.
    """
    w, client = api_client
    issue = world.make_issue(w, "enter", "Backlog")
    assert _ticker(issue) is None

    before_runs = db.fetchone(get_database_url(), "SELECT count(*) AS n FROM agent_run")["n"]
    r = client.patch_issue(w.workspace_slug, w.project_id, issue,
                           {"state": w.states["In Progress"]["id"]})
    assert r.status_code == 200, r.text

    assert _issue_state(issue) == w.states["In Progress"]["id"]
    tick = _ticker(issue)
    assert tick is not None
    assert tick["enabled"] is True
    assert tick["disarm_reason"] == ""
    assert tick["next_run_at"] is not None
    assert tick["used"] == 0
    assert db.fetchone(get_database_url(), "SELECT count(*) AS n FROM agent_run")["n"] == before_runs


def test_enter_bucket_bounces_to_backlog_when_pod_has_no_runner(api_client):
    """Backlog → In Progress with a (runnerless) default pod: loud bounce.

    The preflight finds no eligible runner, so the issue is moved back to
    Backlog, a system comment explains why, and the ticker disarms with
    `left_ticking_state` (leaving the bucket disarms; the pool is kept).
    """
    w, client = api_client
    world.make_pod(w.workspace_id, w.project_id, is_default=True)
    issue = world.make_issue(w, "bounce", "Backlog")

    comments_before = db.fetchone(get_database_url(), 
        "SELECT count(*) AS n FROM issue_comments WHERE issue_id=%s", (issue,))["n"]
    r = client.patch_issue(w.workspace_slug, w.project_id, issue,
                           {"state": w.states["In Progress"]["id"]})
    assert r.status_code == 200, r.text

    assert _issue_state(issue) == w.states["Backlog"]["id"]
    tick = _ticker(issue)
    assert tick is not None
    assert tick["enabled"] is False
    assert tick["disarm_reason"] == "left_ticking_state"
    assert tick["next_run_at"] is None
    comments = db.fetchall(get_database_url(), 
        "SELECT comment_html FROM issue_comments WHERE issue_id=%s ORDER BY created_at",
        (issue,))
    assert len(comments) == comments_before + 1
    assert "no eligible runner" in comments[-1]["comment_html"]


def test_move_between_rooms_retimes_clock(api_client):
    """In Progress → In Review (human, no active run): still no pod, no run,
    clock stays armed and `next_run_at` moves to now + the review cadence."""
    w, client = api_client
    issue = world.make_issue(w, "move", "In Progress")
    world.set_ticker(issue)

    r = client.patch_issue(w.workspace_slug, w.project_id, issue,
                           {"state": w.states["In Review"]["id"]})
    assert r.status_code == 200, r.text

    assert _issue_state(issue) == w.states["In Review"]["id"]
    tick = _ticker(issue)
    assert tick["enabled"] is True
    assert tick["next_run_at"] is not None
    assert db.fetchone(get_database_url(), 
        "SELECT count(*) AS n FROM agent_run WHERE work_item_id=%s", (issue,))["n"] == 0


def test_leave_bucket_disarms_and_keeps_pool(api_client):
    """In Progress → Done: the clock goes dormant, `next_run_at` cleared,
    budget counters untouched."""
    w, client = api_client
    issue = world.make_issue(w, "leave", "In Progress")
    world.set_ticker(issue, used=3, granted=0, waited=0)

    r = client.patch_issue(w.workspace_slug, w.project_id, issue,
                           {"state": w.states["Done"]["id"]})
    assert r.status_code == 200, r.text

    tick = _ticker(issue)
    assert tick["enabled"] is False
    assert tick["disarm_reason"] == "left_ticking_state"
    assert tick["next_run_at"] is None
    assert (tick["used"], tick["granted"], tick["waited"]) == (3, 0, 0)


def test_non_trigger_state_creates_no_ticker(api_client):
    """Backlog → Unstarted: not a delegation trigger, clock untouched."""
    w, client = api_client
    issue = world.make_issue(w, "notrigger", "Backlog")

    r = client.patch_issue(w.workspace_slug, w.project_id, issue,
                           {"state": w.states["Unstarted"]["id"]})
    assert r.status_code == 200, r.text
    assert _ticker(issue) is None
