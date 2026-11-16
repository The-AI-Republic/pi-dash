"""`POST cycles/date-check/` across DST transitions (PIDASHCONV-1191).

Django `convert_to_utc` (`utils/timezone_converter.py:40-94`, pytz 2025.2)
localizes local *midnight* with `local_tz.localize(...)` — the default
`is_dst=False`, which never raises: an ambiguous time resolves to standard
time, a gap to the pre-gap standard offset — and only then adds `+1s`
(start) / `+23:59` (end) as a fixed-offset `timedelta`. The instant is
therefore always defined and anchored to midnight's offset.

The Rust port localized 00:00:01 / 23:59 directly with `.single()`, so a
DST transition at midnight answers 500, and a mid-day transition shifts
the end instant by an hour. These tests pin Django's bytes with the
project timezone set to America/Santiago.

NEWPIDASH-212 pins the same rule on the write path (`POST cycles/`):
`convert_to_utc` is shared by date-check and create/PATCH, so the end
instant must stay anchored to midnight's offset in America/New_York and
a gap midnight in Africa/Cairo must answer 201, not 500.
"""

from datetime import datetime, timezone

import pytest

from .conftest import create_cycle_between, cycle_urls, set_project_timezone

pytestmark = pytest.mark.contract

SANTIAGO = "America/Santiago"


def _tenant(make_tenant):
    tenant = make_tenant()
    set_project_timezone(tenant, SANTIAGO)
    return tenant


def _date_check(tenant, start, end):
    urls = cycle_urls(tenant)
    return tenant["client"].post(
        urls["date_check"], json={"start_date": start, "end_date": end}
    )


def test_date_check_ambiguous_end_fall_back(make_tenant):
    # 2026-04-04: America/Santiago falls back at 24:00, so 23:59 wall time is
    # ambiguous. Django localizes midnight (still DST, -03:00) then adds
    # 23:59, giving an end of 2026-04-05T02:59:00Z — not 23:59's own
    # standard offset (2026-04-05T03:59:00Z).
    tenant = _tenant(make_tenant)

    r = _date_check(tenant, "2026-04-01", "2026-04-04")
    assert r.status_code == 200, r.text[:500]
    assert r.json() == {"status": True}

    # A cycle starting 03:30Z on 04-05 sits outside Django's window (ends
    # 02:59Z) but inside a 23:59-own-offset window (ends 03:59Z).
    create_cycle_between(
        tenant,
        datetime(2026, 4, 5, 3, 30, tzinfo=timezone.utc),
        datetime(2026, 4, 6, 3, 30, tzinfo=timezone.utc),
    )
    r = _date_check(tenant, "2026-04-01", "2026-04-04")
    assert r.status_code == 200, r.text[:500]
    assert r.json() == {"status": True}


def test_date_check_nonexistent_start_spring_forward(make_tenant):
    # 2026-09-06: America/Santiago springs forward at 00:00, so 00:00:01 does
    # not exist. Django localizes midnight with the pre-gap standard offset
    # (-04:00): start 2026-09-06T04:00:01Z, end 2026-09-07T03:59:00Z.
    tenant = _tenant(make_tenant)

    r = _date_check(tenant, "2026-09-06", "2026-09-10")
    assert r.status_code == 200, r.text[:500]
    assert r.json() == {"status": True}

    # A cycle ending 03:30Z on 09-06 is before Django's start (04:00:01Z)
    # but after a DST-offset start (03:00:01Z).
    create_cycle_between(
        tenant,
        datetime(2026, 9, 5, 3, 30, tzinfo=timezone.utc),
        datetime(2026, 9, 6, 3, 30, tzinfo=timezone.utc),
    )
    r = _date_check(tenant, "2026-09-06", "2026-09-10")
    assert r.status_code == 200, r.text[:500]
    assert r.json() == {"status": True}


def _instant(tenant, name, start_date, end_date):
    urls = cycle_urls(tenant)
    r = tenant["client"].post(
        urls["cycles"], json={"name": name, "start_date": start_date, "end_date": end_date}
    )
    assert r.status_code == 201, r.text[:500]
    body = r.json()
    return (
        datetime.fromisoformat(body["start_date"].replace("Z", "+00:00")),
        datetime.fromisoformat(body["end_date"].replace("Z", "+00:00")),
    )


def test_create_end_fall_back_uses_midnight_offset(make_tenant):
    # NEWPIDASH-212: 2026-11-01 in America/New_York is the fall-back day.
    # Midnight is EDT (-04) so Django's end is midnight+23:59 =
    # 2026-11-02T03:59:00Z; localising 23:59 (EST) gives 04:59Z.
    # (End only: start has Django's same-day `now` branch, which would
    # flake when the run date equals the test date in the project zone.)
    tenant = make_tenant()
    set_project_timezone(tenant, "America/New_York")
    _, end = _instant(tenant, "Fall back", "2026-11-01", "2026-11-01")
    assert end == datetime(2026, 11, 2, 3, 59, tzinfo=timezone.utc)


def test_create_end_spring_forward_uses_midnight_offset(make_tenant):
    # 2027-03-14 in America/New_York is the spring-forward day. Midnight
    # is EST (-05) so Django's end is midnight+23:59 = 2027-03-15T04:59Z;
    # localising 23:59 (EDT) gives 03:59Z.
    tenant = make_tenant()
    set_project_timezone(tenant, "America/New_York")
    _, end = _instant(tenant, "Spring forward", "2027-03-14", "2027-03-14")
    assert end == datetime(2027, 3, 15, 4, 59, tzinfo=timezone.utc)


def test_create_start_gap_midnight_does_not_500(make_tenant):
    # Egypt's DST starts at 00:00 on 2026-04-24, so midnight does not
    # exist. Django answers 201 with the pre-transition (+02) midnight:
    # start 2026-04-23T22:00:01Z; the Rust port answered 500.
    tenant = make_tenant()
    set_project_timezone(tenant, "Africa/Cairo")
    start, end = _instant(tenant, "Cairo gap", "2026-04-24", "2026-04-30")
    assert start == datetime(2026, 4, 23, 22, 0, 1, tzinfo=timezone.utc)
    assert end == datetime(2026, 4, 30, 20, 59, tzinfo=timezone.utc)


def test_date_check_conflict_inside_transition_window(make_tenant):
    # Same transition day, but a cycle planted squarely inside the queried
    # window still conflicts (the verdict, not just the status code).
    tenant = _tenant(make_tenant)
    create_cycle_between(
        tenant,
        datetime(2026, 9, 7, 12, 0, tzinfo=timezone.utc),
        datetime(2026, 9, 8, 12, 0, tzinfo=timezone.utc),
    )
    r = _date_check(tenant, "2026-09-06", "2026-09-10")
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert body["status"] is False
    assert body["error"].startswith(
        "You have a cycle already on the given dates"
    )
