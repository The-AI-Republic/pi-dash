"""Paren-form BYDAY contract (PIDASHCONV-1299).

dateutil accepts the ``TH(+1)`` paren spelling — and the degenerate
``MO(1€`` (the dropped last char leaves a valid int) — and expands both
as the first weekday of the month. The Rust backend must accept, fire,
and list them identically: before the fix the install panicked (dropped
connection) on the non-ASCII tail and stored a NULL ``next_run_at`` on
the ASCII form.
"""

import datetime

from .test_occurrences import occurrences
from .test_project_bindings import bind_list
from .test_workspace_schedulers import sched_list


def _scheduler(org, slug):
    r = org.request(
        "POST", sched_list(org), "admin",
        json={"slug": slug, "name": slug, "prompt": "x", "color": "#10b981"},
    )
    assert r.status_code == 201, r.text
    return r.json()


def _install(org, scheduler_id, rrule):
    return org.request(
        "POST", bind_list(org), "admin",
        json={
            "scheduler": scheduler_id,
            "project": org.project["id"],
            "dtstart": "2026-01-05T09:00:00+00:00",
            "tzid": "UTC",
            "rrule": rrule,
        },
    )


def _parse_instant(value):
    return datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))


def _first_weekday_of_month(year, month, weekday):
    """Date of the first `weekday` (Monday=0) of the month."""
    first = datetime.date(year, month, 1)
    return first + datetime.timedelta(days=(weekday - first.weekday()) % 7)


def _expected_fires(start, end, weekday):
    """First-`weekday`-of-month 09:00 UTC fires within `[start, end]`."""
    fires = []
    year, month = start.year, start.month
    while True:
        candidate = datetime.datetime.combine(
            _first_weekday_of_month(year, month, weekday),
            datetime.time(9, 0),
            tzinfo=datetime.timezone.utc,
        )
        if candidate > end:
            return fires
        if candidate >= start:
            fires.append(candidate)
        month += 1
        if month > 12:
            year, month = year + 1, 1


def _assert_paren_binding(org, slug, rrule, weekday):
    sched = _scheduler(org, slug)
    before = datetime.datetime.now(datetime.timezone.utc)
    r = _install(org, sched["id"], rrule)
    assert r.status_code == 201, r.text
    body = r.json()
    window_end = before + datetime.timedelta(days=75)
    expected = _expected_fires(before, window_end, weekday)
    assert len(expected) >= 2
    # The install computes the first fire strictly after now.
    assert body["next_run_at"] is not None
    assert _parse_instant(body["next_run_at"]) == expected[0]
    # The occurrences view lists the same monthly series.
    r = org.request(
        "GET", occurrences(org), "guest",
        params={"from": before.isoformat(), "to": window_end.isoformat()},
    )
    assert r.status_code == 200, r.text
    rows = [
        occ for occ in r.json()["occurrences"]
        if occ["binding_id"] == body["id"] and occ["kind"] == "scheduled"
    ]
    assert [_parse_instant(occ["dtstart"]) for occ in rows] == expected


def test_paren_byday_non_ascii_tail_matches_django(org):
    _assert_paren_binding(org, "paren-euro", "FREQ=MONTHLY;BYDAY=MO(1€", 0)


def test_paren_byday_ascii_matches_django(org):
    _assert_paren_binding(org, "paren-ascii", "FREQ=MONTHLY;BYDAY=TH(+1)", 3)
