# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Minimal GitHub REST client used by the issue-sync background task.

Scope is intentionally narrow: PAT auth, the few endpoints the sync loop
needs, paginated iteration via the Link header. See
.ai_design/github_sync/design.md §6.3 for the consuming flow.
"""

from __future__ import annotations

import re
from datetime import datetime
from typing import Iterable, Iterator, Optional
from urllib.parse import urlencode

import requests

GITHUB_API_BASE = "https://api.github.com"
DEFAULT_TIMEOUT_SECONDS = 30


class GithubAuthError(Exception):
    """401 from GitHub — token missing/invalid/expired."""


class GithubPermissionError(Exception):
    """403 from GitHub — token lacks scope or hit secondary rate limit."""


class GithubNotFoundError(Exception):
    """404 from GitHub — issue/repo deleted, transferred, or never existed."""


class GithubClient:
    def __init__(self, token: str, *, timeout: int = DEFAULT_TIMEOUT_SECONDS, api_base: str = GITHUB_API_BASE):
        if not token:
            raise GithubAuthError("empty token")
        self._token = token
        self._timeout = timeout
        self._api_base = api_base.rstrip("/")

    @classmethod
    def for_installation(cls, installation_id: int, *, timeout: int = DEFAULT_TIMEOUT_SECONDS):
        from pi_dash.utils.github_app_auth import installation_token

        return cls(token=installation_token(installation_id), timeout=timeout)

    # ----- HTTP plumbing -----

    def _headers(self) -> dict:
        return {
            "Authorization": f"Bearer {self._token}",
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "pi-dash-github-sync",
        }

    def _request(self, method: str, url: str, **kwargs) -> requests.Response:
        kwargs.setdefault("timeout", self._timeout)
        response = requests.request(method, url, headers=self._headers(), **kwargs)
        if response.status_code == 401:
            raise GithubAuthError(response.text)
        if response.status_code == 403:
            raise GithubPermissionError(response.text)
        if response.status_code == 404:
            raise GithubNotFoundError(response.text)
        response.raise_for_status()
        return response

    @staticmethod
    def _next_url(response: requests.Response) -> Optional[str]:
        """Parse the `next` URL from a paginated response's Link header."""
        link = response.headers.get("Link", "")
        if not link:
            return None
        for part in link.split(","):
            match = re.match(r'\s*<([^>]+)>;\s*rel="next"', part)
            if match:
                return match.group(1)
        return None

    def _paginate(self, path: str, params: Optional[dict] = None) -> Iterator[dict]:
        url = f"{self._api_base}{path}"
        if params:
            url = f"{url}?{urlencode(params)}"
        while url:
            response = self._request("GET", url)
            for item in response.json():
                yield item
            url = self._next_url(response)

    # ----- Endpoints -----

    def get_authenticated_user(self) -> dict:
        """GET /user — used to validate a PAT on connect."""
        return self._request("GET", f"{self._api_base}/user").json()

    def list_user_repos(self, *, page: int = 1, per_page: int = 100) -> tuple[list[dict], bool]:
        """One page of /user/repos with the affiliation filter required to
        surface org repos. Returns (repos, has_next_page)."""
        params = {
            "affiliation": "owner,collaborator,organization_member",
            "per_page": per_page,
            "sort": "updated",
            "page": page,
        }
        url = f"{self._api_base}/user/repos?{urlencode(params)}"
        response = self._request("GET", url)
        return response.json(), self._next_url(response) is not None

    def get_repo(self, owner: str, name: str) -> dict:
        """GET /repos/{owner}/{repo} — used to verify a bind request's
        (owner, name, repository_id) consistency."""
        return self._request("GET", f"{self._api_base}/repos/{owner}/{name}").json()

    def list_installation_repositories(self, *, page: int = 1, per_page: int = 100) -> tuple[list[dict], bool, int]:
        """One page of repos visible to the current installation token."""
        params = {"per_page": per_page, "page": page}
        url = f"{self._api_base}/installation/repositories?{urlencode(params)}"
        response = self._request("GET", url)
        payload = response.json()
        return (
            payload.get("repositories") or [],
            self._next_url(response) is not None,
            int(payload.get("total_count") or 0),
        )

    def list_all_open_issues(self, owner: str, name: str) -> Iterable[dict]:
        """Paginated /issues with state=open. PRs are returned alongside
        issues — caller must filter via the `pull_request` field."""
        return self._paginate(
            f"/repos/{owner}/{name}/issues",
            {"state": "open", "per_page": 100, "sort": "updated", "direction": "desc"},
        )

    def list_all_repo_comments(self, owner: str, name: str) -> Iterable[dict]:
        """Paginated /issues/comments — repo-wide; covers every issue and PR.
        Caller must filter to mirrored issues."""
        return self._paginate(
            f"/repos/{owner}/{name}/issues/comments",
            {"per_page": 100, "sort": "updated", "direction": "asc"},
        )

    def post_issue_comment(self, owner: str, name: str, issue_number: int, body: str) -> dict:
        """POST /repos/{owner}/{repo}/issues/{number}/comments."""
        url = f"{self._api_base}/repos/{owner}/{name}/issues/{issue_number}/comments"
        response = self._request("POST", url, json={"body": body})
        return response.json()

    def get_pull_request(self, owner: str, name: str, number: int) -> dict:
        """GET /repos/{owner}/{repo}/pulls/{number} — used to snapshot a PR's
        title/state/draft/merged when an issue link is attached."""
        return self._request("GET", f"{self._api_base}/repos/{owner}/{name}/pulls/{number}").json()


