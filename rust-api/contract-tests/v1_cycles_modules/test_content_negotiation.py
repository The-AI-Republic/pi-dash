"""Contract tests: v1 content negotiation (PIDASHCONV-627, D-20).

Pins the live backend's request-body behavior on the cycle + module write
paths: 415 for unknown/missing content-type, form parsing (last-wins,
blank rules, members arrays), multipart (text fields, file collisions),
charset handling, and the empty-body rule. Every test runs against Django
and against the Rust server with identical expectations, except the
missing-content-type 415, where runserver (wsgiref) renders the echoed
type as ``text/plain`` while prod Django (uvicorn/ASGI) and Rust render
``""`` — that test asserts the status plus the message prefix/suffix.
"""

import socket
import urllib.parse
import uuid

import pytest

from _harness import api, db

SLUG = db.WS_A_SLUG
PROJ = db.PROJ_A_ID

CYCLES = api.cycles_url(SLUG, PROJ)
MODULES = api.modules_url(SLUG, PROJ)

CREATE_KEYS = {
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "start_date",
    "end_date",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "progress_snapshot",
    "archived_at",
    "logo_props",
    "timezone",
    "version",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "owned_by",
}

MODULE_CREATE_KEYS = {
    "id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "name",
    "description",
    "start_date",
    "target_date",
    "status",
    "lead",
    "project",
    "workspace",
    "external_source",
    "external_id",
}


def _name(prefix):
    return f"{prefix} {uuid.uuid4().hex[:8]}"


def _raw_post(admin_client, path, content_type_bytes, body):
    """POST with a verbatim Content-Type header (httpx cannot emit obs-text)."""
    parts = urllib.parse.urlparse(str(admin_client.base_url))
    sock = socket.create_connection((parts.hostname, parts.port or 80))
    try:
        request = (
            b"POST " + path.encode() + b" HTTP/1.1\r\n"
            b"Host: contract\r\n"
            b"X-Api-Key: " + db.ADMIN_TOKEN.encode() + b"\r\n"
            b"Content-Type: " + content_type_bytes + b"\r\n"
            b"Content-Length: " + str(len(body)).encode() + b"\r\n"
            b"Connection: close\r\n\r\n" + body
        )
        sock.sendall(request)
        response = b""
        while True:
            chunk = sock.recv(65536)
            if not chunk:
                break
            response += chunk
    finally:
        sock.close()
    head, _, response_body = response.partition(b"\r\n\r\n")
    status = int(head.split(b"\r\n", 1)[0].split(b" ")[1])
    return status, response_body


class TestUnsupportedMediaType:
    def test_text_plain_cycle_create_415_exact(self, admin_client):
        r = admin_client.post(
            CYCLES,
            content=b'{"name": "x"}',
            headers={"Content-Type": "text/plain"},
        )
        assert r.status_code == 415
        assert r.text == '{"detail":"Unsupported media type \\"text/plain\\" in request."}'

    def test_text_plain_echoes_full_header_verbatim(self, admin_client):
        ct = "Text/Plain; Charset=UTF-8"
        r = admin_client.post(
            CYCLES,
            content=b'{"name": "x"}',
            headers={"Content-Type": ct},
        )
        assert r.status_code == 415
        assert r.json() == {"detail": f'Unsupported media type "{ct}" in request.'}

    def test_415_on_module_create_patch_and_add(self, admin_client):
        body = b'{"name": "x"}'
        headers = {"Content-Type": "text/plain"}
        expect = {"detail": 'Unsupported media type "text/plain" in request.'}
        r = admin_client.post(MODULES, content=body, headers=headers)
        assert r.status_code == 415 and r.json() == expect
        detail = api.module_detail_url(SLUG, PROJ, db.MODULE_ACTIVE_ID)
        r = admin_client.patch(detail, content=body, headers=headers)
        assert r.status_code == 415 and r.json() == expect
        add = api.module_issues_url(SLUG, PROJ, db.MODULE_ACTIVE_ID)
        r = admin_client.post(add, content=body, headers=headers)
        assert r.status_code == 415 and r.json() == expect

    def test_415_on_cycle_patch_add_transfer(self, admin_client):
        body = b'{"name": "x"}'
        headers = {"Content-Type": "text/plain"}
        detail = api.cycle_detail_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID)
        r = admin_client.patch(detail, content=body, headers=headers)
        assert r.status_code == 415
        add = api.cycle_issues_url(SLUG, PROJ, db.CYCLE_ACTIVE_ID)
        r = admin_client.post(add, content=body, headers=headers)
        assert r.status_code == 415
        transfer = api.cycle_transfer_url(SLUG, PROJ, db.CYCLE_DRAFT_ID)
        r = admin_client.post(transfer, content=body, headers=headers)
        assert r.status_code == 415

    def test_missing_content_type_415(self, admin_client):
        # httpx sends no Content-Type for bare bytes; runserver renders the
        # echoed type as "text/plain" while prod Django and Rust render "".
        client = admin_client
        request = client.build_request("POST", CYCLES, content=b'{"name": "x"}')
        assert "content-type" not in request.headers
        r = client.send(request)
        assert r.status_code == 415
        text = r.text
        assert text.startswith('{"detail":"Unsupported media type \\"')
        assert text.endswith('\\" in request."}')

    def test_no_parse_paths_never_415(self, admin_client):
        headers = {"Content-Type": "text/plain"}
        r = admin_client.get(CYCLES, headers=headers)
        assert r.status_code == 200
        r = admin_client.get(api.modules_url(SLUG, PROJ), headers=headers)
        assert r.status_code == 200
        missing = api.cycle_detail_url(SLUG, PROJ, str(uuid.uuid4()))
        r = admin_client.delete(missing, headers=headers)
        assert r.status_code == 404

    def test_json_wildcards_match_json_parser(self, admin_client):
        for ct in ("*/*", "application/*"):
            r = admin_client.post(
                CYCLES,
                content=f'{{"name": "{_name("CT")}"}}'.encode(),
                headers={"Content-Type": ct},
            )
            assert r.status_code == 201, ct
            assert CREATE_KEYS <= set(r.json().keys())

    def test_obs_text_content_type_415_echo(self, admin_client):
        # Obs-text bytes surface as latin-1 chars and echo verbatim (the
        # body carries U+00E9 as UTF-8 on both backends).
        status, body = _raw_post(admin_client, CYCLES, b"text/pl\xe9in", b'{"name": "x"}')
        assert status == 415
        assert body == (
            b'{"detail":"Unsupported media type \\"text/pl\xc3\xa9in\\" in request."}'
        )


