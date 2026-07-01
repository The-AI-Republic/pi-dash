# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from typing import Any


@dataclass(frozen=True)
class ParsedRepository:
    provider: str
    host_url: str
    namespace: str
    name: str
    full_name: str
    clone_url: str


@dataclass(frozen=True)
class ParsedCodeReview:
    provider: str
    host_url: str
    namespace: str
    repo_name: str
    external_iid: str
    url: str


@dataclass
class RemoteRepository:
    provider: str
    external_id: str
    namespace: str
    name: str
    full_name: str
    web_url: str
    clone_url_http: str = ""
    clone_url_ssh: str = ""
    default_branch: str = ""
    is_private: bool = False
    metadata: dict[str, Any] = field(default_factory=dict)


@dataclass
class GitProviderCapabilities:
    read_repositories: bool = False
    read_issues: bool = False
    write_comments: bool = False
    manage_webhooks: bool = False
    clone: bool = False

    def as_dict(self) -> dict[str, bool]:
        return {
            "read_repositories": self.read_repositories,
            "read_issues": self.read_issues,
            "write_comments": self.write_comments,
            "manage_webhooks": self.manage_webhooks,
            "clone": self.clone,
        }


@dataclass
class RemoteIssue:
    external_id: str
    external_iid: str
    title: str
    body: str
    state: str
    author: str
    web_url: str
    created_at: datetime | None = None
    updated_at: datetime | None = None
    metadata: dict[str, Any] = field(default_factory=dict)


@dataclass
class RemoteComment:
    external_id: str
    body: str
    author: str
    web_url: str = ""
    created_at: datetime | None = None
    updated_at: datetime | None = None
    metadata: dict[str, Any] = field(default_factory=dict)


@dataclass
class RemoteCodeReview:
    external_id: str
    external_iid: str
    title: str
    state: str
    merged: bool
    draft: bool
    web_url: str
    updated_at: datetime | None = None
    metadata: dict[str, Any] = field(default_factory=dict)


@dataclass
class ProviderWebhookEvent:
    provider: str
    event: str
    action: str
    repository_ref: dict[str, Any] = field(default_factory=dict)
    code_review_ref: dict[str, Any] = field(default_factory=dict)
    issue_ref: dict[str, Any] = field(default_factory=dict)
    payload: dict[str, Any] = field(default_factory=dict)


@dataclass
class RepositoryPage:
    repositories: list[RemoteRepository]
    page: int
    has_next_page: bool
