# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

import json
from collections.abc import Iterable, Mapping
from datetime import datetime

from pi_dash.integrations.git.adapters.base import (
    GitProviderAuthError,
    GitProviderNotFoundError,
    GitProviderPermissionError,
)
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
from pi_dash.license.utils.encryption import decrypt_data
from pi_dash.utils.github_client import (
    GithubAuthError,
    GithubClient,
    GithubNotFoundError,
    GithubPermissionError,
    parse_github_pull_request_url,
    parse_github_repo_url,
    parse_issue_number_from_url,
    pr_snapshot_from_payload,
)

GITHUB_HOST = "https://github.com"


def _parse_dt(value: str | None) -> datetime | None:
    if not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


class GitHubAdapter:
    key = "github"
    display_name = "GitHub"
    code_review_term = "pull request"

    def parse_repo_url(self, url: str) -> ParsedRepository | None:
        parsed = parse_github_repo_url(url)
        if parsed is None:
            return None
        owner, name = parsed
        owner = owner.lower()
        name = name.lower()
        return ParsedRepository(
            provider=self.key,
            host_url=GITHUB_HOST,
            namespace=owner,
            name=name,
            full_name=f"{owner}/{name}",
            clone_url=url.strip(),
        )

    def parse_code_review_url(self, url: str) -> ParsedCodeReview | None:
        parsed = parse_github_pull_request_url(url)
        if parsed is None:
            return None
        owner, name, number = parsed
        owner = owner.lower()
        name = name.lower()
        return ParsedCodeReview(
            provider=self.key,
            host_url=GITHUB_HOST,
            namespace=owner,
            repo_name=name,
            external_iid=str(number),
            url=f"{GITHUB_HOST}/{owner}/{name}/pull/{number}",
        )

    def _token(self, credential: dict) -> str:
        token = credential.get("token") or ""
        if token:
            try:
                return decrypt_data(token)
            except Exception:
                return token
        raise GitProviderAuthError("GitHub token is missing")

    def _client(self, credential: dict) -> GithubClient:
        if credential.get("auth_type") == "github_app" and credential.get("installation_id"):
            return GithubClient.for_installation(int(credential["installation_id"]))
        return GithubClient(token=self._token(credential))

    def _map_error(self, exc: Exception) -> Exception:
        if isinstance(exc, GithubAuthError):
            return GitProviderAuthError(str(exc))
        if isinstance(exc, GithubPermissionError):
            return GitProviderPermissionError(str(exc))
        if isinstance(exc, GithubNotFoundError):
            return GitProviderNotFoundError(str(exc))
        return exc

    def verify_provider_account(self, credential: dict) -> dict:
        try:
            return self._client(credential).get_authenticated_user()
        except Exception as exc:
            raise self._map_error(exc)

    def credential_capabilities(self, credential: dict) -> GitProviderCapabilities:
        auth_type = credential.get("auth_type") or "pat"
        return GitProviderCapabilities(
            read_repositories=True,
            read_issues=True,
            write_comments=auth_type == "pat",
            manage_webhooks=auth_type == "github_app",
            clone=False,
        )

    def _remote_repo(self, payload: dict) -> RemoteRepository:
        owner = ((payload.get("owner") or {}).get("login") or payload.get("owner") or "").lower()
        name = (payload.get("name") or "").lower()
        full_name = (payload.get("full_name") or f"{owner}/{name}").lower()
        namespace = full_name.rsplit("/", 1)[0] if "/" in full_name else owner
        return RemoteRepository(
            provider=self.key,
            external_id=str(payload.get("id") or ""),
            namespace=namespace,
            name=name,
            full_name=full_name,
            web_url=payload.get("html_url") or f"{GITHUB_HOST}/{full_name}",
            clone_url_http=payload.get("clone_url") or "",
            clone_url_ssh=payload.get("ssh_url") or "",
            default_branch=payload.get("default_branch") or "",
            is_private=bool(payload.get("private")),
            metadata=payload,
        )

    def list_repositories(self, credential: dict, page: int = 1) -> RepositoryPage:
        try:
            repos, has_next = self._client(credential).list_user_repos(page=page)
        except Exception as exc:
            raise self._map_error(exc)
        return RepositoryPage(
            repositories=[self._remote_repo(repo) for repo in repos],
            page=page,
            has_next_page=has_next,
        )

    def get_repository(self, credential: dict, parsed: ParsedRepository) -> RemoteRepository:
        try:
            return self._remote_repo(self._client(credential).get_repo(parsed.namespace, parsed.name))
        except Exception as exc:
            raise self._map_error(exc)

    def list_open_issues(self, credential: dict, repository: RemoteRepository) -> Iterable[RemoteIssue]:
        owner, name = repository.full_name.split("/", 1)
        try:
            issues = self._client(credential).list_all_open_issues(owner, name)
            for issue in issues:
                if "pull_request" in issue:
                    continue
                user = issue.get("user") or {}
                yield RemoteIssue(
                    external_id=str(issue.get("id") or ""),
                    external_iid=str(issue.get("number") or ""),
                    title=issue.get("title") or "",
                    body=issue.get("body") or "",
                    state=issue.get("state") or "",
                    author=user.get("login") or "",
                    web_url=issue.get("html_url") or "",
                    created_at=_parse_dt(issue.get("created_at")),
                    updated_at=_parse_dt(issue.get("updated_at")),
                    metadata=issue,
                )
        except Exception as exc:
            raise self._map_error(exc)

    def list_issue_comments(
        self,
        credential: dict,
        repository: RemoteRepository,
        issue_iid: str,
    ) -> Iterable[RemoteComment]:
        owner, name = repository.full_name.split("/", 1)
        try:
            for comment in self._client(credential).list_all_repo_comments(owner, name):
                if str(parse_issue_number_from_url(comment.get("issue_url") or "")) != str(issue_iid):
                    continue
                user = comment.get("user") or {}
                yield RemoteComment(
                    external_id=str(comment.get("id") or ""),
                    body=comment.get("body") or "",
                    author=user.get("login") or "",
                    web_url=comment.get("html_url") or "",
                    created_at=_parse_dt(comment.get("created_at")),
                    updated_at=_parse_dt(comment.get("updated_at")),
                    metadata=comment,
                )
        except Exception as exc:
            raise self._map_error(exc)

    def post_issue_comment(
        self,
        credential: dict,
        repository: RemoteRepository,
        issue_iid: str,
        body: str,
    ) -> RemoteComment:
        owner, name = repository.full_name.split("/", 1)
        try:
            comment = self._client(credential).post_issue_comment(owner, name, int(issue_iid), body)
        except Exception as exc:
            raise self._map_error(exc)
        user = comment.get("user") or {}
        return RemoteComment(
            external_id=str(comment.get("id") or ""),
            body=comment.get("body") or "",
            author=user.get("login") or "",
            web_url=comment.get("html_url") or "",
            created_at=_parse_dt(comment.get("created_at")),
            updated_at=_parse_dt(comment.get("updated_at")),
            metadata=comment,
        )

    def get_code_review(self, credential: dict, parsed: ParsedCodeReview) -> RemoteCodeReview:
        try:
            pr = self._client(credential).get_pull_request(parsed.namespace, parsed.repo_name, int(parsed.external_iid))
        except Exception as exc:
            raise self._map_error(exc)
        snapshot = pr_snapshot_from_payload(pr)
        state = "merged" if snapshot.get("merged") else snapshot.get("state") or "open"
        return RemoteCodeReview(
            external_id=str(pr.get("id") or ""),
            external_iid=str(pr.get("number") or parsed.external_iid),
            title=snapshot.get("title") or pr.get("title") or "",
            state=state,
            merged=bool(snapshot.get("merged")),
            draft=bool(snapshot.get("draft")),
            web_url=pr.get("html_url") or parsed.url,
            updated_at=snapshot.get("pr_updated_at"),
            metadata=pr,
        )

    def normalize_webhook(self, raw_body: bytes, headers: Mapping[str, str]) -> ProviderWebhookEvent:
        payload = json.loads(raw_body.decode("utf-8") or "{}")
        event = headers.get("X-GitHub-Event") or headers.get("x-github-event") or ""
        return ProviderWebhookEvent(
            provider=self.key,
            event=event,
            action=payload.get("action") or "",
            payload=payload,
        )
