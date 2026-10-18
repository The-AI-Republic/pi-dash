# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""License malformed-JSON messages match CPython byte-for-byte (PIDASHCONV-1128).

Malformed bodies 400 on both backends with DRF's ``ParseError`` detail
(``JSON parse error - <CPython message>``). Rust previously rendered
serde errors (or a generic message on admins); it now parses through the
shared ``json_cpython`` layer. Each case pins the exact detail per
endpoint on both backends.
"""

import pytest

CASES = [
    ("PATCH", "/api/instances/"),
    ("POST", "/api/instances/admins/"),
    ("PATCH", "/api/instances/configurations/"),
    ("POST", "/api/instances/email-credentials-check/"),
    ("POST", "/api/instances/workspaces/"),
]

# (body-bytes, expected detail): CPython json error texts, verified live
# against Django 2026-10-17.
MALFORMED = [
    (
        b"{bad}",
        "JSON parse error - Expecting property name enclosed in double quotes: "
        "line 1 column 2 (char 1)",
    ),
    (
        b'{"a":}',
        "JSON parse error - Expecting value: line 1 column 6 (char 5)",
    ),
    (
        b"   ",
        "JSON parse error - Expecting value: line 1 column 4 (char 3)",
    ),
    (
        b'{"a":1,}',
        "JSON parse error - Expecting property name enclosed in double quotes: "
        "line 1 column 8 (char 7)",
    ),
]


def _send(admin_api, method, path, body):
    headers = {"Content-Type": "application/json"}
    if method == "PATCH":
        return admin_api.patch(path, content=body, headers=headers)
    return admin_api.post(path, content=body, headers=headers)


@pytest.mark.parametrize("method,path", CASES)
@pytest.mark.parametrize("body,detail", MALFORMED)
def test_malformed_json_message_is_cpython(admin_api, method, path, body, detail):
    res = _send(admin_api, method, path, body)
    assert res.status_code == 400
    assert res.json() == {"detail": detail}
