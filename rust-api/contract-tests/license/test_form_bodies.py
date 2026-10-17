# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""License form/multipart bodies match DRF's default parsers (PIDASHCONV-1117).

The five license write endpoints declare no ``parser_classes``, so Django
serves ``FormParser``/``MultiPartParser`` alongside JSON. These tests pin
that form/multipart bodies serve with identical field semantics on both
backends: last-wins, percent-decoding, HTML-form empty-string rules,
file handling, multipart framing, and the upload caps.

Deliberate pin limits (all probed, see the issue workpad):

* Upload caps (``TooManyFieldsSent`` / ``TooManyFilesSent`` /
  ``RequestDataTooBig``) escape every license view into the base
  ``handle_exception``: prod Django answers the generic 500 JSON on all
  five endpoints. PATCH endpoints pin that byte-exactly. POST over-cap
  bodies are NOT pinned here: under local ``DEBUG`` the Django Debug
  Toolbar re-parses the body in middleware after the view and replaces
  the 500 with a traceback-400 page, so no shared pin exists (prod,
  verified with ``DEBUG=False``, answers 500 like Rust). The decoder
  mapping for the POST endpoints is pinned by Rust unit tests instead.
* Every POST multipart framing error asserts status-only: the toolbar
  re-parse likewise replaces the JSON ``ParseError`` with the fixed
  400 page locally, while prod Django answers the JSON ``ParseError``
  exactly like Rust. PATCH framing errors pin byte-exact bodies.
* Member over-cap POST bodies are not pinned at all: local Django 400s
  via the toolbar while prod and Rust 403. Member PATCH over-cap pins
  the exact 403.
* Live SMTP sends are not pinned (console/locmem vs always-SMTP is a
  filed follow-up); only the receiver-required validation is covered.
* Malformed-JSON message text is a filed follow-up (serde vs CPython
  messages); untouched by this issue.
