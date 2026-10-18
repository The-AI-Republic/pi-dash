# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Sticky form bodies obey DRF ``Field.get_value`` HTML-form rules
(PIDASHCONV-1213, follow-up to PIDASHCONV-1158).

``StickyViewSet`` declares no ``parser_classes``, so a urlencoded/multipart
body becomes a ``QueryDict``; the serializer then applies the HTML-form
``get_value`` rules (``rest_framework/fields.py:402-424``, gated on
``is_html_input``) instead of the JSON rules. On an ``''`` value:

* a nullable field without ``allow_blank`` arrives as ``None`` — ``deleted_at``
  and the audit FKs ``created_by``/``updated_by`` clear (they stay 200);
* a not-required field without ``allow_blank`` is the ``empty`` sentinel — the
  ``FloatField(required=False)`` ``sort_order`` is skipped and its default
  kept (not ``A valid number is required.``);
* a ``JSONField`` string is marked as a JSON string and ``json.loads``-ed
  (``fields.py:1758-1788``) — ``description=foo``/``description=`` are both
  ``Value must be valid JSON.`` and ``description={"a":1}`` stores the object.

The blank ``allow_blank`` text fields (``name``, ``description_html``,
``description_stripped``, ``color``, ``background_color``) pass ``''`` through
as ``""`` on both paths and are asserted as controls.
"""

FORM = "application/x-www-form-urlencoded"


def _stickies(org):
    return f"/api/v1/workspaces/{org.workspace['slug']}/stickies/"


def _make_sticky(org, role="admin", **overrides):
    payload = {"name": "seed", "description_html": "<p>seed</p>", **overrides}
    r = org.request("POST", _stickies(org), role, json=payload)
    assert r.status_code == 201, r.text[:500]
    return r.json()


def _form(org, method, path, body, role="admin"):
    return org.request(
        method,
        path,
        role,
        content=body,
        headers={"Content-Type": FORM},
    )


# --- '' clears a nullable field without allow_blank ------------------------


def test_form_empty_deleted_at_clears(org):
    created = _make_sticky(org)
    r = _form(org, "PATCH", f"{_stickies(org)}{created['id']}/", b"deleted_at=")
    assert r.status_code == 200, r.text[:500]
    assert r.json()["deleted_at"] is None


def test_form_empty_created_by_clears(org):
    created = _make_sticky(org)
    assert created["created_by"] == org.admin["id"]
    r = _form(org, "PATCH", f"{_stickies(org)}{created['id']}/", b"created_by=")
    assert r.status_code == 200, r.text[:500]
    assert r.json()["created_by"] is None


def test_form_empty_updated_by_is_accepted(org):
    created = _make_sticky(org)
    r = _form(org, "PATCH", f"{_stickies(org)}{created['id']}/", b"updated_by=")
    assert r.status_code == 200, r.text[:500]
    # The acting user overwrites the audit column on save.
    assert r.json()["updated_by"] == org.admin["id"]


# --- '' skips a not-required field without allow_blank ---------------------


def test_form_empty_sort_order_skips_and_keeps_default(org):
    created = _make_sticky(org)
    assert created["sort_order"] == 65535.0
    r = _form(org, "PATCH", f"{_stickies(org)}{created['id']}/", b"sort_order=")
    assert r.status_code == 200, r.text[:500]
    assert r.json()["sort_order"] == 65535.0


def test_form_empty_sort_order_on_create_keeps_default(org):
    r = _form(org, "POST", _stickies(org), b"name=note&sort_order=")
    assert r.status_code == 201, r.text[:500]
    assert r.json()["sort_order"] == 65535.0


# --- JSONField form values are json.loads-ed -------------------------------


def test_form_description_bad_json_is_a_field_error(org):
    created = _make_sticky(org)
    r = _form(org, "PATCH", f"{_stickies(org)}{created['id']}/", b"description=foo")
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"description": ["Value must be valid JSON."]}


def test_form_description_empty_is_a_field_error(org):
    created = _make_sticky(org)
    r = _form(org, "PATCH", f"{_stickies(org)}{created['id']}/", b"description=")
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"description": ["Value must be valid JSON."]}


def test_form_logo_props_bad_json_is_a_field_error_on_create(org):
    r = _form(org, "POST", _stickies(org), b"name=note&logo_props=foo")
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"logo_props": ["Value must be valid JSON."]}


def test_form_description_valid_json_is_stored(org):
    created = _make_sticky(org)
    r = _form(
        org,
        "PATCH",
        f"{_stickies(org)}{created['id']}/",
        b'description={"a":1}',
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["description"] == {"a": 1}


# --- controls: the allow_blank text fields do not diverge ------------------


def test_form_blank_allow_blank_fields_are_empty_strings(org):
    created = _make_sticky(org)
    r = _form(
        org,
        "PATCH",
        f"{_stickies(org)}{created['id']}/",
        b"name=&color=&background_color=&description_html=",
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert body["name"] == ""
    assert body["color"] == ""
    assert body["background_color"] == ""
    assert body["description_html"] == ""
