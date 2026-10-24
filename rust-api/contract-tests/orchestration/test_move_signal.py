"""Cross-project move fires the state transition (PIDASHCONV-1495).

A move changes the issue's state to the target project's default
non-triage state. Django's Issue post_save signal reconciles the
orchestration ticker on that change; Rust fired it into stub
MoveSignalSeams, so leaving a ticking state never disarmed the ticker
and entering one never armed it or created the entry run.

Both move routes are covered: the v1 API route (X-Api-Key) and the
app-tier route (session cookie). Both read ``{"project": <target>}``
(the target project id or identifier).
"""

from datetime import datetime, timedelta, timezone

from _harness import db, world
from _harness.db import get_database_url


def _ticker(issue_id):
    return db.fetchone(
        get_database_url(),
        "SELECT * FROM issue_agent_ticker WHERE issue_id=%s",
        (issue_id,),
    )


def _issue_row(issue_id):
    return db.fetchone(
        get_database_url(),
        "SELECT project_id, state_id FROM issues WHERE id=%s",
        (issue_id,),
    )


def _runs(issue_id):
    return db.fetchall(
        get_database_url(),
        "SELECT id, status, trigger, parent_run_id FROM agent_run "
        "WHERE work_item_id=%s ORDER BY created_at",
        (issue_id,),
    )


def _future(minutes=60):
    return (datetime.now(timezone.utc) + timedelta(minutes=minutes)).isoformat()


def _make_target(w, default_name, default_group):
    """A second project in the world with `default_name` as default state."""
    import secrets

    tgt_proj, tgt_ident = world.make_project(
        w.workspace_id, w.user_id, "T" + secrets.token_hex(2).upper()
    )
    world.add_project_member(tgt_proj, w.workspace_id, w.user_id)
    states = {}
    if default_name != "Backlog":
        states["Backlog"] = world.make_state(
            tgt_proj, w.workspace_id, "Backlog", "backlog", sequence=1.0
        )
    states[default_name] = world.make_state(
        tgt_proj, w.workspace_id, default_name, default_group,
        default=True, sequence=2.0,
    )
    db.execute(
        get_database_url(),
        "UPDATE projects SET default_state_id=%s WHERE id=%s",
        (states[default_name], tgt_proj),
    )
    return tgt_proj, tgt_ident, states


def _v1_move(client, w, issue, target):
    return client.client.post(
        f"/api/v1/workspaces/{w.workspace_slug}/projects/{w.project_id}"
        f"/work-items/{issue}/move/",
        json={"project": target},
    )


def _web_move(client, w, issue, target):
    return client.post(
        f"/api/workspaces/{w.workspace_slug}/projects/{w.project_id}"
        f"/work-items/{issue}/move/",
        json={"project": target},
    )


def test_v1_move_out_of_ticking_disarms_ticker(api_client):
    """v1 move In Progress -> Backlog-default project disarms the ticker."""
    w, client = api_client
    tgt_proj, _, tgt_states = _make_target(w, "Backlog", "backlog")
    issue = world.make_issue(w, "v1leave", "In Progress")
    world.set_ticker(issue, used=3, granted=0, waited=0, next_run_at=_future())

    r = _v1_move(client, w, issue, tgt_proj)
    assert r.status_code == 200, r.text

    row = _issue_row(issue)
    assert str(row["project_id"]) == tgt_proj
    assert str(row["state_id"]) == tgt_states["Backlog"]
    tick = _ticker(issue)
    assert tick["enabled"] is False
    assert tick["disarm_reason"] == "left_ticking_state"
    assert tick["next_run_at"] is None
    assert (tick["used"], tick["granted"], tick["waited"]) == (3, 0, 0)


def test_web_move_out_of_ticking_disarms_ticker(web_client):
    """Web move In Progress -> Backlog-default project disarms the ticker."""
    w, client = web_client
    tgt_proj, _, tgt_states = _make_target(w, "Backlog", "backlog")
    issue = world.make_issue(w, "webleave", "In Progress")
    world.set_ticker(issue, used=3, granted=0, waited=0, next_run_at=_future())

    r = _web_move(client, w, issue, tgt_proj)
    assert r.status_code == 200, r.text

    row = _issue_row(issue)
    assert str(row["project_id"]) == tgt_proj
    assert str(row["state_id"]) == tgt_states["Backlog"]
    tick = _ticker(issue)
    assert tick["enabled"] is False
    assert tick["disarm_reason"] == "left_ticking_state"
    assert tick["next_run_at"] is None
    assert (tick["used"], tick["granted"], tick["waited"]) == (3, 0, 0)


def _runner_target(w):
    """Target project defaulting to In Progress with a pod + online runner."""
    tgt_proj, _, tgt_states = _make_target(w, "In Progress", "started")
    pod = world.make_pod(w.workspace_id, tgt_proj, is_default=True)
    world.make_online_runner(w.user_id, w.workspace_id, pod)
    return tgt_proj, tgt_states, pod


