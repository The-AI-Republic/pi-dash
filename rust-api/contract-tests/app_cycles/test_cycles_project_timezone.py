"""Cycle response dates render in the project's timezone (PDASHOSS01-327).

CycleViewSet returns raw ``.values()`` dicts converted by
``user_timezone_converter(.., project_timezone)``, so the requester's own zone
never reaches ``start_date``/``end_date``. The rest of the suite runs with both
zones on UTC, where the two readings cannot be told apart.
"""

from datetime import datetime, timezone

import pytest

from _harness import env
from _harness.seed import ADMIN, GUEST
from .conftest import _create_cycle, cycle_urls, set_project_timezone
from .test_cycles_access import _join_project

pytestmark = pytest.mark.contract

TOKYO = "Asia/Tokyo"
START = datetime(2030, 1, 15, 20, 0, 0, 123456, tzinfo=timezone.utc)
END = datetime(2030, 1, 22, 20, 0, 0, 123456, tzinfo=timezone.utc)

# project zone -> (start_date, end_date). London in winter is a named zone at
# offset zero, and New York lands on a different calendar day than Tokyo.
RENDERED = {
    "UTC": ("2030-01-15T20:00:00.123456Z", "2030-01-22T20:00:00.123456Z"),
    "Europe/London": ("2030-01-15T20:00:00.123456Z", "2030-01-22T20:00:00.123456Z"),
    "America/New_York": (
        "2030-01-15T15:00:00.123456-05:00",
        "2030-01-22T15:00:00.123456-05:00",
    ),
    TOKYO: (
        "2030-01-16T05:00:00.123456+09:00",
        "2030-01-23T05:00:00.123456+09:00",
    ),
}


def _set_user_timezone(tenant, tz):
    with tenant["seed"].conn.cursor() as cur:
        cur.execute(
            "update users set user_timezone=%s where id=%s", (tz, tenant["user"]["id"])
        )


def _dates(body):
    return body["start_date"], body["end_date"]


@pytest.fixture()
def dated_cycle(make_tenant):
    """An owner and a project guest, both in Asia/Tokyo, and a dated cycle."""
    owner = make_tenant(role=ADMIN)
    guest = make_tenant(role=GUEST)
    _join_project(owner, guest, GUEST)
    _set_user_timezone(owner, TOKYO)
    _set_user_timezone(guest, TOKYO)
    cycle = _create_cycle(
        owner["seed"], workspace_id=owner["workspace"]["id"],
        project_id=owner["project"]["id"], owner_id=owner["user"]["id"],
        name="Dated", start_date=START, end_date=END,
    )
    return {"owner": owner, "guest": guest, "pk": cycle["id"]}


@pytest.mark.parametrize("project_tz", sorted(RENDERED))
def test_reads_render_in_project_zone(dated_cycle, project_tz):
    owner, guest = dated_cycle["owner"], dated_cycle["guest"]
    pk = dated_cycle["pk"]
    set_project_timezone(owner, project_tz)
    urls = cycle_urls(owner)
    for client in (owner["client"], guest["client"]):
        r = client.get(urls["detail"](pk))
        assert r.status_code == 200, r.text[:500]
        assert _dates(r.json()) == RENDERED[project_tz]

        r = client.get(urls["cycles"])
        assert r.status_code == 200, r.text[:500]
        row = next(item for item in r.json() if item["id"] == pk)
        assert _dates(row) == RENDERED[project_tz]


def test_patch_renders_in_project_zone(dated_cycle):
    owner, pk = dated_cycle["owner"], dated_cycle["pk"]
    set_project_timezone(owner, "America/New_York")
    r = owner["client"].patch(cycle_urls(owner)["detail"](pk), json={"name": "Renamed"})
    assert r.status_code == 200, r.text[:500]
    assert _dates(r.json()) == RENDERED["America/New_York"]


def test_create_renders_in_project_zone(dated_cycle):
    owner = dated_cycle["owner"]
    set_project_timezone(owner, "America/New_York")
    r = owner["client"].post(
        cycle_urls(owner)["cycles"],
        json={"name": "Created", "start_date": "2031-03-03", "end_date": "2031-03-05"},
    )
    assert r.status_code == 201, r.text[:500]
    owner["seed"].track("cycles", "id", r.json()["id"])
    assert _dates(r.json()) == (
        "2031-03-03T00:00:01-05:00",
        "2031-03-05T23:59:00-05:00",
    )


def test_unknown_project_zone_is_key_error_400(dated_cycle):
    owner, pk = dated_cycle["owner"], dated_cycle["pk"]
    set_project_timezone(owner, "Not/AZone")
    urls = cycle_urls(owner)
    for url in (urls["detail"](pk), urls["cycles"]):
        r = owner["client"].get(url)
        assert r.status_code == 400, r.text[:500]
        assert r.json() == {"error": "The required key does not exist."}


def test_archived_list_stays_utc(dated_cycle):
    """The archived read has no converter, so it stays in stored UTC."""
    owner = dated_cycle["owner"]
    set_project_timezone(owner, "America/New_York")
    with owner["seed"].conn.cursor() as cur:
        cur.execute(
            "update cycles set archived_at=now() where id=%s", (dated_cycle["pk"],)
        )
    r = owner["client"].get(
        f"/api/workspaces/{owner['workspace']['slug']}"
        f"/projects/{owner['project']['id']}/archived-cycles/"
    )
    assert r.status_code == 200, r.text[:500]
    row = next(item for item in r.json() if item["id"] == dated_cycle["pk"])
    assert _dates(row) == RENDERED["UTC"]
