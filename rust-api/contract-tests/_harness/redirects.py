# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Redirect + error_code assertion helpers for form-POST View suites.

The session/email-password Views answer form POSTs with 302 redirects:
success carries no ``error_code`` query param, failure carries
``error_code``/``error_message`` (see
``pi_dash.authentication.adapter.error``). Redirects are never followed
so the 302 semantics stay observable. Never import Django here.
"""

from urllib.parse import parse_qs, urlparse


def location_of(response) -> str:
    """Return the Location of a redirect response, asserting it redirects."""
    assert response.status_code in (301, 302, 303, 307, 308), (
        f"expected redirect, got {response.status_code}: {response.text[:300]!r}"
    )
    location = response.headers.get("location")
    assert location, "redirect without a Location header"
    return location


def query_of(location: str) -> dict:
    """First-value mapping of the query params on a Location URL."""
    return {k: v[0] for k, v in parse_qs(urlparse(location).query).items()}


def assert_error_redirect(response, *, base: str, error_code: int) -> str:
    """302 to ``base`` carrying the given numeric ``error_code`` param."""
    location = location_of(response)
    assert location.startswith(base), (
        f"redirect {location!r} does not start with {base!r}"
    )
    params = query_of(location)
    assert params.get("error_code") == str(error_code), (
        f"want error_code={error_code}, got params {params} at {location!r}"
    )
    return location


def assert_success_redirect(response, *, base: str) -> str:
    """302 to ``base`` carrying no ``error_code`` param."""
    location = location_of(response)
    assert location.startswith(base), (
        f"redirect {location!r} does not start with {base!r}"
    )
    params = query_of(location)
    assert "error_code" not in params, (
        f"success redirect carries error params {params} at {location!r}"
    )
    return location
