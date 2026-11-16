"""Cycle dates share one IANA release across backends (PDASHOSS01-331).

Django renders them with pytz and Rust with chrono-tz. Paraguay moved to
permanent UTC-3 in tzdata 2025a, so while Django pinned pytz 2024.1 a July
date in America/Asuncion came back with -04:00 from Django and -03:00 from
Rust. The requester's zone equals the project's so these cases pin the tzdata
release alone, not which zone the date is rendered in.
"""

import uuid
from datetime import datetime, timezone

import pytest

from .conftest import _create_cycle, cycle_urls

pytestmark = pytest.mark.contract

ZONE = "America/Asuncion"
START = datetime(2026, 7, 1, 12, 0, 0, 123456, tzinfo=timezone.utc)
END = datetime(2027, 1, 15, 12, 0, 0, tzinfo=timezone.utc)
START_WIRE = "2026-07-01T09:00:00.123456-03:00"
END_WIRE = "2027-01-15T09:00:00-03:00"


@pytest.fixture()
def asuncion_cycle(make_tenant):
    tenant = make_tenant()
    with tenant["seed"].conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET timezone = %s WHERE id = %s",
            (ZONE, tenant["project"]["id"]),
        )
        cur.execute(
            "UPDATE users SET user_timezone = %s WHERE id = %s",
            (ZONE, tenant["user"]["id"]),
        )
    cycle = _create_cycle(
        tenant["seed"], workspace_id=tenant["workspace"]["id"],
        project_id=tenant["project"]["id"], owner_id=tenant["user"]["id"],
        name=f"Asuncion {uuid.uuid4().hex[:6]}", start_date=START, end_date=END,
    )
    return tenant, cycle


def test_retrieve_renders_current_paraguay_offset(asuncion_cycle):
    tenant, cycle = asuncion_cycle
    resp = tenant["client"].get(cycle_urls(tenant)["detail"](cycle["id"]))
    assert resp.status_code == 200, resp.text
    body = resp.json()
    assert body["start_date"] == START_WIRE
    assert body["end_date"] == END_WIRE


def test_list_renders_current_paraguay_offset(asuncion_cycle):
    tenant, cycle = asuncion_cycle
    resp = tenant["client"].get(cycle_urls(tenant)["cycles"])
    assert resp.status_code == 200, resp.text
    row = next(item for item in resp.json() if item["id"] == cycle["id"])
    assert row["start_date"] == START_WIRE
    assert row["end_date"] == END_WIRE
