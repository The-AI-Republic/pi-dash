"""Re-entrancy guards: agent moves queue, human moves dispatch.

An agent's own state move from inside its run (X-Pi-Dash-Run-Id, or the
inferred active run of the caller) must not dispatch a second concurrent
run — reconcile queues a counting entry instead. A human move while a run
is active queues a free entry. A human move with the issue free dispatches
now (or stops at no-pod-available with the clock armed).
"""

from _harness import api, db, world
from _harness.db import get_database_url


def _seed_active_run(w, issue, pod, creator=None):
    return world.make_run(issue, w.workspace_id, creator or w.user_id, pod, status="running")


def _ticker(issue):
    return db.fetchone(get_database_url(), "SELECT * FROM issue_agent_ticker WHERE issue_id=%s", (issue,))


def _run_count(issue):
    return db.fetchone(get_database_url(), "SELECT count(*) AS n FROM agent_run WHERE work_item_id=%s", (issue,))["n"]


def test_agent_header_move_queues_counting_entry(api_client):
    """PATCH with X-Pi-Dash-Run-Id naming the caller's active run: no new
    run; the clock holds a counting (non-free) pending entry."""
    w, client = api_client
    pod = world.make_pod(w.workspace_id, w.project_id)
    issue = world.make_issue(w, "guard", "In Progress")
    world.set_ticker(issue)
    run = _seed_active_run(w, issue, pod)

    r = client.patch_issue(w.workspace_slug, w.project_id, issue,
                           {"state": w.states["In Review"]["id"]}, run_id=run)
    assert r.status_code == 200, r.text

    assert _run_count(issue) == 1
    tick = _ticker(issue)
    assert tick["pending_entry"] is True
    assert tick["pending_entry_free"] is False


def test_other_human_move_queues_free_entry(api_client):
    """A different admin moves the issue while someone else's run is active:
    queued, and free (does not spend the pool)."""
    w, client = api_client
    pod = world.make_pod(w.workspace_id, w.project_id)
    issue = world.make_issue(w, "free", "In Progress")
    world.set_ticker(issue, used=4)
    _seed_active_run(w, issue, pod)

    other = world.make_user(world.slug("other-human") + "@example.com")
    world.add_workspace_member(w.workspace_id, other, world.ROLE_ADMIN)
    world.add_project_member(w.project_id, w.workspace_id, other, world.ROLE_ADMIN)
    other_client = api.Api(world.make_token(other, w.workspace_id))

    r = other_client.patch_issue(w.workspace_slug, w.project_id, issue,
                                 {"state": w.states["In Review"]["id"]})
    assert r.status_code == 200, r.text

    assert _run_count(issue) == 1
    tick = _ticker(issue)
    assert tick["pending_entry"] is True
    assert tick["pending_entry_free"] is True
    assert tick["used"] == 4


def test_bad_run_header_rejected(api_client):
    """A malformed X-Pi-Dash-Run-Id is a 400, not a silent human move."""
    w, client = api_client
    issue = world.make_issue(w, "badheader", "In Progress")

    r = client.patch_issue(w.workspace_slug, w.project_id, issue,
                           {"state": w.states["In Review"]["id"]}, run_id="not-a-uuid")
    assert r.status_code == 400, r.text
    assert _ticker(issue) is None


def test_foreign_run_header_is_plain_move(api_client):
    """A header naming a run on another issue (or finished) is ignored: the
    request is treated as a human move, not refused."""
    w, client = api_client
    pod = world.make_pod(w.workspace_id, w.project_id)
    issue = world.make_issue(w, "plain", "In Progress")
    other_issue = world.make_issue(w, "plain-other", "In Progress")
    foreign = _seed_active_run(w, other_issue, pod)

    r = client.patch_issue(w.workspace_slug, w.project_id, issue,
                           {"state": w.states["In Review"]["id"]}, run_id=foreign)
    assert r.status_code == 200, r.text
    # Human move on a free issue: dispatch-now path, stops at
    # no-pod-available (the seeded pod is not a default) with the clock
    # armed — and crucially no queued entry.
    tick = _ticker(issue)
    assert tick["enabled"] is True
    assert tick["pending_entry"] is False
