# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for the merged issue history read.

``GET .../issues/{issue_id}/history/`` without ``activity_type`` used to sort
the querysets of ``IssueActivity`` and ``IssueComment`` model rows with a key
that subscripts the row (``instance["created_at"]``), so the first element
raised ``TypeError: 'IssueActivity' object is not subscriptable`` and every
merged read returned HTTP 500. The endpoint now serializes each side with its
existing serializer and merges the resulting plain dicts, which is what these
tests pin: one ascending list, rows shaped exactly like the split reads, and
``created_at__gt`` still filtering both halves.

The read path needs a real Postgres database (the standard ``contract``
surface, same as ``test_issue_search.py``).
"""

from datetime import timedelta
from urllib.parse import quote

import pytest
from django.utils import timezone
from rest_framework import status as http_status

from pi_dash.db.models import (
    Issue,
    IssueActivity,
    IssueComment,
    Project,
    ProjectMember,
)


@pytest.fixture
def history_project(db, workspace, create_user):
    project = Project.objects.create(
        name="History Test Project",
        identifier="HIST",
        workspace=workspace,
        created_by=create_user,
    )
    ProjectMember.objects.create(
        project=project,
        member=create_user,
        role=20,
        is_active=True,
    )
    return project


@pytest.fixture
def history_issue(history_project, create_user):
    return Issue.objects.create(
        name="history target",
        description_html="<p>body</p>",
        description_stripped="body",
        project=history_project,
        workspace=history_project.workspace,
        created_by=create_user,
    )


def _history_url(workspace, project, issue, query=""):
    return f"/api/workspaces/{workspace.slug}/projects/{project.id}/issues/{issue.id}/history/{query}"


def _create_activity(project, issue, actor, *, field, old_value, new_value, created_at):
    row = IssueActivity.objects.create(
        issue=issue,
        project=project,
        workspace=project.workspace,
        actor=actor,
        verb="updated",
        field=field,
        old_value=old_value,
        new_value=new_value,
    )
    IssueActivity.objects.filter(pk=row.pk).update(created_at=created_at)
    return row


def _create_comment(project, issue, actor, *, body, created_at):
    row = IssueComment.objects.create(
        issue=issue,
        project=project,
        workspace=project.workspace,
        actor=actor,
        comment_html=f"<p>{body}</p>",
        comment_stripped=body,
    )
    IssueComment.objects.filter(pk=row.pk).update(created_at=created_at)
    return row


@pytest.fixture
def seeded_history(history_project, history_issue, create_user):
    """A creation entry, a comment, a state update, and a later comment.

    The timestamps deliberately interleave the two kinds so a merge that only
    concatenates the querysets (properties first, then comments) fails the
    ordering assertion.
    """
    base = timezone.now() - timedelta(minutes=10)
    creation = _create_activity(
        history_project,
        history_issue,
        create_user,
        field=None,
        old_value=None,
        new_value=None,
        created_at=base,
    )
    first_comment = _create_comment(
        history_project,
        history_issue,
        create_user,
        body="first comment",
        created_at=base + timedelta(minutes=1),
    )
    state_update = _create_activity(
        history_project,
        history_issue,
        create_user,
        field="state",
        old_value="Todo",
        new_value="In Progress",
        created_at=base + timedelta(minutes=2),
    )
    second_comment = _create_comment(
        history_project,
        history_issue,
        create_user,
        body="second comment",
        created_at=base + timedelta(minutes=3),
    )
    return {
        "base": base,
        "activities": [creation, state_update],
        "comments": [first_comment, second_comment],
    }


@pytest.mark.contract
@pytest.mark.django_db
class TestIssueHistoryMergedRead:
    def test_merged_read_returns_both_kinds_in_chronological_order(
        self, session_client, workspace, history_project, history_issue, seeded_history
    ):
        response = session_client.get(_history_url(workspace, history_project, history_issue))

        assert response.status_code == http_status.HTTP_200_OK
        payload = response.json()
        assert len(payload) == 4

        created_at_values = [row["created_at"] for row in payload]
        assert created_at_values == sorted(created_at_values)

        activity_ids = {str(row.id) for row in seeded_history["activities"]}
        comment_ids = {str(row.id) for row in seeded_history["comments"]}
        returned_ids = [row["id"] for row in payload]
        assert set(returned_ids) == activity_ids | comment_ids

        # The property rows and the comment rows are genuinely interleaved.
        kinds = ["activity" if row["id"] in activity_ids else "comment" for row in payload]
        assert kinds == ["activity", "comment", "activity", "comment"]

        for row in payload:
            assert isinstance(row["id"], str)
            assert isinstance(row["created_at"], str)
            assert row["actor_detail"]["id"] == str(seeded_history["activities"][0].actor_id)

    def test_merged_rows_match_the_split_read_shapes(
        self, session_client, workspace, history_project, history_issue, seeded_history
    ):
        props = session_client.get(
            _history_url(workspace, history_project, history_issue, "?activity_type=issue-property")
        )
        comments = session_client.get(
            _history_url(workspace, history_project, history_issue, "?activity_type=issue-comment")
        )
        merged = session_client.get(_history_url(workspace, history_project, history_issue))

        assert props.status_code == http_status.HTTP_200_OK
        assert comments.status_code == http_status.HTTP_200_OK
        assert merged.status_code == http_status.HTTP_200_OK

        merged_by_id = {row["id"]: row for row in merged.json()}
        for row in props.json():
            assert merged_by_id[row["id"]] == row
        for row in comments.json():
            assert merged_by_id[row["id"]] == row

    def test_merged_read_honours_created_at_gt_on_both_halves(
        self, session_client, workspace, history_project, history_issue, seeded_history
    ):
        cutoff = seeded_history["base"] + timedelta(minutes=1, seconds=30)
        response = session_client.get(
            _history_url(
                workspace,
                history_project,
                history_issue,
                f"?created_at__gt={quote(cutoff.isoformat())}",
            )
        )

        assert response.status_code == http_status.HTTP_200_OK
        returned_ids = {row["id"] for row in response.json()}
        assert returned_ids == {
            str(seeded_history["activities"][1].id),
            str(seeded_history["comments"][1].id),
        }

    def test_merged_read_with_no_entries_returns_an_empty_list(
        self, session_client, workspace, history_project, create_user
    ):
        empty_issue = Issue.objects.create(
            name="no activity",
            project=history_project,
            workspace=history_project.workspace,
            created_by=create_user,
        )

        response = session_client.get(_history_url(workspace, history_project, empty_issue))

        assert response.status_code == http_status.HTTP_200_OK
        assert response.json() == []
