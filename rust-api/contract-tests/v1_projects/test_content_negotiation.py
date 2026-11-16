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


# ---------------------------------------------------------------------------
# PIDASHCONV-1391: member, invitation and state writes.
#
# Unlike the project views (plain-dict ``{**request.data}`` spread), these
# views pass ``request.data`` itself to the serializer, so a form/multipart
# body arrives with DRF HTML-input semantics: last value wins per key, a
# blank value on a non-required field without ``allow_blank`` behaves as
# absent, and a blank ``member`` fails like null (``RelatedField``
# forces ``''`` to ``None``). A media type no parser accepts 415s with the
# DRF ``{"detail": ...}`` body (lowercase D, verified live — the base
# ``handle_exception`` does not remap it). Empty bodies are ``{}``.
# ---------------------------------------------------------------------------

MKEY = "members"
IKEY = "invites"
SKEY = "states"

TEXT_415 = '{"detail":"Unsupported media type \\"text/plain\\" in request."}'
CT_415_PREFIX = '{"detail":"Unsupported media type \\"'
CT_415_SUFFIX = '\\" in request."}'


def _members_base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects/{seed['project']['id']}/members"


def _invites_base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/invitations"


def _states_base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects/{seed['project']['id']}/states"


def _count(conn, table, column, value):
    return db.fetch_one(
        conn, f"SELECT COUNT(*) AS c FROM {table} WHERE {column} = %s", (value,)
    )["c"]


def _fresh_candidate(conn, seed, tag):
    user = db.create_user(conn, tag, first_name="CtNeg")
    db.add_workspace_member(conn, seed["ws_a"]["id"], user["id"], db.MEMBER)
    return user


def _own_membership(conn, seed, tag):
    user = _fresh_candidate(conn, seed, tag)
    db.add_project_member(
        conn, seed["ws_a"]["id"], seed["project"]["id"], user["id"], db.ADMIN
    )
    row = db.fetch_one(
        conn,
        "SELECT id FROM project_members WHERE project_id = %s AND member_id = %s",
        (seed["project"]["id"], user["id"]),
    )
    return str(row["id"])


def _send_no_content_type(c, method, path, body):
    request = c.build_request(method, path, content=body)
    assert "content-type" not in request.headers
    return c.send(request)


# --- member POST ---


def test_member_create_text_plain_415(seed, conn):
    tag = db.new_tag()
    user = _fresh_candidate(conn, seed, tag)
    before = _count(conn, "project_members", "project_id", seed["project"]["id"])
    with http.client(seed["keys"][MKEY]) as c:
        r = c.post(
            _members_base(seed) + "/",
            content=('{"member": "%s", "role": 15}' % user["id"]).encode(),
            headers={"Content-Type": "text/plain"},
        )
    assert r.status_code == 415, r.text
    assert r.text == TEXT_415
    assert _count(conn, "project_members", "project_id", seed["project"]["id"]) == before


def test_member_create_missing_content_type_415(seed, conn):
    tag = db.new_tag()
    user = _fresh_candidate(conn, seed, tag)
    before = _count(conn, "project_members", "project_id", seed["project"]["id"])
    with http.client(seed["keys"][MKEY]) as c:
        r = _send_no_content_type(
            c,
            "POST",
            _members_base(seed) + "/",
            ('{"member": "%s", "role": 15}' % user["id"]).encode(),
        )
    assert r.status_code == 415, r.text
    assert r.text.startswith(CT_415_PREFIX)
    assert r.text.endswith(CT_415_SUFFIX)
    assert _count(conn, "project_members", "project_id", seed["project"]["id"]) == before


def test_member_create_urlencoded_json_string_400(seed, conn):
    tag = db.new_tag()
    user = _fresh_candidate(conn, seed, tag)
    before = _count(conn, "project_members", "project_id", seed["project"]["id"])
    with http.client(seed["keys"][MKEY]) as c:
        r = c.post(
            _members_base(seed) + "/",
            content=('{"member": "%s", "role": 15}' % user["id"]).encode(),
            headers=_form_headers(),
        )
    assert r.status_code == 400, r.text
    assert r.json() == {"member": ["This field is required."]}
    assert _count(conn, "project_members", "project_id", seed["project"]["id"]) == before


