# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Django imports
from django.conf import settings
from django.core.exceptions import ImproperlyConfigured
from django.http import HttpRequest

# Third party imports
from rest_framework.request import Request

# Module imports
from pi_dash.utils.ip_address import get_client_ip


def base_host(
    request: Request | HttpRequest,
    is_admin: bool = False,
    is_space: bool = False,
    is_app: bool = False,
) -> str:
    """Utility function to return host / origin from the request"""
    # Calculate the base origin from request
    base_origin = settings.WEB_URL or settings.APP_BASE_URL

    if not base_origin:
        raise ImproperlyConfigured("APP_BASE_URL or WEB_URL is not set")

    # Admin redirection
    if is_admin:
        admin_base_path = getattr(settings, "ADMIN_BASE_PATH", None)
        if not isinstance(admin_base_path, str):
            admin_base_path = "/god-mode/"
        if not admin_base_path.startswith("/"):
            admin_base_path = "/" + admin_base_path
        if not admin_base_path.endswith("/"):
            admin_base_path += "/"

        if settings.ADMIN_BASE_URL:
            return settings.ADMIN_BASE_URL + admin_base_path
        else:
            return base_origin + admin_base_path

    # Space redirection
    if is_space:
        space_base_path = getattr(settings, "SPACE_BASE_PATH", None)
        if not isinstance(space_base_path, str):
            space_base_path = "/spaces/"
        if not space_base_path.startswith("/"):
            space_base_path = "/" + space_base_path
        if not space_base_path.endswith("/"):
            space_base_path += "/"

        if settings.SPACE_BASE_URL:
            return settings.SPACE_BASE_URL + space_base_path
        else:
            return base_origin + space_base_path

    # App Redirection
    if is_app:
        if settings.APP_BASE_URL:
            return settings.APP_BASE_URL
        else:
            return base_origin

    return base_origin


def web_base_url() -> str | None:
    """Return the configured public web (frontend) origin, or ``None``.

    The value comes from deployment configuration (``WEB_URL``, falling back to
    ``APP_BASE_URL``) — never from the inbound request host, which may be a
    proxy or a non-public hostname and would produce links that do not work for
    the user. Returns ``None`` when neither is configured so callers can *omit*
    a url field rather than emit a wrong or relative one. The trailing slash is
    stripped so callers can join paths with a leading ``/``.
    """
    base_origin = settings.WEB_URL or settings.APP_BASE_URL
    if not base_origin:
        return None
    return base_origin.rstrip("/")


def issue_web_url(workspace_slug, project_identifier, sequence_id) -> str | None:
    """Build the absolute, human-clickable web URL for an issue, or ``None``.

    Uses the canonical browse route (``/<slug>/browse/<PROJ>-<seq>``), which the
    web UI resolves directly (the UUID route redirects to it). Returns ``None``
    when the web base URL is unconfigured or any identifier part is missing, so
    the caller can omit the field entirely.
    """
    base = web_base_url()
    if not base or not workspace_slug or not project_identifier or sequence_id is None:
        return None
    return f"{base}/{workspace_slug}/browse/{project_identifier}-{sequence_id}"


def user_ip(request: Request | HttpRequest) -> str:
    return get_client_ip(request=request)
