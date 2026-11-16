"""Contract: intake writes strip with MLStripper (PIDASHCONV-1439).

``Issue.save`` computes ``description_stripped`` with
``pi_dash.utils.html_processor.strip_tags`` (``db/models/issue.py:21``) —
``MLStripper`` (``HTMLParser`` with ``convert_charrefs=True``), so
entities decode and a ``<`` that is not a tag stays as text. The
app_intake slice used Django-verbatim ``strip_tags`` on create and PATCH,
so rows written by Rust differed from Django's for the same input (same
class as PIDASHCONV-1168 and PIDASHCONV-1258, at the app_intake call
sites).
"""

import pytest

from .conftest import intake_urls

pytestmark = pytest.mark.contract

HTML = "<p>Tom &amp; Jerry &lt;3</p>"
DECODED = "Tom & Jerry <3"


def _stripped(tenant, issue_id):
    row = tenant["seeder"].db.fetchone(
        "SELECT description_stripped FROM issues WHERE id = %s", (issue_id,)
    )
    return row["description_stripped"]


def test_intake_create_stripped_decodes_entities(admin):
    # PIDASHCONV-1439: Issue.save (db/models/issue.py:329-334) strips with
    # MLStripper, so the issues row carries DECODED.
    urls = intake_urls(admin)
    r = admin["client"].post(
        urls["intake_issues"],
        json={"issue": {"name": "entity probe", "description_html": HTML}},
    )
    assert r.status_code == 200, r.text[:500]
    issue_id = r.json()["issue"]["id"]
    assert _stripped(admin, issue_id) == DECODED


def test_intake_patch_stripped_decodes_entities(admin):
    # PIDASHCONV-1439: the update arm (db/models/issue.py:344-349)
    # recomputes with the same stripper on PATCH.
    urls = intake_urls(admin)
    r = admin["client"].post(
        urls["intake_issues"], json={"issue": {"name": "entity patch probe"}}
    )
    assert r.status_code == 200, r.text[:500]
    issue_id = r.json()["issue"]["id"]
    r = admin["client"].patch(
        f"{urls['intake_issues']}{issue_id}/",
        json={"issue": {"description_html": HTML}},
    )
    assert r.status_code == 200, r.text[:500]
    assert _stripped(admin, issue_id) == DECODED
