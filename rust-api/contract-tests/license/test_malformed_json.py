# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""License malformed-JSON is a 400 envelope (PIDASHCONV-1128, tier 3).

Malformed bodies 400 on both backends with DRF's ``ParseError`` detail
(``JSON parse error - <message>``). The message text is CPython's on
Django and serde's on Rust (PIDASHCONV-1555, rule 13 tier 3); cases
assert status and envelope shape only, not message bytes.
"""

import pytest

CASES = [
    ("PATCH", "/api/instances/"),
    ("POST", "/api/instances/admins/"),
    ("PATCH", "/api/instances/configurations/"),
    ("POST", "/api/instances/email-credentials-check/"),
    ("POST", "/api/instances/workspaces/"),
]

# Malformed bodies (tier 3: message texts differ by backend).
MALFORMED = [
    b"{bad}",
    b'{"a":}',
    b"   ",
    b'{"a":1,}',
]


def _send(admin_api, method, path, body):
    headers = {"Content-Type": "application/json"}
    if method == "PATCH":
        return admin_api.patch(path, content=body, headers=headers)
    return admin_api.post(path, content=body, headers=headers)


@pytest.mark.parametrize("method,path", CASES)
@pytest.mark.parametrize("body", MALFORMED)
def test_malformed_json_is_400_envelope(admin_api, method, path, body):
    res = _send(admin_api, method, path, body)
    assert res.status_code == 400
    detail = res.json()["detail"]
    assert detail.startswith("JSON parse error - "), detail
