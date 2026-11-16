"""Contract tests: v1 project scheduler runs (PIDASHCONV-1061).

Ports ``TestProjectSchedulerRuns`` from Django's
``apps/api/pi_dash/tests/contract/api/test_schedulers.py`` (PDASHOSS01-225)
plus the run shape pin and cursor pagination.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import http  # noqa: E402

KEY = "rn"

RUN_KEYS = [
    "id", "scheduler", "scheduler_binding", "project", "status",
    "trigger", "phase_kind", "error_code", "created_at", "started_at",
    "ended_at", "issues",
]

FORBIDDEN_BODY = {"detail": "You do not have permission to perform this action."}
NOT_FOUND_BODY = {"detail": "No SchedulerBinding matches the given query."}


def url(seed, scheduler_id=None):
    sid = scheduler_id if scheduler_id is not None else seed["scheduler"]["id"]
    return (
        f"/api/v1/workspaces/{seed['ws_a']['slug']}"
        f"/projects/{seed['project']['id']}/schedulers/{sid}/runs/"
    )


def test_member_lists_runs_with_touched_issues(seed):
    r = http.get(seed["keys"][KEY], url(seed))
    body = r.json()
    rows = body["results"]
    assert body["total_count"] == 3
    assert [row["id"] for row in rows] == [
        str(seed["run_new"]["id"]),
        str(seed["run_mid"]["id"]),
        str(seed["run_old"]["id"]),
    ]
    for row in rows:
        assert list(row.keys()) == RUN_KEYS
    old = rows[2]
    assert old["status"] == "failed"
    assert old["error_code"] == "watchdog_timeout"
    assert old["scheduler"] == str(seed["scheduler"]["id"])
    assert old["scheduler_binding"] == str(seed["binding"]["id"])
    assert old["project"] == str(seed["project"]["id"])
    assert old["trigger"] == "scheduler"
    touched = old["issues"]
    assert len(touched) == 1
    assert list(touched[0].keys()) == ["id", "identifier", "name"]
    assert touched[0]["identifier"] == f"{seed['project']['identifier']}-11"
    assert touched[0]["name"] == "Filed by scheduler"
    assert rows[0]["issues"] == []


def test_issue_attribution_scoped_to_callers_projects(seed):
    # The mid run commented in the visible project and in a project the
    # caller cannot access: only the visible issue is attributed.
    r = http.get(seed["keys"][KEY], url(seed))
    rows = {row["id"]: row for row in r.json()["results"]}
    touched = {issue["name"] for issue in rows[str(seed["run_mid"]["id"])]["issues"]}
    assert touched == {"Visible to caller"}


def test_per_page_bounds_the_page(seed):
    r = http.get(seed["keys"][KEY], url(seed), params={"per_page": 2})
    body = r.json()
    assert len(body["results"]) == 2
    assert body["total_count"] == 3
    assert body["count"] == 2
    assert body["next_page_results"] is True
    assert body["next_cursor"] == "2:1:0"


def test_cursor_walks_pages(seed):
    first = http.get(seed["keys"][KEY], url(seed), params={"per_page": 1}).json()
    assert [row["id"] for row in first["results"]] == [str(seed["run_new"]["id"])]
    second = http.get(
        seed["keys"][KEY],
        url(seed),
        params={"per_page": 1, "cursor": first["next_cursor"]},
    ).json()
    assert [row["id"] for row in second["results"]] == [str(seed["run_mid"]["id"])]
    assert second["prev_page_results"] is True


def test_404_when_not_installed_on_project(seed):
    r = http.get(
        seed["keys"][KEY], url(seed, seed["bare_scheduler"]["id"]), expect=404
    )
    assert r.json() == NOT_FOUND_BODY


def test_nonmember_denied(seed):
    r = http.get(seed["keys"]["nonmember"], url(seed), expect=403)
    assert r.json() == FORBIDDEN_BODY


def test_outsider_denied(seed):
    r = http.get(seed["keys"]["outsider"], url(seed), expect=403)
    assert r.json() == FORBIDDEN_BODY


def test_issues_list_one_entry_per_comment_newest_first(repeat_seed):
    # PIDASHCONV-1204: a run that commented twice on one issue lists that
    # issue once per comment, newest comment first (Django's ``.distinct()``
    # carries ``IssueComment.Meta.ordering`` = ``-created_at``, and the
    # ordering column joins the ``SELECT DISTINCT`` list). Three comments on
    # I1 (t0), I2 (t1), I1 (t2) yield ``[I1, I2, I1]`` in that order.
    seed = repeat_seed
    r = http.get(seed["keys"][KEY], url(seed))
    rows = {row["id"]: row for row in r.json()["results"]}
    touched = rows[str(seed["run"]["id"])]["issues"]
    identifier = seed["project"]["identifier"]
    assert [issue["id"] for issue in touched] == [
        str(seed["issue_1"]["id"]),
        str(seed["issue_2"]["id"]),
        str(seed["issue_1"]["id"]),
    ]
    assert [issue["identifier"] for issue in touched] == [
        f"{identifier}-21",
        f"{identifier}-22",
        f"{identifier}-21",
    ]
    assert [issue["name"] for issue in touched] == [
        "Repeated issue",
        "Once issue",
        "Repeated issue",
    ]
