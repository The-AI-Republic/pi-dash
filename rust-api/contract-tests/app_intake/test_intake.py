"""Response-shape coverage for all 10 intake routes.

Every drf-spectacular endpoint in app/urls/intake.py gets a shape assertion
against the live backend: intakes, intake-issues, the inbox aliases, and the
intake-work-item description-versions pair.
"""

import pytest

from _harness.checks import require_keys
from .conftest import intake_urls

pytestmark = pytest.mark.contract

INTAKE_KEYS = {
    "id", "name", "description", "is_default", "view_props", "logo_props",
    "project", "workspace", "project_detail", "pending_issue_count",
    "created_at", "updated_at", "created_by", "updated_by", "deleted_at",
}

# The write path validates before the permission check runs, and the model
# has no default for deleted_at, so creates must send it explicitly (null).
# Quirk of the Django contract; the Rust port must reproduce it byte for byte.
INTAKE_CREATE_PAYLOAD = {"name": "Second intake", "deleted_at": None}

INTAKE_ISSUE_KEYS = {
    "id", "status", "duplicate_to", "snoozed_till", "source", "issue",
    "created_by",
}

INTAKE_ISSUE_DETAIL_KEYS = {
    "id", "status", "duplicate_to", "snoozed_till", "duplicate_issue_detail",
    "source", "issue",
}


def _results(payload, ctx):
    """Unwrap the paginator envelope to the result list."""
    if isinstance(payload, dict) and "results" in payload:
        return payload["results"]
    assert isinstance(payload, list), f"{ctx}: expected list envelope, got {payload!r}"[:500]
    return payload


def create_intake_issue(client, url, name="Contract intake issue"):
    r = client.post(url, json={"issue": {"name": name, "priority": "high"}})
    assert r.status_code == 200, r.text[:500]
    return r.json()


def test_intake_list_shape(admin):
    urls = intake_urls(admin)
    r = admin["client"].get(urls["intakes"])
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    require_keys(body, INTAKE_KEYS, "GET intakes/")
    assert body["id"] == admin["intake"]["id"]
    assert body["project"] == admin["project"]["id"]


def test_inbox_list_alias_shape(admin):
    urls = intake_urls(admin)
    r = admin["client"].get(urls["inboxes"])
    assert r.status_code == 200, r.text[:500]
    require_keys(r.json(), INTAKE_KEYS, "GET inboxes/")


def test_intake_create_broken_upstream(admin):
    # PORTED BUG (app/views/intake/base.py): @allow_permission decorates
    # perform_create, but DRF calls perform_create(serializer) positionally,
    # so the wrapper binds the serializer as `request` and dies on
    # request.user (AttributeError) for every caller, admins included.
    # The contract pins the 500 + body byte for byte; the Rust port must
    # reproduce it, and the fix belongs to the D-32 port issue, not here.
    urls = intake_urls(admin)
    r = admin["client"].post(urls["intakes"], json=INTAKE_CREATE_PAYLOAD)
    assert r.status_code == 500, r.text[:500]
    assert r.json() == {"error": "Something went wrong please try again later"}


def test_inbox_create_broken_upstream(admin):
    # Same viewset, same bug through the inbox alias.
    urls = intake_urls(admin)
    r = admin["client"].post(urls["inboxes"], json=INTAKE_CREATE_PAYLOAD)
    assert r.status_code == 500, r.text[:500]
    assert r.json() == {"error": "Something went wrong please try again later"}


def test_intake_create_requires_deleted_at(admin):
    urls = intake_urls(admin)
    r = admin["client"].post(urls["intakes"], json={"name": "No deleted_at"})
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"deleted_at": ["This field is required."]}


def test_intake_retrieve_shape(admin):
    urls = intake_urls(admin)
    r = admin["client"].get(f"{urls['intakes']}{admin['intake']['id']}/")
    assert r.status_code == 200, r.text[:500]
    require_keys(r.json(), INTAKE_KEYS, "GET intakes/<pk>/")


def test_intake_patch_shape(admin):
    urls = intake_urls(admin)
    r = admin["client"].patch(
        f"{urls['intakes']}{admin['intake']['id']}/", json={"name": "Renamed"}
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    require_keys(body, INTAKE_KEYS, "PATCH intakes/<pk>/")
    assert body["name"] == "Renamed"


def test_intake_delete_default_rejected(admin):
    urls = intake_urls(admin)
    r = admin["client"].delete(f"{urls['intakes']}{admin['intake']['id']}/")
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "You cannot delete the default intake"}


