# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tenant isolation: app vs spaces behavior for the same credentials.

Recorded from the Python sources (no assumptions):

- app ``sign-in/`` logs in with ``is_app=True`` and redirects through
  ``get_safe_redirect_url(APP_BASE, redirection_path)`` — the redirection
  path never starts with ``/`` so success lands on the bare app base
- ``spaces/sign-in/`` logs in with ``is_space=True``, skips the
  post-auth invitation workflow, and redirects to
  ``SPACE_BASE + validated_next_path`` — success lands on the space base
- the DRF email-check twins are behaviorally identical
"""

from _harness.redirects import location_of

from .conftest import reprime


def test_sign_in_success_locations_differ(browser, base, space_base, user):
    app = browser().post("/auth/sign-in/", data={"email": user["email"], "password": user["password"]})
    space = browser().post(
        "/auth/spaces/sign-in/", data={"email": user["email"], "password": user["password"]}
    )
    assert app.status_code == 302 and space.status_code == 302
    assert location_of(app) == base
    assert location_of(space) == space_base
    assert location_of(app) != location_of(space)


def test_sign_out_locations_differ(browser, base, space_base, user):
    app_client = browser()
    app_client.post("/auth/sign-in/", data={"email": user["email"], "password": user["password"]})
    space_client = browser()
    space_client.post(
        "/auth/spaces/sign-in/", data={"email": user["email"], "password": user["password"]}
    )
    reprime(app_client)
    reprime(space_client)
    assert location_of(app_client.post("/auth/sign-out/")) == base
    assert location_of(space_client.post("/auth/spaces/sign-out/")) == space_base


def test_email_check_twins_agree(anon, user):
    app = anon.post("/auth/email-check/", json={"email": user["email"]}).json()
    space = anon.post("/auth/spaces/email-check/", json={"email": user["email"]}).json()
    assert app == space == {"existing": True, "status": "CREDENTIAL"}


def test_forgot_password_twins_agree(anon, user):
    app = anon.post("/auth/forgot-password/", json={"email": user["email"]}).json()
    space = anon.post("/auth/spaces/forgot-password/", json={"email": user["email"]}).json()
    assert app == space == {"message": "Check your email to reset your password"}
