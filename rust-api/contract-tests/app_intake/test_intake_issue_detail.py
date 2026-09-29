"""Contract tests for the intake-issue detail handlers (PIDASHCONV-395).

Covers `PATCH` / `GET` / `DELETE intake-issues/<pk>/` (+ the
`inbox-issues/<pk>/` aliases) and
`GET intake-work-items/<id>/description-versions[/<pk>/]` —
`app/views/intake/base.py:329-637`. Uses the suite fixtures (`admin`,
`make_tenant`, `intake_urls`), so the same tests run against Django
and against the Rust handlers through the proxy.
"""

import pytest

from _harness.auth import login_session
from _harness.checks import require_keys
from _harness.http import api_client
from _harness.seed import GUEST, MEMBER
from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract


def _teammate(admin, settings, *, role):
    """A second user inside the admin tenant's own workspace/project."""
    seeder = admin["seeder"]
    user = seeder.create_user()
    workspace_id = admin["workspace"]["id"]
    project_id = admin["project"]["id"]
    seeder.create_workspace_member(workspace_id, user["id"], role=role)
    seeder.create_project_member(workspace_id, project_id, user["id"], role=role)
    client = api_client(settings.base_url)
    return login_session(client, email=user["email"], password=user["password"])

INTAKE_ISSUE_DETAIL_KEYS = [
    "id",
    "status",
    "duplicate_to",
    "snoozed_till",
    "duplicate_issue_detail",
    "source",
    "issue",
]

VERSION_KEYS = [
    "id",
    "workspace",
    "project",
    "issue",
    "last_saved_at",
    "owned_by",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
]

VERSION_ENVELOPE_KEYS = [
    "prev_cursor",
    "cursor",
    "next_cursor",
    "prev_page_results",
    "next_page_results",
    "page_count",
    "total_results",
    "total_pages",
    "results",
]

VERSION_DETAIL_KEYS = VERSION_KEYS + [
    "description_binary",
    "description_html",
    "description_stripped",
    "description_json",
]

MISSING = "11111111-1111-4111-8111-111111111111"


