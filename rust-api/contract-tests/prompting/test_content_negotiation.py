"""PUT/preview bodies negotiate Content-Type like DRF (PIDASHCONV-1230).

Django's prompting views read ``request.data``, so DRF's default parsers
(JSON, Form, MultiPart) decide how the body is decoded: form input works,
an unsupported media type is 415, and a malformed JSON body is DRF's 400
with CPython's message.  For POST, ``SessionAuthentication.enforce_csrf``
reads ``request.POST`` and so parses the body between the CSRF-cookie
check and the token comparison; the parse error therefore wins over a
missing header and over every view gate (workspace, membership, kind).
"""

MALFORMED_JSON = b"["
MALFORMED_DETAIL = {
    "detail": "JSON parse error - Expecting value: line 1 column 2 (char 1)"
}


def _detail_url(slug, section_key="implementation"):
    return f"/api/workspaces/{slug}/prompt-sections/{section_key}"


def _preview_url(slug, kind="coding-task"):
    return f"/api/workspaces/{slug}/prompts/{kind}/preview"


def test_put_form_body_saves_override(api, tenant_a, write_a):
    """application/x-www-form-urlencoded reaches the view as a QueryDict."""
    r = api.put(
        _detail_url(tenant_a["workspace"]["slug"]),
        headers={
            **write_a,
            "Content-Type": "application/x-www-form-urlencoded",
        },
        content=b"body=hello&scope=workspace",
    )
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["section_key"] == "implementation"
    assert body["body"] == "hello"
    assert body["is_workspace_level"] is True


def test_put_json_body_under_text_plain_is_415(api, tenant_a, write_a):
    """No default parser handles text/plain: DRF's UnsupportedMediaType."""
    r = api.put(
        _detail_url(tenant_a["workspace"]["slug"]),
        headers={**write_a, "Content-Type": "text/plain"},
        content=b'{"scope":"workspace","body":"hello"}',
    )
    assert r.status_code == 415, r.text
    assert r.json() == {
        "detail": 'Unsupported media type "text/plain" in request.'
    }


def test_preview_parses_body_before_missing_csrf_header(api, tenant_a, write_a):
    headers = {
        key: value
        for key, value in write_a.items()
        if key.lower() != "x-csrftoken"
    }
    r = api.post(
        _preview_url(tenant_a["workspace"]["slug"]),
        headers={**headers, "Content-Type": "application/json"},
        content=MALFORMED_JSON,
    )
    assert r.status_code == 400, r.text
    assert r.json() == MALFORMED_DETAIL


def test_preview_form_body_supplies_csrf_token(api, tenant_a, issue_a, write_a):
    """After the parse, the token phase reads csrfmiddlewaretoken from POST."""
    headers = {
        key: value
        for key, value in write_a.items()
        if key.lower() != "x-csrftoken"
    }
    body = (
        f"issue_id={issue_a}&csrfmiddlewaretoken={write_a['X-CSRFToken']}"
    ).encode()
    r = api.post(
        _preview_url(tenant_a["workspace"]["slug"]),
        headers={
            **headers,
            "Content-Type": "application/x-www-form-urlencoded",
        },
        content=body,
    )
    assert r.status_code == 200, r.text
    assert r.json()["kind"] == "coding-task"


def test_preview_parses_body_before_unknown_workspace(api, write_a):
    r = api.post(
        _preview_url("no-such-workspace"),
        headers={**write_a, "Content-Type": "application/json"},
        content=MALFORMED_JSON,
    )
    assert r.status_code == 400, r.text
    assert r.json() == MALFORMED_DETAIL


def test_preview_parses_body_before_membership_gate(api, tenant_a, write_b):
    r = api.post(
        _preview_url(tenant_a["workspace"]["slug"]),
        headers={**write_b, "Content-Type": "application/json"},
        content=MALFORMED_JSON,
    )
    assert r.status_code == 400, r.text
    assert r.json() == MALFORMED_DETAIL


def test_preview_parses_body_before_unknown_kind(api, tenant_a, write_a):
    r = api.post(
        _preview_url(tenant_a["workspace"]["slug"], "no-such-kind"),
        headers={**write_a, "Content-Type": "application/json"},
        content=MALFORMED_JSON,
    )
    assert r.status_code == 400, r.text
    assert r.json() == MALFORMED_DETAIL
