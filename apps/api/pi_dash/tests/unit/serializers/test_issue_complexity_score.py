# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""``Issue.complexity_score`` — the AI-reasoning-complexity rating field.

Rule: only integers 0..10 are accepted. ``0`` means "no score" (unrated) and
is the default; a rated issue takes 1..10. Anything outside that range is
rejected by the write serializer. How the score is derived is out of scope.
"""

import pytest
from rest_framework.exceptions import ValidationError

from pi_dash.app.serializers.issue import IssueCreateSerializer
from pi_dash.db.models import Issue, Project, State


@pytest.fixture
def project(db, workspace):
    return Project.objects.create(name="Complexity Test", identifier="CX", workspace=workspace)


@pytest.fixture
def state(db, project, workspace):
    return State.objects.create(name="Backlog", project=project, workspace=workspace, group="backlog", default=True)


@pytest.fixture
def issue(db, project, workspace, state):
    return Issue.objects.create(project=project, workspace=workspace, name="Rate me", state=state)


def _serializer(issue, value):
    return IssueCreateSerializer(
        instance=issue,
        data={"complexity_score": value},
        partial=True,
        context={"project_id": str(issue.project_id), "workspace_id": str(issue.workspace_id)},
    )


@pytest.mark.unit
@pytest.mark.django_db
class TestComplexityScore:
    def test_default_is_zero(self, issue):
        # A freshly created issue is unrated.
        assert issue.complexity_score == 0

    @pytest.mark.parametrize("value", [0, 1, 5, 10])
    def test_accepts_in_range(self, issue, value):
        serializer = _serializer(issue, value)
        assert serializer.is_valid(), serializer.errors
        assert serializer.validated_data["complexity_score"] == value

    @pytest.mark.parametrize("value", [-1, 11, 100])
    def test_rejects_out_of_range(self, issue, value):
        serializer = _serializer(issue, value)
        with pytest.raises(ValidationError):
            serializer.is_valid(raise_exception=True)

    def test_saved_value_round_trips(self, issue):
        serializer = _serializer(issue, 7)
        assert serializer.is_valid(), serializer.errors
        serializer.save()
        issue.refresh_from_db()
        assert issue.complexity_score == 7
