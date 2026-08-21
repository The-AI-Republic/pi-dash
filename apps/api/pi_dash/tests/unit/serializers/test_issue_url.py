# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from django.test import override_settings

from pi_dash.api.serializers.issue import IssueCommentSerializer, IssueSerializer
from pi_dash.db.models import (
    Issue,
    IssueComment,
    Project,
    User,
    Workspace,
)


@pytest.fixture
def issue(db):
    user = User.objects.create(email="url_test@example.com", first_name="URL", last_name="Test")
    workspace = Workspace.objects.create(name="Eng", slug="eng", owner=user)
    project = Project.objects.create(name="Engine", identifier="eng", workspace=workspace)
    return Issue.objects.create(name="Add a url field", workspace=workspace, project=project)


@pytest.mark.unit
class TestIssueSerializerUrl:
    @override_settings(WEB_URL="https://pi-dash.example.com", APP_BASE_URL=None)
    def test_url_present_and_absolute(self, issue):
        data = IssueSerializer(issue).data
        assert data["url"] == f"https://pi-dash.example.com/eng/browse/ENG-{issue.sequence_id}"

    @override_settings(WEB_URL=None, APP_BASE_URL=None)
    def test_url_omitted_when_base_unset(self, issue):
        data = IssueSerializer(issue).data
        assert "url" not in data


@pytest.mark.unit
class TestIssueCommentSerializerUrl:
    @override_settings(WEB_URL="https://pi-dash.example.com", APP_BASE_URL=None)
    def test_url_links_to_issue(self, issue):
        comment = IssueComment.objects.create(
            issue=issue,
            project=issue.project,
            workspace=issue.workspace,
            comment_html="<p>hi</p>",
        )
        data = IssueCommentSerializer(comment).data
        assert data["url"] == f"https://pi-dash.example.com/eng/browse/ENG-{issue.sequence_id}"

    @override_settings(WEB_URL=None, APP_BASE_URL=None)
    def test_url_omitted_when_base_unset(self, issue):
        comment = IssueComment.objects.create(
            issue=issue,
            project=issue.project,
            workspace=issue.workspace,
            comment_html="<p>hi</p>",
        )
        data = IssueCommentSerializer(comment).data
        assert "url" not in data