def test_member_create_form_creates(seed, conn):
    tag = db.new_tag()
    user = _fresh_candidate(conn, seed, tag)
    before = _count(conn, "project_members", "project_id", seed["project"]["id"])
    with http.client(seed["keys"][MKEY]) as c:
        r = c.post(
            _members_base(seed) + "/",
            content=("member=%s&role=15" % user["id"]).encode(),
            headers=_form_headers(),
        )
    assert r.status_code == 201, r.text
    assert r.json()["member"] == str(user["id"])
    assert r.json()["role"] == 15
    assert _count(conn, "project_members", "project_id", seed["project"]["id"]) == before + 1


def test_member_create_multipart_creates(seed, conn):
    tag = db.new_tag()
    user = _fresh_candidate(conn, seed, tag)
    before = _count(conn, "project_members", "project_id", seed["project"]["id"])
    with http.client(seed["keys"][MKEY]) as c:
        r = c.post(
            _members_base(seed) + "/",
            files={"member": (None, str(user["id"])), "role": (None, "15")},
        )
    assert r.status_code == 201, r.text
    assert r.json()["member"] == str(user["id"])
    assert r.json()["role"] == 15
    assert _count(conn, "project_members", "project_id", seed["project"]["id"]) == before + 1


def test_member_create_blank_role_skipped(seed, conn):
    # ``role=`` on a form behaves as absent: the model default (5) applies.
    tag = db.new_tag()
    user = _fresh_candidate(conn, seed, tag)
    with http.client(seed["keys"][MKEY]) as c:
        r = c.post(
            _members_base(seed) + "/",
            content=("member=%s&role=" % user["id"]).encode(),
            headers=_form_headers(),
        )
    assert r.status_code == 201, r.text
    assert r.json()["role"] == 5


def test_member_create_blank_member_is_null(seed, conn):
    # ``RelatedField.run_validation`` forces ``''`` to ``None``.
    with http.client(seed["keys"][MKEY]) as c:
        r = c.post(
            _members_base(seed) + "/",
            content=b"member=&role=15",
            headers=_form_headers(),
        )
    assert r.status_code == 400, r.text
    assert r.json() == {"member": ["This field may not be null."]}


# --- member PATCH ---


def test_member_patch_text_plain_415(seed, conn):
    mid = _own_membership(conn, seed, db.new_tag())
    with http.client(seed["keys"][MKEY]) as c:
        r = c.patch(
            f"{_members_base(seed)}/{mid}/",
            content=b'{"role": 5}',
            headers={"Content-Type": "text/plain"},
        )
    assert r.status_code == 415, r.text
    assert r.text == TEXT_415
    row = db.fetch_one(conn, "SELECT role FROM project_members WHERE id = %s", (mid,))
    assert row["role"] == db.ADMIN


def test_member_patch_missing_content_type_415(seed, conn):
    mid = _own_membership(conn, seed, db.new_tag())
    with http.client(seed["keys"][MKEY]) as c:
        r = _send_no_content_type(c, "PATCH", f"{_members_base(seed)}/{mid}/", b'{"role": 5}')
    assert r.status_code == 415, r.text
    assert r.text.startswith(CT_415_PREFIX)
    assert r.text.endswith(CT_415_SUFFIX)
    row = db.fetch_one(conn, "SELECT role FROM project_members WHERE id = %s", (mid,))
    assert row["role"] == db.ADMIN


def test_member_patch_urlencoded_json_string_noop(seed, conn):
    mid = _own_membership(conn, seed, db.new_tag())
    with http.client(seed["keys"][MKEY]) as c:
        r = c.patch(
            f"{_members_base(seed)}/{mid}/",
            content=b'{"role": 5}',
            headers=_form_headers(),
        )
    assert r.status_code == 200, r.text
    assert r.json()["role"] == db.ADMIN


