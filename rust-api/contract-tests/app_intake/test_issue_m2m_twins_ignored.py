"""Contract tests: intake PATCH ignores the issue M2M twins (PIDASHCONV-1442).

On the app tier the issue branch goes through `IssueCreateSerializer`
(`app/serializers/issue.py`), which declares only `label_ids` /
`assignee_ids` as writable. `labels` / `assignees` are M2M fields with a
custom through, so DRF auto-maps them read-only and silently drops the
keys: the rest of the payload still saves and the answer is 200 — even
for `assignees: ["bogus"]`, which never reaches validation.
"""

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract


def test_patch_issue_labels_empty_ignored(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(
        admin["client"], urls["intake_issues"], name="CT1442 labels"
    )
    issue_id = created["issue"]["id"]
    r = admin["client"].patch(
        f"{urls['intake_issues']}{issue_id}/",
        json={"issue": {"name": "CT1442 labels saved", "labels": []}},
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["name"] == "CT1442 labels saved"
    r = admin["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["name"] == "CT1442 labels saved"


def test_patch_issue_assignees_bogus_ignored(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(
        admin["client"], urls["intake_issues"], name="CT1442 assignees"
    )
    issue_id = created["issue"]["id"]
    r = admin["client"].patch(
        f"{urls['intake_issues']}{issue_id}/",
        json={"issue": {"name": "CT1442 assignees saved", "assignees": ["bogus"]}},
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["name"] == "CT1442 assignees saved"
    r = admin["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue"]["name"] == "CT1442 assignees saved"
