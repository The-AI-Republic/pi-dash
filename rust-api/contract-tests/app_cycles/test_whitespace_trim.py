"""Whitespace trimming for cycle write fields (NEWPIDASH-213).

Django's CycleWriteSerializer (ModelSerializer fields="__all__") builds
CharField(trim_whitespace=True) for every text column: "   " -> 400 blank,
" Sprint " -> stored "Sprint", max_length checked after strip.
This pins the same behaviour on the Rust port.
"""

import uuid

import pytest

from .conftest import cycle_urls

pytestmark = pytest.mark.contract


def _post_name(admin, name):
    urls = cycle_urls(admin)
    return admin["client"].post(urls["cycles"], json={"name": name})


def test_post_whitespace_only_name_rejected(admin):
    r = _post_name(admin, "   ")
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"name": ["This field may not be blank."]}, r.text[:500]


def test_post_padded_name_is_trimmed(admin):
    r = _post_name(admin, "  Sprint  ")
    assert r.status_code == 201, r.text[:500]
    assert r.json()["name"] == "Sprint", r.json()
    # DB row must also be trimmed
    cid = r.json()["id"]
    with admin["seed"].conn.cursor() as cur:
        cur.execute("SELECT name FROM cycles WHERE id=%s", (cid,))
        assert cur.fetchone()[0] == "Sprint"


def test_post_name_length_checked_after_strip(admin):
    # 253 x's + 3 spaces = 256 raw, 253 trimmed -> must pass (255 limit after strip)
    r = _post_name(admin, "x" * 253 + "   ")
    assert r.status_code == 201, r.text[:500]
    assert r.json()["name"] == "x" * 253

    # 256 x's -> trimmed still 256 -> 400 length
    r2 = _post_name(admin, "x" * 256)
    assert r2.status_code == 400, r2.text[:500]
    assert r2.json() == {"name": ["Ensure this field has no more than 255 characters."]}


def test_post_newline_whitespace_only_rejected(admin):
    r = _post_name(admin, "\n\t\r ")
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"name": ["This field may not be blank."]}


def test_post_name_with_embedded_tabs_trimmed(admin):
    r = _post_name(admin, "\tSprint\t")
    assert r.status_code == 201, r.text[:500]
    assert r.json()["name"] == "Sprint"


def test_patch_whitespace_only_name_rejected(admin):
    urls = cycle_urls(admin)
    pk = admin["cycle"]["id"]
    r = admin["client"].patch(urls["detail"](pk), json={"name": "   "})
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"name": ["This field may not be blank."]}


def test_patch_padded_name_is_trimmed(admin):
    urls = cycle_urls(admin)
    pk = admin["cycle"]["id"]
    r = admin["client"].patch(urls["detail"](pk), json={"name": "  Patched  "})
    assert r.status_code == 200, r.text[:500]
    assert r.json()["name"] == "Patched"
    with admin["seed"].conn.cursor() as cur:
        cur.execute("SELECT name FROM cycles WHERE id=%s", (pk,))
        assert cur.fetchone()[0] == "Patched"


def test_post_description_is_trimmed(admin):
    urls = cycle_urls(admin)
    r = admin["client"].post(
        urls["cycles"],
        json={"name": "WithDesc", "description": "  hello world  "},
    )
    assert r.status_code == 201, r.text[:500]
    assert r.json()["description"] == "hello world", r.json()
    cid = r.json()["id"]
    with admin["seed"].conn.cursor() as cur:
        cur.execute("SELECT description FROM cycles WHERE id=%s", (cid,))
        assert cur.fetchone()[0] == "hello world"


def test_post_description_whitespace_only_stored_trimmed(admin):
    urls = cycle_urls(admin)
    # TextField(blank=True): "   " trims to "" and is accepted, stored as ""
    r = admin["client"].post(
        urls["cycles"],
        json={"name": "WsDesc", "description": "   "},
    )
    assert r.status_code == 201, r.text[:500]
    assert r.json()["description"] == "", repr(r.json()["description"])


def test_post_external_fields_trimmed(admin):
    urls = cycle_urls(admin)
    r = admin["client"].post(
        urls["cycles"],
        json={
            "name": "ExtTrim",
            "external_source": "  jira  ",
            "external_id": "  EXT-1  ",
        },
    )
    assert r.status_code == 201, r.text[:500]
    body = r.json()
    assert body["external_source"] == "jira", body
    assert body["external_id"] == "EXT-1", body
    cid = body["id"]
    with admin["seed"].conn.cursor() as cur:
        cur.execute(
            "SELECT external_source, external_id FROM cycles WHERE id=%s", (cid,)
        )
        src, ext = cur.fetchone()
        assert src == "jira"
        assert ext == "EXT-1"


def test_post_external_source_length_checked_after_strip(admin):
    urls = cycle_urls(admin)
    # 253 + 3 spaces -> trimmed 253 passes; 256 -> fails
    r = admin["client"].post(
        urls["cycles"],
        json={"name": "ExtLen", "external_source": "y" * 253 + "   "},
    )
    assert r.status_code == 201, r.text[:500]
    assert r.json()["external_source"] == "y" * 253

    r2 = admin["client"].post(
        urls["cycles"],
        json={"name": "ExtLen2", "external_source": "y" * 256},
    )
    assert r2.status_code == 400, r2.text[:500]
    assert r2.json() == {"external_source": ["Ensure this field has no more than 255 characters."]}


def test_patch_external_fields_trimmed(admin):
    urls = cycle_urls(admin)
    pk = admin["cycle"]["id"]
    r = admin["client"].patch(
        urls["detail"](pk),
        json={"external_source": "  github  ", "external_id": "  abc  "},
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["external_source"] == "github"
    assert r.json()["external_id"] == "abc"


def test_post_timezone_trimmed_accepted(admin):
    urls = cycle_urls(admin)
    # Timezone with surrounding whitespace must be accepted after trim
    r = admin["client"].post(
        urls["cycles"],
        json={"name": "TZTrim", "timezone": "  UTC  "},
    )
    assert r.status_code == 201, r.text[:500]
    # The created row should have timezone UTC (trimmed)
    with admin["seed"].conn.cursor() as cur:
        cur.execute("SELECT timezone FROM cycles WHERE id=%s", (r.json()["id"],))
        assert cur.fetchone()[0] == "UTC"
