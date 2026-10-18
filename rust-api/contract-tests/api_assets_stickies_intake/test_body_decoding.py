# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Sticky/intake body decoding matches DRF ``Request._parse`` (PIDASHCONV-1158).

``StickyViewSet`` and ``IntakeIssueAPIEndpoint`` declare no
``parser_classes``, so DRF's default parsers apply (``JSONParser``,
``FormParser``, ``MultiPartParser``). Two classes the Rust decoders used to
415 with ``Unsupported media type`` must instead serve like Django:

(a) form/multipart bodies: a urlencoded (or multipart) POST becomes a
    ``QueryDict``; sticky create serializes it (201) and intake create reads
    ``request.data.get("issue", {}).get("name", False)`` — so a form without
    an ``issue`` field is the 400 name-required and a string ``issue`` is the
    500 ``AttributeError``;
(b) the ``*/json``, ``*/*`` and ``application/*`` content types DRF's
    ``_MediaType.match`` resolves to ``JSONParser``, served byte-for-byte
    like ``application/json`` on sticky and intake create/update.

The controls the fix must not regress: ``text/plain`` with a body is still
415, and malformed JSON is still the 400 ``ParseError``.
"""

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


def _intake(org):
    return (
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/intake-issues/"
    )


def _make_sticky(org, role="admin", name="seed"):
    r = org.request(
        "POST",
        _stickies(org),
        role,
        json={"name": name, "description_html": "<p>seed</p>"},
    )
    assert r.status_code == 201, r.text[:500]
    return r.json()


def _make_intake_issue(org, role="admin", name="seed"):
    return org.create_intake_issue(role=role, name=name)


# --- (a) form bodies -------------------------------------------------------


def test_sticky_create_form_encoded(org):
    r = org.request(
        "POST",
        _stickies(org),
        "admin",
        content=b"name=note&description_html=<p>hi</p>",
        headers={"Content-Type": FORM},
    )
    assert r.status_code == 201, r.text[:500]
    body = r.json()
    assert set(body) == STICKY_KEYS
    assert body["name"] == "note"
    assert body["description_html"] == "<p>hi</p>"
    assert body["description_stripped"] == "hi"
    assert body["owner"] == org.admin["id"]


def test_sticky_create_form_last_value_wins(org):
    r = org.request(
        "POST",
        _stickies(org),
        "admin",
        content=b"name=first&name=second&description_html=<p>x</p>",
        headers={"Content-Type": FORM},
    )
    assert r.status_code == 201, r.text[:500]
    assert r.json()["name"] == "second"


def test_sticky_update_form_encoded(org):
    created = _make_sticky(org)
    r = org.request(
        "PATCH",
        f"{_stickies(org)}{created['id']}/",
        "admin",
        content=b"name=renamed",
        headers={"Content-Type": FORM},
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert body["name"] == "renamed"
    assert body["description_html"] == "<p>seed</p>"


def test_sticky_create_multipart(org):
    r = org.request(
        "POST",
        _stickies(org),
        "admin",
        files={
            "name": (None, "form note"),
            "description_html": (None, "<p>multi</p>"),
        },
    )
    assert r.status_code == 201, r.text[:500]
    body = r.json()
    assert body["name"] == "form note"
    assert body["description_html"] == "<p>multi</p>"


def test_intake_create_form_missing_issue_is_name_required(org):
    r = org.request(
        "POST",
        _intake(org),
        "admin",
        content=b"foo=bar",
        headers={"Content-Type": FORM},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "Name is required"}


def test_intake_create_form_issue_string_is_500(org):
    r = org.request(
        "POST",
        _intake(org),
        "admin",
        content=b"issue=abc",
        headers={"Content-Type": FORM},
    )
    assert r.status_code == 500, r.text[:500]


# --- (b) JSON wildcards ----------------------------------------------------

WILDCARDS = ["*/json", "*/*", "application/*"]


@pytest.mark.parametrize("content_type", WILDCARDS)
def test_sticky_create_wildcard_serves(org, content_type):
    r = org.request(
        "POST",
        _stickies(org),
        "admin",
        content=b'{"name":"wc","description_html":"<p>hi</p>"}',
        headers={"Content-Type": content_type},
    )
    assert r.status_code == 201, r.text[:500]
    body = r.json()
    assert set(body) == STICKY_KEYS
    assert body["name"] == "wc"


@pytest.mark.parametrize("content_type", WILDCARDS)
def test_sticky_update_wildcard_serves(org, content_type):
    created = _make_sticky(org)
    r = org.request(
        "PATCH",
        f"{_stickies(org)}{created['id']}/",
        "admin",
        content=b'{"name":"wc-renamed"}',
        headers={"Content-Type": content_type},
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["name"] == "wc-renamed"


@pytest.mark.parametrize("content_type", WILDCARDS)
def test_intake_create_wildcard_serves(org, content_type):
    r = org.request(
        "POST",
        _intake(org),
        "admin",
        content=b'{"issue":{"name":"wc item"}}',
        headers={"Content-Type": content_type},
    )
    assert r.status_code == 201, r.text[:500]
    body = r.json()
    assert body["issue_detail"]["name"] == "wc item"


@pytest.mark.parametrize("content_type", WILDCARDS)
def test_intake_update_wildcard_serves(org, content_type):
    created = _make_intake_issue(org)
    r = org.request(
        "PATCH",
        f"{_intake(org)}{created['issue']}/",
        "admin",
        content=b'{"issue":{"name":"wc patched"}}',
        headers={"Content-Type": content_type},
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["issue_detail"]["name"] == "wc patched"


# --- controls that must not regress ---------------------------------------


def test_sticky_create_text_plain_still_415(org):
    r = org.request(
        "POST",
        _stickies(org),
        "admin",
        content=b"hello",
        headers={"Content-Type": "text/plain"},
    )
    assert r.status_code == 415
    assert r.json() == {"detail": 'Unsupported media type "text/plain" in request.'}


def test_sticky_create_malformed_json_still_400(org):
    r = org.request(
        "POST",
        _stickies(org),
        "admin",
        content=b"{oops",
        headers={"Content-Type": "application/json"},
    )
    assert r.status_code == 400
    assert r.text.startswith('{"detail":"JSON parse error - ')
