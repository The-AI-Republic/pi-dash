# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``pi_dash.bgtasks._rrule``: cron→RRULE conversion, RRULE
validation, and next-fire expansion.

These are pure-Python tests with no DB, so they're cheap to run.
"""

from __future__ import annotations

from datetime import datetime, timedelta, timezone as dt_timezone

import pytest

from pi_dash.bgtasks._rrule import (
    CronConversionError,
    RRuleValidationError,
    cron_to_rrule,
    next_fire_from_rrule,
    occurrences_between,
    validate_rrule_string,
)


# ---------------------------------------------------------------------------
# cron_to_rrule — happy path
# ---------------------------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize(
    "cron,expected_substr",
    [
        # MINUTELY
        ("* * * * *", "FREQ=MINUTELY"),
        ("*/5 * * * *", "FREQ=MINUTELY;INTERVAL=5"),
        ("*/30 * * * *", "FREQ=MINUTELY;INTERVAL=30"),
        # HOURLY
        ("0 * * * *", "FREQ=HOURLY"),
        ("15 * * * *", "BYMINUTE=15"),
        # "0 */6 * * *" classifies as DAILY because hour has a finite set
        # of values (0/6/12/18); the converter materializes the BYHOUR list
        # rather than emitting HOURLY;INTERVAL=6. Both forms fire at the
        # same instants; the BYHOUR form is also unambiguous about which
        # hours each day are active. test_next_fire_minutely_interval
        # confirms the timing semantics for INTERVAL cases.
        ("0 */6 * * *", "BYHOUR=0,6,12,18"),
        # DAILY
        ("0 9 * * *", "FREQ=DAILY"),
        ("0 9 * * *", "BYHOUR=9"),
        ("0 9 * * *", "BYMINUTE=0"),
        # WEEKLY
        ("0 9 * * 1-5", "FREQ=WEEKLY"),
        ("0 9 * * 1-5", "BYDAY=MO,TU,WE,TH,FR"),
        ("0 0 * * 0", "FREQ=WEEKLY"),
        ("0 0 * * 0", "BYDAY=SU"),
        # MONTHLY
        ("0 0 1 * *", "FREQ=MONTHLY"),
        ("0 0 1 * *", "BYMONTHDAY=1"),
        ("15 14 1,15 * *", "BYMONTHDAY=1,15"),
        # YEARLY
        ("0 0 * 6 *", "FREQ=YEARLY"),
        ("0 0 * 6 *", "BYMONTH=6"),
    ],
)
def test_cron_to_rrule_contains_expected(cron: str, expected_substr: str):
    rrule = cron_to_rrule(cron)
    assert expected_substr in rrule, f"cron {cron!r} → {rrule!r} (missing {expected_substr!r})"


@pytest.mark.unit
def test_cron_to_rrule_normalizes_sunday_7_to_0():
    # cron allows 0 or 7 for Sunday; both should produce SU.
    a = cron_to_rrule("0 0 * * 0")
    b = cron_to_rrule("0 0 * * 7")
    assert "BYDAY=SU" in a
    assert "BYDAY=SU" in b


# ---------------------------------------------------------------------------
# cron_to_rrule — rejections
# ---------------------------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize(
    "cron",
    [
        "",                  # empty
        "not a cron",        # garbage
        "* * * *",           # 4 fields
        "* * * * * *",       # 6 fields (croniter accepts; we don't)
        "60 * * * *",        # minute out of range
        "* 24 * * *",        # hour out of range
        "* * 0 * *",         # dom out of range (cron is 1-based)
        "* * 32 * *",        # dom out of range
        "* * * 13 *",        # month out of range
        "* * * * 8",         # dow out of range (after 7→0 normalization, still 8)
        "*/0 * * * *",       # zero interval
    ],
)
def test_cron_to_rrule_rejects_bad_input(cron: str):
    with pytest.raises(CronConversionError):
        cron_to_rrule(cron)


@pytest.mark.unit
def test_cron_to_rrule_rejects_dom_and_dow_together():
    # Vixie OR semantics can't be expressed as a single RRULE.
    with pytest.raises(CronConversionError, match="day-of-month and day-of-week"):
        cron_to_rrule("0 9 1 * 1")


# ---------------------------------------------------------------------------
# validate_rrule_string
# ---------------------------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize(
    "rrule",
    [
        "FREQ=MINUTELY",
        "FREQ=MINUTELY;INTERVAL=5",
        "FREQ=HOURLY;BYMINUTE=15",
        "FREQ=DAILY;BYHOUR=9;BYMINUTE=0",
        "FREQ=WEEKLY;BYDAY=MO,WE,FR",
        "FREQ=MONTHLY;BYMONTHDAY=1",
        "FREQ=YEARLY;BYMONTH=6",
        "RRULE:FREQ=DAILY",  # full iCal-line prefix — accepted via rrulestr
        "",                   # empty = single-shot, allowed
    ],
)
def test_validate_rrule_accepts_valid(rrule: str):
    validate_rrule_string(rrule)  # no exception


@pytest.mark.unit
def test_validate_rrule_rejects_secondly():
    with pytest.raises(RRuleValidationError, match="FREQ=SECONDLY"):
        validate_rrule_string("FREQ=SECONDLY")


@pytest.mark.unit
def test_validate_rrule_rejects_malformed():
    with pytest.raises(RRuleValidationError, match="invalid RRULE"):
        validate_rrule_string("not a real rrule")


# ---------------------------------------------------------------------------
# next_fire_from_rrule
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_next_fire_simple_daily():
    # Anchored Monday Jan 1, 9am UTC; daily at 9. Now = noon same day → next is Jan 2 9am.
    dtstart = datetime(2026, 1, 1, 9, 0, tzinfo=dt_timezone.utc)
    now = datetime(2026, 1, 1, 12, 0, tzinfo=dt_timezone.utc)
    nxt = next_fire_from_rrule(
        dtstart=dtstart,
        rrule_str="FREQ=DAILY;BYHOUR=9;BYMINUTE=0",
        now=now,
    )
    assert nxt == datetime(2026, 1, 2, 9, 0, tzinfo=dt_timezone.utc)


@pytest.mark.unit
def test_next_fire_minutely_interval():
    dtstart = datetime(2026, 1, 1, 0, 0, tzinfo=dt_timezone.utc)
    now = datetime(2026, 1, 1, 0, 12, tzinfo=dt_timezone.utc)
    nxt = next_fire_from_rrule(
        dtstart=dtstart,
        rrule_str="FREQ=MINUTELY;INTERVAL=5",
        now=now,
    )
    # Next 5-min boundary after 00:12 is 00:15
    assert nxt == datetime(2026, 1, 1, 0, 15, tzinfo=dt_timezone.utc)


@pytest.mark.unit
def test_next_fire_single_shot_empty_rrule():
    dtstart = datetime(2026, 6, 1, 9, 0, tzinfo=dt_timezone.utc)
    now = datetime(2026, 1, 1, 0, 0, tzinfo=dt_timezone.utc)
    nxt = next_fire_from_rrule(dtstart=dtstart, rrule_str="", now=now)
    assert nxt == dtstart


@pytest.mark.unit
def test_next_fire_single_shot_in_the_past():
    dtstart = datetime(2026, 1, 1, 9, 0, tzinfo=dt_timezone.utc)
    now = datetime(2026, 6, 1, 0, 0, tzinfo=dt_timezone.utc)
    nxt = next_fire_from_rrule(dtstart=dtstart, rrule_str="", now=now)
    assert nxt is None


@pytest.mark.unit
def test_next_fire_honors_exdates():
    dtstart = datetime(2026, 1, 1, 9, 0, tzinfo=dt_timezone.utc)
    now = datetime(2026, 1, 1, 12, 0, tzinfo=dt_timezone.utc)
    skip = datetime(2026, 1, 2, 9, 0, tzinfo=dt_timezone.utc)
    nxt = next_fire_from_rrule(
        dtstart=dtstart,
        rrule_str="FREQ=DAILY;BYHOUR=9;BYMINUTE=0",
        exdates=[skip],
        now=now,
    )
    # Jan 2 is excluded; next is Jan 3
    assert nxt == datetime(2026, 1, 3, 9, 0, tzinfo=dt_timezone.utc)


@pytest.mark.unit
def test_next_fire_honors_rdates():
    dtstart = datetime(2026, 1, 1, 9, 0, tzinfo=dt_timezone.utc)
    now = datetime(2026, 1, 1, 12, 0, tzinfo=dt_timezone.utc)
    extra = datetime(2026, 1, 1, 15, 0, tzinfo=dt_timezone.utc)
    nxt = next_fire_from_rrule(
        dtstart=dtstart,
        rrule_str="FREQ=DAILY;BYHOUR=9;BYMINUTE=0",
        rdates=[extra],
        now=now,
    )
    # The extra one-off at 15:00 same day comes before tomorrow 09:00
    assert nxt == extra


@pytest.mark.unit
def test_next_fire_returns_none_on_bad_rrule():
    dtstart = datetime(2026, 1, 1, 9, 0, tzinfo=dt_timezone.utc)
    nxt = next_fire_from_rrule(
        dtstart=dtstart,
        rrule_str="garbage",
        now=datetime(2026, 1, 1, 0, 0, tzinfo=dt_timezone.utc),
    )
    assert nxt is None


# ---------------------------------------------------------------------------
# occurrences_between (PR2 will exercise this in earnest; smoke-test here)
# ---------------------------------------------------------------------------


@pytest.mark.unit
def test_occurrences_between_daily_window():
    dtstart = datetime(2026, 1, 1, 9, 0, tzinfo=dt_timezone.utc)
    out, has_more = occurrences_between(
        dtstart=dtstart,
        rrule_str="FREQ=DAILY;BYHOUR=9;BYMINUTE=0",
        window_start=datetime(2026, 1, 1, 0, 0, tzinfo=dt_timezone.utc),
        window_end=datetime(2026, 1, 7, 23, 59, tzinfo=dt_timezone.utc),
    )
    # Jan 1, 2, 3, 4, 5, 6, 7 at 09:00 → 7 occurrences
    assert len(out) == 7
    assert has_more is False
    assert out[0] == dtstart


@pytest.mark.unit
def test_occurrences_between_honors_cap():
    dtstart = datetime(2026, 1, 1, 0, 0, tzinfo=dt_timezone.utc)
    out, has_more = occurrences_between(
        dtstart=dtstart,
        rrule_str="FREQ=MINUTELY",
        window_start=dtstart,
        window_end=dtstart + timedelta(days=10),
        cap=100,
    )
    assert len(out) == 100
    assert has_more is True
