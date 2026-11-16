"""Contract test: intake description-versions list leaves last_saved_at in UTC.

PIDASHCONV-1436. Django `IntakeWorkItemDescriptionVersionEndpoint`
(`app/views/intake/base.py:569-637`) projects a `.values()` dict through
`user_timezone_converter` over `created_at`/`updated_at` only, so
`last_saved_at` stays UTC and DRF's JSON encoder renders it `...Z`.
There is no serializer field, so `TimezoneMixin` never applies. The
app_issues twin renders it as stored; the app_intake port wrongly
shifted it into the actor zone.
"""

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue
from .test_intake_issue_detail import _wait_for_versions

pytestmark = pytest.mark.contract

ZONE = "Asia/Kolkata"
OFFSET = "+05:30"


def test_versions_list_last_saved_at_stays_utc(make_tenant):
    tenant = make_tenant()
    tenant["_database"].execute(
        "UPDATE users SET user_timezone = %s WHERE id = %s",
        (ZONE, tenant["user"]["id"]),
    )
    urls = intake_urls(tenant)
    created = create_intake_issue(
        tenant["client"], urls["intake_issues"], name="TZ probe"
    )
    body = _wait_for_versions(
        tenant["client"], urls["versions"](created["issue"]["id"])
    )
    row = body["results"][0]
    assert row["last_saved_at"] is not None
    assert row["last_saved_at"].endswith("Z"), row["last_saved_at"]
    # Control: the converted fields really shifted, so the Z above is
    # meaningful rather than a UTC-test-user accident.
    assert row["created_at"].endswith(OFFSET), row["created_at"]
    assert row["updated_at"].endswith(OFFSET), row["updated_at"]
