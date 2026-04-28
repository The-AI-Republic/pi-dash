# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for the GithubClient HTTP wrapper.

Covers Link-header parsing for pagination, the issue-URL parser used in
comment-parent resolution, and the error-class mapping. Real HTTP plumbing
is mocked via `requests` patches.
"""

from unittest.mock import MagicMock, patch

import pytest

from pi_dash.utils.github_client import (
    GithubAuthError,
    GithubClient,
    GithubNotFoundError,
    GithubPermissionError,
    parse_issue_number_from_url,
)


@pytest.mark.unit
class TestParseIssueNumber:
    def test_valid_url(self):
        url = "https://api.github.com/repos/owner/repo/issues/42"
        assert parse_issue_number_from_url(url) == 42

    def test_high_number(self):
        url = "https://api.github.com/repos/o/r/issues/123456"
        assert parse_issue_number_from_url(url) == 123456

    def test_pull_request_url_does_not_match(self):
        # PR comment URLs use /pulls/, not /issues/. Filtering them out is
        # essential — a comment with a /pulls/ url would otherwise resolve
        # to a number that has no local Issue.
        url = "https://api.github.com/repos/o/r/pulls/42"
        assert parse_issue_number_from_url(url) is None

    def test_empty_string_returns_none(self):
        assert parse_issue_number_from_url("") is None

    def test_no_match_returns_none(self):
        assert parse_issue_number_from_url("not-a-url") is None


@pytest.mark.unit
class TestNextUrl:
    def test_extracts_next_link(self):
        response = MagicMock()
        response.headers = {
            "Link": (
                '<https://api.github.com/repositories/1/issues?page=2>; rel="next", '
                '<https://api.github.com/repositories/1/issues?page=5>; rel="last"'
            )
        }
        assert GithubClient._next_url(response) == "https://api.github.com/repositories/1/issues?page=2"

    def test_no_link_header(self):
        response = MagicMock()
        response.headers = {}
        assert GithubClient._next_url(response) is None

    def test_only_prev_and_last(self):
        response = MagicMock()
        response.headers = {
            "Link": (
                '<https://api.github.com/x?page=1>; rel="prev", '
                '<https://api.github.com/x?page=3>; rel="last"'
            )
        }
        assert GithubClient._next_url(response) is None


@pytest.mark.unit
class TestErrorMapping:
    def _make_response(self, status_code: int):
        response = MagicMock()
        response.status_code = status_code
        response.headers = {}
        response.text = "boom"
        response.raise_for_status = MagicMock()
        return response

    @patch("pi_dash.utils.github_client.requests.request")
    def test_401_raises_auth_error(self, mock_request):
        mock_request.return_value = self._make_response(401)
        client = GithubClient(token="t")
        with pytest.raises(GithubAuthError):
            client.get_authenticated_user()

    @patch("pi_dash.utils.github_client.requests.request")
    def test_403_raises_permission_error(self, mock_request):
        mock_request.return_value = self._make_response(403)
        client = GithubClient(token="t")
        with pytest.raises(GithubPermissionError):
            client.get_authenticated_user()

    @patch("pi_dash.utils.github_client.requests.request")
    def test_404_raises_not_found(self, mock_request):
        mock_request.return_value = self._make_response(404)
        client = GithubClient(token="t")
        with pytest.raises(GithubNotFoundError):
            client.get_repo("o", "r")

    def test_empty_token_raises(self):
        with pytest.raises(GithubAuthError):
            GithubClient(token="")
