"""Contract: attachment/activity edge divergences (PIDASHCONV-789).

Follow-up to the PIDASHCONV-675 second review (PR #1115): reachable
divergences on unusual attachment/activity requests. Every test drives a
handler end to end over HTTP (item 10) — INSERT-then-sign order, PATCH /
DELETE updates — and pins the byte-level edge behavior (items 1-6) against
both the Django and the Rust legs.

Items 7-9 (S3 signer hosts/partitions/ports/base paths) are
storage-settings-dependent and cannot vary per request, so they are pinned
by the frozen botocore golden vectors in Rust
(``fixtures/v1_work_items/handlers/activity_signing.golden.json``), not
here.
"""

import base64
import json
import uuid

import pytest

from .conftest import issue_url

pytestmark = pytest.mark.contract

PDF = "application/pdf"


def sub_url(world, issue_id, *parts):
    return issue_url(world, issue_id) + "/".join(parts) + "/"


def attachment_url(world, issue_id):
    return sub_url(world, issue_id, "attachments")


def activities_url(world, issue_id):
    return sub_url(world, issue_id, "activities")


def policy_conditions(policy_b64):
    padded = policy_b64 + "=" * (-len(policy_b64) % 4)
    return json.loads(base64.b64decode(padded))["conditions"]


def test_attachment_post_filename_token_starts_with(api, world, db):
    """A name ending in ${filename} turns the second {"key"} condition into
    a starts-with on the key prefix (botocore generate_presigned_post)."""
    issue_id = world["issue"]["id"]
    ws_id = world["workspace"]["id"]
    response = api.post(
        attachment_url(world, issue_id),
        json={"name": "x${filename}", "type": PDF, "size": 10},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert set(body) == {"upload_data", "asset_id", "attachment", "asset_url"}
    conditions = policy_conditions(body["upload_data"]["fields"]["policy"])
    assert conditions[5][0] == "starts-with"
    assert conditions[5][1] == "$key"
    assert conditions[5][2].startswith(f"{ws_id}/")
    assert conditions[5][2].endswith("-x")
    # Insert-before-sign: the row the signature was minted for persists.
    row = db.fetchone(
        "SELECT attributes, asset, size, entity_type, is_uploaded"
        " FROM file_assets WHERE id = %s",
        (body["asset_id"],),
    )
    assert row is not None
    assert row["attributes"]["name"] == "x${filename}"
    assert row["entity_type"] == "ISSUE_ATTACHMENT"
    assert row["is_uploaded"] is False
    assert body["upload_data"]["fields"]["key"] == row["asset"]
    # The list only shows uploaded rows; flip the flag and it appears.
    assert api.get(attachment_url(world, issue_id)).json() == []
    db.execute(
        "UPDATE file_assets SET is_uploaded = TRUE WHERE id = %s",
        (body["asset_id"],),
    )
    listed = api.get(attachment_url(world, issue_id)).json()
    assert [row["id"] for row in listed] == [body["asset_id"]]


def test_attachment_post_plain_name_exact_key(api, world):
    """Control: without the token the policy pins the exact key twice."""
    issue_id = world["issue"]["id"]
    response = api.post(
        attachment_url(world, issue_id),
        json={"name": "plain.pdf", "type": PDF, "size": 10},
    )
    assert response.status_code == 200, response.text
    fields = response.json()["upload_data"]["fields"]
    conditions = policy_conditions(fields["policy"])
    assert conditions[5] == {"key": fields["key"]}
    assert fields["key"].endswith("-plain.pdf")


@pytest.mark.parametrize(
    ("spelling", "canonical"),
    [("1e2", "100.0"), ("1.50", "1.5"), ("0.5e1", "5.0")],
)
def test_attachment_post_float_spellings(api, world, spelling, canonical):
    """Non-canonical JSON number spellings echo as the stored float."""
    issue_id = world["issue"]["id"]
    raw = (
        '{"name": "spell.pdf", "type": "%s", "size": %s}' % (PDF, spelling)
    ).encode()
    response = api.post(
        attachment_url(world, issue_id),
        content=raw,
        headers={"Content-Type": "application/json"},
    )
    assert response.status_code == 200, response.text
    assert f'"size":{canonical}' in response.text
    assert response.json()["attachment"]["attributes"]["size"] == float(canonical)


def test_attachment_post_external_negative_zero(api, world, db):
    """Raw -0 externals store and echo as "0" (PIDASHCONV-1121).

    -0 is falsy so it skips the dup probe, but still stores through the
    CharField prep (str(int(-0)) == "0") on both backends. Raw bytes are
    load-bearing: Python json.dumps(-0) emits 0.
    """
    issue_id = world["issue"]["id"]
    response = api.post(
        attachment_url(world, issue_id),
        content=(
            '{"name": "ext.pdf", "type": "%s", "size": 10,'
            ' "external_id": -0, "external_source": -0}' % PDF
        ).encode(),
        headers={"Content-Type": "application/json"},
    )
    assert response.status_code == 200, response.text
    attachment = response.json()["attachment"]
    assert attachment["external_id"] == "0"
    assert attachment["external_source"] == "0"
    row = db.fetchone(
        "SELECT external_id, external_source FROM file_assets WHERE id = %s",
        (response.json()["asset_id"],),
    )
    assert row["external_id"] == "0"
    assert row["external_source"] == "0"


def test_attachment_post_numeric_name_spelling(api, world):
    """A numeric name rides the storage key in its float spelling."""
    issue_id = world["issue"]["id"]
    response = api.post(
        attachment_url(world, issue_id),
        content=('{"name": 1e2, "type": "%s", "size": 10}' % PDF).encode(),
        headers={"Content-Type": "application/json"},
    )
    assert response.status_code == 200, response.text
    assert response.json()["upload_data"]["fields"]["key"].endswith("-100.0")


def test_attachment_post_float_external_tokens(api, world, db):
    """Raw float tokens in the externals stringify like Python `str(float)`:
    `1e3` stores and echoes `1000.0`, not serde's normalized `1e+3`
    (PIDASHCONV-1130)."""
    issue_id = world["issue"]["id"]
    raw = (
        '{"name": "f.pdf", "type": "%s", "size": 10,'
        ' "external_id": 1e3, "external_source": "s"}' % PDF
    ).encode()
    response = api.post(
        attachment_url(world, issue_id),
        content=raw,
        headers={"Content-Type": "application/json"},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["attachment"]["external_id"] == "1000.0"
    assert body["attachment"]["external_source"] == "s"
    row = db.fetchone(
        "SELECT external_id, external_source FROM file_assets WHERE id = %s",
        (body["asset_id"],),
    )
    assert row["external_id"] == "1000.0"
    assert row["external_source"] == "s"


def test_attachment_post_nested_float_external(api, world, db):
    """A float nested in a container external stringifies the whole
    container (`repr`), so `[1e3]` stores and echoes `[1000.0]`
    (PIDASHCONV-1130)."""
    issue_id = world["issue"]["id"]
    raw = (
        '{"name": "n.pdf", "type": "%s", "size": 10,'
        ' "external_id": [1e3], "external_source": "s"}' % PDF
    ).encode()
    response = api.post(
        attachment_url(world, issue_id),
        content=raw,
        headers={"Content-Type": "application/json"},
    )
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["attachment"]["external_id"] == "[1000.0]"
    row = db.fetchone(
        "SELECT external_id FROM file_assets WHERE id = %s",
        (body["asset_id"],),
    )
    assert row["external_id"] == "[1000.0]"


def test_attachment_post_multipart_file_matrix(api, world):
    """An uploaded file part is truthy; the missing field trips first (400).
    A text size trips min() before the MIME check (500)."""
    url = attachment_url(world, world["issue"]["id"])
    file_part = ("n.pdf", b"%PDF-bytes", PDF)
    # File part named `name`, no size -> the missing size 400s.
    response = api.post(url, files={"name": file_part}, data={"type": PDF})
    assert response.status_code == 400, response.text
    assert response.json() == {"error": "Invalid request.", "status": False}
    # File part named `size`, no name -> the missing name 400s.
    response = api.post(url, files={"size": file_part}, data={"type": PDF})
    assert response.status_code == 400, response.text
    # File name + text size + bad type -> min(str) 500s before the type check.
    response = api.post(
        url,
        files={"name": file_part},
        data={"size": "10", "type": "text/plain"},
    )
    assert response.status_code == 500, response.text
    # All-text multipart -> the text size 500s in min().
    response = api.post(
        url, data={"name": "n.pdf", "type": PDF, "size": "10"}
    )
    assert response.status_code == 500, response.text
    # JSON string size takes the same path.
    response = api.post(url, json={"name": "n.pdf", "type": PDF, "size": "10"})
    assert response.status_code == 500, response.text


def test_activity_expand_deleted_issue(api, world, seeder, db):
    """expand=issue on a soft-deleted issue renders the issue (unscoped join)."""
    issue_id = world["issue"]["id"]
    activity = seeder.create_issue_activity(
        world["workspace"]["id"], world["project"]["id"], issue_id, verb="updated"
    )
    db.execute("UPDATE issues SET deleted_at = now() WHERE id = %s", (issue_id,))
    try:
        response = api.get(activities_url(world, issue_id), params={"expand": "issue"})
        assert response.status_code == 200, response.text
        rows = response.json()["results"]
        assert len(rows) == 1
        assert isinstance(rows[0]["issue"], dict)
        assert rows[0]["issue"]["id"] == issue_id
        detail = api.get(
            activities_url(world, issue_id) + f"{activity['id']}/",
            params={"expand": "issue"},
        )
        assert detail.status_code == 200, detail.text
        assert detail.json()["issue"]["id"] == issue_id
    finally:
        db.execute("UPDATE issues SET deleted_at = NULL WHERE id = %s", (issue_id,))


def test_comment_expand_deleted_issue(api, world, db):
    """Same unscoped expand on the merged social handler."""
    issue_id = world["issue"]["id"]
    comment = api.post(
        sub_url(world, issue_id, "comments"),
        json={"comment_html": "<p>hi</p>", "access": "EXTERNAL"},
    )
    assert comment.status_code == 201, comment.text
    db.execute("UPDATE issues SET deleted_at = now() WHERE id = %s", (issue_id,))
    try:
        response = api.get(
            sub_url(world, issue_id, "comments"), params={"expand": "issue"}
        )
        assert response.status_code == 200, response.text
        assert response.json()["results"][0]["issue"]["id"] == issue_id
    finally:
        db.execute("UPDATE issues SET deleted_at = NULL WHERE id = %s", (issue_id,))


def test_activity_order_per_page_precedence(api, world, seeder):
    """order_by validates eagerly at queryset build, before per_page parses."""
    issue_id = world["issue"]["id"]
    activity = seeder.create_issue_activity(
        world["workspace"]["id"], world["project"]["id"], issue_id, verb="created"
    )
    list_url = activities_url(world, issue_id)
    detail_url = list_url + f"{activity['id']}/"
    assert (
        api.get(list_url, params={"order_by": "bogus", "per_page": "abc"}).status_code
        == 500
    )
    assert api.get(list_url, params={"order_by": "bogus"}).status_code == 500
    assert api.get(list_url, params={"per_page": "abc"}).status_code == 400
    assert api.get(list_url, params={"order_by": "", "per_page": "abc"}).status_code == 500
    assert (
        api.get(detail_url, params={"order_by": "bogus", "per_page": "abc"}).status_code
        == 500
    )
    assert api.get(detail_url, params={"per_page": "abc"}).status_code == 200


@pytest.mark.parametrize("zone", ["../x", "", "a/b/../c"])
def test_activity_timezone_path_arms_500(api, world, db, zone):
    """Path-shaped timezones escape zoneinfo and 500 (ValueError)."""
    db.execute(
        "UPDATE users SET user_timezone = %s WHERE id = %s",
        (zone, world["owner"]["id"]),
    )
    try:
        response = api.get(activities_url(world, world["issue"]["id"]))
        assert response.status_code == 500, response.text
    finally:
        db.execute(
            "UPDATE users SET user_timezone = 'UTC' WHERE id = %s",
            (world["owner"]["id"],),
        )


def test_activity_timezone_unknown_name_400(api, world, db):
    """Unknown zone names 400 through the KeyError branch."""
    db.execute(
        "UPDATE users SET user_timezone = 'No/Such' WHERE id = %s",
        (world["owner"]["id"],),
    )
    try:
        response = api.get(activities_url(world, world["issue"]["id"]))
        assert response.status_code == 400, response.text
        assert response.json() == {"error": "The required key does not exist."}
    finally:
        db.execute(
            "UPDATE users SET user_timezone = 'UTC' WHERE id = %s",
            (world["owner"]["id"],),
        )


_1385_OLD_TS = "2026-01-01T00:00:00+00:00"
_1385_NEW_TS = "2026-06-01T00:00:00+00:00"


def test_attachment_post_external_duplicate_picks_newest(api, world, seeder, db):
    """The external-duplicate 409 names the NEWEST duplicate (PIDASHCONV-1385).

    Django answers the dup probe with ``.first()`` over
    ``Meta.ordering = ("-created_at",)``, so the 409 ``id`` is the newest
    matching ``ISSUE_ATTACHMENT`` row. The older seed goes in first with
    the smaller id (the PIDASHCONV-1277 arrangement): an unordered
    ``LIMIT 1`` returns the older row under every plan (heap order, pkey
    id order), so the pre-fix Rust backend fails here while Django and
    the fixed Rust backend pass.
    """
    issue_id = world["issue"]["id"]
    first, second = str(uuid.uuid4()), str(uuid.uuid4())
    older_id, newer_id = (first, second) if first < second else (second, first)
    external_id = f"dup-{seeder.tag}"
    external_source = "probe-1385"
    insert = (
        "INSERT INTO file_assets (id, created_at, updated_at, attributes,"
        " asset, user_id, workspace_id, project_id, issue_id, entity_type,"
        " is_deleted, is_archived, external_id, external_source, size,"
        " is_uploaded) VALUES (%s, %s, now(), '{}', %s, %s, %s, %s, %s,"
        " 'ISSUE_ATTACHMENT', false, false, %s, %s, 10, true)"
    )
    for asset_id, stamp, suffix in (
        (older_id, _1385_OLD_TS, "older"),
        (newer_id, _1385_NEW_TS, "newer"),
    ):
        db.execute(
            insert,
            (
                asset_id,
                stamp,
                f"{world['workspace']['id']}/{asset_id}-{suffix}.pdf",
                world["owner"]["id"],
                world["workspace"]["id"],
                world["project"]["id"],
                issue_id,
                external_id,
                external_source,
            ),
        )
        seeder.track.add("file_assets", asset_id)
    response = api.post(
        attachment_url(world, issue_id),
        json={
            "name": "p.pdf",
            "type": PDF,
            "size": 10,
            "external_id": external_id,
            "external_source": external_source,
        },
    )
    assert response.status_code == 409, response.text
    assert response.json() == {
        "error": "Issue with the same external id and external source already exists",
        "id": newer_id,
    }