class TestFormBodies:
    def test_form_create_cycle_201(self, admin_client):
        name = _name("CT")
        r = admin_client.post(CYCLES, data={"name": name})
        assert r.status_code == 201
        body = r.json()
        assert CREATE_KEYS <= set(body.keys())
        assert body["name"] == name
        assert body["start_date"] is None and body["end_date"] is None

    def test_form_create_module_201(self, admin_client, db_conn):
        name = _name("CT")
        r = admin_client.post(MODULES, data={"name": name})
        assert r.status_code == 201
        body = r.json()
        assert MODULE_CREATE_KEYS <= set(body.keys())
        assert body["name"] == name

    def test_form_last_value_wins(self, admin_client):
        r = admin_client.post(
            CYCLES,
            content=b"name=first&name=second",
            headers={"Content-Type": "application/x-www-form-urlencoded"},
        )
        assert r.status_code == 201
        assert r.json()["name"] == "second"

    def test_form_unicode_round_trip(self, admin_client):
        name = "café \U0001f600"
        r = admin_client.post(CYCLES, data={"name": name})
        assert r.status_code == 201
        assert r.json()["name"] == name

    def test_form_cycle_dates_blank_none_timezone_skipped(self, admin_client):
        r = admin_client.post(
            CYCLES,
            data={"name": _name("CT"), "start_date": "", "end_date": "", "timezone": ""},
        )
        assert r.status_code == 201
        body = r.json()
        assert body["start_date"] is None and body["end_date"] is None
        assert body["timezone"] == "UTC"

    def test_form_module_dates_blank_none_status_skipped(self, admin_client):
        r = admin_client.post(
            MODULES,
            data={"name": _name("CT"), "start_date": "", "target_date": "", "status": ""},
        )
        assert r.status_code == 201
        body = r.json()
        assert body["start_date"] is None and body["target_date"] is None
        assert body["status"] == "planned"

    def test_form_patch_blank_status_skipped(self, admin_client):
        detail = api.module_detail_url(SLUG, PROJ, db.MODULE_COMPLETED_ID)
        before = admin_client.get(detail).json()["status"]
        r = admin_client.patch(detail, data={"status": ""})
        assert r.status_code == 200
        assert r.json()["status"] == before

    def test_form_members_single_becomes_array(self, admin_client, db_conn):
        name = _name("CT")
        r = admin_client.post(
            MODULES, data={"name": name, "members": db.MEMBER_ID}
        )
        assert r.status_code == 201
        module_id = r.json()["id"]
        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT member_id FROM module_members WHERE module_id = %s",
                [module_id],
            )
            rows = cur.fetchall()
        assert [str(row[0]) for row in rows] == [db.MEMBER_ID]

    def test_form_missing_members_201(self, admin_client):
        r = admin_client.post(MODULES, data={"name": _name("CT")})
        assert r.status_code == 201

    def test_form_invalid_name_still_400(self, admin_client):
        r = admin_client.post(CYCLES, data={})
        assert r.status_code == 400
        assert "name" in r.json()


class TestFormBlankDates:
    # `Field.get_value`: a present form `''` on an `allow_null` date field is
    # None — but the both-or-neither create gate still sees `''` as present.
    GATE_400 = {"error": "Both start date and end date are either required or are to be null"}

    def test_form_create_blank_plus_missing_gate_400(self, admin_client):
        r = admin_client.post(CYCLES, data={"name": _name("CT"), "start_date": ""})
        assert r.status_code == 400
        assert r.json() == self.GATE_400
        r = admin_client.post(CYCLES, data={"name": _name("CT"), "end_date": ""})
        assert r.status_code == 400
        assert r.json() == self.GATE_400

    def test_form_create_blank_plus_value_201_null(self, admin_client):
        r = admin_client.post(
            CYCLES,
            data={"name": _name("CT"), "start_date": "", "end_date": "2027-02-01T00:00:00Z"},
        )
        assert r.status_code == 201
        body = r.json()
        assert body["start_date"] is None and body["end_date"] is not None
        r = admin_client.post(
            CYCLES,
            data={"name": _name("CT"), "start_date": "2027-01-01T00:00:00Z", "end_date": ""},
        )
        assert r.status_code == 201
        body = r.json()
        assert body["start_date"] is not None and body["end_date"] is None

    def test_form_patch_blank_date_clears_cycle(self, admin_client):
        detail = api.cycle_detail_url(SLUG, PROJ, db.CYCLE_DRAFT_ID)
        r = admin_client.patch(
            detail,
            json={"start_date": "2027-01-01T00:00:00Z", "end_date": "2027-02-01T00:00:00Z"},
        )
        assert r.status_code == 200
        r = admin_client.patch(detail, data={"start_date": ""})
        assert r.status_code == 200
        body = r.json()
        assert body["start_date"] is None
        assert body["end_date"] is not None

    def test_form_patch_blank_date_clears_module(self, admin_client):
        detail = api.module_detail_url(SLUG, PROJ, db.MODULE_ACTIVE_ID)
        r = admin_client.patch(detail, json={"start_date": "2027-01-01", "target_date": "2027-02-01"})
        assert r.status_code == 200
        r = admin_client.patch(detail, data={"target_date": ""})
        assert r.status_code == 200
        body = r.json()
        assert body["target_date"] is None
        assert body["start_date"] == "2027-01-01"


