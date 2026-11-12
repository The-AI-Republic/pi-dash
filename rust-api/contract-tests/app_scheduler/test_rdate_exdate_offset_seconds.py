"""rdates/exdates whose UTC offset carries seconds (PIDASHCONV-1302).

Both backends store ``parsed.isoformat()`` for rdates/exdates, and Python
3.12 renders ``:SS`` (plus ``.ffffff``) in the UTC offset whenever they are
non-zero. Django reads them back with ``fromisoformat``; the Rust jobs
``parse_iso_utc`` used RFC 3339 only (offsets just ``±HH:MM``) and silently
dropped them, so an exdated occurrence kept firing and a one-off rdate with
such an offset never fired.
"""

import datetime

from .test_occurrences import occurrences
from .test_project_binding_detail import bind_detail
from .test_project_bindings import _install, _scheduler


OFFSET = datetime.timedelta(hours=5, minutes=30, seconds=15)
OFFSET_MICRO = OFFSET + datetime.timedelta(microseconds=500_000)


def _floor_hour(moment):
    return moment.replace(minute=0, second=0, microsecond=0)


def _with_offset(moment_utc, offset, tail):
    """Render ``moment_utc`` as a wall clock with a seconds-carrying offset."""
    wall = (moment_utc + offset).replace(tzinfo=None)
    return wall.isoformat() + tail


def _grid():
    base = _floor_hour(
        datetime.datetime.now(datetime.timezone.utc)
    ) - datetime.timedelta(hours=1)
    return base


def _scheduled_instants(rows):
    return {
        datetime.datetime.fromisoformat(o["dtstart"])
        for o in rows
        if o["kind"] == "scheduled"
    }


def test_install_honors_offset_seconds_exdates_and_rdates(org):
    sched = _scheduler(org, slug="offsecs")
    base = _grid()
    e2 = base + datetime.timedelta(hours=2)
    e3 = base + datetime.timedelta(hours=3)
    one_off = base + datetime.timedelta(hours=2, minutes=30, microseconds=500_000)
    ex2 = _with_offset(e2, OFFSET, "+05:30:15")
    ex3 = _with_offset(e3, OFFSET, "+05:30:15")
    rdate = _with_offset(one_off, OFFSET_MICRO, "+05:30:15.500000")
    r = _install(
        org, sched["id"], dtstart=base.isoformat(),
        rdates=[rdate], exdates=[ex2, ex3],
    )
    assert r.status_code == 201, r.text
    body = r.json()
    # Both backends store parsed.isoformat(); these inputs are already canonical.
    assert body["rdates"] == [rdate]
    assert body["exdates"] == [ex2, ex3]
    # The exdates knock out the first two future hourly fires, so the one-off
    # rdate is next. Pre-fix Rust dropped all three entries: next was e2.
    assert datetime.datetime.fromisoformat(body["next_run_at"]) == one_off
    r = org.request(
        "GET", occurrences(org), "guest",
        params={
            "from": base.isoformat(),
            "to": (base + datetime.timedelta(hours=5)).isoformat(),
        },
    )
    assert r.status_code == 200, r.text
    got = _scheduled_instants(r.json()["occurrences"])
    assert e2 not in got
    assert e3 not in got
    assert one_off in got
    assert base + datetime.timedelta(hours=4) in got
    assert base + datetime.timedelta(hours=5) in got


def test_patch_exdates_with_offset_seconds_recompute_next_run(org):
    sched = _scheduler(org, slug="offsecs-patch")
    base = _grid()
    e2 = base + datetime.timedelta(hours=2)
    e3 = base + datetime.timedelta(hours=3)
    ex2 = _with_offset(e2, OFFSET, "+05:30:15")
    ex3 = _with_offset(e3, OFFSET, "+05:30:15")
    r = _install(org, sched["id"], dtstart=base.isoformat())
    assert r.status_code == 201, r.text
    binding = r.json()
    assert datetime.datetime.fromisoformat(binding["next_run_at"]) == e2
    r = org.request(
        "PATCH", bind_detail(org, binding["id"]), "admin",
        json={"exdates": [ex2, ex3]},
    )
    assert r.status_code == 200, r.text
    assert r.json()["exdates"] == [ex2, ex3]
    # With the first two future fires exdated, next advances past them.
    # Pre-fix Rust dropped the exdates: next stayed e2.
    assert (
        datetime.datetime.fromisoformat(r.json()["next_run_at"])
        == base + datetime.timedelta(hours=4)
    )
