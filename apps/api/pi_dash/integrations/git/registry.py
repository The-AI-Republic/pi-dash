# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from pi_dash.integrations.git.adapters.github import GitHubAdapter
from pi_dash.integrations.git.adapters.gitlab import GitLabAdapter
from pi_dash.integrations.git.dtos import ParsedCodeReview, ParsedRepository


_ADAPTERS = {
    GitHubAdapter.key: GitHubAdapter(),
    GitLabAdapter.key: GitLabAdapter(),
}


def get_adapter(provider: str):
    adapter = _ADAPTERS.get((provider or "").lower())
    if adapter is None:
        raise KeyError(f"Unsupported Git provider: {provider}")
    return adapter


def all_adapters():
    return list(_ADAPTERS.values())


def parse_repository_url(url: str) -> ParsedRepository | None:
    for adapter in all_adapters():
        parsed = adapter.parse_repo_url(url)
        if parsed is not None:
            return parsed
    return None


def parse_code_review_url(url: str) -> ParsedCodeReview | None:
    for adapter in all_adapters():
        parsed = adapter.parse_code_review_url(url)
        if parsed is not None:
            return parsed
    return None


def provider_payload() -> list[dict]:
    return [
        {
            "key": adapter.key,
            "display_name": adapter.display_name,
            "code_review_term": adapter.code_review_term,
        }
        for adapter in all_adapters()
    ]
