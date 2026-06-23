# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.integrations.git.registry import parse_code_review_url, parse_repository_url


pytestmark = pytest.mark.unit


def test_parse_github_repository_and_pull_request():
    repo = parse_repository_url("https://github.com/Acme/Web.git")
    assert repo.provider == "github"
    assert repo.host_url == "https://github.com"
    assert repo.namespace == "acme"
    assert repo.name == "web"
    assert repo.full_name == "acme/web"

    review = parse_code_review_url("https://github.com/Acme/Web/pull/42")
    assert review.provider == "github"
    assert review.namespace == "acme"
    assert review.repo_name == "web"
    assert review.external_iid == "42"
    assert review.url == "https://github.com/acme/web/pull/42"


def test_parse_gitlab_nested_repository_and_merge_request(settings):
    settings.GITLAB_ALLOWED_HOSTS = ["https://gitlab.example.com"]

    repo = parse_repository_url("git@gitlab.example.com:platform/backend/api.git")
    assert repo.provider == "gitlab"
    assert repo.host_url == "https://gitlab.example.com"
    assert repo.namespace == "platform/backend"
    assert repo.name == "api"
    assert repo.full_name == "platform/backend/api"

    review = parse_code_review_url("https://gitlab.example.com/platform/backend/api/-/merge_requests/17")
    assert review.provider == "gitlab"
    assert review.host_url == "https://gitlab.example.com"
    assert review.namespace == "platform/backend"
    assert review.repo_name == "api"
    assert review.external_iid == "17"


def test_gitlab_rejects_unconfigured_hosts(settings):
    settings.GITLAB_ALLOWED_HOSTS = []
    settings.GITLAB_HOST = "https://gitlab.com"

    assert parse_repository_url("https://evil.example.com/acme/web") is None
    assert parse_code_review_url("https://evil.example.com/acme/web/-/merge_requests/1") is None
