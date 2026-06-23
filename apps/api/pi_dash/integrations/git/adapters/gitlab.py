# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

import json
import re
from collections.abc import Iterable, Mapping
from datetime import datetime
from urllib.parse import quote, urlparse

import requests
from django.conf import settings

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

DEFAULT_TIMEOUT_SECONDS = 30


def _normalize_host(host: str) -> str:
    host = (host or "").strip().rstrip("/")
    if not host:
        return "https://gitlab.com"
    if not host.startswith(("http://", "https://")):
        host = f"https://{host}"
    return host.rstrip("/")


def _allowed_hosts() -> set[str]:
    configured = {"https://gitlab.com"}
    gitlab_host = getattr(settings, "GITLAB_HOST", "") or ""
    if gitlab_host:
        configured.add(_normalize_host(gitlab_host))

    raw_allowed_hosts = getattr(settings, "GITLAB_ALLOWED_HOSTS", []) or []
    if isinstance(raw_allowed_hosts, str):
        raw_allowed_hosts = raw_allowed_hosts.split(",")
    for host in raw_allowed_hosts:
        if not host:
            continue
        configured.add(_normalize_host(host))
    return configured


def _parse_dt(value: str | None) -> datetime | None:
    if not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def _strip_git_suffix(value: str) -> str:
    return value[:-4] if value.endswith(".git") else value


def _split_full_path(path: str) -> tuple[str, str] | None:
    path = _strip_git_suffix(path.strip("/"))
    if not path or "/" not in path:
        return None
    namespace, name = path.rsplit("/", 1)
    return namespace, name


class GitLabClient:
    def __init__(self, token: str, *, host_url: str, timeout: int = DEFAULT_TIMEOUT_SECONDS):
        if not token:
            raise GitProviderAuthError("GitLab token is missing")
        self._token = token
        self._host_url = _normalize_host(host_url)
        self._api_base = f"{self._host_url}/api/v4"
        self._timeout = timeout

    def _headers(self) -> dict:
        return {
            "PRIVATE-TOKEN": self._token,
            "Accept": "application/json",
            "User-Agent": "pi-dash-gitlab-sync",
        }

    def _request(self, method: str, path: str, **kwargs) -> requests.Response:
        kwargs.setdefault("timeout", self._timeout)
        url = path if path.startswith("http") else f"{self._api_base}{path}"
        response = requests.request(method, url, headers=self._headers(), **kwargs)
        if response.status_code == 401:
            raise GitProviderAuthError(response.text)
        if response.status_code == 403:
            raise GitProviderPermissionError(response.text)
        if response.status_code == 404:
            raise GitProviderNotFoundError(response.text)
        response.raise_for_status()
        return response

    @staticmethod
    def _has_next(response: requests.Response) -> bool:
        return bool(response.headers.get("X-Next-Page"))

    def _paginate(self, path: str, params: dict | None = None) -> Iterable[dict]:
        params = dict(params or {})
        params.setdefault("per_page", 100)
        params.setdefault("page", 1)
        while True:
            response = self._request("GET", path, params=params)
            yield from response.json()
            next_page = response.headers.get("X-Next-Page")
            if not next_page:
                break
            params["page"] = next_page

    def get_authenticated_user(self) -> dict:
        return self._request("GET", "/user").json()

    def list_projects(self, *, page: int = 1, per_page: int = 100) -> tuple[list[dict], bool]:
        response = self._request(
            "GET",
            "/projects",
            params={
                "membership": True,
                "simple": True,
                "order_by": "last_activity_at",
                "sort": "desc",
                "page": page,
                "per_page": per_page,
            },
        )
        return response.json(), self._has_next(response)

    def get_project(self, full_path_or_id: str) -> dict:
        encoded = quote(str(full_path_or_id), safe="")
        return self._request("GET", f"/projects/{encoded}").json()

    def list_open_issues(self, project_id: str) -> Iterable[dict]:
        return self._paginate(
            f"/projects/{quote(str(project_id), safe='')}/issues",
            {"state": "opened", "order_by": "updated_at", "sort": "desc"},
        )

    def list_issue_notes(self, project_id: str, issue_iid: str) -> Iterable[dict]:
        return self._paginate(
            f"/projects/{quote(str(project_id), safe='')}/issues/{issue_iid}/notes",
            {"order_by": "updated_at", "sort": "asc"},
        )

    def post_issue_note(self, project_id: str, issue_iid: str, body: str) -> dict:
        return self._request(
            "POST",
            f"/projects/{quote(str(project_id), safe='')}/issues/{issue_iid}/notes",
            data={"body": body},
        ).json()

    def get_merge_request(self, project_id: str, mr_iid: str) -> dict:
        return self._request(
            "GET",
            f"/projects/{quote(str(project_id), safe='')}/merge_requests/{mr_iid}",
        ).json()