def test_member_patch_form_applies(seed, conn):
    mid = _own_membership(conn, seed, db.new_tag())
    with http.client(seed["keys"][MKEY]) as c:
        r = c.patch(
            f"{_members_base(seed)}/{mid}/", content=b"role=5", headers=_form_headers()
        )
    assert r.status_code == 200, r.text
    assert r.json()["role"] == 5


def test_member_patch_multipart_applies(seed, conn):
    mid = _own_membership(conn, seed, db.new_tag())
    with http.client(seed["keys"][MKEY]) as c:
        r = c.patch(f"{_members_base(seed)}/{mid}/", files={"role": (None, "5")})
    assert r.status_code == 200, r.text
    assert r.json()["role"] == 5


# --- invitation POST ---


def test_invite_create_text_plain_415(seed, conn):
    tag = db.new_tag()
    before = _count(conn, "workspace_member_invites", "workspace_id", seed["ws_a"]["id"])
    with http.client(seed["keys"][IKEY]) as c:
        r = c.post(
            _invites_base(seed) + "/",
            content=('{"email": "%s@neg.test", "role": 15}' % tag).encode(),
            headers={"Content-Type": "text/plain"},
        )
    assert r.status_code == 415, r.text
    assert r.text == TEXT_415
    assert _count(conn, "workspace_member_invites", "workspace_id", seed["ws_a"]["id"]) == before


def test_invite_create_missing_content_type_415(seed, conn):
    tag = db.new_tag()
    before = _count(conn, "workspace_member_invites", "workspace_id", seed["ws_a"]["id"])
    with http.client(seed["keys"][IKEY]) as c:
        r = _send_no_content_type(
            c,
            "POST",
            _invites_base(seed) + "/",
            ('{"email": "%s@neg.test", "role": 15}' % tag).encode(),
        )
    assert r.status_code == 415, r.text
    assert r.text.startswith(CT_415_PREFIX)
    assert r.text.endswith(CT_415_SUFFIX)
    assert _count(conn, "workspace_member_invites", "workspace_id", seed["ws_a"]["id"]) == before


def test_invite_create_urlencoded_json_string_400(seed, conn):
    tag = db.new_tag()
    before = _count(conn, "workspace_member_invites", "workspace_id", seed["ws_a"]["id"])
    with http.client(seed["keys"][IKEY]) as c:
        r = c.post(
            _invites_base(seed) + "/",
            content=('{"email": "%s@neg.test", "role": 15}' % tag).encode(),
            headers=_form_headers(),
        )
    assert r.status_code == 400, r.text
    assert r.json() == {"email": ["This field is required."]}
    assert _count(conn, "workspace_member_invites", "workspace_id", seed["ws_a"]["id"]) == before


def test_invite_create_form_creates(seed, conn):
    tag = db.new_tag()
    email = f"{tag}@neg.test"
    before = _count(conn, "workspace_member_invites", "workspace_id", seed["ws_a"]["id"])
    with http.client(seed["keys"][IKEY]) as c:
        r = c.post(
            _invites_base(seed) + "/",
            content=("email=%s&role=15" % email).encode(),
            headers=_form_headers(),
        )
    assert r.status_code == 201, r.text
    assert r.json()["email"] == email
    assert r.json()["role"] == 15
    assert _count(conn, "workspace_member_invites", "workspace_id", seed["ws_a"]["id"]) == before + 1


def test_invite_create_multipart_creates(seed, conn):
    tag = db.new_tag()
    email = f"{tag}@neg.test"
    before = _count(conn, "workspace_member_invites", "workspace_id", seed["ws_a"]["id"])
    with http.client(seed["keys"][IKEY]) as c:
        r = c.post(
            _invites_base(seed) + "/",
            files={"email": (None, email), "role": (None, "15")},
        )
    assert r.status_code == 201, r.text
    assert r.json()["email"] == email
    assert r.json()["role"] == 15
    assert _count(conn, "workspace_member_invites", "workspace_id", seed["ws_a"]["id"]) == before + 1


