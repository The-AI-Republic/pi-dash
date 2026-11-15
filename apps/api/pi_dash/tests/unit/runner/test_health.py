# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for ``GET /api/v1/runner/health/``.

The endpoint must report the protocol version the API actually
*enforces* (``settings.RUNNER_PROTOCOL_VERSION``) — not a hardcoded
copy that can drift — plus the installed package version, so an
operator can date a deployment from the outside. See
``views/register.py::_api_version`` for the incident that motivated
this.
"""

from __future__ import annotations

from django.conf import settings


def test_health_reports_enforced_protocol_version(api_client):
    resp = api_client.get("/api/v1/runner/health/")
    assert resp.status_code == 200
    body = resp.json()
    assert body["ok"] is True
    assert body["protocol_version"] == settings.RUNNER_PROTOCOL_VERSION


def test_health_reports_api_version(api_client):
    resp = api_client.get("/api/v1/runner/health/")
    body = resp.json()
    # Installed package version, or the explicit "unknown" fallback when
    # running uninstalled from source — never absent.
    assert isinstance(body["api_version"], str)
    assert body["api_version"]
