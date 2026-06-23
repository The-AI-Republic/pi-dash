# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Git provider read tool — pull-request / merge-request status.

The agent checks merge state live. This tool **never raises**:
``state="unknown"`` is a normal answer the loop's auto-close prompt is written
around ("only act when clearly established"). Available to chat and loop runs
alike. See
``.ai_design/loop_project_management/design.md`` §8.2.
"""

from __future__ import annotations

import logging
from urllib.parse import quote

from django.conf import settings
from pydantic_ai import RunContext

from pi_dash.assistant import ssrf
from pi_dash.assistant.runtime.agent import assistant
from pi_dash.assistant.runtime.deps import AssistantDeps
from pi_dash.assistant.tools import _scoping
from pi_dash.integrations.git.registry import parse_code_review_url
from pi_dash.license.utils.encryption import decrypt_data

logger = logging.getLogger(__name__)

_API_HOST = "https://api.github.com"
_TIMEOUT_S = 10


def _unknown(reason: str) -> dict:
    return {"state": "unknown", "reason": reason}


def _maybe_decrypt(token: str | None) -> str | None:
    if not token:
        return None
    try:
        return decrypt_data(token)
    except Exception:
        return token


def _find_token(deps: AssistantDeps, provider: str, namespace: str, repo: str) -> str | None:
    """Best-effort access token from a binding on a project the user can access."""
    from pi_dash.db.models import GitRepositoryBinding, GithubRepositorySync

    binding = (
        GitRepositoryBinding.objects.filter(
            project__in=_scoping.member_projects(deps),
            repository__provider=provider,
            repository__namespace__iexact=namespace,
            repository__name__iexact=repo,
        )
        .select_related("provider_account")
        .order_by("-created_at")
        .first()
    )
    if binding is not None:
        config = binding.provider_account.credential_config or {}
        return _maybe_decrypt(config.get("token"))

    if provider != "github":
        return None
    sync = (
        GithubRepositorySync.objects.filter(
            project__in=_scoping.member_projects(deps),
            repository__owner__iexact=namespace,
            repository__name__iexact=repo,
        )
        .order_by("-created_at")
        .first()
    )
    if sync is None:
        return None
    creds = sync.credentials or {}
    token = creds.get("access_token") or creds.get("token")
    return _maybe_decrypt(token)


@assistant.tool
def get_pull_request_status(ctx: RunContext[AssistantDeps], url: str) -> dict:
    """Check whether a GitHub pull request or GitLab merge request is merged.

    Pass a full PR/MR URL. Returns ``{"state": "merged"|"open"|"closed"|"unknown", ...}``.
    An unsupported URL, rate limit, or network error returns ``unknown``.
    """
    deps = ctx.deps

    # Per-run budget — protects the unauthenticated GitHub rate limit.
    cap = int(getattr(settings, "LOOP_PR_LOOKUPS_PER_RUN", 15))
    if deps.budget.pr_lookups >= cap:
        return _unknown("budget_exhausted")
    deps.budget.pr_lookups += 1

    parsed = parse_code_review_url((url or "").strip())
    if parsed is None:
        return _unknown("unsupported_url")

    if parsed.provider == "github":
        api_url = f"{_API_HOST}/repos/{parsed.namespace}/{parsed.repo_name}/pulls/{parsed.external_iid}"
    elif parsed.provider == "gitlab":
        project_path = quote(f"{parsed.namespace}/{parsed.repo_name}", safe="")
        api_url = f"{parsed.host_url}/api/v4/projects/{project_path}/merge_requests/{parsed.external_iid}"
    else:
        return _unknown("unsupported_url")

    if ssrf.is_blocked(api_url):
        return _unknown("blocked")

    headers = {"Accept": "application/json"}
    token = _find_token(deps, parsed.provider, parsed.namespace, parsed.repo_name)
    if token and parsed.provider == "github":
        headers["X-GitHub-Api-Version"] = "2022-11-28"
        headers["Authorization"] = f"Bearer {token}"
    elif token and parsed.provider == "gitlab":
        headers["PRIVATE-TOKEN"] = token

    import httpx

    try:
        with httpx.Client(timeout=_TIMEOUT_S, follow_redirects=False) as client:
            resp = client.get(api_url, headers=headers)
    except Exception:  # noqa: BLE001 — any transport failure is just "unknown"
        logger.warning(
            "get_pull_request_status: network error for %s:%s/%s#%s",
            parsed.provider,
            parsed.namespace,
            parsed.repo_name,
            parsed.external_iid,
        )
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

    provider_state = data.get("state")
    if data.get("merged") or data.get("merged_at") or provider_state == "merged":
        state = "merged"
    elif provider_state == "closed":
        state = "closed"
    else:
        state = "open"
    return {
        "state": state,
        "title": data.get("title"),
        "merged_at": data.get("merged_at"),
        "url": parsed.url,
        "provider": parsed.provider,
    }
