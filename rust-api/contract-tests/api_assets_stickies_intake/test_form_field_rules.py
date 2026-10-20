# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Sticky form bodies apply DRF's HTML-form field rules (PIDASHCONV-1213).

PIDASHCONV-1158 routed urlencoded/multipart sticky bodies into the sticky
serializer as a last-wins field map, but that only reproduces the
``FormParser``/``QueryDict`` *field map* — not the DRF serializer
``Field.get_value`` HTML-form rules (``rest_framework/fields.py``, gated on
``html.is_html_input``). On a form body:

* ``''`` on a nullable field arrives as ``None`` (clears it): ``deleted_at``
  (``allow_null``; ``blank=True`` gives a ``DateTimeField`` no
  ``allow_blank``) and the nullable audit FKs (``RelatedField``);
* ``''`` on a not-required field without ``allow_blank`` skips the field:
  ``sort_order`` (``FloatField(required=False)``, default kept);
* a ``JSONField`` value is marked ``is_json_string`` and ``json.loads``-ed,
  so a non-JSON string (empty included) is ``Value must be valid JSON.``
  and a valid JSON string parses into the stored value;
* the other fields allow blank, so ``''`` matches the JSON path.

Every case is probed against live Django by the same suite, so the
assertions are the shared contract, not a Rust-only expectation. The
JSON-path analogues (a ``RelatedField`` forcing ``""`` → ``None`` on JSON)
are out of scope here (PIDASHCONV-1127).
"""

import json
import urllib.parse

import pytest

FORM = "application/x-www-form-urlencoded"
STICKY_KEYS = {
    "background_color", "color", "created_at", "created_by", "deleted_at",
    "description", "description_binary", "description_html",
    "description_stripped", "id", "logo_props", "name", "owner",
    "sort_order", "updated_at", "updated_by", "workspace",
}


def _stickies(org):
    return f"/api/v1/workspaces/{org.workspace['slug']}/stickies/"


def _form(**fields):
    """Encode ordered ``key=value`` pairs the way an HTML form POST would.

    ``None`` values are skipped (absent field); everything else is
    percent-encoded (an empty string stays the bare ``key=``).
    """
    pairs = [(k, v) for k, v in fields.items() if v is not None]
    return urllib.parse.urlencode(pairs)


def _make_sticky(org, role="admin", **fields):
    payload = {"name": "seed", "description_html": "<p>seed</p>"}
    payload.update(fields)
    r = org.request("POST", _stickies(org), role, json=payload)
    assert r.status_code == 201, r.text[:500]
    return r.json()


def _patch_form(org, pk, body, role="admin"):
    return org.request(
        "PATCH",
        f"{_stickies(org)}{pk}/",
        role,
        content=body.encode(),
        headers={"Content-Type": FORM},
    )


def _post_form(org, body, role="admin"):
    return org.request(
        "POST",
        _stickies(org),
        role,
        content=body.encode(),
        headers={"Content-Type": FORM},
    )


# --- '' on allow_null fields clears ---------------------------------------


def test_create_form_blank_deleted_at_is_null(org):
    r = _post_form(org, _form(name="note", deleted_at=""))
    assert r.status_code == 201, r.text[:500]
    assert r.json()["deleted_at"] is None


def test_patch_form_blank_deleted_at_clears(org):
    created = _make_sticky(org)
    r = _patch_form(org, created["id"], _form(deleted_at=""))
    assert r.status_code == 200, r.text[:500]
    assert r.json()["deleted_at"] is None


def test_patch_form_blank_created_by_clears(org):
    # `BaseModel.save` stamps `created_by` from the request user on create,
    # so a form `created_by=` must clear it (not 400 an invalid UUID).
    created = _make_sticky(org)
    assert created["created_by"] == org.admin["id"]
    r = _patch_form(org, created["id"], _form(created_by=""))
    assert r.status_code == 200, r.text[:500]
    assert r.json()["created_by"] is None


def test_patch_form_blank_updated_by_stamps_actor(org):
    # `save()` overwrites `updated_by` with the request user: a form `''`
    # is accepted (None) and then replaced by the actor.
    created = _make_sticky(org)
    r = _patch_form(org, created["id"], _form(updated_by=""))
    assert r.status_code == 200, r.text[:500]
    assert r.json()["updated_by"] == org.admin["id"]


# --- '' on a not-required field without allow_blank skips -----------------


def test_patch_form_blank_sort_order_is_skipped(org):
    created = _make_sticky(org)
    before = created["sort_order"]
    r = _patch_form(org, created["id"], _form(sort_order=""))
    assert r.status_code == 200, r.text[:500]
    assert r.json()["sort_order"] == before


def test_create_form_blank_sort_order_is_skipped(org):
    r = _post_form(org, _form(name="note", sort_order="", description_html="<p>x</p>"))
    assert r.status_code == 201, r.text[:500]
    # Default kept (no earlier rows in this workspace).
    assert r.json()["sort_order"] == 65535


# --- JSONField values are json.loads-ed -----------------------------------


@pytest.mark.parametrize("field", ["description", "logo_props"])
@pytest.mark.parametrize("raw", ["foo", ""])
def test_form_json_field_invalid_is_400(org, field, raw):
    created = _make_sticky(org)
    r = _patch_form(org, created["id"], _form(**{field: raw}))
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {field: ["Value must be valid JSON."]}


def test_create_form_json_field_invalid_is_400(org):
    r = _post_form(org, _form(name="note", description="foo"))
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"description": ["Value must be valid JSON."]}


def test_form_json_field_valid_string_parses(org):
    created = _make_sticky(org)
    r = _patch_form(
        org,
        created["id"],
        _form(description=json.dumps({"blocks": []}), logo_props=json.dumps([1, 2])),
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert body["description"] == {"blocks": []}
    assert body["logo_props"] == [1, 2]


# --- non-divergent blank text fields keep JSON behavior -------------------


def test_create_form_blank_text_fields_are_empty_strings(org):
    r = _post_form(
        org,
        _form(
            name="",
            description_html="",
            color="",
            background_color="",
        ),
    )
    assert r.status_code == 201, r.text[:500]
    body = r.json()
    assert set(body) == STICKY_KEYS
    assert body["name"] == ""
    assert body["description_html"] == ""
    assert body["color"] == ""
    assert body["background_color"] == ""


def test_patch_form_blank_text_fields_are_empty_strings(org):
    created = _make_sticky(org, name="orig", color="#fff", background_color="#000")
    r = _patch_form(org, created["id"], _form(name="", color="", background_color=""))
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert body["name"] == ""
    assert body["color"] == ""
    assert body["background_color"] == ""