"""

import pytest

FORM = "application/x-www-form-urlencoded"
B = "formBoundary7"
MCT = f"multipart/form-data; boundary={B}"

SERVER_ERROR = {"error": "Something went wrong please try again later"}
EMAIL_REQUIRED = {"error": "Email is required"}
RECEIVER_REQUIRED = {"error": "Receiver email is required"}
ADMIN_403 = {"detail": "You do not have permission to perform this action."}
ANON_401 = {"detail": "Authentication credentials were not provided."}


def mp(*parts, boundary=B):
    body = b""
    for headers, payload in parts:
        body += b"--" + boundary.encode() + b"\r\n"
        for h in headers:
            body += h.encode() + b"\r\n"
        body += b"\r\n" + payload + b"\r\n"
    body += b"--" + boundary.encode() + b"--\r\n"
    return body


def field(name, value, cte=None):
    if isinstance(value, str):
        value = value.encode()
    headers = [f'Content-Disposition: form-data; name="{name}"']
    if cte is not None:
        headers.append(f"Content-Transfer-Encoding: {cte}")
    return (headers, value)


def filepart(name, filename, payload, ctype="text/plain", cte=None):
    if isinstance(payload, str):
        payload = payload.encode()
    headers = [
        f'Content-Disposition: form-data; name="{name}"; filename="{filename}"',
        f"Content-Type: {ctype}",
    ]
    if cte is not None:
        headers.append(f"Content-Transfer-Encoding: {cte}")
    return (headers, payload)


def _send(client, method, path, body, content_type):
    headers = {"Content-Type": content_type}
    if method == "PATCH":
        return client.patch(path, content=body, headers=headers)
    return client.post(path, content=body, headers=headers)


# ---------------------------------------------------------------------------
# PATCH /api/instances/


def test_instance_form_updates_name(admin_api):
    res = _send(admin_api, "PATCH", "/api/instances/", b"instance_name=Probe", FORM)
    assert res.status_code == 200
    assert res.json()["instance_name"] == "Probe"


def test_instance_form_last_wins(admin_api):
    res = _send(admin_api, "PATCH", "/api/instances/", b"instance_name=A&instance_name=B", FORM)
    assert res.status_code == 200
    assert res.json()["instance_name"] == "B"


def test_instance_form_decodes_plus_percent_and_semicolon(admin_api):
    res = _send(admin_api, "PATCH", "/api/instances/", b"instance_name=+%2B%3B%25", FORM)
    assert res.status_code == 200
    assert res.json()["instance_name"] == "+;%"
    res = _send(admin_api, "PATCH", "/api/instances/", b"instance_name=X;n=Y", FORM)
    assert res.status_code == 200
    assert res.json()["instance_name"] == "X;n=Y"


def test_instance_form_empty_name_is_blank(admin_api):
    res = _send(admin_api, "PATCH", "/api/instances/", b"instance_name=", FORM)
    assert res.status_code == 400
    assert res.json() == {"instance_name": ["This field may not be blank."]}


def test_instance_form_empty_skips_defaulted_fields(admin_api, world):
    res = _send(admin_api, "PATCH", "/api/instances/", b"edition=", FORM)
    assert res.status_code == 200
    assert res.json()["edition"] == "PI_DASH_COMMUNITY"
    res = _send(admin_api, "PATCH", "/api/instances/", b"is_verified=", FORM)
    assert res.status_code == 200
    assert res.json()["is_verified"] is False
    assert res.json()["instance_name"] == world["instance"]["name"]


def test_instance_form_empty_clears_nullable_fields(admin_api):
    res = _send(admin_api, "PATCH", "/api/instances/", b"deleted_at=", FORM)
    assert res.status_code == 200
    assert res.json()["deleted_at"] is None
    res = _send(admin_api, "PATCH", "/api/instances/", b"created_by=", FORM)
    assert res.status_code == 200
    assert res.json()["created_by"] is None


@pytest.mark.parametrize(
    "value,expected",
    [("yes", True), ("ON", True), ("True", True), ("1", True), ("0", False), ("off", False)],
)
def test_instance_form_boolean_spellings(admin_api, value, expected):
    res = _send(admin_api, "PATCH", "/api/instances/", f"is_verified={value}".encode(), FORM)
    assert res.status_code == 200
    assert res.json()["is_verified"] is expected


@pytest.mark.parametrize("value", ["2", "+1+", "maybe"])
def test_instance_form_boolean_rejects(admin_api, value):
    res = _send(admin_api, "PATCH", "/api/instances/", f"is_verified={value}".encode(), FORM)
    assert res.status_code == 400
    assert res.json() == {"is_verified": ["Must be a valid boolean."]}


def test_instance_form_bad_datetime(admin_api):
    res = _send(admin_api, "PATCH", "/api/instances/", b"deleted_at=notadate", FORM)
    assert res.status_code == 400
    assert res.json() == {
        "deleted_at": [
            "Datetime has wrong format. Use one of these formats instead: "
            "YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
        ]
    }


def test_instance_multipart_field_serves(admin_api):
    res = _send(
        admin_api, "PATCH", "/api/instances/", mp(field("instance_name", "MP")), MCT
    )
    assert res.status_code == 200
    assert res.json()["instance_name"] == "MP"


def test_instance_multipart_file_is_not_a_string(admin_api):
    res = _send(
        admin_api, "PATCH", "/api/instances/", mp(filepart("instance_name", "n.txt", "x")), MCT
    )
    assert res.status_code == 400
    assert res.json() == {"instance_name": ["Not a valid string."]}


def test_instance_multipart_file_pk_names_the_file(admin_api):
    res = _send(
        admin_api, "PATCH", "/api/instances/", mp(filepart("created_by", "c.txt", "x")), MCT
    )
    assert res.status_code == 400
    assert res.json() == {"created_by": ["\u201cc.txt\u201d is not a valid UUID."]}


def test_instance_multipart_file_datetime_is_invalid(admin_api):
    res = _send(
        admin_api, "PATCH", "/api/instances/", mp(filepart("deleted_at", "d.txt", "x")), MCT
    )
    assert res.status_code == 400
    assert res.json() == {
        "deleted_at": [
            "Datetime has wrong format. Use one of these formats instead: "
            "YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
        ]
    }


def test_instance_multipart_wrong_boundary_parses_whole_body(admin_api):
    res = _send(
        admin_api,
        "PATCH",
        "/api/instances/",
        mp(field("instance_name", "MP")),
        "multipart/form-data; boundary=WRONG",
    )
    assert res.status_code == 200
    assert res.json()["instance_name"] == "MP\r\n--formBoundary7--"


def test_instance_multipart_midline_delimiter_splits(admin_api):
    body = (
        b"--" + B.encode() + b'\r\nContent-Disposition: form-data; name="instance_name"\r\n\r\nv--'
        + B.encode() + b'q\r\nContent-Disposition: form-data; name="domain"\r\n\r\nw\r\n--'
        + B.encode() + b"--\r\n"
    )
    res = _send(admin_api, "PATCH", "/api/instances/", body, MCT)
    assert res.status_code == 200
    assert res.json()["instance_name"] == "v"
    assert res.json()["domain"] == "w"


def test_instance_multipart_preamble_field_parses_first(admin_api):
    body = (
        b'junk\r\nContent-Disposition: form-data; name="domain"\r\n\r\nPreD\r\n'
        + b"--" + B.encode() + b'\r\nContent-Disposition: form-data; name="instance_name"\r\n\r\nReal\r\n'
        + b"--" + B.encode() + b"--\r\n"
    )
    res = _send(admin_api, "PATCH", "/api/instances/", body, MCT)
    assert res.status_code == 200
    assert res.json()["instance_name"] == "Real"
    assert res.json()["domain"] == "PreD"


def test_instance_multipart_truncated_tail_applies_trimmed(admin_api):
    body = b"--" + B.encode() + b'\r\nContent-Disposition: form-data; name="instance_name"\r\n\r\nT\r\n\r\n'
    res = _send(admin_api, "PATCH", "/api/instances/", body, MCT)
    assert res.status_code == 200
    assert res.json()["instance_name"] == "T"


def test_instance_multipart_missing_boundary_400(admin_api):
    res = _send(
        admin_api, "PATCH", "/api/instances/", b"--B\r\n--B--\r\n", "multipart/form-data"
    )
    assert res.status_code == 400
    assert res.json() == {"detail": "Multipart form parse error - Invalid boundary in multipart: None"}


def test_instance_multipart_case_variant_rejected(admin_api):
    # DRF routes case-insensitively, but Django's parser `__init__`
    # checks `multipart/` case-sensitively: 400 with the verbatim type.
    res = _send(
        admin_api,
        "PATCH",
        "/api/instances/",
        mp(field("instance_name", "MP"), boundary="B2"),
        "Multipart/Form-Data; boundary=B2",
    )
    assert res.status_code == 400
    assert res.json() == {
        "detail": "Multipart form parse error - Invalid Content-Type: Multipart/Form-Data; boundary=B2"
    }


def test_instance_multipart_quoted_semicolon_boundary_serves(admin_api):
    res = _send(
        admin_api,
        "PATCH",
        "/api/instances/",
        mp(field("instance_name", "Semi"), boundary="a;b"),
        'multipart/form-data; boundary="a;b"',
    )
    assert res.status_code == 200
    assert res.json()["instance_name"] == "Semi"


def test_instance_multipart_base64_field_decodes(admin_api):
    res = _send(
        admin_api, "PATCH", "/api/instances/", mp(field("instance_name", "TXA=", cte="base64")), MCT
    )
    assert res.status_code == 200
    assert res.json()["instance_name"] == "Mp"


def test_instance_multipart_base64_field_bad_falls_back(admin_api):
    # Undecodable field base64 keeps the raw bytes (`b64decode` fallback).
    res = _send(
        admin_api, "PATCH", "/api/instances/", mp(field("instance_name", "abc", cte="base64")), MCT
    )
    assert res.status_code == 200
    assert res.json()["instance_name"] == "abc"


def test_instance_multipart_base64_field_all_pad_is_empty(admin_api):
    # All-padding base64 decodes to empty (`binascii` leniency), and the
    # empty name fails blank like a form `''`.
    res = _send(
        admin_api, "PATCH", "/api/instances/", mp(field("instance_name", "====", cte="base64")), MCT
    )
    assert res.status_code == 400
    assert res.json() == {"instance_name": ["This field may not be blank."]}


def test_instance_multipart_base64_file_undecodable_400(admin_api):
    res = _send(
        admin_api,
        "PATCH",
        "/api/instances/",
        mp(filepart("instance_name", "n.txt", "abc", cte="base64")),
        MCT,
    )
    assert res.status_code == 400
    assert res.json() == {"detail": "Multipart form parse error - Could not decode base64 data."}


def test_instance_form_1000_ok_1001_500(admin_api):
    ok = "&".join(f"u{i}=v" for i in range(1000)).encode()
    res = _send(admin_api, "PATCH", "/api/instances/", ok, FORM)
    assert res.status_code == 200
    over = "&".join(f"u{i}=v" for i in range(1001)).encode()
    res = _send(admin_api, "PATCH", "/api/instances/", over, FORM)
    assert res.status_code == 500
    assert res.json() == SERVER_ERROR


def test_instance_multipart_1000_ok_1001_500(admin_api):
    ok = (
        b"".join(
            b"--" + B.encode() + b'\r\nContent-Disposition: form-data; name="u%d"\r\n\r\nv\r\n' % i
            for i in range(1000)
        )
        + b"--" + B.encode() + b"--\r\n"
    )
    res = _send(admin_api, "PATCH", "/api/instances/", ok, MCT)
    assert res.status_code == 200
    over = (
        b"".join(
            b"--" + B.encode() + b'\r\nContent-Disposition: form-data; name="u%d"\r\n\r\nv\r\n' % i
            for i in range(1001)
        )
        + b"--" + B.encode() + b"--\r\n"
    )
    res = _send(admin_api, "PATCH", "/api/instances/", over, MCT)
    assert res.status_code == 500
    assert res.json() == SERVER_ERROR


def test_instance_multipart_101_files_500(admin_api):
    res = _send(
        admin_api,
        "PATCH",
        "/api/instances/",
        mp(*[filepart(f"f{i}", f"f{i}.txt", "x") for i in range(101)]),
        MCT,
    )
    assert res.status_code == 500
    assert res.json() == SERVER_ERROR


def test_instance_form_auth_order(member_api, anon_api):
    res = _send(member_api, "PATCH", "/api/instances/", b"\xff\xfe&", FORM)
    assert res.status_code == 403
    assert res.json() == ADMIN_403
    res = _send(anon_api, "PATCH", "/api/instances/", b"\xff\xfe&", FORM)
    assert res.status_code == 401
    assert res.json() == ANON_401
    over = "&".join(f"u{i}=v" for i in range(1001)).encode()
    res = _send(member_api, "PATCH", "/api/instances/", over, FORM)
    assert res.status_code == 403
    assert res.json() == ADMIN_403


# ---------------------------------------------------------------------------
# POST /api/instances/admins/


def test_admins_form_creates(admin_api, world):
    world["db"].make_user("newcomer@example.com")
    res = _send(admin_api, "POST", "/api/instances/admins/", b"email=newcomer@example.com", FORM)
    assert res.status_code == 201
    assert res.json()["user_detail"]["email"] == "newcomer@example.com"
    assert res.json()["role"] == 20


def test_admins_form_requires_email(admin_api):
    res = _send(admin_api, "POST", "/api/instances/admins/", b"email=", FORM)
    assert res.status_code == 400
    assert res.json() == EMAIL_REQUIRED


def test_admins_form_email_verbatim_lookup(admin_api):
    res = _send(admin_api, "POST", "/api/instances/admins/", b"email=+member%40example.com+", FORM)
    assert res.status_code == 404


def test_admins_form_duplicate_is_invalid_payload(admin_api):
    res = _send(admin_api, "POST", "/api/instances/admins/", b"email=admin@example.com", FORM)
    assert res.status_code == 400
    assert res.json() == {"error": "The payload is not valid"}


@pytest.mark.parametrize(
    "role,echo",
    [
        ("20", 20),
        ("+20+", " 20 "),
        ("%2B20", "+20"),
        ("1_0", "1_0"),
        ("%D9%A2%D9%A0", "٢٠"),
        ("%C2%A020", " 20"),
    ],
)
def test_admins_form_role_echoes_raw_unless_20(admin_api, role, echo):
    res = _send(
        admin_api, "POST", "/api/instances/admins/", f"email=member@example.com&role={role}".encode(), FORM
    )
    assert res.status_code == 201
    assert res.json()["role"] == echo


@pytest.mark.parametrize("role", ["1__2", "", "20.5", "0x10", "99999999999"])
def test_admins_form_role_unparsable_500(admin_api, role):
    res = _send(
        admin_api, "POST", "/api/instances/admins/", f"email=member@example.com&role={role}".encode(), FORM
    )
    assert res.status_code == 500
    assert res.json() == SERVER_ERROR


def test_admins_role_echoes_raw_json(admin_api, world):
    res = admin_api.post("/api/instances/admins/", json={"email": "member@example.com", "role": True})
    assert res.status_code == 201
    assert res.json()["role"] is True
    world["db"].make_user("floater@example.com")
    res = admin_api.post("/api/instances/admins/", json={"email": "floater@example.com", "role": 20.5})
    assert res.status_code == 201
    assert res.json()["role"] == 20.5


def test_admins_multipart_email_serves(admin_api):
    res = _send(
        admin_api, "POST", "/api/instances/admins/", mp(field("email", "member@example.com")), MCT
    )
    assert res.status_code == 201
    assert res.json()["user_detail"]["email"] == "member@example.com"


def test_admins_multipart_email_file_names_user(admin_api):
    res = _send(
        admin_api,
        "POST",
        "/api/instances/admins/",
        mp(filepart("email", "member@example.com", "x")),
        MCT,
    )
    assert res.status_code == 201
    assert res.json()["user_detail"]["email"] == "member@example.com"


def test_admins_multipart_role_file_500(admin_api):
    res = _send(
        admin_api,
        "POST",
        "/api/instances/admins/",
        mp(field("email", "member@example.com"), filepart("role", "20", "x")),
        MCT,
    )
    assert res.status_code == 500
    assert res.json() == SERVER_ERROR


def test_admins_form_role_leading_zeros_201(admin_api):
    # `int()` takes the value, not the width: 11 chars still fit `i32`.
    # The echo is the raw input (`str(value) != "20"`).
    res = _send(
        admin_api, "POST", "/api/instances/admins/", b"email=member@example.com&role=00000000001", FORM
    )
    assert res.status_code == 201
    assert res.json()["role"] == "00000000001"


def test_admins_form_role_control_char_500(admin_api):
    # `int()` never strips U+001C (unlike every other whitespace).
    res = _send(
        admin_api, "POST", "/api/instances/admins/", b"email=member@example.com&role=%1C20", FORM
    )
    assert res.status_code == 500
    assert res.json() == SERVER_ERROR


def test_admins_multipart_framing_400(admin_api):
    res = _send(admin_api, "POST", "/api/instances/admins/", b"--B\r\n--B--\r\n", "multipart/form-data")
    assert res.status_code == 400


def test_admins_multipart_auth_order(member_api, anon_api):
    res = _send(
        member_api, "POST", "/api/instances/admins/", mp(field("email", "x@y.zz")), MCT
    )
    assert res.status_code == 403
    assert res.json() == ADMIN_403
    res = _send(anon_api, "POST", "/api/instances/admins/", mp(field("email", "x@y.zz")), MCT)
    assert res.status_code == 401
    assert res.json() == ANON_401


# ---------------------------------------------------------------------------
# PATCH /api/instances/configurations/


def test_configs_form_updates_value(admin_api, world):
    world["db"].make_config("CONTRACT_PLAIN", "old", category="general")
    res = _send(admin_api, "PATCH", "/api/instances/configurations/", b"CONTRACT_PLAIN=new", FORM)
    assert res.status_code == 200
    (item,) = res.json()
    assert item["key"] == "CONTRACT_PLAIN"
    assert item["value"] == "new"


def test_configs_form_strips_whitespace(admin_api, world):
    world["db"].make_config("CONTRACT_PADDED", "old", category="general")
    res = _send(admin_api, "PATCH", "/api/instances/configurations/", b"CONTRACT_PADDED=+padded+", FORM)
    assert res.status_code == 200
    assert res.json()[0]["value"] == "padded"


def test_configs_form_unknown_keys_ignored(admin_api):
    res = _send(admin_api, "PATCH", "/api/instances/configurations/", b"NO_SUCH_KEY=x", FORM)
    assert res.status_code == 200
    assert res.json() == []


def test_configs_form_empty_is_empty_list(admin_api):
    res = _send(admin_api, "PATCH", "/api/instances/configurations/", b"", FORM)
    assert res.status_code == 200
    assert res.json() == []


def test_configs_multipart_field_serves(admin_api, world):
    world["db"].make_config("CONTRACT_PLAIN", "old", category="general")
    res = _send(
        admin_api, "PATCH", "/api/instances/configurations/", mp(field("CONTRACT_PLAIN", "mp")), MCT
    )
    assert res.status_code == 200
    assert res.json()[0]["value"] == "mp"


def test_configs_multipart_file_stores_filename(admin_api, world):
    world["db"].make_config("CONTRACT_PLAIN", "old", category="general")
    res = _send(
        admin_api,
        "PATCH",
        "/api/instances/configurations/",
        mp(filepart("CONTRACT_PLAIN", "h.txt", "x")),
        MCT,
    )
    assert res.status_code == 200
    assert res.json()[0]["value"] == "h.txt"


def test_configs_over_cap_500(admin_api):
    over = "&".join(f"u{i}=v" for i in range(1001)).encode()
    res = _send(admin_api, "PATCH", "/api/instances/configurations/", over, FORM)
    assert res.status_code == 500
    assert res.json() == SERVER_ERROR
    over_mp = (
        b"".join(
            b"--" + B.encode() + b'\r\nContent-Disposition: form-data; name="u%d"\r\n\r\nv\r\n' % i
            for i in range(1001)
        )
        + b"--" + B.encode() + b"--\r\n"
    )
    res = _send(admin_api, "PATCH", "/api/instances/configurations/", over_mp, MCT)
    assert res.status_code == 500
    assert res.json() == SERVER_ERROR
    res = _send(
        admin_api,
        "PATCH",
        "/api/instances/configurations/",
        mp(*[filepart(f"f{i}", f"f{i}.txt", "x") for i in range(101)]),
        MCT,
    )
    assert res.status_code == 500
    assert res.json() == SERVER_ERROR


def test_configs_multipart_missing_boundary_400(admin_api):
    res = _send(
        admin_api, "PATCH", "/api/instances/configurations/", b"--B\r\n--B--\r\n", "multipart/form-data"
    )
    assert res.status_code == 400
    assert res.json() == {"detail": "Multipart form parse error - Invalid boundary in multipart: None"}


# ---------------------------------------------------------------------------
# POST /api/instances/email-credentials-check/


def test_mailcheck_form_requires_receiver(admin_api):
    res = _send(admin_api, "POST", "/api/instances/email-credentials-check/", b"receiver_email=", FORM)
    assert res.status_code == 400
    assert res.json() == RECEIVER_REQUIRED


def test_mailcheck_multipart_requires_receiver(admin_api):
    res = _send(
        admin_api,
        "POST",
        "/api/instances/email-credentials-check/",
        mp(field("receiver_email", "")),
        MCT,
    )
    assert res.status_code == 400
    assert res.json() == RECEIVER_REQUIRED


# ---------------------------------------------------------------------------
# POST /api/instances/workspaces/


def test_workspaces_form_creates(admin_api):
    res = _send(
        admin_api,
        "POST",
        "/api/instances/workspaces/",
        b"name=N&slug=probe-space&background_color=%2300ff00",
        FORM,
    )
    assert res.status_code == 201
    body = res.json()
    assert body["name"] == "N"
    assert body["slug"] == "probe-space"
    assert body["background_color"] == "#00ff00"
    assert body["timezone"] == "UTC"


def test_workspaces_form_requires_name_and_slug(admin_api):
    res = _send(admin_api, "POST", "/api/instances/workspaces/", b"name=Only+Name", FORM)
    assert res.status_code == 400
    assert res.json() == {"error": "Both name and slug are required"}
    res = _send(admin_api, "POST", "/api/instances/workspaces/", b"name=&slug=s1", FORM)
    assert res.status_code == 400
    assert res.json() == {"error": "Both name and slug are required"}
    res = _send(admin_api, "POST", "/api/instances/workspaces/", b"name=N&slug=", FORM)
    assert res.status_code == 400
    assert res.json() == {"error": "Both name and slug are required"}


def test_workspaces_form_empty_rules(admin_api):
    res = _send(admin_api, "POST", "/api/instances/workspaces/", b"name=N&slug=s2&organization_size=", FORM)
    assert res.status_code == 201
    assert res.json()["organization_size"] == ""
    res = _send(admin_api, "POST", "/api/instances/workspaces/", b"name=N&slug=s3&timezone=", FORM)
    assert res.status_code == 201
    assert res.json()["timezone"] == "UTC"
    res = _send(
        admin_api, "POST", "/api/instances/workspaces/", b"name=N&slug=s5&background_color=", FORM
    )
    assert res.status_code == 201
    assert res.json()["background_color"].startswith("#")
    res = _send(admin_api, "POST", "/api/instances/workspaces/", b"name=N&slug=s6&logo=", FORM)
    assert res.status_code == 201
    assert res.json()["logo"] == ""
    res = _send(admin_api, "POST", "/api/instances/workspaces/", b"name=N&slug=s7&logo_asset=", FORM)
    assert res.status_code == 201
    assert res.json()["logo_asset"] is None
    res = _send(admin_api, "POST", "/api/instances/workspaces/", b"name=N&slug=s8&deleted_at=", FORM)
    assert res.status_code == 201
    assert res.json()["deleted_at"] is None


def test_workspaces_form_timezone_choice_verbatim(admin_api):
    res = _send(admin_api, "POST", "/api/instances/workspaces/", b"name=N&slug=s4&timezone=+UTC+", FORM)
    assert res.status_code == 400
    assert res.json() == {"timezone": ['" UTC " is not a valid choice.']}


def test_workspaces_form_restricted_slug(admin_api):
    res = _send(admin_api, "POST", "/api/instances/workspaces/", b"name=N&slug=api", FORM)
    assert res.status_code == 400
    assert res.json() == {"slug": ["Slug is not valid"]}


def test_workspaces_form_uppercase_slug_echoes(admin_api):
    res = _send(admin_api, "POST", "/api/instances/workspaces/", b"name=Acme+2&slug=ACME", FORM)
    assert res.status_code == 201
    assert res.json()["slug"] == "ACME"


def test_workspaces_multipart_creates(admin_api):
    res = _send(
        admin_api,
        "POST",
        "/api/instances/workspaces/",
        mp(field("name", "M"), field("slug", "mp-space")),
        MCT,
    )
    assert res.status_code == 201
    assert res.json()["slug"] == "mp-space"


def test_workspaces_multipart_logo_file_rejected(admin_api):
    res = _send(
        admin_api,
        "POST",
        "/api/instances/workspaces/",
        mp(field("name", "L"), field("slug", "s11"), filepart("logo", "lg.png", "PNG")),
        MCT,
    )
    assert res.status_code == 400
    assert res.json() == {"logo": ["Not a valid string."]}


def test_workspaces_multipart_asset_file_names_file(admin_api):
    res = _send(
        admin_api,
        "POST",
        "/api/instances/workspaces/",
        mp(field("name", "L"), field("slug", "s12"), filepart("logo_asset", "zz", "PNG")),
        MCT,
    )
    assert res.status_code == 400
    assert res.json() == {"logo_asset": ["\u201czz\u201d is not a valid UUID."]}


def test_workspaces_multipart_timezone_sees_trailing_breaks(admin_api):
    body = (
        b"--" + B.encode() + b'\r\nContent-Disposition: form-data; name="name"\r\n\r\nN\r\n'
        + b"--" + B.encode() + b'\r\nContent-Disposition: form-data; name="slug"\r\n\r\ns9\r\n'
        + b"--" + B.encode() + b'\r\nContent-Disposition: form-data; name="timezone"\r\n\r\nUTC\r\n\r\n'
    )
    res = _send(admin_api, "POST", "/api/instances/workspaces/", body, MCT)
    assert res.status_code == 400
    assert res.json() == {"timezone": ['"UTC\r\n\r\n" is not a valid choice.']}


def test_workspaces_multipart_name_trims_trailing_breaks(admin_api):
    body = (
        b"--" + B.encode() + b'\r\nContent-Disposition: form-data; name="name"\r\n\r\nN\r\n\r\n'
        + b"--" + B.encode() + b'\r\nContent-Disposition: form-data; name="slug"\r\n\r\ns10\r\n'
        + b"--" + B.encode() + b"--\r\n"
    )
    res = _send(admin_api, "POST", "/api/instances/workspaces/", body, MCT)
    assert res.status_code == 201
    assert res.json()["name"] == "N"


def test_workspaces_multipart_base64_file_sizes_decoded(admin_api):
    # `len(file)` is the DECODED size: 108 chars of `Q` decode to 81
    # bytes, tripping the view's 80-char name limit on both backends.
    res = _send(
        admin_api,
        "POST",
        "/api/instances/workspaces/",
        mp(
            field("slug", "s13"),
            filepart("name", "n.txt", "Q" * 108, cte="base64"),
        ),
        MCT,
    )
    assert res.status_code == 400
    assert res.json() == {"error": "The maximum length for name is 80 and for slug is 48"}