_ISSUE_URL_RE = re.compile(r"/repos/[^/]+/[^/]+/issues/(\d+)$")


def parse_issue_number_from_url(issue_url: str) -> Optional[int]:
    """Extract the issue number from a GitHub comment's `issue_url` field."""
    if not issue_url:
        return None
    match = _ISSUE_URL_RE.search(issue_url)
    return int(match.group(1)) if match else None


# Accept the two formats users commonly paste:
#   - HTTPS:  https://github.com/<owner>/<repo>[.git][/]
#   - SSH:    git@github.com:<owner>/<repo>[.git]
# Anything else (gitlab, bitbucket, self-hosted, GH enterprise on a different
# host) deliberately fails — PR 65 only ships github.com.
_HTTPS_REPO_RE = re.compile(r"^https?://github\.com/(?P<owner>[^/\s]+)/(?P<name>[^/\s]+?)(?:\.git)?/?$")
_SSH_REPO_RE = re.compile(r"^git@github\.com:(?P<owner>[^/\s]+)/(?P<name>[^/\s]+?)(?:\.git)?$")


def parse_github_repo_url(url: str) -> Optional[tuple[str, str]]:
    """Parse a github.com repo URL into ``(owner, name)``.

    Returns ``None`` if the URL is empty, malformed, or points at a non-github
    host. ``name`` has any trailing ``.git`` stripped so callers can use it
    against the REST API directly.
    """
    if not url:
        return None
    candidate = url.strip()
    for pattern in (_HTTPS_REPO_RE, _SSH_REPO_RE):
        match = pattern.match(candidate)
        if match:
            return match.group("owner"), match.group("name")
    return None


# github.com PR URL, e.g. https://github.com/<owner>/<repo>/pull/<number>.
# github.com only, matching parse_github_repo_url's host scope.
_HTTPS_PR_RE = re.compile(
    r"^https?://github\.com/(?P<owner>[^/\s]+)/(?P<name>[^/\s]+?)/pull/(?P<number>\d+)(?:/[^\s]*)?$"
)


def parse_github_pull_request_url(url: str) -> Optional[tuple[str, str, int]]:
    """Parse a github.com PR URL into ``(owner, name, number)``.

    Returns ``None`` if the URL is empty, malformed, points at a non-github
    host, or is not a ``/pull/<n>`` URL (e.g. an ``/issues/<n>`` URL).
    """
    if not url:
        return None
    match = _HTTPS_PR_RE.match(url.strip())
    if not match:
        return None
    return match.group("owner"), match.group("name"), int(match.group("number"))


def _parse_iso8601(value: Optional[str]) -> Optional[datetime]:
    if not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def pr_snapshot_from_payload(pull_request: dict) -> dict:
    """Map a GitHub pull-request object (REST API or webhook payload) onto the
    display-only snapshot fields stored on ``GithubPullRequestLink``.

    GitHub reports ``merged_at`` rather than a boolean on the webhook payload,
    so derive ``merged`` from either ``merged`` or ``merged_at``. ``pr_updated_at``
    is returned as a parsed (tz-aware) ``datetime`` for direct model assignment.
    """
    merged = bool(pull_request.get("merged") or pull_request.get("merged_at"))
    return {
        "title": (pull_request.get("title") or "")[:500],
        "state": "closed" if (pull_request.get("state") == "closed") else "open",
        "merged": merged,
        "draft": bool(pull_request.get("draft")),
        "pr_updated_at": _parse_iso8601(pull_request.get("updated_at")),
    }