class TestBlankRelational:
    # `RelatedField.run_validation` forces `''` to None on every path, so
    # form and JSON blanks behave alike: `owned_by`/`lead` default (cycle)
    # or null out, and `members` items fail the null check per index.
    NULL_ITEM = {"members": {"0": ["This field may not be null."]}}

    def test_form_blank_pk_becomes_none(self, admin_client):
        r = admin_client.post(MODULES, data={"name": _name("CT"), "lead": ""})
        assert r.status_code == 201
        assert r.json()["lead"] is None
        r = admin_client.post(CYCLES, data={"name": _name("CT"), "owned_by": ""})
        assert r.status_code == 201
        assert r.json()["owned_by"] == db.ADMIN_ID

    def test_json_blank_pk_same_as_form(self, admin_client):
        r = admin_client.post(CYCLES, json={"name": _name("CT"), "owned_by": ""})
        assert r.status_code == 201
        assert r.json()["owned_by"] == db.ADMIN_ID
        r = admin_client.post(MODULES, json={"name": _name("CT"), "lead": ""})
        assert r.status_code == 201
        assert r.json()["lead"] is None

    def test_members_empty_item_null_error_both_paths(self, admin_client):
        r = admin_client.post(MODULES, data={"name": _name("CT"), "members": ""})
        assert r.status_code == 400
        assert r.json() == self.NULL_ITEM
        r = admin_client.post(MODULES, json={"name": _name("CT"), "members": [""]})
        assert r.status_code == 400
        assert r.json() == self.NULL_ITEM


class TestIndexedMembers:
    # DRF `parse_html_list`: `members[N]` assembles the array when the exact
    # key is absent (exact-key `getlist` wins); error indexes are list
    # positions; dict-form renders the `MultiValueDict` echo.

    def test_form_indexed_members_sparse_201(self, admin_client):
        r = admin_client.post(
            MODULES,
            content=f"name={_name('CT')}&members%5B0%5D={db.MEMBER_ID}&members%5B2%5D={db.MEMBER_ID}".encode(),
            headers={"Content-Type": "application/x-www-form-urlencoded"},
        )
        assert r.status_code == 201
        assert r.json()["members"] == [db.MEMBER_ID]

    def test_form_indexed_exact_wins(self, admin_client):
        r = admin_client.post(
            MODULES,
            content=f"name={_name('CT')}&members={db.MEMBER_ID}&members%5B0%5D=not-a-uuid".encode(),
            headers={"Content-Type": "application/x-www-form-urlencoded"},
        )
        assert r.status_code == 201
        assert r.json()["members"] == [db.MEMBER_ID]

    def test_form_indexed_bad_uuid_position_zero(self, admin_client):
        r = admin_client.post(
            MODULES,
            content=b"name=idxbad&members%5B1%5D=not-a-uuid",
            headers={"Content-Type": "application/x-www-form-urlencoded"},
        )
        assert r.status_code == 400
        assert r.json() == {"members": {"0": ["\u201cnot-a-uuid\u201d is not a valid UUID."]}}

    def test_form_indexed_dict_form_mvd_echo(self, admin_client):
        r = admin_client.post(
            MODULES,
            content=b"name=idxdict&members%5B0%5Dx=1&members%5B0%5Dy=2",
            headers={"Content-Type": "application/x-www-form-urlencoded"},
        )
        assert r.status_code == 400
        assert r.json() == {
            "members": {
                "0": ["\u201c<MultiValueDict: {'x': ['1'], 'y': ['2']}>\u201d is not a valid UUID."]
            }
        }

    def test_multipart_indexed_file_members(self, admin_client):
        # File under `members[0]` echoes its filename at list position 0;
        # mixed text/file indexes keep index order (not texts-then-files).
        r = admin_client.post(
            MODULES,
            data={"name": _name("CT")},
            files={"members[0]": ("f.txt", b"hi", "text/plain")},
        )
        assert r.status_code == 400
        assert r.json() == {"members": {"0": ["\u201cf.txt\u201d is not a valid UUID."]}}
        r = admin_client.post(
            MODULES,
            data={"name": _name("CT"), "members[1]": "not-a-uuid"},
            files={"members[0]": ("i.txt", b"hi", "text/plain")},
        )
        assert r.status_code == 400
        assert r.json() == {
            "members": {
                "0": ["\u201ci.txt\u201d is not a valid UUID."],
                "1": ["\u201cnot-a-uuid\u201d is not a valid UUID."],
            }
        }