def test_retrieve_shape(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(
        admin["client"], urls["intake_issues"], name="Detail probe"
    )
    issue_id = created["issue"]["id"]
    response = admin["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert response.status_code == 200, response.text
    body = response.json()
    require_keys(body, INTAKE_ISSUE_DETAIL_KEYS, "GET intake-issues/<pk>/")
    assert body["id"] == created["id"]
    assert body["issue"]["name"] == "Detail probe"
    assert body["issue"]["priority"] == "high"
    assert body["source"] == "IN_APP"


def test_retrieve_alias_matches(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    first = admin["client"].get(f"{urls['intake_issues']}{issue_id}/")
    second = admin["client"].get(f"{urls['inbox_issues']}{issue_id}/")
    assert first.status_code == 200, first.text
    assert second.status_code == 200, second.text
    assert first.json()["id"] == second.json()["id"]


def test_retrieve_missing_404(admin):
    urls = intake_urls(admin)
    response = admin["client"].get(f"{urls['intake_issues']}{MISSING}/")
    assert response.status_code == 404, response.text
    assert response.json() == {"error": "The required object does not exist."}


def test_patch_rename(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    response = admin["client"].patch(
        f"{urls['intake_issues']}{issue_id}/",
        json={"issue": {"name": "Renamed detail"}},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    require_keys(body, INTAKE_ISSUE_DETAIL_KEYS, "PATCH intake-issues/<pk>/")
    assert body["issue"]["name"] == "Renamed detail"
    assert body["issue"]["updated_by"] is not None
    again = admin["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert again.json()["issue"]["name"] == "Renamed detail"


def test_patch_alias(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    response = admin["client"].patch(
        f"{urls['inbox_issues']}{issue_id}/", json={"issue": {"priority": "low"}}
    )
    assert response.status_code == 200, response.text
    assert response.json()["issue"]["priority"] == "low"


def test_patch_blank_name_400(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    response = admin["client"].patch(
        f"{urls['intake_issues']}{issue_id}/", json={"issue": {"name": ""}}
    )
    assert response.status_code == 400, response.text
    assert response.json() == {"name": ["This field may not be blank."]}


def test_patch_bad_status_400(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    response = admin["client"].patch(
        f"{urls['intake_issues']}{issue_id}/", json={"status": 7}
    )
    assert response.status_code == 400, response.text
    assert response.json() == {"status": ['"7" is not a valid choice.']}


def test_patch_accept_without_default_400(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    response = admin["client"].patch(
        f"{urls['intake_issues']}{issue_id}/", json={"status": 1}
    )
    assert response.status_code == 400, response.text
    assert response.json() == {
        "status": ["Cannot accept intake issue: No default state found for the project"]
    }


def test_patch_triage_duplicate_rejected(admin):
    """`duplicate_to` is an auto FK field, so it binds the triage-hiding
    default manager: another triage intake issue is not a valid target."""
    urls = intake_urls(admin)
    first = create_intake_issue(admin["client"], urls["intake_issues"])
    second = create_intake_issue(
        admin["client"], urls["intake_issues"], name="Second intake"
    )
    response = admin["client"].patch(
        f"{urls['intake_issues']}{first['issue']['id']}/",
        json={"duplicate_to": second["issue"]["id"]},
    )
    assert response.status_code == 400, response.text
    assert list(response.json()) == ["duplicate_to"]


def test_patch_silent_migration_path(admin):
    """`skip_activity` with a top-level `description_html` saves the new
    description but emits no issue-branch activity (the migration-update
    silent path)."""
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    response = admin["client"].patch(
        f"{urls['intake_issues']}{issue_id}/",
        json={
            "skip_activity": True,
            "description_html": "<p>migrated</p>",
            "issue": {"description_html": "<p>migrated</p>"},
        },
    )
    assert response.status_code == 200, response.text
    assert response.json()["issue"]["description_html"] == "<p>migrated</p>"


def test_destroy_then_get_404(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(
        admin["client"], urls["intake_issues"], name="Doomed detail"
    )
    issue_id = created["issue"]["id"]
    deleted = admin["client"].delete(f"{urls['intake_issues']}{issue_id}/")
    assert deleted.status_code == 204, deleted.text
    assert deleted.content == b""
    gone = admin["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert gone.status_code == 404, gone.text
    again = admin["client"].delete(f"{urls['intake_issues']}{issue_id}/")
    assert again.status_code == 404, again.text


def test_destroy_missing_404(admin):
    urls = intake_urls(admin)
    response = admin["client"].delete(f"{urls['intake_issues']}{MISSING}/")
    assert response.status_code == 404, response.text


def test_versions_list_shape(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    response = admin["client"].get(urls["versions"](issue_id))
    assert response.status_code == 200, response.text
    body = response.json()
    require_keys(body, VERSION_ENVELOPE_KEYS, "GET description-versions/")
    assert body["total_results"] >= 1
    for row in body["results"]:
        require_keys(row, VERSION_KEYS, "version row")


def test_versions_detail_shape(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    listed = admin["client"].get(urls["versions"](issue_id))
    version_id = listed.json()["results"][0]["id"]
    response = admin["client"].get(f"{urls['versions'](issue_id)}{version_id}/")
    assert response.status_code == 200, response.text
    require_keys(response.json(), VERSION_DETAIL_KEYS, "GET description-versions/<pk>/")


def test_versions_missing_404(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    response = admin["client"].get(f"{urls['versions'](issue_id)}{MISSING}/")
    assert response.status_code == 404, response.text


def test_guest_cannot_read_other_issue(admin, make_tenant):
    tenant = make_tenant(role=GUEST)
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    denied = tenant["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert denied.status_code == 403, denied.text
    versions = tenant["client"].get(urls["versions"](issue_id))
    assert versions.status_code == 403, versions.text


def test_member_cannot_patch_other_issue(admin, make_tenant):
    tenant = make_tenant(role=MEMBER)
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    denied = tenant["client"].patch(
        f"{urls['intake_issues']}{issue_id}/", json={"issue": {"name": "Hijack"}}
    )
    assert denied.status_code == 403, denied.text


def test_teammate_member_cannot_patch_admin_issue(admin, settings):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    member = _teammate(admin, settings, role=MEMBER)
    try:
        denied = member.patch(
            f"{urls['intake_issues']}{issue_id}/",
            json={"issue": {"name": "Hijack"}},
        )
        assert denied.status_code == 403, denied.text
        assert denied.json() == {"error": "You don't have the required permissions."}
        allowed = member.get(f"{urls['intake_issues']}{issue_id}/")
        assert allowed.status_code == 200, allowed.text
    finally:
        member.close()


def test_teammate_guest_cannot_read_admin_issue(admin, settings):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    guest = _teammate(admin, settings, role=GUEST)
    try:
        denied = guest.get(f"{urls['intake_issues']}{issue_id}/")
        assert denied.status_code == 403, denied.text
        versions = guest.get(urls["versions"](issue_id))
        assert versions.status_code == 403, versions.text
    finally:
        guest.close()
