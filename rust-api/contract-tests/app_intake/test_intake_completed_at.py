"""Intake create drops a client-supplied completed_at (PIDASHCONV-1432).

Django's ``Issue.save`` clears ``completed_at`` whenever the state's group
is not completed (``db/models/issue.py``); intake create always assigns the
triage state, so a client-supplied timestamp must not survive. The Rust port
used to bind the parsed value into the INSERT while the create response
hardcoded null, hiding the stored timestamp until later reads.
"""

import pytest

from .conftest import intake_urls

pytestmark = pytest.mark.contract

SENT_COMPLETED_AT = "2026-01-01T00:00:00Z"


def test_intake_create_ignores_client_completed_at(make_tenant):
    tenant = make_tenant()
    urls = intake_urls(tenant)
    r = tenant["client"].post(
        urls["intake_issues"],
        json={"issue": {"name": "x", "completed_at": SENT_COMPLETED_AT}},
    )
    assert r.status_code == 200, r.text[:500]
    issue_id = r.json()["issue"]["id"]
    g = tenant["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert g.status_code == 200, g.text[:500]
    assert g.json()["issue"]["completed_at"] is None