class TestMultipart:
    def test_multipart_texts_match_form(self, admin_client):
        # Filename-less parts are fields: this is multipart text, not a file.
        name = _name("CT")
        r = admin_client.post(
            MODULES, files={"name": (None, name), "description": (None, "mp")}
        )
        assert r.status_code == 201
        assert r.json()["name"] == name
        assert r.json()["description"] == "mp"

    def test_multipart_unknown_file_500_after_save(self, admin_client, db_conn):
        name = _name("CT")
        r = admin_client.post(
            CYCLES, data={"name": name}, files={"att": ("a.txt", b"hi")}
        )
        assert r.status_code == 500
        with db_conn.cursor() as cur:
            cur.execute("SELECT id FROM cycles WHERE name = %s", [name])
            assert cur.fetchone() is not None

    def test_multipart_char_file_400(self, admin_client):
        r = admin_client.post(
            CYCLES, files={"name": ("n.txt", b"x")}, data={"description": "d"}
        )
        assert r.status_code == 400
        assert r.json() == {"name": ["Not a valid string."]}

    def test_multipart_module_char_file_400(self, admin_client):
        r = admin_client.post(
            MODULES, files={"name": ("n.txt", b"x")}, data={"description": "d"}
        )
        assert r.status_code == 400
        assert r.json() == {"name": ["Not a valid string."]}

    def test_multipart_lead_file_400_echoes_filename(self, admin_client):
        r = admin_client.post(
            MODULES,
            data={"name": _name("CT")},
            files={"lead": ("not-a-uuid.txt", b"x")},
        )
        assert r.status_code == 400
        assert r.json() == {"lead": ["\u201cnot-a-uuid.txt\u201d is not a valid UUID."]}

    def test_multipart_members_text_plus_file(self, admin_client, db_conn):
        r = admin_client.post(
            MODULES,
            data={"name": _name("CT"), "members": db.MEMBER_ID},
            files={"members": ("m.bin", b"x")},
        )
        assert r.status_code == 400
        assert r.json() == {"members": {"1": ["\u201cm.bin\u201d is not a valid UUID."]}}

    def test_multipart_add_issues_file_500(self, admin_client):
        add = api.module_issues_url(SLUG, PROJ, db.MODULE_ACTIVE_ID)
        r = admin_client.post(add, files={"issues": ("i.bin", b"x")})
        assert r.status_code == 500

    def test_multipart_transfer_file_400(self, admin_client):
        transfer = api.cycle_transfer_url(SLUG, PROJ, db.CYCLE_DRAFT_ID)
        r = admin_client.post(transfer, files={"new_cycle_id": ("t.bin", b"x")})
        assert r.status_code == 400
        assert r.json() == {"error": "Please provide valid detail"}

    def test_multipart_mixed_error_order(self, admin_client):
        # Field errors follow declaration order even when a file error in a
        # later field mixes with a text error in an earlier one.
        r = admin_client.post(
            MODULES, data={"name": ""}, files={"target_date": ("t.bin", b"x")}
        )
        assert r.status_code == 400
        assert list(r.json().keys()) == ["name", "target_date"]

    def test_multipart_mixed_error_order_cycle(self, admin_client):
        r = admin_client.post(
            CYCLES, data={"name": ""}, files={"owned_by": ("o.bin", b"x")}
        )
        assert r.status_code == 400
        assert list(r.json().keys()) == ["name", "owned_by"]

    def test_multipart_missing_boundary_400(self, admin_client):
        r = admin_client.post(
            CYCLES,
            content=b"--b\r\n\r\n--b--\r\n",
            headers={"Content-Type": "multipart/form-data"},
        )
        assert r.status_code == 400
        assert r.json() == {"detail": "Multipart form parse error - Invalid boundary in multipart: None"}

    def test_multipart_non_ascii_filename_echo(self, admin_client):
        r = admin_client.post(
            MODULES,
            data={"name": _name("CT")},
            files={"lead": ("café.txt", b"hi", "text/plain")},
        )
        assert r.status_code == 400
        assert r.json() == {"lead": ["\u201ccaf\u00e9.txt\u201d is not a valid UUID."]}

    def test_multipart_space_before_colon_ignored(self, admin_client):
        # Django 4.2 compares the part header name verbatim, so a space
        # before the colon kills the match (the part is nameless data).
        r = admin_client.post(
            CYCLES,
            content=(
                b"--b\r\nContent-Disposition : form-data; name=\"name\"\r\n\r\nv\r\n--b--\r\n"
            ),
            headers={"Content-Type": "multipart/form-data; boundary=b"},
        )
        assert r.status_code == 400
        assert "name" in r.json()


class TestCharset:
    def test_explicit_utf8_charset_201(self, admin_client):
        r = admin_client.post(
            CYCLES,
            content=f'{{"name": "{_name("CT")}"}}'.encode(),
            headers={"Content-Type": "application/json; charset=utf-8"},
        )
        assert r.status_code == 201

    def test_bogus_charset_ignored(self, admin_client):
        r = admin_client.post(
            CYCLES,
            content=f'{{"name": "{_name("CT")}"}}'.encode(),
            headers={"Content-Type": "application/json; charset=bogus-charset"},
        )
        assert r.status_code == 201

    def test_latin1_charset_honored(self, admin_client):
        r = admin_client.post(
            CYCLES,
            content='{"name": "caf\xe9"}'.encode("latin-1"),
            headers={"Content-Type": "application/json; charset=iso-8859-1"},
        )
        assert r.status_code == 201
        assert r.json()["name"] == "café"

    def test_utf16_json_201(self, admin_client):
        name = _name("CT")
        r = admin_client.post(
            CYCLES,
            content=f'{{"name": "{name}"}}'.encode("utf-16"),
            headers={"Content-Type": "application/json; charset=utf-16"},
        )
        assert r.status_code == 201
        assert r.json()["name"] == name

    def test_form_latin1_fallback(self, admin_client):
        # Invalid utf-8 form bytes fall back to whole-body latin-1.
        r = admin_client.post(
            CYCLES,
            content=b"name=caf\xe9",
            headers={"Content-Type": "application/x-www-form-urlencoded"},
        )
        assert r.status_code == 201
        assert r.json()["name"] == "café"


class TestEmptyBodies:
    def test_empty_body_with_weird_ct_skips_parse(self, admin_client):
        # Content-Length 0 never parses and never 415s: the request runs
        # with {} and fails only on the missing name.
        r = admin_client.post(
            CYCLES, content=b"", headers={"Content-Type": "text/plain"}
        )
        assert r.status_code == 400
        assert "name" in r.json()

    def test_empty_body_missing_ct(self, admin_client):
        request = admin_client.build_request("POST", CYCLES, content=b"")
        assert "content-type" not in request.headers
        r = admin_client.send(request)
        assert r.status_code == 400
        assert "name" in r.json()


