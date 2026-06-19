# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

import hashlib
import hmac
import time
from datetime import datetime, timezone as dt_timezone
from typing import Any

import jwt
import requests
from django.core.cache import cache

from pi_dash.license.utils.instance_value import get_configuration_value

GITHUB_API_BASE = "https://api.github.com"
GITHUB_WEB_BASE = "https://github.com"
DEFAULT_TIMEOUT_SECONDS = 30
INSTALLATION_TOKEN_CACHE_PREFIX = "github_app_installation_token"


class GithubAppConfigError(Exception):
    """GitHub App is not configured for this Pi Dash instance."""


class GithubAppAuthError(Exception):
    """GitHub App authentication failed."""


def get_github_app_config() -> dict[str, str]:
    app_id, app_slug, private_key, webhook_secret, client_id, client_secret = get_configuration_value(
        [
            {"key": "GITHUB_APP_ID", "default": None},
            {"key": "GITHUB_APP_SLUG", "default": None},
            {"key": "GITHUB_APP_PRIVATE_KEY", "default": None},
            {"key": "GITHUB_APP_WEBHOOK_SECRET", "default": None},
            {"key": "GITHUB_APP_CLIENT_ID", "default": None},
            {"key": "GITHUB_APP_CLIENT_SECRET", "default": None},
        ]
    )
    return {
        "app_id": (app_id or "").strip(),
        "app_slug": (app_slug or "").strip(),
        "private_key": (private_key or "").strip(),
        "webhook_secret": (webhook_secret or "").strip(),
        "client_id": (client_id or "").strip(),
        "client_secret": (client_secret or "").strip(),
    }


def require_github_app_config(*, oauth: bool = False, webhook: bool = False) -> dict[str, str]:
    config = get_github_app_config()
    required = ["app_id", "app_slug", "private_key"]
    if oauth:
        required.extend(["client_id", "client_secret"])
    if webhook:
        required.append("webhook_secret")
    missing = [key for key in required if not config.get(key)]
    if missing:
        raise GithubAppConfigError(f"GitHub App config missing: {', '.join(missing)}")
    return config


def build_app_jwt() -> str:
    config = require_github_app_config()
    now = int(time.time())
    payload = {
        # GitHub recommends issuing slightly in the past to avoid clock drift.
        "iat": now - 60,
        "exp": now + (9 * 60),
        "iss": config["app_id"],
    }
    return jwt.encode(payload, config["private_key"], algorithm="RS256")


def app_headers() -> dict[str, str]:
    return {
        "Authorization": f"Bearer {build_app_jwt()}",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "pi-dash-github-app",
    }


def user_headers(token: str) -> dict[str, str]:
    return {
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "pi-dash-github-app",
    }


def parse_github_datetime(value: str | None):
    if not value:
        return None
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def exchange_user_code(code: str) -> str:
    config = require_github_app_config(oauth=True)
    response = requests.post(
        f"{GITHUB_WEB_BASE}/login/oauth/access_token",
        headers={"Accept": "application/json"},
        data={
            "client_id": config["client_id"],
            "client_secret": config["client_secret"],
            "code": code,
        },
        timeout=DEFAULT_TIMEOUT_SECONDS,
    )
    response.raise_for_status()
    payload = response.json()
    token = payload.get("access_token")
    if not token:
        raise GithubAppAuthError(
            payload.get("error_description") or payload.get("error") or "GitHub did not return a user token"
        )
    return token


def list_user_installations(user_token: str) -> list[dict[str, Any]]:
    installations: list[dict[str, Any]] = []
    url = f"{GITHUB_API_BASE}/user/installations?per_page=100"
    while url:
        response = requests.get(url, headers=user_headers(user_token), timeout=DEFAULT_TIMEOUT_SECONDS)
        response.raise_for_status()
        payload = response.json()
        installations.extend(payload.get("installations") or [])
        url = None
        for link_part in (response.headers.get("Link") or "").split(","):
            link_part = link_part.strip()
            if 'rel="next"' in link_part and link_part.startswith("<"):
                url = link_part.split(">", 1)[0][1:]
                break
    return installations


def verify_user_can_access_installation(user_token: str, installation_id: int) -> dict[str, Any] | None:
    for installation in list_user_installations(user_token):
        if int(installation.get("id") or 0) == int(installation_id):
            return installation
    return None


def get_installation(installation_id: int) -> dict[str, Any]:
    response = requests.get(
        f"{GITHUB_API_BASE}/app/installations/{installation_id}",
        headers=app_headers(),
        timeout=DEFAULT_TIMEOUT_SECONDS,
    )
    response.raise_for_status()
    return response.json()


def mint_installation_token(installation_id: int) -> dict[str, Any]:
    response = requests.post(
        f"{GITHUB_API_BASE}/app/installations/{installation_id}/access_tokens",
        headers=app_headers(),
        timeout=DEFAULT_TIMEOUT_SECONDS,
    )
    response.raise_for_status()
    return response.json()


def installation_token(installation_id: int) -> str:
    cache_key = f"{INSTALLATION_TOKEN_CACHE_PREFIX}:{installation_id}"
    cached = cache.get(cache_key)
    if cached:
        return cached

    payload = mint_installation_token(installation_id)
    token = payload.get("token")
    if not token:
        raise GithubAppAuthError("GitHub did not return an installation token")

    expires_at = parse_github_datetime(payload.get("expires_at"))
    ttl = 55 * 60
    if expires_at:
        ttl = max(60, int((expires_at - datetime.now(dt_timezone.utc)).total_seconds()) - 60)
    cache.set(cache_key, token, ttl)
    return token


def revoke_installation_cache(installation_id: int) -> None:
    cache.delete(f"{INSTALLATION_TOKEN_CACHE_PREFIX}:{installation_id}")


def verify_webhook_signature(raw_body: bytes, signature: str | None) -> bool:
    config = require_github_app_config(webhook=True)
    if not signature or not signature.startswith("sha256="):
        return False
    expected = "sha256=" + hmac.new(
        config["webhook_secret"].encode("utf-8"),
        raw_body,
        hashlib.sha256,
    ).hexdigest()
    return hmac.compare_digest(expected, signature)
