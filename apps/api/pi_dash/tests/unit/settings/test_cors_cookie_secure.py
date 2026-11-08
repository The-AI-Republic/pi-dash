# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

pytestmark = pytest.mark.unit

from pi_dash.settings.common import compute_secure_origins


def test_empty_is_not_secure():
    assert compute_secure_origins([]) is False


def test_https_only_is_secure():
    assert compute_secure_origins(["https://a.example"]) is True


def test_http_only_is_not_secure():
    assert compute_secure_origins(["http://a.example"]) is False


def test_https_plus_windows_desktop_is_secure():
    # PIDESKAPP-176: http://tauri.localhost must not downgrade an HTTPS deployment
    origins = ["https://a.example", "http://tauri.localhost"]
    assert compute_secure_origins(origins) is True


def test_https_plus_tauri_localhost_is_secure():
    origins = ["https://a.example", "tauri://localhost"]
    assert compute_secure_origins(origins) is True


def test_https_plus_https_tauri_localhost_is_secure():
    origins = ["https://a.example", "https://tauri.localhost"]
    assert compute_secure_origins(origins) is True


def test_plain_http_plus_desktop_is_not_secure():
    origins = ["http://a.example", "http://tauri.localhost"]
    assert compute_secure_origins(origins) is False


def test_no_http_substring_is_secure():
    assert compute_secure_origins(["localhost:3000"]) is True
