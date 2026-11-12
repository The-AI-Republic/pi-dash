"""Malformed-JSON handling on the four project routes that read a body.

DRF's default parsers do not include a content-type override on these
views, so ``JSONParser`` handles ``application/json``: the first
``request.data`` access runs ``json.loads`` and a decode failure raises
``ParseError``, which ``handle_exception`` renders as
``400 {"detail": "JSON parse error - ..."}``.

A JSON body that parses to a non-object (list/scalar) is a different
failure: DRF returns the value as-is and the view calls ``.get`` on a
list/scalar — ``AttributeError`` → the generic 500 envelope. Both
backends must agree on all four routes (mint POST, PATCH, bulk POST,
duplicate POST).
"""

import uuid

from .test_duplicate import dup_path
from .test_project_assets import (
    bulk_path,
    proj_base,
    proj_detail,
    proj_payload,
)

SERVER_ERROR = {"error": "Something went wrong please try again later"}
# Truncated object: json.loads raises before any view logic runs.
MALFORMED = b"{oops"
JSON_HEADERS = {"Content-Type": "application/json"}


def _assert_parse_400(r):
    assert r.status_code == 400, r.text
    body = r.json()
    assert set(body.keys()) == {"detail"}, body
    assert body["detail"].startswith("JSON parse error - "), body["detail"]


def _send(org, method, path, payload, role="admin"):
    return org.request(method, path, role, content=payload, headers=JSON_HEADERS)


def _mint(org):
    r = org.request("POST", proj_base(org), "admin", json=proj_payload())
    assert r.status_code == 200, r.text
    return r.json()["asset_id"]


# --- malformed (truncated) JSON: DRF ParseError → 400 on both backends ---


def test_mint_malformed_json_400(org):
    _assert_parse_400(_send(org, "POST", proj_base(org), MALFORMED))


def test_patch_malformed_json_400(org):
    asset_id = _mint(org)
    _assert_parse_400(_send(org, "PATCH", proj_detail(org, asset_id), MALFORMED))


def test_bulk_malformed_json_400(org):
    _assert_parse_400(_send(org, "POST", bulk_path(org, str(uuid.uuid4())), MALFORMED))


def test_duplicate_malformed_json_400(org):
    _assert_parse_400(_send(org, "POST", dup_path(org, str(uuid.uuid4())), MALFORMED))


# --- non-object JSON (list): `.get` on a list → 500 on both backends ---


def test_mint_json_list_500(org):
    r = _send(org, "POST", proj_base(org), b"[1, 2, 3]")
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR


def test_patch_json_list_500(org):
    asset_id = _mint(org)
    r = _send(org, "PATCH", proj_detail(org, asset_id), b"[1, 2, 3]")
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR


def test_bulk_json_list_500(org):
    r = _send(org, "POST", bulk_path(org, str(uuid.uuid4())), b"[1, 2, 3]")
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR


def test_duplicate_json_list_500(org):
    r = _send(org, "POST", dup_path(org, str(uuid.uuid4())), b"[1, 2, 3]")
    assert r.status_code == 500, r.text
    assert r.json() == SERVER_ERROR
