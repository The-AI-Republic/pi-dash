# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``relations_summary`` / ``has_open_blockers`` on issue read payloads
(PDASHOSS01-197)."""

import pytest

from pi_dash.api.serializers.issue import IssueSerializer as ApiIssueSerializer
from pi_dash.app.serializers.issue import IssueDetailSerializer
from pi_dash.db.models import Issue, IssueRelation, State


@pytest.fixture
def issues(workspace, project, create_user):
    todo = State.objects.create(name="Todo", project=project, group="unstarted")
    done = State.objects.create(name="Done", project=project, group="completed")

    def make(name, state):
        return Issue.objects.create(
            name=name, workspace=workspace, project=project, state=state, created_by=create_user
        )

    handler, model, query, downstream = (
        make("Handler", todo),
        make("Model", todo),
        make("Query", done),
        make("Downstream", todo),
    )
    for dependent, blocker in ((handler, model), (handler, query), (downstream, handler)):
        IssueRelation.objects.create(
            issue=dependent,
            related_issue=blocker,
            relation_type="blocked_by",
            project=project,
            workspace=workspace,
        )
    return {"handler": handler, "model": model, "query": query, "downstream": downstream}


def _ident(issue):
    return f"{issue.project.identifier}-{issue.sequence_id}"


def _expected(issues):
    return {
        "blocked_by": [
            {"identifier": _ident(issues["model"]), "state": "Todo", "state_group": "unstarted"},
            {"identifier": _ident(issues["query"]), "state": "Done", "state_group": "completed"},
        ],
        "blocking": [
            {"identifier": _ident(issues["downstream"]), "state": "Todo", "state_group": "unstarted"},
        ],
    }


@pytest.mark.unit
class TestAppIssueDetailSerializer:
    def test_carries_summary_and_open_flag(self, issues):
        data = IssueDetailSerializer(issues["handler"]).data
        assert data["relations_summary"] == _expected(issues)
        assert data["has_open_blockers"] is True

    def test_open_flag_tracks_each_issues_own_blockers(self, issues):
        data = IssueDetailSerializer(issues["downstream"]).data
        # Handler (its only blocker) is still open.
        assert data["has_open_blockers"] is True
        data = IssueDetailSerializer(issues["model"]).data
        assert data["relations_summary"]["blocked_by"] == []
        assert data["has_open_blockers"] is False


@pytest.mark.unit
class TestApiIssueSerializer:
    def test_single_item_carries_summary(self, issues):
        data = ApiIssueSerializer(issues["handler"]).data
        assert data["relations_summary"] == _expected(issues)
        assert data["has_open_blockers"] is True

    def test_resolved_only_blockers_report_false(self, issues):
        issues["model"].state = issues["query"].state
        issues["model"].save()
        data = ApiIssueSerializer(issues["handler"]).data
        assert data["has_open_blockers"] is False
        assert [b["state_group"] for b in data["relations_summary"]["blocked_by"]] == ["completed", "completed"]

    def test_list_payload_omits_summary(self, issues):
        rows = ApiIssueSerializer(list(issues.values()), many=True).data
        assert rows and all("relations_summary" not in r and "has_open_blockers" not in r for r in rows)

    def test_fields_filter_is_honoured(self, issues):
        data = ApiIssueSerializer(issues["handler"], fields=["id", "name"]).data
        assert "relations_summary" not in data and "has_open_blockers" not in data
        data = ApiIssueSerializer(issues["handler"], fields=["id", "has_open_blockers"]).data
        assert data["has_open_blockers"] is True and "relations_summary" not in data