def test_invite_create_blank_role_skipped(seed, conn):
    tag = db.new_tag()
    email = f"{tag}@neg.test"
    with http.client(seed["keys"][IKEY]) as c:
        r = c.post(
            _invites_base(seed) + "/",
            content=("email=%s&role=" % email).encode(),
            headers=_form_headers(),
        )
    assert r.status_code == 201, r.text
    assert r.json()["role"] == 5


def test_invite_create_bad_multipart_exact(seed, conn):
    # A multipart body with no boundary is DRF's ``ParseError``, not a 415:
    # the base ``handle_exception`` renders it as ``{"detail": ...}``.
    before = _count(conn, "workspace_member_invites", "workspace_id", seed["ws_a"]["id"])
    with http.client(seed["keys"][IKEY]) as c:
        r = c.post(
            _invites_base(seed) + "/",
            content=b"this is not multipart",
            headers={"Content-Type": "multipart/form-data"},
        )
    assert r.status_code == 400, r.text
    assert r.text == '{"detail":"Multipart form parse error - Invalid boundary in multipart: None"}'
    assert _count(conn, "workspace_member_invites", "workspace_id", seed["ws_a"]["id"]) == before


# --- invitation PATCH ---


def test_invite_patch_text_plain_415(seed, conn):
    inv = db.create_invite(conn, seed["ws_a"]["id"], db.new_tag())
    with http.client(seed["keys"][IKEY]) as c:
        r = c.patch(
            f"{_invites_base(seed)}/{inv['id']}/",
            content=b'{"role": 5}',
            headers={"Content-Type": "text/plain"},
        )
    assert r.status_code == 415, r.text
    assert r.text == TEXT_415
    row = db.fetch_one(
        conn, "SELECT role FROM workspace_member_invites WHERE id = %s", (inv["id"],)
    )
    assert row["role"] == db.MEMBER


def test_invite_patch_missing_content_type_415(seed, conn):
    inv = db.create_invite(conn, seed["ws_a"]["id"], db.new_tag())
    with http.client(seed["keys"][IKEY]) as c:
        r = _send_no_content_type(
            c, "PATCH", f"{_invites_base(seed)}/{inv['id']}/", b'{"role": 5}'
        )
    assert r.status_code == 415, r.text
    assert r.text.startswith(CT_415_PREFIX)
    assert r.text.endswith(CT_415_SUFFIX)
    row = db.fetch_one(
        conn, "SELECT role FROM workspace_member_invites WHERE id = %s", (inv["id"],)
    )
    assert row["role"] == db.MEMBER


def test_invite_patch_urlencoded_json_string_noop(seed, conn):
    inv = db.create_invite(conn, seed["ws_a"]["id"], db.new_tag())
    with http.client(seed["keys"][IKEY]) as c:
        r = c.patch(
            f"{_invites_base(seed)}/{inv['id']}/",
            content=b'{"role": 5}',
            headers=_form_headers(),
        )
    assert r.status_code == 200, r.text
    assert r.json()["role"] == db.MEMBER


def test_invite_patch_form_applies(seed, conn):
    inv = db.create_invite(conn, seed["ws_a"]["id"], db.new_tag())
    with http.client(seed["keys"][IKEY]) as c:
        r = c.patch(
            f"{_invites_base(seed)}/{inv['id']}/",
            content=b"role=5",
            headers=_form_headers(),
        )
    assert r.status_code == 200, r.text
    assert r.json()["role"] == 5


def test_invite_patch_multipart_applies(seed, conn):
    inv = db.create_invite(conn, seed["ws_a"]["id"], db.new_tag())
    with http.client(seed["keys"][IKEY]) as c:
        r = c.patch(f"{_invites_base(seed)}/{inv['id']}/", files={"role": (None, "5")})
    assert r.status_code == 200, r.text
    assert r.json()["role"] == 5


