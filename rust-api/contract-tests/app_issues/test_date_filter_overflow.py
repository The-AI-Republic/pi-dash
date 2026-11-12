"""Huge relative date magnitudes: HTTP 500 on both backends, never dropped.

PIDASHCONV-1318: ``?start_date=20000000000000_weeks;after;fromnow`` overflows
the calendar. Django raises ``OverflowError`` in ``timedelta()`` and answers
its generic 500; Rust must map it to ``DateOverflow`` -> 500 instead of
panicking the handler task (a panic drops the connection: no ``CatchPanic``
layer, so httpx raises instead of returning a response).
"""

SERVER_ERROR = {"error": "Something went wrong please try again later"}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def test_huge_relative_weeks_is_500_not_dropped(clients, seed):
    admin = clients["admin"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = admin.get(
        f"{_base(ws, pid)}/issues/"
        "?start_date=20000000000000_weeks;after;fromnow"
    )
    assert resp.status_code == 500
    assert resp.json() == SERVER_ERROR
