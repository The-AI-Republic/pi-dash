"""Progress bytes: empty-cycle total_estimate_points is float 0.0 (PIDASHCONV-1415).

Django's ``Sum("value_as_float", default=Value(0), output_field=FloatField())``
(``base.py:709``) compiles to ``COALESCE(SUM(...), 0)`` typed double precision,
and ``base.py:774`` renders the total raw (no ``or 0``), so a cycle with no
points-estimated issues answers ``"total_estimate_points":0.0``. The five
group keys go through ``or 0`` and stay int ``0``. Parsed-JSON comparisons
(``0 == 0.0``) cannot see the difference, so this test compares raw bytes.
"""

import pytest

from .conftest import create_cycle_for, cycle_urls

pytestmark = pytest.mark.contract


def test_empty_cycle_total_is_float_zero_bytes(make_tenant):
    tenant = make_tenant()
    urls = cycle_urls(tenant)
    cycle_id = create_cycle_for(tenant, name="Empty progress bytes")["id"]
    r = tenant["client"].get(urls["progress"](cycle_id))
    assert r.status_code == 200, r.text[:500]
    assert b'"total_estimate_points":0.0' in r.content, r.content
    assert b'"completed_estimate_points":0,' in r.content, r.content
