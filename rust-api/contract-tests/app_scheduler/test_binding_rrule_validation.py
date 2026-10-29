"""Binding install/PATCH reject RRULEs dateutil's constructor rejects.

``validate_rrule_string`` runs the rule through ``rrulestr`` with an aware
anchor on the Django side, so ``rrule.__init__`` rejects a naive UNTIL, an
out-of-range BYSETPOS, and out-of-range BYHOUR/BYMINUTE/BYSECOND with 400
(``{"rrule": ["invalid RRULE: ..."]}``). The token-only Rust check accepted
and stored them (PIDASHCONV-1301).
"""

import pytest

from .test_project_binding_detail import bind_detail
from .test_project_bindings import _install, _scheduler

INVALID_RRULE_VECTORS = [
    (
        "FREQ=DAILY;UNTIL=20271231",
        "invalid RRULE: RRULE UNTIL values must be specified in UTC when "
        "DTSTART is timezone-aware",
    ),
    (
        "FREQ=DAILY;UNTIL=20271231T000000",
        "invalid RRULE: RRULE UNTIL values must be specified in UTC when "
        "DTSTART is timezone-aware",
    ),
    (
        "FREQ=DAILY;BYSETPOS=0",
        "invalid RRULE: bysetpos must be between 1 and 366, or between "
        "-366 and -1",
    ),
    ("FREQ=DAILY;BYMINUTE=60", "invalid RRULE: minute must be in 0..59"),
    ("FREQ=DAILY;BYHOUR=25", "invalid RRULE: hour must be in 0..23"),
    ("FREQ=DAILY;BYHOUR=24", "invalid RRULE: hour must be in 0..23"),
    ("FREQ=WEEKLY;BYSECOND=61", "invalid RRULE: second must be in 0..59"),
    # Awareness must match in either direction: a naive embedded DTSTART
    # with an aware UNTIL raises the same message (dateutil rrule.py:462-472).
    (
        "DTSTART:20270101T000000\nRRULE:FREQ=DAILY;UNTIL=20271231T000000Z",
        "invalid RRULE: RRULE UNTIL values must be specified in UTC when "
        "DTSTART is timezone-aware",
    ),
]


@pytest.mark.parametrize("rrule,expected", INVALID_RRULE_VECTORS)
def test_install_rejects_semantically_invalid_rrule(org, rrule, expected):
    # (scheduler, project) is unique, so each vector installs under its own
    # scheduler. A rejection writes no row.
    sched = _scheduler(org, slug=f"rr-{org.tag}")
    r = _install(org, sched["id"], rrule=rrule)
    assert r.status_code == 400, (rrule, r.status_code, r.text)
    assert r.json() == {"rrule": [expected]}, rrule
    row = org.conn.execute(
        "SELECT count(*) FROM scheduler_bindings WHERE project_id=%s AND rrule=%s",
        (org.project["id"], rrule),
    ).fetchone()
    assert row[0] == 0, rrule


@pytest.mark.parametrize("rrule,expected", INVALID_RRULE_VECTORS)
def test_patch_rejects_semantically_invalid_rrule(org, rrule, expected):
    sched = _scheduler(org, slug=f"rp-{org.tag}")
    r = _install(org, sched["id"])
    assert r.status_code == 201, r.text
    bid = r.json()["id"]
    r = org.request(
        "PATCH", bind_detail(org, bid), "admin", json={"rrule": rrule}
    )
    assert r.status_code == 400, (rrule, r.status_code, r.text)
    assert r.json() == {"rrule": [expected]}, rrule
    # A rejected PATCH leaves the stored rule untouched.
    row = org.conn.execute(
        "SELECT rrule FROM scheduler_bindings WHERE id=%s", (bid,)
    ).fetchone()
    assert row[0] == "FREQ=HOURLY"


def test_install_embedded_naive_dtstart_allows_naive_until(org):
    # `rrulestr` substitutes a DTSTART carried in the rule text for the
    # aware anchor, so a naive DTSTART + naive UNTIL is a valid (naive)
    # dateutil rule. Only the implicit aware anchor rejects a naive UNTIL.
    rrule = "DTSTART:20270101T000000\nRRULE:FREQ=DAILY;UNTIL=20271231"
    sched = _scheduler(org, slug=f"rn-{org.tag}")
    r = _install(org, sched["id"], rrule=rrule)
    assert r.status_code == 201, r.text
    assert r.json()["rrule"] == rrule