def test_invite_patch_form_email_guard(seed, conn):
    # ``request.data.get("email")`` runs on the form map too.
    inv = db.create_invite(conn, seed["ws_a"]["id"], db.new_tag())
    with http.client(seed["keys"][IKEY]) as c:
        r = c.patch(
            f"{_invites_base(seed)}/{inv['id']}/",
            content=b"email=zz@neg.test",
            headers=_form_headers(),
        )
    assert r.status_code == 400, r.text
    assert r.json() == {
        "error": "Email cannot be updated after invite is created.",
        "code": "EMAIL_CANNOT_BE_UPDATED",
    }


def test_invite_patch_form_blank_email_validates(seed, conn):
    # Falsy ``email`` skips the guard but still runs field validation.
    inv = db.create_invite(conn, seed["ws_a"]["id"], db.new_tag())
    with http.client(seed["keys"][IKEY]) as c:
        r = c.patch(
            f"{_invites_base(seed)}/{inv['id']}/",
            content=b"email=",
            headers=_form_headers(),
        )
    assert r.status_code == 400, r.text
    assert r.json() == {"email": ["This field may not be blank."]}


# --- state POST (answers 200, not 201) ---


def test_state_create_text_plain_415(seed, conn):
    tag = db.new_tag()
    before = _count(conn, "states", "project_id", seed["project"]["id"])
    with http.client(seed["keys"][SKEY]) as c:
        r = c.post(
            _states_base(seed) + "/",
            content=('{"name": "S %s", "color": "#fff"}' % tag).encode(),
            headers={"Content-Type": "text/plain"},
        )
    assert r.status_code == 415, r.text
    assert r.text == TEXT_415
    assert _count(conn, "states", "project_id", seed["project"]["id"]) == before


def test_state_create_missing_content_type_415(seed, conn):
    tag = db.new_tag()
    before = _count(conn, "states", "project_id", seed["project"]["id"])
    with http.client(seed["keys"][SKEY]) as c:
        r = _send_no_content_type(
            c,
            "POST",
            _states_base(seed) + "/",
            ('{"name": "S %s", "color": "#fff"}' % tag).encode(),
        )
    assert r.status_code == 415, r.text
    assert r.text.startswith(CT_415_PREFIX)
    assert r.text.endswith(CT_415_SUFFIX)
    assert _count(conn, "states", "project_id", seed["project"]["id"]) == before


def test_state_create_urlencoded_json_string_400(seed, conn):
    tag = db.new_tag()
    before = _count(conn, "states", "project_id", seed["project"]["id"])
    with http.client(seed["keys"][SKEY]) as c:
        r = c.post(
            _states_base(seed) + "/",
            content=('{"name": "S %s", "color": "#fff"}' % tag).encode(),
            headers=_form_headers(),
        )
    assert r.status_code == 400, r.text
    assert r.json() == {
        "name": ["This field is required."],
        "color": ["This field is required."],
    }
    assert _count(conn, "states", "project_id", seed["project"]["id"]) == before


def test_state_create_form_creates(seed, conn):
    tag = db.new_tag()
    name = f"CT NegSt {tag}"
    before = _count(conn, "states", "project_id", seed["project"]["id"])
    with http.client(seed["keys"][SKEY]) as c:
        r = c.post(
            _states_base(seed) + "/",
            content=("name=%s&color=%%23fff" % name.replace(" ", "+")).encode(),
            headers=_form_headers(),
        )
    assert r.status_code == 200, r.text
    assert r.json()["name"] == name
    assert r.json()["color"] == "#fff"
    assert r.json()["group"] == "backlog"
    assert _count(conn, "states", "project_id", seed["project"]["id"]) == before + 1


def test_state_create_multipart_creates(seed, conn):
    tag = db.new_tag()
    name = f"CT NegSm {tag}"
    before = _count(conn, "states", "project_id", seed["project"]["id"])
    with http.client(seed["keys"][SKEY]) as c:
        r = c.post(
            _states_base(seed) + "/",
            files={"name": (None, name), "color": (None, "#fff")},
        )
    assert r.status_code == 200, r.text
    assert r.json()["name"] == name
    assert r.json()["color"] == "#fff"
    assert _count(conn, "states", "project_id", seed["project"]["id"]) == before + 1