class GitLabAdapter:
    key = "gitlab"
    display_name = "GitLab"
    code_review_term = "merge request"

    _ssh_repo_re = re.compile(r"^git@(?P<host>[^:]+):(?P<path>.+?)(?:\.git)?$")

    def _token(self, credential: dict) -> str:
        token = credential.get("token") or ""
        if token:
            try:
                return decrypt_data(token) or token
            except Exception:
                return token
        raise GitProviderAuthError("GitLab token is missing")

    def _client(self, credential: dict) -> GitLabClient:
        host_url = credential.get("host_url") or getattr(settings, "GITLAB_HOST", "") or "https://gitlab.com"
        normalized_host_url = _normalize_host(host_url)
        if not self._host_allowed(normalized_host_url):
            raise GitProviderPermissionError(
                "GitLab host is not allowed; ask an instance admin to add it to GITLAB_ALLOWED_HOSTS."
            )
        return GitLabClient(self._token(credential), host_url=normalized_host_url)

    def _host_allowed(self, host_url: str) -> bool:
        return _normalize_host(host_url) in _allowed_hosts()

    def parse_repo_url(self, url: str) -> ParsedRepository | None:
        candidate = (url or "").strip()
        if not candidate:
            return None
        ssh = self._ssh_repo_re.match(candidate)
        if ssh:
            host_url = _normalize_host(ssh.group("host"))
            if not self._host_allowed(host_url):
                return None
            split = _split_full_path(ssh.group("path"))
            if split is None:
                return None
            namespace, name = split
            return ParsedRepository(
                provider=self.key,
                host_url=host_url,
                namespace=namespace,
                name=name,
                full_name=f"{namespace}/{name}",
                clone_url=candidate,
            )

        parsed = urlparse(candidate)
        host_url = _normalize_host(parsed.netloc)
        if parsed.scheme not in {"http", "https", "ssh"} or not self._host_allowed(host_url):
            return None
        path = parsed.path
        if "/-/" in path:
            path = path.split("/-/", 1)[0]
        split = _split_full_path(path)
        if split is None:
            return None
        namespace, name = split
        return ParsedRepository(
            provider=self.key,
            host_url=host_url,
            namespace=namespace,
            name=name,
            full_name=f"{namespace}/{name}",
            clone_url=candidate,
        )

    def parse_code_review_url(self, url: str) -> ParsedCodeReview | None:
        candidate = (url or "").strip()
        parsed = urlparse(candidate)
        host_url = _normalize_host(parsed.netloc)
        if parsed.scheme not in {"http", "https"} or not self._host_allowed(host_url):
            return None
        marker = "/-/merge_requests/"
        if marker not in parsed.path:
            return None
        repo_path, mr_part = parsed.path.split(marker, 1)
        mr_iid = mr_part.strip("/").split("/", 1)[0]
        if not mr_iid.isdigit():
            return None
        split = _split_full_path(repo_path)
        if split is None:
            return None
        namespace, name = split
        return ParsedCodeReview(
            provider=self.key,
            host_url=host_url,
            namespace=namespace,
            repo_name=name,
            external_iid=mr_iid,
            url=f"{host_url}/{namespace}/{name}/-/merge_requests/{mr_iid}",
        )

    def verify_provider_account(self, credential: dict) -> dict:
        return self._client(credential).get_authenticated_user()

    def credential_capabilities(self, credential: dict) -> GitProviderCapabilities:
        return GitProviderCapabilities(
            read_repositories=True,
            read_issues=True,
            write_comments=True,
            manage_webhooks=True,
            clone=False,
        )

    def _remote_repo(self, payload: dict) -> RemoteRepository:
        full_name = payload.get("path_with_namespace") or payload.get("path") or ""
        namespace, name = _split_full_path(full_name) or ("", payload.get("path") or "")
        return RemoteRepository(
            provider=self.key,
            external_id=str(payload.get("id") or ""),
            namespace=namespace,
            name=name,
            full_name=full_name,
            web_url=payload.get("web_url") or "",
            clone_url_http=payload.get("http_url_to_repo") or "",
            clone_url_ssh=payload.get("ssh_url_to_repo") or "",
            default_branch=payload.get("default_branch") or "",
            is_private=payload.get("visibility") == "private",
            metadata=payload,
        )

    def list_repositories(self, credential: dict, page: int = 1) -> RepositoryPage:
        repos, has_next = self._client(credential).list_projects(page=page)
        return RepositoryPage(
            repositories=[self._remote_repo(repo) for repo in repos],
            page=page,
            has_next_page=has_next,
        )

    def get_repository(self, credential: dict, parsed: ParsedRepository) -> RemoteRepository:
        return self._remote_repo(self._client(credential).get_project(parsed.full_name))

    def list_open_issues(self, credential: dict, repository: RemoteRepository) -> Iterable[RemoteIssue]:
        project_id = repository.external_id or repository.full_name
        for issue in self._client(credential).list_open_issues(project_id):
            author = issue.get("author") or {}
            yield RemoteIssue(
                external_id=str(issue.get("id") or ""),
                external_iid=str(issue.get("iid") or ""),
                title=issue.get("title") or "",
                body=issue.get("description") or "",
                state=issue.get("state") or "",
                author=author.get("username") or "",
                web_url=issue.get("web_url") or "",
                created_at=_parse_dt(issue.get("created_at")),
                updated_at=_parse_dt(issue.get("updated_at")),
                metadata=issue,
            )

    def list_issue_comments(
        self,
        credential: dict,
        repository: RemoteRepository,
        issue_iid: str,
    ) -> Iterable[RemoteComment]:
        project_id = repository.external_id or repository.full_name
        for note in self._client(credential).list_issue_notes(project_id, issue_iid):
            if note.get("system"):
                continue
            author = note.get("author") or {}
            yield RemoteComment(
                external_id=str(note.get("id") or ""),
                body=note.get("body") or "",
                author=author.get("username") or "",
                web_url="",
                created_at=_parse_dt(note.get("created_at")),
                updated_at=_parse_dt(note.get("updated_at")),
                metadata=note,
            )

    def post_issue_comment(
        self,
        credential: dict,
        repository: RemoteRepository,
        issue_iid: str,
        body: str,
    ) -> RemoteComment:
        project_id = repository.external_id or repository.full_name
        note = self._client(credential).post_issue_note(project_id, issue_iid, body)
        author = note.get("author") or {}
        return RemoteComment(
            external_id=str(note.get("id") or ""),
            body=note.get("body") or "",
            author=author.get("username") or "",
            created_at=_parse_dt(note.get("created_at")),
            updated_at=_parse_dt(note.get("updated_at")),
            metadata=note,
        )

    def get_code_review(self, credential: dict, parsed: ParsedCodeReview) -> RemoteCodeReview:
        project_key = f"{parsed.namespace}/{parsed.repo_name}"
        mr = self._client(credential).get_merge_request(project_key, parsed.external_iid)
        state = mr.get("state") or "opened"
        merged = state == "merged"
        normalized_state = "merged" if merged else ("closed" if state == "closed" else "open")
        draft = bool(mr.get("draft")) or (mr.get("work_in_progress") is True)
        return RemoteCodeReview(
            external_id=str(mr.get("id") or ""),
            external_iid=str(mr.get("iid") or parsed.external_iid),
            title=mr.get("title") or "",
            state=normalized_state,
            merged=merged,
            draft=draft,
            web_url=mr.get("web_url") or parsed.url,
            updated_at=_parse_dt(mr.get("updated_at")),
            metadata=mr,
        )

    def normalize_webhook(self, raw_body: bytes, headers: Mapping[str, str]) -> ProviderWebhookEvent:
        payload = json.loads(raw_body.decode("utf-8") or "{}")
        event = headers.get("X-Gitlab-Event") or headers.get("X-GitLab-Event") or ""
        attrs = payload.get("object_attributes") or {}
        return ProviderWebhookEvent(
            provider=self.key,
            event=event,
            action=attrs.get("action") or payload.get("event_type") or "",
            repository_ref=payload.get("project") or {},
            code_review_ref=attrs if "merge_request" in event.lower() else {},
            issue_ref=attrs if "issue" in event.lower() else {},
            payload=payload,
        )