def test_v1_move_into_ticking_arms_ticker_and_creates_entry_run(api_client):
    """v1 move Backlog -> In-Progress-default project arms + entry run."""
    w, client = api_client
    tgt_proj, tgt_states, _ = _runner_target(w)
    issue = world.make_issue(w, "v1enter", "Backlog")
    assert _ticker(issue) is None

    r = _v1_move(client, w, issue, tgt_proj)
    assert r.status_code == 200, r.text

    row = _issue_row(issue)
    assert str(row["project_id"]) == tgt_proj
    assert str(row["state_id"]) == tgt_states["In Progress"]
    tick = _ticker(issue)
    assert tick is not None
    assert tick["enabled"] is True
    assert tick["next_run_at"] is not None
    runs = _runs(issue)
    assert len(runs) == 1, runs
    assert runs[0]["status"] in ("queued", "assigned"), runs[0]
    assert runs[0]["trigger"] == "state_transition", runs[0]


def test_web_move_into_ticking_arms_ticker_and_creates_entry_run(web_client):
    """Web move Backlog -> In-Progress-default project arms + entry run."""
    w, client = web_client
    tgt_proj, tgt_states, _ = _runner_target(w)
    issue = world.make_issue(w, "webenter", "Backlog")
    assert _ticker(issue) is None

    r = _web_move(client, w, issue, tgt_proj)
    assert r.status_code == 200, r.text

    row = _issue_row(issue)
    assert str(row["project_id"]) == tgt_proj
    assert str(row["state_id"]) == tgt_states["In Progress"]
    tick = _ticker(issue)
    assert tick is not None
    assert tick["enabled"] is True
    assert tick["next_run_at"] is not None
    runs = _runs(issue)
    assert len(runs) == 1, runs
    assert runs[0]["status"] in ("queued", "assigned"), runs[0]
    assert runs[0]["trigger"] == "state_transition", runs[0]


def test_v1_move_with_handoff_arms_ticker_without_entry_run(api_client):
    """v1 move with a QUEUED run: ticker arms, handoff replaces, no entry run.

    The QUEUED source run hands off immediately (cancelled + a fresh
    target-project replacement), so the transition defers dispatch: the
    ticker arms but no ``state_transition`` run is created.
    """
    w, client = api_client
    tgt_proj, tgt_states, _ = _runner_target(w)
    src_pod = world.make_pod(w.workspace_id, w.project_id)
    issue = world.make_issue(w, "v1handoff", "Backlog")
    src_run = world.make_run(
        issue, w.workspace_id, w.user_id, src_pod, status="queued"
    )

    r = _v1_move(client, w, issue, tgt_proj)
    assert r.status_code == 200, r.text

    row = _issue_row(issue)
    assert str(row["project_id"]) == tgt_proj
    assert str(row["state_id"]) == tgt_states["In Progress"]
    tick = _ticker(issue)
    assert tick is not None
    assert tick["enabled"] is True
    assert tick["next_run_at"] is not None
    runs = _runs(issue)
    by_id = {str(run["id"]): run for run in runs}
    assert by_id[str(src_run)]["status"] == "cancelled", runs
    replacements = [
        run for run in runs if str(run["parent_run_id"] or "") == str(src_run)
    ]
    assert len(replacements) == 1, runs
    assert [run for run in runs if run["trigger"] == "state_transition"] == []


def test_web_move_with_handoff_arms_ticker_without_entry_run(web_client):
    """Web move with a QUEUED run: ticker arms, handoff replaces, no entry run."""
    w, client = web_client
    tgt_proj, tgt_states, _ = _runner_target(w)
    src_pod = world.make_pod(w.workspace_id, w.project_id)
    issue = world.make_issue(w, "webhandoff", "Backlog")
    src_run = world.make_run(
        issue, w.workspace_id, w.user_id, src_pod, status="queued"
    )

    r = _web_move(client, w, issue, tgt_proj)
    assert r.status_code == 200, r.text

    row = _issue_row(issue)
    assert str(row["project_id"]) == tgt_proj
    assert str(row["state_id"]) == tgt_states["In Progress"]
    tick = _ticker(issue)
    assert tick is not None
    assert tick["enabled"] is True
    assert tick["next_run_at"] is not None
    runs = _runs(issue)
    by_id = {str(run["id"]): run for run in runs}
    assert by_id[str(src_run)]["status"] == "cancelled", runs
    replacements = [
        run for run in runs if str(run["parent_run_id"] or "") == str(src_run)
    ]
    assert len(replacements) == 1, runs
    assert [run for run in runs if run["trigger"] == "state_transition"] == []
