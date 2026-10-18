"""Contract tests: v1_projects request content negotiation (PIDASHCONV-1149).

The project write views pass ``data={**request.data}`` (a plain ``dict``),
so a form/multipart body reaches the serializer as the raw QueryDict
storage — ``{key: [raw values]}``, every field a list — and therefore
always fails field validation with the serializer's own type errors
(``"Not a valid string."`` for the CharFields). A media type no DRF parser
accepts (``text/plain``, or a missing ``Content-Type`` with a body) 415s
before any serializer runs. Both endpoints (create POST, detail PATCH)
behave the same way.

Every test runs against Django and against the Rust server with identical
expectations, except the missing-``Content-Type`` 415 where runserver
(wsgiref) renders the echoed type as ``text/plain`` while prod Django
(uvicorn/ASGI) and Rust render ``""`` — that test asserts the status plus
the message prefix/suffix (same convention as PIDASHCONV-627).
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import http, db  # noqa: E402

KEY = "projects"

FORM_CREATE_BODY = "name=F&identifier=F1"
FORM_CREATE_ERRORS = {
    "name": ["Not a valid string."],
    "identifier": ["Not a valid string."],
}
FORM_PATCH_BODY = "description=x"
FORM_PATCH_ERRORS = {"description": ["Not a valid string."]}


def base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects"


def _form_headers():
    return {"Content-Type": "application/x-www-form-urlencoded"}


def test_urlencoded_create_matches_django(seed):
    # `{**request.data}` spreads the QueryDict, so name/identifier arrive as
    # one-element lists and the CharFields answer "Not a valid string." in
    # field order.
    with http.client(seed["keys"][KEY]) as c:
        r = c.post(base(seed) + "/", content=FORM_CREATE_BODY, headers=_form_headers())
    assert r.status_code == 400, r.text
    assert r.json() == FORM_CREATE_ERRORS


def test_multipart_create_matches_django(seed):
    with http.client(seed["keys"][KEY]) as c:
        r = c.post(
            base(seed) + "/",
            files={"name": (None, "F"), "identifier": (None, "F1")},
        )
    assert r.status_code == 400, r.text
    assert r.json() == FORM_CREATE_ERRORS


def test_urlencoded_patch_matches_django(seed):
    pid = seed["project"]["id"]
    with http.client(seed["keys"][KEY]) as c:
        r = c.patch(
            f"{base(seed)}/{pid}/",
            content=FORM_PATCH_BODY,
            headers=_form_headers(),
        )
    assert r.status_code == 400, r.text
    assert r.json() == FORM_PATCH_ERRORS


def test_multipart_patch_matches_django(seed):
    pid = seed["project"]["id"]
    with http.client(seed["keys"][KEY]) as c:
        r = c.patch(f"{base(seed)}/{pid}/", files={"description": (None, "x")})
    assert r.status_code == 400, r.text
    assert r.json() == FORM_PATCH_ERRORS


def test_text_plain_create_415_exact(seed):
    with http.client(seed["keys"][KEY]) as c:
        r = c.post(
            base(seed) + "/",
            content=b'{"name": "T", "identifier": "T1"}',
            headers={"Content-Type": "text/plain"},
        )
    assert r.status_code == 415
    assert r.text == '{"detail":"Unsupported media type \\"text/plain\\" in request."}'


def test_text_plain_patch_415_exact(seed):
    pid = seed["project"]["id"]
    with http.client(seed["keys"][KEY]) as c:
        r = c.patch(
            f"{base(seed)}/{pid}/",
            content=b'{"description": "x"}',
            headers={"Content-Type": "text/plain"},
        )
    assert r.status_code == 415
    assert r.text == '{"detail":"Unsupported media type \\"text/plain\\" in request."}'


def test_missing_content_type_415(seed):
    # httpx sends no Content-Type for bare bytes; runserver renders the
    # echoed type as "text/plain" while prod Django and Rust render "".
    with http.client(seed["keys"][KEY]) as c:
        request = c.build_request(
            "POST", base(seed) + "/", content=b'{"name": "T", "identifier": "T1"}'
        )
        assert "content-type" not in request.headers
        r = c.send(request)
    assert r.status_code == 415
    assert r.text.startswith('{"detail":"Unsupported media type \\"')
    assert r.text.endswith('\\" in request."}')


def test_json_create_still_works(seed, conn):
    # Same path, JSON content type: the negotiated JSON arm keeps the
    # existing decode + serializer flow.
    tag = db.new_tag()
    name = f"CT NegJson {tag}"
    ident = f"N{tag.upper()}"[:12]
    with http.client(seed["keys"][KEY]) as c:
        r = c.post(
            base(seed) + "/",
            content=('{"name": "%s", "identifier": "%s"}' % (name, ident)).encode(),
            headers={"Content-Type": "application/json"},
        )
    assert r.status_code == 201, r.text
    assert r.json()["name"] == name
