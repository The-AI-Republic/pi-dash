# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""GitHub read tool — pull-request merge status.

Nothing in the DB materializes PR merge state (issue links are free-form URLs,
the GitHub integration syncs issues/comments not PRs), so the agent checks
merge state live. This tool **never raises**: ``state="unknown"`` is a normal
answer the loop's auto-close prompt is written around ("only act when clearly
established"). Available to chat and loop runs alike. See
``.ai_design/loop_project_management/design.md`` §8.2.
"""

from __future__ import annotations

import logging
import re

from django.conf import settings
from pydantic_ai import RunContext

from pi_dash.assistant import ssrf
from pi_dash.assistant.runtime.agent import assistant
from pi_dash.assistant.runtime.deps import AssistantDeps
from pi_dash.assistant.tools import _scoping

logger = logging.getLogger(__name__)

_PR_URL_RE = re.compile(
    r"^https://github\.com/(?P<owner>[\w.-]+)/(?P<repo>[\w.-]+)/pull/(?P<num>\d+)"
)
_API_HOST = "https://api.github.com"
_TIMEOUT_S = 10


def _unknown(reason: str) -> dict:
    return {"state": "unknown", "reason": reason}


def _find_token(deps: AssistantDeps, owner: str, repo: str) -> str | None:
    """Best-effort access token from a GithubRepositorySync on a project the
    user can access whose repository matches ``owner/repo``. Returns None when
    none is found (the call then proceeds unauthenticated)."""
    from pi_dash.db.models import GithubRepositorySync

    sync = (
        GithubRepositorySync.objects.filter(
            project__in=_scoping.member_projects(deps),
            repository__owner__iexact=owner,
            repository__name__iexact=repo,
        )
        .order_by("-created_at")
        .first()
    )
    if sync is None:
        return None
    creds = sync.credentials or {}
    token = creds.get("access_token") or creds.get("token")
    return token or None


@assistant.tool
def get_pull_request_status(ctx: RunContext[AssistantDeps], url: str) -> dict:
    """Check whether a GitHub pull request is merged.

    Pass a full PR URL like ``https://github.com/owner/repo/pull/123``. Returns
    ``{"state": "merged"|"open"|"closed"|"unknown", ...}``. A non-GitHub or
    unparseable URL, a rate limit, or a network error all return ``unknown`` —
    treat ``unknown`` as "could not establish; do not act".
    """
    deps = ctx.deps

    # Per-run budget — protects the unauthenticated GitHub rate limit.
    cap = int(getattr(settings, "LOOP_PR_LOOKUPS_PER_RUN", 15))
    if deps.budget.pr_lookups >= cap:
        return _unknown("budget_exhausted")
    deps.budget.pr_lookups += 1

    match = _PR_URL_RE.match((url or "").strip())
    if match is None:
        return _unknown("unsupported_url")
    owner, repo, num = match.group("owner"), match.group("repo"), match.group("num")

    api_url = f"{_API_HOST}/repos/{owner}/{repo}/pulls/{num}"
    if ssrf.is_blocked(api_url):
        return _unknown("blocked")

    headers = {"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"}
    token = _find_token(deps, owner, repo)
    if token:
        headers["Authorization"] = f"Bearer {token}"

    import httpx

    try:
        with httpx.Client(timeout=_TIMEOUT_S, follow_redirects=False) as client:
            resp = client.get(api_url, headers=headers)
    except Exception:  # noqa: BLE001 — any transport failure is just "unknown"
        logger.warning("get_pull_request_status: network error for %s/%s#%s", owner, repo, num)
        return _unknown("network_error")

    if resp.status_code == 404:
        return _unknown("not_found")
    if resp.status_code in (403, 429):
        return _unknown("rate_limited")
    if resp.status_code == 451:
        return _unknown("blocked")
    if resp.status_code != 200:
        return _unknown(f"http_{resp.status_code}")

    try:
        data = resp.json()
    except Exception:  # noqa: BLE001
        return _unknown("bad_response")

    if data.get("merged") or data.get("merged_at"):
        state = "merged"
    elif data.get("state") == "closed":
        state = "closed"
    else:
        state = "open"
    return {
        "state": state,
        "title": data.get("title"),
        "merged_at": data.get("merged_at"),
        "url": f"https://github.com/{owner}/{repo}/pull/{num}",
    }
