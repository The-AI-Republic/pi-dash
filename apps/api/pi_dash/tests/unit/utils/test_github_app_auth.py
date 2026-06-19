# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for github_app_auth helpers."""

import pytest

from pi_dash.utils.github_app_auth import _normalize_private_key

PEM = "-----BEGIN RSA PRIVATE KEY-----\nMIIabc\nDEF==\n-----END RSA PRIVATE KEY-----\n"


@pytest.mark.unit
class TestNormalizePrivateKey:
    def test_escaped_newlines_become_real(self):
        # As stored single-line in an env/SSM var: literal backslash-n.
        escaped = PEM.replace("\n", "\\n")
        assert "\\n" in escaped  # precondition: the stored form is escaped
        result = _normalize_private_key(escaped)
        assert result == PEM.strip()
        assert "\\n" not in result and "\n" in result

    def test_escaped_crlf_becomes_real(self):
        escaped = PEM.replace("\n", "\\r\\n")
        result = _normalize_private_key(escaped)
        assert result == PEM.strip()
        assert "\\r" not in result and "\\n" not in result

    def test_real_newline_key_unchanged(self):
        assert _normalize_private_key(PEM) == PEM.strip()

    @pytest.mark.parametrize("value", ["", None, "   "])
    def test_empty_or_none(self, value):
        assert _normalize_private_key(value) == ""
