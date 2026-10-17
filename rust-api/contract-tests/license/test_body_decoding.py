# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""License JSON body decoding matches DRF ``Request._parse`` (PIDASHCONV-1102).

(i) non-JSON content-type with a body is 415 with the verbatim header in
the detail; (ii) a missing content-type with a body is 415 (runserver
echoes ``text/plain`` while prod Django and Rust echo ``""``, so the
test asserts status plus the message prefix/suffix); (iii) an empty body
is ``{}`` without parsing, so validation runs. Case-variant
``Application/JSON`` serves: Django lowercases the base in
``parse_header_parameters``. The ``*/json`` wildcard serves too: DRF
``_MediaType.match`` honors ``*`` on either side.
"""

import pytest

TEXT_PLAIN_415 = {"detail": 'Unsupported media type "text/plain" in request.'}

# (method, path, serve-body-bytes): the body parses and serves under JSON.
CASES = [
    ("PATCH", "/api/instances/", b'{"instance_name":"Probe"}'),
    ("POST", "/api/instances/admins/", b'{"email":"ghost@example.com"}'),
    ("PATCH", "/api/instances/configurations/", b'{"NO_SUCH_KEY":"x"}'),
    ("POST", "/api/instances/email-credentials-check/", b'{"receiver_email":""}'),
    ("POST", "/api/instances/workspaces/", b'{"name":"N","slug":"probe-space"}'),
]


def _send(admin_api, method, path, body, headers):
    if method == "PATCH":
        return admin_api.patch(path, content=body, headers=headers)
    return admin_api.post(path, content=body, headers=headers)


@pytest.mark.parametrize("method,path,body", CASES)
def test_text_plain_with_body_is_415_exact(admin_api, method, path, body):
    res = _send(admin_api, method, path, body, {"Content-Type": "text/plain"})
    assert res.status_code == 415
    assert res.json() == TEXT_PLAIN_415


@pytest.mark.parametrize("method,path,body", CASES)
def test_missing_content_type_with_body_is_415(admin_api, method, path, body):
    request = admin_api.build_request(method, path, content=body)
    assert "content-type" not in request.headers
    res = admin_api.send(request)
    assert res.status_code == 415
    text = res.text
    assert text.startswith('{"detail":"Unsupported media type \\"')
    assert text.endswith('\\" in request."}')


def test_empty_body_instance_patch_is_noop_200(admin_api, world):
    request = admin_api.build_request(
        "PATCH", "/api/instances/", content=b"", headers={"Content-Type": "application/json"}
    )
    res = admin_api.send(request)
    assert res.status_code == 200
    assert res.json()["instance_name"] == world["instance"]["name"]


def test_empty_body_admins_post_requires_email(admin_api):
    request = admin_api.build_request(
        "POST",
        "/api/instances/admins/",
        content=b"",
        headers={"Content-Type": "application/json"},
    )
    res = admin_api.send(request)
    assert res.status_code == 400
    assert res.json() == {"error": "Email is required"}


def test_empty_body_configurations_patch_is_empty_list(admin_api):
    request = admin_api.build_request(
        "PATCH",
        "/api/instances/configurations/",
        content=b"",
        headers={"Content-Type": "application/json"},
    )
    res = admin_api.send(request)
    assert res.status_code == 200
    assert res.json() == []


def test_empty_body_email_check_requires_receiver(admin_api):
    request = admin_api.build_request(
        "POST",
        "/api/instances/email-credentials-check/",
        content=b"",
        headers={"Content-Type": "application/json"},
    )
    res = admin_api.send(request)
    assert res.status_code == 400
    assert res.json() == {"error": "Receiver email is required"}


def test_empty_body_workspace_post_requires_name_and_slug(admin_api):
    request = admin_api.build_request(
        "POST",
        "/api/instances/workspaces/",
        content=b"",
        headers={"Content-Type": "application/json"},
    )
    res = admin_api.send(request)
    assert res.status_code == 400
    assert res.json() == {"error": "Both name and slug are required"}


def test_case_variant_json_instance_patch_serves(admin_api):
    res = admin_api.patch(
        "/api/instances/",
        content=b'{"instance_name":"Probe"}',
        headers={"Content-Type": "Application/JSON"},
    )
    assert res.status_code == 200
    assert res.json()["instance_name"] == "Probe"


def test_case_variant_json_admins_post_serves(admin_api):
    res = admin_api.post(
        "/api/instances/admins/",
        content=b'{"email":"ghost@example.com"}',
        headers={"Content-Type": "Application/JSON"},
    )
    assert res.status_code == 404
    assert res.json() == {"error": "The required object does not exist."}


def test_case_variant_json_configurations_patch_serves(admin_api):
    res = admin_api.patch(
        "/api/instances/configurations/",
        content=b'{"NO_SUCH_KEY":"x"}',
        headers={"Content-Type": "Application/JSON"},
    )
    assert res.status_code == 200
    assert res.json() == []


def test_case_variant_json_email_check_serves(admin_api):
    res = admin_api.post(
        "/api/instances/email-credentials-check/",
        content=b'{"receiver_email":""}',
        headers={"Content-Type": "Application/JSON"},
    )
    assert res.status_code == 400
    assert res.json() == {"error": "Receiver email is required"}


def test_case_variant_json_workspace_post_serves(admin_api):
    res = admin_api.post(
        "/api/instances/workspaces/",
        content=b'{"name":"N","slug":"probe-space"}',
        headers={"Content-Type": "Application/JSON"},
    )
    assert res.status_code == 201
    assert res.json()["slug"] == "probe-space"


def test_wildcard_subtype_json_instance_patch_serves(admin_api):
    res = admin_api.patch(
        "/api/instances/",
        content=b'{"instance_name":"Probe"}',
        headers={"Content-Type": "*/json"},
    )
    assert res.status_code == 200
    assert res.json()["instance_name"] == "Probe"


def test_wildcard_subtype_json_admins_post_serves(admin_api):
    res = admin_api.post(
        "/api/instances/admins/",
        content=b'{"email":"ghost@example.com"}',
        headers={"Content-Type": "*/json"},
    )
    assert res.status_code == 404
    assert res.json() == {"error": "The required object does not exist."}


def test_wildcard_subtype_json_configurations_patch_serves(admin_api):
    res = admin_api.patch(
        "/api/instances/configurations/",
        content=b'{"NO_SUCH_KEY":"x"}',
        headers={"Content-Type": "*/json"},
    )
    assert res.status_code == 200
    assert res.json() == []


def test_wildcard_subtype_json_email_check_serves(admin_api):
    res = admin_api.post(
        "/api/instances/email-credentials-check/",
        content=b'{"receiver_email":""}',
        headers={"Content-Type": "*/json"},
    )
    assert res.status_code == 400
    assert res.json() == {"error": "Receiver email is required"}


def test_wildcard_subtype_json_workspace_post_serves(admin_api):
    res = admin_api.post(
        "/api/instances/workspaces/",
        content=b'{"name":"N","slug":"probe-space"}',
        headers={"Content-Type": "*/json"},
    )
    assert res.status_code == 201
    assert res.json()["slug"] == "probe-space"
