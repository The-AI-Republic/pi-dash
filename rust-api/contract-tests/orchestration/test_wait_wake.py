"""Wait / wake transitions: `pidash issue wait` buys back one tick; Re-tick
re-grants a spent pool. Responses are always 200 — a refusal is a no-op
with a machine-readable reason, never an error."""

from _harness import db, world
from _harness.db import get_database_url


def _ticker(issue):
    return db.fetchone(get_database_url(), "SELECT * FROM issue_agent_ticker WHERE issue_id=%s", (issue,))


def test_wait_buys_back_one_tick(api_client):
    w, client = api_client
    issue = world.make_issue(w, "wait", "In Progress")
    world.set_ticker(issue, used=3)

    r = client.post_wait(w.workspace_slug, w.project_id, issue)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["applied"] is True
    assert body["reason"] == "waited"
    assert body["waited"] == 1
    assert body["cap"] == 11  # pool 10 + 1 bought back

    tick = _ticker(issue)
    assert tick["waited"] == 1
    assert tick["used"] == 3  # the ending run still spent nothing extra


def test_wait_allowance_bounds_total(api_client):
    """One extra pool: repeated waits eventually report wait_cap_reached and
    stop raising the cap."""
    w, client = api_client
    issue = world.make_issue(w, "waitcap", "In Progress")
    world.set_ticker(issue, used=1)

    last = None
    for _ in range(12):  # pool 10 + allowance 10 + headroom
        r = client.post_wait(w.workspace_slug, w.project_id, issue)
        assert r.status_code == 200, r.text
        last = r.json()
        if not last["applied"]:
            break
    assert last["applied"] is False
    assert last["reason"] == "wait_cap_reached"
    tick = _ticker(issue)
    assert tick["waited"] == 10  # exactly one extra pool bought back


def test_wait_without_ticker_is_noop(api_client):
    w, client = api_client
    issue = world.make_issue(w, "notick", "In Progress")

    r = client.post_wait(w.workspace_slug, w.project_id, issue)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["applied"] is False
    assert body["reason"] == "no_ticker"


def test_wait_on_infinite_pool_is_noop(api_client):
    w, client = api_client
    db.execute(get_database_url(), "UPDATE projects SET agent_default_max_ticks=-1 WHERE id=%s", (w.project_id,))
    issue = world.make_issue(w, "infpool", "In Progress")
    world.set_ticker(issue)

    r = client.post_wait(w.workspace_slug, w.project_id, issue)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["applied"] is False
    assert body["reason"] == "infinite_pool"


def test_retick_noop_while_pool_unspent(api_client):
    """Re-tick fires only on an exhausted pool in a ticking state: with
    budget left it reports granted=false and writes nothing."""
    w, client = api_client
    issue = world.make_issue(w, "reticknoop", "In Progress")
    world.set_ticker(issue, used=2)

    r = client.post_retick(w.workspace_slug, w.project_id, issue)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["granted"] is False

    tick = _ticker(issue)
    assert tick["granted"] == 0
    assert tick["used"] == 2


def test_retick_rolls_back_when_dispatch_fails(api_client):
    """Spent pool + ticking state grants, but with no pod the run cannot be
    created: the whole Re-tick rolls back — granted=false, ticker untouched."""
    w, client = api_client
    issue = world.make_issue(w, "retickfail", "In Progress")
    world.set_ticker(issue, used=10)

    r = client.post_retick(w.workspace_slug, w.project_id, issue)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["granted"] is False
    assert body["reason"] == "dispatch-failed"

    tick = _ticker(issue)
    assert tick["granted"] == 0
    assert tick["used"] == 10
    assert db.fetchone(get_database_url(), 
        "SELECT count(*) AS n FROM agent_run WHERE work_item_id=%s", (issue,))["n"] == 0
