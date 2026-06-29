# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from collections.abc import Iterable, Mapping
from typing import Protocol

from pi_dash.integrations.git.dtos import (
    GitProviderCapabilities,
    ParsedCodeReview,
    ParsedRepository,
    ProviderWebhookEvent,
    RemoteCodeReview,
    RemoteComment,
    RemoteIssue,
    RemoteRepository,
    RepositoryPage,
)


class GitProviderError(Exception):
    """Base class for provider integration failures."""


class GitProviderAuthError(GitProviderError):
    """Provider rejected the configured credential."""


class GitProviderPermissionError(GitProviderError):
    """Provider credential lacks permission for the requested action."""


class GitProviderNotFoundError(GitProviderError):
    """Provider resource was not found or is not visible to the credential."""


class GitProviderAdapter(Protocol):
    key: str
    display_name: str
    code_review_term: str

    def parse_repo_url(self, url: str) -> ParsedRepository | None:
        ...

    def parse_code_review_url(self, url: str) -> ParsedCodeReview | None:
        ...

    def verify_provider_account(self, credential: dict) -> dict:
        ...

    def credential_capabilities(self, credential: dict) -> GitProviderCapabilities:
        ...

    def list_repositories(self, credential: dict, page: int = 1) -> RepositoryPage:
        ...

    def get_repository(self, credential: dict, parsed: ParsedRepository) -> RemoteRepository:
        ...

    def list_open_issues(self, credential: dict, repository: RemoteRepository) -> Iterable[RemoteIssue]:
        ...

    def list_issue_comments(
        self,
        credential: dict,
        repository: RemoteRepository,
        issue_iid: str,
    ) -> Iterable[RemoteComment]:
        ...

    def post_issue_comment(
        self,
        credential: dict,
        repository: RemoteRepository,
        issue_iid: str,
        body: str,
    ) -> RemoteComment:
        ...

    def get_code_review(self, credential: dict, parsed: ParsedCodeReview) -> RemoteCodeReview:
        ...

    def normalize_webhook(self, raw_body: bytes, headers: Mapping[str, str]) -> ProviderWebhookEvent:
        ...
