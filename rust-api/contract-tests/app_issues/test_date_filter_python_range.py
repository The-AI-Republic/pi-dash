"""Mid-range relative dates outside Python's date range: HTTP 500 on both.

PIDASHCONV-1474: ``?start_date=1428572_weeks;after;fromnow`` (10,000,004
days; far beyond year 9999) fits ``chrono::NaiveDate`` (years
-262143..262143)
but not Python ``date`` (years 1..9999). Django raises ``OverflowError``
in ``now + timedelta`` and answers its generic 500; Rust must map the
out-of-range result to ``DateOverflow`` -> 500. Same for the past side
(``before;past``), which underflows year 1.
"""

SERVER_ERROR = {"error": "Something went wrong please try again later"}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def test_mid_range_future_weeks_is_500(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.get(
        f"{_base(ws, pid)}/issues/"
        "?start_date=1428572_weeks;after;fromnow"
    )
    assert resp.status_code == 500
    assert resp.json() == SERVER_ERROR


def test_mid_range_past_weeks_is_500(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.get(
        f"{_base(ws, pid)}/issues/"
        "?start_date=1428572_weeks;before;past"
    )
    assert resp.status_code == 500
    assert resp.json() == SERVER_ERROR
