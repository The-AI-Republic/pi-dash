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

import uuid

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


def _delete_module_members(db_conn, module_id):
    with db_conn.cursor() as cur:
        cur.execute("DELETE FROM module_members WHERE module_id = %s", [module_id])


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

    def test_form_cycle_dates_and_timezone_blank_skipped(self, admin_client):
        r = admin_client.post(
            CYCLES,
            data={"name": _name("CT"), "start_date": "", "end_date": "", "timezone": ""},
        )
        assert r.status_code == 201
        body = r.json()
        assert body["start_date"] is None and body["end_date"] is None
        assert body["timezone"] == "UTC"

    def test_form_module_dates_and_status_blank_skipped(self, admin_client):
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
        try:
            with db_conn.cursor() as cur:
                cur.execute(
                    "SELECT member_id FROM module_members WHERE module_id = %s",
                    [module_id],
                )
                rows = cur.fetchall()
            assert [str(row[0]) for row in rows] == [db.MEMBER_ID]
        finally:
            _delete_module_members(db_conn, module_id)

    def test_form_missing_members_201(self, admin_client):
        r = admin_client.post(MODULES, data={"name": _name("CT")})
        assert r.status_code == 201

    def test_form_invalid_name_still_400(self, admin_client):
        r = admin_client.post(CYCLES, data={})
        assert r.status_code == 400
        assert "name" in r.json()


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