def test_state_create_blank_optionals_skipped(seed, conn):
    # Blank ``sequence``/``group``/``default``/``is_triage`` behave as absent.
    tag = db.new_tag()
    name = f"CT NegSo {tag}"
    with http.client(seed["keys"][SKEY]) as c:
        r = c.post(
            _states_base(seed) + "/",
            content=(
                "name=%s&color=%%23fff&sequence=&group=&default=&is_triage="
                % name.replace(" ", "+")
            ).encode(),
            headers=_form_headers(),
        )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["group"] == "backlog"
    assert body["default"] is False
    assert body["is_triage"] is False
    assert isinstance(body["sequence"], float)


def test_state_create_blank_text_kept(seed, conn):
    # ``description``/``external_*`` allow blank: ``''`` is stored, and the
    # falsy external values skip the 409 pre-check like ``request.data.get``.
    tag = db.new_tag()
    name = f"CT NegSx {tag}"
    with http.client(seed["keys"][SKEY]) as c:
        r = c.post(
            _states_base(seed) + "/",
            content=(
                "name=%s&color=%%23fff&description=&external_id=&external_source="
                % name.replace(" ", "+")
            ).encode(),
            headers=_form_headers(),
        )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["description"] == ""
    assert body["external_id"] == ""
    assert body["external_source"] == ""


# --- state PATCH ---


def test_state_patch_text_plain_415(seed, conn):
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], db.new_tag())
    with http.client(seed["keys"][SKEY]) as c:
        r = c.patch(
            f"{_states_base(seed)}/{st['id']}/",
            content=b'{"description": "x"}',
            headers={"Content-Type": "text/plain"},
        )
    assert r.status_code == 415, r.text
    assert r.text == TEXT_415
    row = db.fetch_one(
        conn, "SELECT description FROM states WHERE id = %s", (st["id"],)
    )
    assert row["description"] != "x"


def test_state_patch_missing_content_type_415(seed, conn):
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], db.new_tag())
    with http.client(seed["keys"][SKEY]) as c:
        r = _send_no_content_type(
            c, "PATCH", f"{_states_base(seed)}/{st['id']}/", b'{"description": "x"}'
        )
    assert r.status_code == 415, r.text
    assert r.text.startswith(CT_415_PREFIX)
    assert r.text.endswith(CT_415_SUFFIX)
    row = db.fetch_one(
        conn, "SELECT description FROM states WHERE id = %s", (st["id"],)
    )
    assert row["description"] != "x"


def test_state_patch_urlencoded_json_string_noop(seed, conn):
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], db.new_tag())
    with http.client(seed["keys"][SKEY]) as c:
        r = c.patch(
            f"{_states_base(seed)}/{st['id']}/",
            content=b'{"description": "x"}',
            headers=_form_headers(),
        )
    assert r.status_code == 200, r.text
    row = db.fetch_one(
        conn, "SELECT description FROM states WHERE id = %s", (st["id"],)
    )
    assert row["description"] != "x"


def test_state_patch_form_applies(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    with http.client(seed["keys"][SKEY]) as c:
        r = c.patch(
            f"{_states_base(seed)}/{st['id']}/",
            content=("description=pd%s&color=%%23123456" % tag).encode(),
            headers=_form_headers(),
        )
    assert r.status_code == 200, r.text
    assert r.json()["description"] == f"pd{tag}"
    assert r.json()["color"] == "#123456"


def test_state_patch_multipart_applies(seed, conn):
    tag = db.new_tag()
    st = db.create_state(conn, seed["ws_a"]["id"], seed["project"]["id"], tag)
    with http.client(seed["keys"][SKEY]) as c:
        r = c.patch(
            f"{_states_base(seed)}/{st['id']}/",
            files={"description": (None, f"pm{tag}")},
        )
    assert r.status_code == 200, r.text
    assert r.json()["description"] == f"pm{tag}"
