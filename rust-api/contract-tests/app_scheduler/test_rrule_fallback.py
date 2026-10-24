"""PIDASHCONV-1300: bindings with RRULEs the ``rrule`` crate refuses.

Installs one binding per issue-table rule via the API and asserts
``next_run_at`` and the occurrences list match Django. Expectations come
from ``dateutil`` here — the same engine Django expands with — computed
against the live ``now``, so the test self-adjusts to wall-clock.

Pre-fix Rust answers 201 with ``next_run_at`` null and empty occurrences
(the crate refuses the rule); post-fix it matches Django on both
backends. ``INTERVAL=70000`` fires in 2218, outside any 90-day window, so
that case asserts ``next_run_at`` only.
"""

import datetime

from dateutil.rrule import rrulestr

from .test_occurrences import occurrences
from .test_project_bindings import _install, _scheduler

DTSTART = datetime.datetime(2026, 10, 19, 12, 0, tzinfo=datetime.timezone.utc)

TABLE_RULES = {
    "weekly-bymonthday": "FREQ=WEEKLY;BYDAY=FR;BYMONTHDAY=13",
    "daily-byyearday": "FREQ=DAILY;BYYEARDAY=-1",
    "monthly-byweekno": "FREQ=MONTHLY;BYWEEKNO=20",
    "interval-wide": "FREQ=DAILY;INTERVAL=70000",
}


def _parse(dtstr):
    return datetime.datetime.fromisoformat(dtstr)


def _install_rule(org, slug, rrule):
    sched = _scheduler(org, slug=slug)
    before = datetime.datetime.now(datetime.timezone.utc)
    r = _install(
        org, sched["id"],
        dtstart=DTSTART.isoformat(), tzid="UTC", rrule=rrule,
    )
    after = datetime.datetime.now(datetime.timezone.utc)
    assert r.status_code == 201, r.text
    return r.json(), before, after


def _scheduled_dtstarts(org, binding_id, window_start, window_end):
    r = org.request(
        "GET", occurrences(org), "guest",
        params={"from": window_start.isoformat(), "to": window_end.isoformat()},
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["has_more"] is False
    return sorted(
        occ["dtstart"]
        for occ in body["occurrences"]
        if occ["binding_id"] == binding_id and occ["kind"] == "scheduled"
    )


def _check_rule(org, rule_key):
    rrule = TABLE_RULES[rule_key]
    body, _, after = _install_rule(org, f"r1300-{rule_key}", rrule)
    assert body["enabled"] is True
    # Fire times are days apart; nothing fires inside the POST round-trip,
    # so `after` pins the same answer the server computed.
    oracle = rrulestr(rrule, dtstart=DTSTART)
    expected_fire = oracle.after(after, inc=False)
    assert expected_fire is not None
    assert body["next_run_at"] is not None, f"{rule_key}: next_run_at is null"
    assert _parse(body["next_run_at"]) == expected_fire
    if rule_key == "interval-wide":
        return
    window_start = expected_fire - datetime.timedelta(days=30)
    window_end = expected_fire + datetime.timedelta(days=30)
    expected_occs = [o.isoformat() for o in oracle.between(window_start, window_end, inc=True)]
    assert expected_occs, f"{rule_key}: oracle found no occurrences in window"
    got = _scheduled_dtstarts(org, body["id"], window_start, window_end)
    assert [ _parse(d) for d in got ] == [ _parse(d) for d in expected_occs ]


def test_weekly_bymonthday_matches_django(org):
    _check_rule(org, "weekly-bymonthday")


def test_daily_byyearday_matches_django(org):
    _check_rule(org, "daily-byyearday")


def test_monthly_byweekno_matches_django(org):
    _check_rule(org, "monthly-byweekno")


def test_interval_wide_matches_django(org):
    _check_rule(org, "interval-wide")