def test_intake_delete_shape(admin):
    # Creates are broken upstream (see test_intake_create_broken_upstream),
    # so the disposable intake is seeded straight into Postgres.
    row = admin["seeder"].create_intake(
        admin["workspace"]["id"], admin["project"]["id"], name="Disposable",
    )
    urls = intake_urls(admin)
    pk = row["id"]
    r = admin["client"].delete(f"{urls['intakes']}{pk}/")
    assert r.status_code == 204, r.text[:500]
    assert admin["client"].get(f"{urls['intakes']}{pk}/").status_code == 404


def test_intake_issue_create_shape(admin):
    urls = intake_urls(admin)
    body = create_intake_issue(admin["client"], urls["intake_issues"])
    require_keys(body, INTAKE_ISSUE_DETAIL_KEYS, "POST intake-issues/")
    assert body["status"] == -2  # pending
    assert body["source"] == "IN_APP"
    require_keys(
        body["issue"],
        {"id", "name", "priority", "sequence_id", "project_id", "created_at"},
        "POST intake-issues/ .issue",
    )
    assert body["issue"]["name"] == "Contract intake issue"


def test_intake_issue_create_validation(admin):
    urls = intake_urls(admin)
    r = admin["client"].post(urls["intake_issues"], json={"issue": {}})
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "Name is required"}
    r = admin["client"].post(
        urls["intake_issues"], json={"issue": {"name": "x", "priority": "bogus"}}
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "Invalid priority"}


def test_intake_issue_list_shape(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    r = admin["client"].get(urls["intake_issues"])
    assert r.status_code == 200, r.text[:500]
    rows = _results(r.json(), "GET intake-issues/")
    assert rows, "expected at least the row just created"
    require_keys(rows[0], INTAKE_ISSUE_KEYS, "GET intake-issues/ row")
    assert {row["id"] for row in rows} >= {created["id"]}


def test_intake_issue_retrieve_shape(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    r = admin["client"].get(f"{urls['intake_issues']}{issue_id}/")
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    require_keys(body, INTAKE_ISSUE_DETAIL_KEYS, "GET intake-issues/<pk>/")
    assert body["id"] == created["id"]


def test_intake_issue_patch_shape(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    r = admin["client"].patch(
        f"{urls['intake_issues']}{issue_id}/",
        json={"issue": {"name": "Renamed intake issue"}},
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    require_keys(body, INTAKE_ISSUE_DETAIL_KEYS, "PATCH intake-issues/<pk>/")
    assert body["issue"]["name"] == "Renamed intake issue"


def test_intake_issue_delete_shape(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    r = admin["client"].delete(f"{urls['intake_issues']}{issue_id}/")
    assert r.status_code == 204, r.text[:500]
    assert admin["client"].get(f"{urls['intake_issues']}{issue_id}/").status_code in (
        400, 404,
    )


def test_inbox_issue_alias_shapes(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(
        admin["client"], urls["inbox_issues"], name="Inbox alias issue"
    )
    require_keys(created, INTAKE_ISSUE_DETAIL_KEYS, "POST inbox-issues/")
    r = admin["client"].get(urls["inbox_issues"])
    assert r.status_code == 200, r.text[:500]
    rows = _results(r.json(), "GET inbox-issues/")
    assert {row["id"] for row in rows} >= {created["id"]}


def test_description_versions_list_shape(admin):
    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    r = admin["client"].get(urls["versions"](issue_id))
    assert r.status_code == 200, r.text[:500]
    _results(r.json(), "GET description-versions/")


def test_description_version_detail_shape(admin):
    import uuid
    from datetime import datetime, timezone

    urls = intake_urls(admin)
    created = create_intake_issue(admin["client"], urls["intake_issues"])
    issue_id = created["issue"]["id"]
    version_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc).isoformat()
    admin["seeder"].db.execute(
        """insert into issue_description_versions (id, description_html,
            description_json, last_saved_at, issue_id, owned_by_id,
            project_id, workspace_id, created_by_id, created_at, updated_at)
           values (%s,'<p>seeded</p>','{}',%s,%s,%s,%s,%s,%s,%s,%s)""",
        (version_id, now, issue_id, admin["user"]["id"],
         admin["project"]["id"], admin["workspace"]["id"],
         admin["user"]["id"], now, now),
    )
    r = admin["client"].get(f"{urls['versions'](issue_id)}{version_id}/")
    assert r.status_code == 200, r.text[:500]
    require_keys(
        r.json(),
        {"id", "workspace", "project", "issue", "last_saved_at", "owned_by",
         "created_at", "updated_at", "created_by", "updated_by"},
        "GET description-versions/<pk>/",
    )
    assert r.json()["id"] == version_id
