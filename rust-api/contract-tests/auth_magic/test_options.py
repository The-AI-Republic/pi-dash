# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: OPTIONS on the six magic-link routes (PIDASHCONV-829).

Pins the proxy arms: the generate routes are DRF ``APIView`` (OPTIONS
answers 200 metadata), the sign-in/up routes are plain Django ``View``
(OPTIONS answers 200 with an ``Allow`` header and an empty body). Axum
answers 405 with an empty body when the arms are missing, so these pins
fail on a POST-only router.
"""

import httpx
import pytest

from _harness import env

GENERATE = {
    "/auth/magic-generate/": "Magic Generate Endpoint",
    "/auth/spaces/magic-generate/": "Magic Generate Space Endpoint",
}

SIGN_ROUTES = [
    "/auth/magic-sign-in/",
    "/auth/magic-sign-up/",
    "/auth/spaces/magic-sign-in/",
    "/auth/spaces/magic-sign-up/",
]

ALLOW = "POST, OPTIONS"


@pytest.mark.contract
class TestMagicOptions:
    @pytest.mark.parametrize("path", sorted(GENERATE))
    def test_generate_options_is_drf_metadata(self, path):
        client = httpx.Client(base_url=env.base_url(), timeout=30.0)

        resp = client.options(path)

        assert resp.status_code == 200, resp.text
        assert resp.headers["allow"] == ALLOW
        body = resp.json()
        assert set(body) == {"name", "description", "renders", "parses"}
        assert body["name"] == GENERATE[path]
        assert body["description"] == ""
        assert body["renders"] == ["application/json"]
        assert body["parses"] == [
            "application/json",
            "application/x-www-form-urlencoded",
            "multipart/form-data",
        ]

    @pytest.mark.parametrize("path", SIGN_ROUTES)
    def test_sign_options_is_allow_only(self, path):
        client = httpx.Client(base_url=env.base_url(), timeout=30.0)

        resp = client.options(path)

        assert resp.status_code == 200, resp.text
        assert resp.headers["allow"] == ALLOW
        assert resp.content == b""
