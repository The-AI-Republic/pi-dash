# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from django.test import override_settings

from pi_dash.utils.host import issue_web_url, web_base_url


@pytest.mark.unit
class TestWebBaseUrl:
    @override_settings(WEB_URL="https://pi-dash.example.com", APP_BASE_URL=None)
    def test_uses_web_url(self):
        assert web_base_url() == "https://pi-dash.example.com"

    @override_settings(WEB_URL="https://pi-dash.example.com/", APP_BASE_URL=None)
    def test_strips_trailing_slash(self):
        assert web_base_url() == "https://pi-dash.example.com"

    @override_settings(WEB_URL=None, APP_BASE_URL="https://app.example.com")
    def test_falls_back_to_app_base_url(self):
        assert web_base_url() == "https://app.example.com"

    @override_settings(WEB_URL=None, APP_BASE_URL=None)
    def test_returns_none_when_unset(self):
        assert web_base_url() is None

    @override_settings(WEB_URL="", APP_BASE_URL="")
    def test_returns_none_when_blank(self):
        assert web_base_url() is None


@pytest.mark.unit
class TestIssueWebUrl:
    @override_settings(WEB_URL="https://pi-dash.example.com", APP_BASE_URL=None)
    def test_builds_browse_url(self):
        assert issue_web_url("eng", "ENG", 42) == "https://pi-dash.example.com/eng/browse/ENG-42"

    @override_settings(WEB_URL=None, APP_BASE_URL=None)
    def test_omitted_when_base_unset(self):
        assert issue_web_url("eng", "ENG", 42) is None

    @override_settings(WEB_URL="https://pi-dash.example.com", APP_BASE_URL=None)
    def test_none_when_parts_missing(self):
        assert issue_web_url(None, "ENG", 42) is None
        assert issue_web_url("eng", None, 42) is None
        assert issue_web_url("eng", "ENG", None) is None
