"""Request-body decoding for the three cycle write routes (PIDASHCONV-1189).

``POST cycles/date-check/``, ``POST cycles/<id>/transfer-issues/`` and
``PATCH cycles/<id>/user-properties/`` declare no ``parser_classes``, so
DRF's defaults apply (``JSONParser``, ``FormParser``, ``MultiPartParser``):

* An empty body parses to ``{}`` without touching a parser.
* Malformed JSON is a ``ParseError`` -> 400 ``{"detail": "JSON parse
  error - <CPython message>"}``.
* ``application/x-www-form-urlencoded`` is parsed to a ``QueryDict`` and
  the view's ``request.data.get(...)`` reads it, so the view runs.
* A content type no default parser matches is 415.
* A valid non-object JSON value has no ``.get`` and raises
  ``AttributeError`` out of the view -> generic 500.

Rust previously ignored the content type and fed every non-object body
straight into ``serde_json``; this suite pins the Django behavior on both
backends.
"""

import pytest

from .conftest import create_cycle_for, cycle_urls

pytestmark = pytest.mark.contract

GENERIC_500 = {"error": "Something went wrong please try again later"}

# A truncated object; CPython's message is the same whatever the route.
MALFORMED_JSON = b'{"start_date":'
MALFORMED_DETAIL = (
    "JSON parse error - Expecting value: line 1 column 15 (char 14)"
)


def _routes(tenant):
    """(method, url) for each of the three write routes under test."""
    urls = cycle_urls(tenant)
    cid = tenant["cycle"]["id"]
    return {
        "date-check": ("POST", urls["date_check"]),
        "transfer-issues": ("POST", urls["transfer"](cid)),
        "user-properties": ("PATCH", urls["user_properties"](cid)),
    }


def _provision_user_properties(tenant):
    """PATCH looks the row up before reading ``request.data`` (a missing
    row is a 404, not a body error), so seed it with a GET first."""
    urls = cycle_urls(tenant)
    r = tenant["client"].get(urls["user_properties"](tenant["cycle"]["id"]))
    assert r.status_code == 200, r.text[:500]

def _send(client, method, url, body, content_type):
    headers = {"Content-Type": content_type}
    if method == "PATCH":
        return client.patch(url, content=body, headers=headers)
    return client.post(url, content=body, headers=headers)


def test_malformed_json_is_400_detail(admin):
    _provision_user_properties(admin)
    for label, (method, url) in _routes(admin).items():
        r = _send(admin["client"], method, url, MALFORMED_JSON, "application/json")
        assert r.status_code == 400, (label, r.text[:500])
        assert r.json() == {"detail": MALFORMED_DETAIL}, label


def test_empty_body_is_served(admin):
    urls = cycle_urls(admin)
    cid = admin["cycle"]["id"]

    r = _send(admin["client"], "POST", urls["date_check"], b"", "application/json")
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "Start date and end date both are required"}

    r = _send(admin["client"], "POST", urls["transfer"](cid), b"", "application/json")
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"error": "New Cycle Id is required"}

    # PATCH keeps every field, saves (bumping updated_at) and answers 201.
    got = admin["client"].get(urls["user_properties"](cid))
    assert got.status_code == 200, got.text[:500]
    before = got.json()
    r = _send(admin["client"], "PATCH", urls["user_properties"](cid), b"", "application/json")
    assert r.status_code == 201, r.text[:500]
    body = r.json()
    assert body["filters"] == before["filters"]
    assert body["display_filters"] == before["display_filters"]
    assert body["updated_at"] > before["updated_at"]


def test_urlencoded_body_is_served(admin):
    urls = cycle_urls(admin)
    cid = admin["cycle"]["id"]

    form = "application/x-www-form-urlencoded"

    r = _send(
        admin["client"],
        "POST",
        urls["date_check"],
        b"start_date=2026-03-01&end_date=2026-03-31",
        form,
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json() == {"status": True}

    target = create_cycle_for(admin, "Urlencoded target")
    r = _send(
        admin["client"],
        "POST",
        urls["transfer"](cid),
        f"new_cycle_id={target['id']}".encode(),
        form,
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json() == {"message": "Success"}

    admin["client"].get(urls["user_properties"](cid))
    r = _send(
        admin["client"], "PATCH", urls["user_properties"](cid), b"filters=x", form
    )
    assert r.status_code == 201, r.text[:500]
    assert r.json()["filters"] == "x"


def test_unsupported_media_type_is_415(admin):
    _provision_user_properties(admin)
    for label, (method, url) in _routes(admin).items():
        r = _send(admin["client"], method, url, b"x=1", "text/plain")
        assert r.status_code == 415, (label, r.text[:500])
        assert r.json() == {
            "detail": 'Unsupported media type "text/plain" in request.'
        }, label


def test_non_object_json_is_generic_500(admin):
    _provision_user_properties(admin)
    for label, (method, url) in _routes(admin).items():
        r = _send(admin["client"], method, url, b"[]", "application/json")
        assert r.status_code == 500, (label, r.text[:500])
        assert r.json() == GENERIC_500, label