SERVER_ERROR = {"error": "Something went wrong please try again later"}


def _cycles_post(admin_client, content_type, body):
    return admin_client.post(CYCLES, content=body, headers={"Content-Type": content_type})


class TestCharsetExotic693:
    """PIDASHCONV-693: the full CPython codec table via `charset=`."""

    # -- JSON + single-byte -------------------------------------------

    def test_json_cp1252_ok(self, admin_client):
        body = '{"name": "caf\xe9"}'.encode("cp1252")
        r = _cycles_post(admin_client, "application/json; charset=cp1252", body)
        assert r.status_code == 201
        assert r.json()["name"] == "café"

    def test_json_cp1252_undef(self, admin_client):
        r = _cycles_post(
            admin_client, "application/json; charset=cp1252", b'{"name": "a\x81b"}'
        )
        assert r.status_code == 400
        assert r.json() == {
            "detail": "JSON parse error - 'charmap' codec can't decode byte 0x81 "
            "in position 11: character maps to <undefined>"
        }

    @pytest.mark.parametrize(
        "charset,text",
        [
            ("cp1251", "Привет"),
            ("iso-8859-2", "Zażółć"),
            ("cp437", "âêî"),
            ("koi8-r", "Привет"),
            ("mac-roman", "ﬁ"),
            ("tis-620", "ก"),
            ("charmap", "caf\xe9"),
            ("iso8859-1", "caf\xe9"),
        ],
    )
    def test_json_single_byte_ok(self, admin_client, charset, text):
        body = ('{"name": "%s"}' % text).encode(charset.replace("-", "_"))
        r = _cycles_post(admin_client, "application/json; charset=%s" % charset, body)
        assert r.status_code == 201
        assert r.json()["name"] == text

    # -- JSON + CJK ----------------------------------------------------

    @pytest.mark.parametrize(
        "charset,text",
        [
            ("shift_jis", "日本語"),
            ("cp932", "日本語"),
            ("shift-jis-2004", "日本語"),
            ("shift-jisx0213", "日本語"),
            ("euc-jp", "日本語"),
            ("euc-jis-2004", "日本語"),
            ("euc-jisx0213", "日本語"),
            ("euc-kr", "한국어"),
            ("cp949", "한국어"),
            ("johab", "한국어"),
            ("big5", "中文"),
            ("cp950", "中文"),
            ("big5hkscs", "中文"),
            ("gb2312", "中文"),
            ("gbk", "中文"),
            ("gb18030", "中文"),
        ],
    )
    def test_json_cjk_ok(self, admin_client, charset, text):
        body = ('{"name": "%s"}' % text).encode(charset)
        r = _cycles_post(admin_client, "application/json; charset=%s" % charset, body)
        assert r.status_code == 201
        assert r.json()["name"] == text

    def test_json_shift_jis_tail(self, admin_client):
        r = _cycles_post(
            admin_client, "application/json; charset=shift_jis", b'{"name": "a\x82"}'
        )
        assert r.status_code == 400
        assert r.json() == {
            "detail": "JSON parse error - 'shift_jis' codec can't decode byte 0x82 "
            "in position 11: illegal multibyte sequence"
        }

    def test_json_euc_jp_illegal(self, admin_client):
        r = _cycles_post(
            admin_client, "application/json; charset=euc-jp", b'{"name": "a\x80b"}'
        )
        assert r.status_code == 400
        assert "illegal multibyte sequence" in r.json()["detail"]

    def test_json_cp932_ff_single(self, admin_client):
        # 0xFF is a mapped single in cp932 (U+F8F3), so the body decodes
        # and fails only as JSON.
        r = _cycles_post(admin_client, "application/json; charset=cp932", b"\xff")
        assert r.status_code == 400
        assert r.json()["detail"].startswith("JSON parse error - Expecting value")

    def test_json_gb18030_4byte(self, admin_client):
        body = '{"name": "𐀀"}'.encode("gb18030")
        r = _cycles_post(admin_client, "application/json; charset=gb18030", body)
        assert r.status_code == 201
        assert r.json()["name"] == "𐀀"

    # -- JSON + hz / iso2022 / utf-7 / escapes --------------------------

    def test_json_hz_ok(self, admin_client):
        body = '{"name": "HZ693"}'.encode("hz")
        r = _cycles_post(admin_client, "application/json; charset=hz", body)
        assert r.status_code == 201
        assert r.json()["name"] == "HZ693"

    def test_json_hz_bad_escape(self, admin_client):
        r = _cycles_post(admin_client, "application/json; charset=hz", b'{"name": "a~xb"}')
        assert r.status_code == 400
        assert r.json() == {
            "detail": "JSON parse error - 'hz' codec can't decode byte 0x7e "
            "in position 11: illegal multibyte sequence"
        }

    @pytest.mark.parametrize("charset", ["iso-2022-jp", "iso-2022-jp-1", "iso-2022-jp-2",
                                         "iso-2022-jp-2004", "iso-2022-jp-3", "iso-2022-jp-ext",
                                         "iso-2022-kr"])
    def test_json_iso2022_ok(self, admin_client, charset):
        body = '{"name": "ISO693"}'.encode(charset)
        r = _cycles_post(admin_client, "application/json; charset=%s" % charset, body)
        assert r.status_code == 201
        assert r.json()["name"] == "ISO693"

    def test_json_iso2022_bad_designation(self, admin_client):
        r = _cycles_post(
            admin_client, "application/json; charset=iso-2022-jp", b'{"name": "a\x1b(Ib"}'
        )
        assert r.status_code == 400
        assert "illegal multibyte sequence" in r.json()["detail"]

    def test_json_utf7_ok(self, admin_client):
        body = '{"name": "U7693é"}'.encode("utf-7")
        r = _cycles_post(admin_client, "application/json; charset=utf-7", body)
        assert r.status_code == 201
        assert r.json()["name"] == "U7693é"

    def test_json_utf7_bare_plus(self, admin_client):
        r = _cycles_post(
            admin_client, "application/json; charset=utf-7", b'{"name": "a+b"}'
        )
        assert r.status_code == 400
        assert "partial character in shift sequence" in r.json()["detail"]

    def test_json_uesc_ok(self, admin_client):
        r = _cycles_post(
            admin_client, "application/json; charset=unicode-escape", b'{"name": "caf\\xe9"}'
        )
        assert r.status_code == 201
        assert r.json()["name"] == "café"

    def test_json_uesc_bad_hex(self, admin_client):
        r = _cycles_post(
            admin_client, "application/json; charset=unicode-escape", b'{"name": "a\\x4zb"}'
        )
        assert r.status_code == 400
        assert "truncated \\xXX escape" in r.json()["detail"]

    def test_json_resc_trailing_backslash_drops(self, admin_client):
        # A trailing backslash drops in stream mode, so the JSON parses
        # and only the serializer complains about the missing name.
        r = _cycles_post(
            admin_client, "application/json; charset=raw-unicode-escape", b'{"a": 1}\\'
        )
        assert r.status_code == 400
        assert r.json() == {"name": ["This field is required."]}

    # -- JSON + punycode / idna -----------------------------------------

    def test_json_punycode_ok(self, admin_client):
        body = '{"name": "PN693"}'.encode("punycode")
        r = _cycles_post(admin_client, "application/json; charset=punycode", body)
        assert r.status_code == 201
        assert r.json()["name"] == "PN693"

    def test_json_punycode_nonascii(self, admin_client):
        r = _cycles_post(admin_client, "application/json; charset=punycode", b"\xff")
        assert r.status_code == 400
        assert r.json() == {
            "detail": "JSON parse error - 'ascii' codec can't decode byte 0xff "
            "in position 0: ordinal not in range(128)"
        }

    def test_json_idna_ok(self, admin_client):
        r = _cycles_post(
            admin_client, "application/json; charset=idna", b'{"name": "IDNA693"}'
        )
        assert r.status_code == 201
        assert r.json()["name"] == "IDNA693"

    def test_json_idna_nonascii(self, admin_client):
        r = _cycles_post(admin_client, "application/json; charset=idna", b"\xff")
        assert r.status_code == 400
        assert r.json() == {
            "detail": "JSON parse error - 'ascii' codec can't decode byte 0xff "
            "in position 0: ordinal not in range(128)"
        }

    # -- JSON + bytes transforms ----------------------------------------

    def test_json_base64_ok(self, admin_client):
        import base64

        body = base64.encodebytes(b'{"name": "B64693"}')
        r = _cycles_post(admin_client, "application/json; charset=base64", body)
        assert r.status_code == 201
        assert r.json()["name"] == "B64693"

    def test_json_base64_ignores_garbage(self, admin_client):
        # Non-alphabet bytes vanish, so `!!!` decodes to empty and fails
        # only as JSON.
        r = _cycles_post(admin_client, "application/json; charset=base64", b"!!!")
        assert r.status_code == 400
        assert r.json()["detail"].startswith("JSON parse error - Expecting value")

    def test_json_base64_bad_padding(self, admin_client):
        r = _cycles_post(admin_client, "application/json; charset=base64", b"ab")
        assert r.status_code == 400
        assert r.json() == {"detail": "JSON parse error - Incorrect padding"}

    def test_json_hex_ok(self, admin_client):
        body = '{"name": "HEX693"}'.encode("utf-8").hex().encode("ascii")
        r = _cycles_post(admin_client, "application/json; charset=hex", body)
        assert r.status_code == 201
        assert r.json()["name"] == "HEX693"

    def test_json_hex_odd(self, admin_client):
        r = _cycles_post(admin_client, "application/json; charset=hex", b"abc")
        assert r.status_code == 400
        assert r.json() == {"detail": "JSON parse error - Odd-length string"}

    def test_json_quopri_ok(self, admin_client):
        import quopri

        body = quopri.encodestring(b'{"name": "QP693"}')
        r = _cycles_post(admin_client, "application/json; charset=quopri", body)
        assert r.status_code == 201
        assert r.json()["name"] == "QP693"

    def test_json_uu_ok(self, admin_client):
        import codecs

        body = codecs.encode(b'{"name": "UU693"}', "uu")
        r = _cycles_post(admin_client, "application/json; charset=uu", body)
        assert r.status_code == 201
        assert r.json()["name"] == "UU693"

    def test_json_uu_missing_begin(self, admin_client):
        r = _cycles_post(admin_client, "application/json; charset=uu", b"xxx")
        assert r.status_code == 400
        assert r.json() == {
            "detail": 'JSON parse error - Missing "begin" line in input data'
        }

    def test_json_zlib_ok(self, admin_client):
        import zlib

        body = zlib.compress(b'{"name": "ZL693"}')
        r = _cycles_post(admin_client, "application/json; charset=zlib", body)
        assert r.status_code == 201
        assert r.json()["name"] == "ZL693"

    def test_json_zlib_bad(self, admin_client):
        r = _cycles_post(admin_client, "application/json; charset=zlib", b"xxx")
        assert r.status_code == 500
        assert r.json() == SERVER_ERROR

    def test_json_bz2_ok(self, admin_client):
        import bz2

        body = bz2.compress(b'{"name": "BZ693"}')
        r = _cycles_post(admin_client, "application/json; charset=bz2", body)
        assert r.status_code == 201
        assert r.json()["name"] == "BZ693"

    def test_json_bz2_junk(self, admin_client):
        r = _cycles_post(admin_client, "application/json; charset=bz2", b"xxx")
        assert r.status_code == 500
        assert r.json() == SERVER_ERROR

    def test_json_bz2_truncated(self, admin_client):
        import bz2

        # A truncated stream is `ValueError`-class: a 400, not a 500.
        body = bz2.compress(b'{"name": "BZ693"}')[:-3]
        r = _cycles_post(admin_client, "application/json; charset=bz2", body)
        assert r.status_code == 400
        assert r.json() == {
            "detail": "JSON parse error - Compressed data ended before the "
            "end-of-stream marker was reached"
        }

    def test_json_rot13_is_500(self, admin_client):
        r = _cycles_post(
            admin_client, "application/json; charset=rot13", b'{"name": "ROT693"}'
        )
        assert r.status_code == 500
        assert r.json() == SERVER_ERROR

    def test_json_undefined_is_400(self, admin_client):
        r = _cycles_post(
            admin_client, "application/json; charset=undefined", b'{"name": "x"}'
        )
        assert r.status_code == 400
        assert r.json() == {"detail": "JSON parse error - undefined encoding"}

    # -- JSON + surrogates -----------------------------------------------

    def test_json_utf7_surrogate_400(self, admin_client):
        # utf-7 emits a lone surrogate; the CharField validator reports it.
        body = b'{"name": "+2AE-693"}'
        r = _cycles_post(admin_client, "application/json; charset=utf-7", body)
        assert r.status_code == 400
        assert r.json() == {
            "name": ["Surrogate characters are not allowed: U+D801."]
        }

    # -- form -------------------------------------------------------------

    def test_form_cp1252_pct(self, admin_client):
        r = _cycles_post(
            admin_client,
            "application/x-www-form-urlencoded; charset=cp1252",
            b"name=caf%E9",
        )
        assert r.status_code == 201
        assert r.json()["name"] == "café"

    def test_form_idna_ascii_ok(self, admin_client):
        # No dots or escapes: idna strict-decodes the ASCII body as-is.
        r = _cycles_post(
            admin_client,
            "application/x-www-form-urlencoded; charset=idna",
            b"name=FIDNA693",
        )
        assert r.status_code == 201
        assert r.json()["name"] == "FIDNA693"

    def test_form_idna_pct_is_500(self, admin_client):
        r = _cycles_post(
            admin_client,
            "application/x-www-form-urlencoded; charset=idna",
            b"a%20=b",
        )
        assert r.status_code == 500
        assert r.json() == SERVER_ERROR

    def test_form_punycode_is_500(self, admin_client):
        r = _cycles_post(
            admin_client,
            "application/x-www-form-urlencoded; charset=punycode",
            b"name=FPUN693",
        )
        assert r.status_code == 500
        assert r.json() == SERVER_ERROR

    def test_form_punycode_nonascii_falls_back(self, admin_client):
        r = _cycles_post(
            admin_client,
            "application/x-www-form-urlencoded; charset=punycode",
            b"name=\xff",
        )
        assert r.status_code == 201
        assert r.json()["name"] == "ÿ"

    @pytest.mark.parametrize("charset", ["base64", "hex", "quopri", "uu", "rot13", "zlib", "bz2"])
    def test_form_transform_is_500(self, admin_client, charset):
        r = _cycles_post(
            admin_client,
            "application/x-www-form-urlencoded; charset=%s" % charset,
            b"name=x",
        )
        assert r.status_code == 500
        assert r.json() == SERVER_ERROR

    def test_form_undefined_is_500(self, admin_client):
        r = _cycles_post(
            admin_client,
            "application/x-www-form-urlencoded; charset=undefined",
            b"name=x",
        )
        assert r.status_code == 500
        assert r.json() == SERVER_ERROR

    def test_form_sjis_tail_is_replacement(self, admin_client):
        r = _cycles_post(
            admin_client,
            "application/x-www-form-urlencoded; charset=shift_jis",
            b"name=%82",
        )
        assert r.status_code == 201
        assert r.json()["name"] == "\ufffd"

    def test_form_utf7_surrogate_400(self, admin_client):
        r = _cycles_post(
            admin_client,
            "application/x-www-form-urlencoded; charset=utf-7",
            b"name=%2B2AE%2D693",
        )
        assert r.status_code == 400
        assert r.json() == {
            "name": ["Surrogate characters are not allowed: U+D801."]
        }

    def test_form_utf7_raw_surrogate_400(self, admin_client):
        # Raw shift bytes in layer-1 (not percent-encoded): the surrogate
        # arises before `parse_qsl` and must still 400 at the CharField.
        r = _cycles_post(
            admin_client,
            "application/x-www-form-urlencoded; charset=utf-7",
            b"name=+2AE-693raw",
        )
        assert r.status_code == 400
        assert r.json() == {
            "name": ["Surrogate characters are not allowed: U+D801."]
        }

    # -- multipart ----------------------------------------------------------

    def test_mp_cp1252_ok(self, admin_client):
        boundary = "BOUND693A"
        body = (
            b"--" + boundary.encode() + b'\r\nContent-Disposition: form-data; name="name"\r\n\r\n'
            + "MP693 café".encode("cp1252") + b"\r\n--" + boundary.encode() + b"--\r\n"
        )
        r = _cycles_post(
            admin_client,
            "multipart/form-data; boundary=%s; charset=cp1252" % boundary,
            body,
        )
        assert r.status_code == 201
        assert r.json()["name"] == "MP693 café"

    @pytest.mark.parametrize(
        "charset", ["idna", "base64", "hex", "quopri", "uu", "zlib", "bz2", "undefined", "rot13"]
    )
    def test_mp_reject_is_500(self, admin_client, charset):
        boundary = "BOUND693B"
        body = (
            b"--" + boundary.encode() + b'\r\nContent-Disposition: form-data; name="name"\r\n\r\n'
            b"x\r\n--" + boundary.encode() + b"--\r\n"
        )
        r = _cycles_post(
            admin_client,
            "multipart/form-data; boundary=%s; charset=%s" % (boundary, charset),
            body,
        )
        assert r.status_code == 500
        assert r.json() == SERVER_ERROR

    def test_mp_punycode_mangles_field_name(self, admin_client):
        # punycode/replace maps `name` to control chars, so the field is
        # missing and only the serializer complains.
        boundary = "BOUND693C"
        body = (
            b"--" + boundary.encode() + b'\r\nContent-Disposition: form-data; name="name"\r\n\r\n'
            b"MPUN693\r\n--" + boundary.encode() + b"--\r\n"
        )
        r = _cycles_post(
            admin_client,
            "multipart/form-data; boundary=%s; charset=punycode" % boundary,
            body,
        )
        assert r.status_code == 400
        assert r.json() == {"name": ["This field is required."]}

    def test_mp_sjis_lone_lead_is_replacement(self, admin_client):
        boundary = "BOUND693D"
        body = (
            b"--" + boundary.encode() + b'\r\nContent-Disposition: form-data; name="name"\r\n\r\n'
            b"\x82\r\n--" + boundary.encode() + b"--\r\n"
        )
        r = _cycles_post(
            admin_client,
            "multipart/form-data; boundary=%s; charset=shift_jis" % boundary,
            body,
        )
        assert r.status_code == 201
        assert r.json()["name"] == "\ufffd"

    def test_mp_utf7_surrogate_400(self, admin_client):
        boundary = "BOUND693E"
        body = (
            b"--" + boundary.encode() + b'\r\nContent-Disposition: form-data; name="name"\r\n\r\n'
            b"+2AE-693\r\n--" + boundary.encode() + b"--\r\n"
        )
        r = _cycles_post(
            admin_client,
            "multipart/form-data; boundary=%s; charset=utf-7" % boundary,
            body,
        )
        assert r.status_code == 400
        assert r.json() == {
            "name": ["Surrogate characters are not allowed: U+D801."]
        }

    # -- normalization -------------------------------------------------------

    def test_charset_dotted_alias_honored(self, admin_client):
        # `iso.8859.1` normalizes to the dot->underscore alias (live).
        body = b"name=caf%E9"
        r = _cycles_post(
            admin_client,
            "application/x-www-form-urlencoded; charset=iso.8859.1",
            body,
        )
        assert r.status_code == 201
        assert r.json()["name"] == "café"

    def test_charset_dotted_module_rejected(self, admin_client):
        # `shift.jis` is neither an alias nor dotless: utf-8 applies and
        # the high byte fails JSON with the utf-8 text.
        r = _cycles_post(
            admin_client, "application/json; charset=shift.jis", b'{"name": "a\xe9"}'
        )
        assert r.status_code == 400
        assert r.json()["detail"].startswith("JSON parse error - 'utf-8' codec")

    @pytest.mark.parametrize("charset", ["ansi", "dbcs"])
    def test_charset_windows_alias_rejected(self, admin_client, charset):
        # `ansi`/`dbcs` alias to Windows-only `mbcs`: LookupError on
        # Linux, so utf-8 applies (review fix: the engine-less module
        # used to reach the dispatcher `todo!` and panic the request).
        r = _cycles_post(
            admin_client, "application/json; charset=%s" % charset, b'{"name": "a\xe9"}'
        )
        assert r.status_code == 400
        assert r.json()["detail"].startswith("JSON parse error - 'utf-8' codec")

    def test_charset_quoted_and_upper(self, admin_client):
        body = '{"name": "caf\xe9"}'.encode("cp1252")
        r = _cycles_post(admin_client, 'application/json; charset="CP1252"', body)
        assert r.status_code == 201
        assert r.json()["name"] == "café"

    # -- utf-8-sig ------------------------------------------------------------

    def test_json_u8sig_ok(self, admin_client):
        r = _cycles_post(
            admin_client, "application/json; charset=utf-8-sig", b'\xef\xbb\xbf{"name": "S693"}'
        )
        assert r.status_code == 201
        assert r.json()["name"] == "S693"

    def test_json_u8sig_tail_drops(self, admin_client):
        # BOM + trailing incomplete byte: both drop, so the empty text
        # fails in `json.loads` before the serializer ever runs.
        r = _cycles_post(
            admin_client, "application/json; charset=utf-8-sig", b"\xef\xbb\xbf\xe4"
        )
        assert r.status_code == 400
        assert r.json() == {
            "detail": "JSON parse error - Expecting value: line 1 column 1 (char 0)"
        }

    def test_json_u8sig_error_positions_post_bom(self, admin_client):
        r = _cycles_post(
            admin_client, "application/json; charset=utf-8-sig", b"\xef\xbb\xbf\xff"
        )
        assert r.status_code == 400
        assert r.json() == {
            "detail": "JSON parse error - 'utf-8' codec can't decode byte 0xff "
            "in position 0: invalid start byte"
        }

    def test_empty_body_with_exotic_charset(self, admin_client):
        # Content-Length 0 never decodes: the exotic charset is inert
        # and the request runs with {}.
        r = _cycles_post(
            admin_client, "application/json; charset=shift_jis", b""
        )
        assert r.status_code == 400
        assert r.json() == {"name": ["This field is required."]}
