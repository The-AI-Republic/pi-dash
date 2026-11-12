"""Binding install/PATCH reject RRULEs dateutil's constructor rejects.

``validate_rrule_string`` runs the rule through ``rrulestr`` with an aware
anchor on the Django side, so ``rrule.__init__`` rejects a naive UNTIL, an
out-of-range BYSETPOS, and out-of-range BYHOUR/BYMINUTE/BYSECOND with 400
(``{"rrule": ["invalid RRULE: ..."]}``). The token-only Rust check accepted
and stored them (PIDASHCONV-1301).
"""

import datetime

import pytest

from . import seed_scheduler as seed_s
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
    # A zero BYDAY ordinal is rejected while parsing (dateutil's `weekday`
    # raises for n == 0; PIDASHCONV-1706). Anchor-independent.
    ("FREQ=WEEKLY;BYDAY=0MO", "invalid RRULE: invalid 'BYDAY': 0MO"),
    ("FREQ=MONTHLY;BYDAY=+0MO", "invalid RRULE: invalid 'BYDAY': +0MO"),
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


# PIDASHCONV-1706: dateutil's `__construct_byset` rejects a rule whose
# listed BYHOUR (HOURLY) / BYMINUTE (MINUTELY) values no INTERVAL step can
# reach from the anchor. The field check anchors on the request time, the
# cross-field re-check on the binding's dtstart.
BYSET_EMPTY = "invalid RRULE: Invalid rrule byxxx generates an empty set."


def test_install_rejects_byset_unreachable_from_dtstart(org):
    # Whichever minute the request lands on, one of the two checks fails:
    # an even now-minute passes the field check but the odd dtstart fails
    # the cross-field re-check; an odd now-minute fails the field check.
    # Both render the same {"rrule": [...]} body, so this is deterministic.
    sched = _scheduler(org, slug=f"bs-{org.tag}")
    r = _install(
        org, sched["id"],
        rrule="FREQ=MINUTELY;INTERVAL=2;BYMINUTE=0",
        dtstart="2026-01-01T08:07:00+00:00",
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"rrule": [BYSET_EMPTY]}
    row = org.conn.execute(
        "SELECT count(*) FROM scheduler_bindings WHERE project_id=%s",
        (org.project["id"],),
    ).fetchone()
    assert row[0] == 0


def test_patch_dtstart_to_unreachable_minute_rejected(org):
    # A dtstart-only PATCH re-checks the stored rule against the new
    # anchor (serializer validate()). The row is seeded straight into
    # Postgres: installing the rule needs an even request minute, which
    # the clock does not promise. The absent rrule key skips field
    # validation on partial update, so only the cross-field check fires.
    sched = _scheduler(org, slug=f"bp-{org.tag}")
    binding = seed_s.create_binding(
        org.conn,
        workspace_id=org.workspace["id"],
        project_id=org.project["id"],
        scheduler_id=sched["id"],
        dtstart="2026-01-01T08:06:00+00:00",
        rrule="FREQ=MINUTELY;INTERVAL=2;BYMINUTE=0",
    )
    r = org.request(
        "PATCH", bind_detail(org, binding["id"]), "admin",
        json={"dtstart": "2026-01-01T08:07:00+00:00"},
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"rrule": [BYSET_EMPTY]}
    row = org.conn.execute(
        "SELECT dtstart FROM scheduler_bindings WHERE id=%s", (binding["id"],)
    ).fetchone()
    stored = row[0]
    if stored.tzinfo is not None:
        stored = stored.astimezone(datetime.timezone.utc).replace(tzinfo=None)
    assert stored.isoformat() == "2026-01-01T08:06:00"


REACHABLE_BYSET_RULES = [
    # gcd(1, base) == 1 keeps every value from any anchor.
    "FREQ=MINUTELY;BYMINUTE=0",
    "FREQ=HOURLY;BYHOUR=7",
    # Every residue class mod INTERVAL is listed, so one value is kept
    # from any anchor: installable whatever the clock says.
    "FREQ=MINUTELY;INTERVAL=2;BYMINUTE=0,1",
    "FREQ=HOURLY;INTERVAL=3;BYHOUR=0,1,2",
]


@pytest.mark.parametrize("rrule", REACHABLE_BYSET_RULES)
def test_install_accepts_reachable_byset_values(org, rrule):
    sched = _scheduler(org, slug=f"br-{org.tag}")
    r = _install(org, sched["id"], rrule=rrule)
    assert r.status_code == 201, (rrule, r.status_code, r.text)
    assert r.json()["rrule"] == rrule
