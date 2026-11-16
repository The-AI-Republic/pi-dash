"""PIDASHCONV-1705: bindings with RRULEs the ``rrule`` crate accepts but
expands differently from ``dateutil``.

Installs one binding per issue-table rule via the API and asserts the
binding stays enabled with ``next_run_at`` and the occurrences list
matching Django. Expectations come from ``dateutil`` here — the same
engine Django expands with — computed against the live ``now``, so the
test self-adjusts to wall-clock.

Pre-fix Rust answers 201 with ``next_run_at`` null (or shifted),
``series_exhausted`` true, and empty/diverged occurrences (the crate
skips valid days/hours or ends the series; ordinal BYDAY fires daily);
post-fix it matches Django on both backends. The fire path keys its
quiet-complete disable off the same ``Ok(None)`` expansion, so the
install ``next_run_at``/``series_exhausted`` pins cover it without
dispatching live agent runs.
"""

import datetime

import pytest
from dateutil.rrule import rrulestr

from _harness import db
from app_scheduler.conftest import make_world
from app_scheduler.test_occurrences import occurrences
from app_scheduler.test_project_bindings import _install, _scheduler

UTC = datetime.timezone.utc

RULE_MINUTELY = "FREQ=MINUTELY;INTERVAL=30;BYHOUR=9,17;BYDAY=MO,WE,FR"
RULE_WEEKLY_ORD = "FREQ=WEEKLY;BYDAY=1MO"
RULE_WEEKLY_MIXED = "FREQ=WEEKLY;BYDAY=TH,+1TU"


@pytest.fixture()
def org():
    conn = db.connect()
    world = make_world(conn)
    yield world
    world.close()
    conn.close()


def _parse(dtstr):
    return datetime.datetime.fromisoformat(dtstr)


def _install_rule(org, slug, rrule, dtstart):
    sched = _scheduler(org, slug=slug)
    before = datetime.datetime.now(UTC)
    r = _install(
        org, sched["id"],
        dtstart=dtstart.isoformat(), tzid="UTC", rrule=rrule,
    )
    after = datetime.datetime.now(UTC)
    assert r.status_code == 201, r.text
    return r.json(), before, after


def _scheduled_dtstarts(org, binding_id, window_start, window_end):
    # The endpoint expands scheduled rows from max(from, server-now); the
    # server instant is unobservable, so both sides are compared strictly
    # after the response lands — deterministic regardless of skew.
    r = org.request(
        "GET", occurrences(org), "guest",
        params={"from": window_start.isoformat(), "to": window_end.isoformat()},
    )
    responded = datetime.datetime.now(UTC)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["has_more"] is False
    return responded, sorted(
        occ["dtstart"]
        for occ in body["occurrences"]
        if occ["binding_id"] == binding_id and occ["kind"] == "scheduled"
    )


def _check_rule(org, slug, rrule, dtstart, window_days=30):
    body, before, after = _install_rule(org, slug, rrule, dtstart)
    # The series is live: enabled, not exhausted, with a next fire.
    assert body["enabled"] is True
    assert body["series_exhausted"] is False, f"{slug}: reads as completed"
    assert body["next_run_at"] is not None, f"{slug}: next_run_at is null"
    oracle = rrulestr(rrule, dtstart=dtstart)
    # The server expanded at some instant inside the POST round-trip; the
    # answer can only be the pre- or post-trip fire (equal unless a fire
    # elapsed mid-POST).
    exp_before = oracle.after(before, inc=False)
    exp_after = oracle.after(after, inc=False)
    assert exp_after is not None
    got = _parse(body["next_run_at"])
    assert got == exp_after or (exp_before != exp_after and got == exp_before), (
        f"{slug}: next_run_at {got} != oracle {exp_after}"
    )
    window_start = after - datetime.timedelta(days=1)
    window_end = after + datetime.timedelta(days=window_days)
    responded, got_occs = _scheduled_dtstarts(
        org, body["id"], window_start, window_end)
    expected_occs = [
        o.isoformat()
        for o in oracle.between(window_start, window_end, inc=True)
        if o > responded
    ]
    got_occs = [d for d in got_occs if _parse(d) > responded]
    assert expected_occs, f"{slug}: oracle found no occurrences in window"
    assert [_parse(d) for d in got_occs] == [_parse(d) for d in expected_occs], (
        f"{slug}: {len(got_occs)} occurrences != {len(expected_occs)} oracle"
    )


def _last_sunday_0600():
    """Most recent Sunday 06:00 UTC at least 24h in the past.

    Mirrors the issue's none-dtstart (a non-BYDAY day at an off-BYHOUR
    hour, a day+ behind ``now``): pre-fix the crate ends this series
    with ``Ok(None)`` instead of firing.
    """
    now = datetime.datetime.now(UTC)
    days_back = (now.weekday() + 1) % 7  # Monday=0 .. Sunday=6
    candidate = (now - datetime.timedelta(days=days_back)).replace(
        hour=6, minute=0, second=0, microsecond=0)
    if candidate > now - datetime.timedelta(hours=24):
        candidate -= datetime.timedelta(days=7)
    return candidate


def test_minutely_byhour_byday_none_dtstart(org):
    _check_rule(org, "r1705-minutely", RULE_MINUTELY, _last_sunday_0600())


def test_weekly_ordinal_byday_is_mondays(org):
    dtstart = (datetime.datetime.now(UTC) - datetime.timedelta(hours=25)).replace(
        microsecond=0)
    _check_rule(org, "r1705-weekly-ord", RULE_WEEKLY_ORD, dtstart)


def test_weekly_mixed_ordinal_byday_keeps_tuesdays(org):
    dtstart = (datetime.datetime.now(UTC) - datetime.timedelta(hours=25)).replace(
        microsecond=0)
    _check_rule(org, "r1705-weekly-mixed", RULE_WEEKLY_MIXED, dtstart)
