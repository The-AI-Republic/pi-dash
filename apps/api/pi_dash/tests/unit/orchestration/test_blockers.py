# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for ``pi_dash.orchestration.blockers`` (PDASHOSS01-197)."""

import pytest
from django.utils import timezone

from pi_dash.db.models import Issue, IssueRelation, Project, State
from pi_dash.orchestration import blockers as b


@pytest.fixture
def states(project):
    # No ``started`` state: creating an issue there fires the orchestration
    # hook, which these tests don't need.
    return {
        "todo": State.objects.create(name="Todo", project=project, group="unstarted"),
        "review": State.objects.create(name="In Review", project=project, group="review"),
        "done": State.objects.create(name="Done", project=project, group="completed"),
        "cancelled": State.objects.create(name="Cancelled", project=project, group="cancelled"),
    }


@pytest.fixture
def make_issue(workspace, project, states, create_user):
    def _make(name, state="todo", **kwargs):
        kwargs.setdefault("project", project)
        return Issue.objects.create(
            name=name,
            workspace=workspace,
            state=states[state] if isinstance(state, str) else state,
            created_by=create_user,
            **kwargs,
        )

    return _make


def _relate(issue, related, relation_type="blocked_by"):
    return IssueRelation.objects.create(
        issue=issue,
        related_issue=related,
        relation_type=relation_type,
        project=issue.project,
        workspace=issue.workspace,
    )


def _ids(issues):
    return {i.id for i in issues}


@pytest.mark.unit
def test_no_blockers(make_issue):
    issue = make_issue("Handler")
    assert b.open_blockers(issue) == []
    assert b.blockers(issue) == []
    assert b.dependents(issue) == []
    assert b.has_open_blockers(issue) is False
    assert not Issue.objects.filter(pk=issue.pk).filter(b.open_blockers_q()).exists()


@pytest.mark.unit
def test_blocked_by_row_stored_on_dependent(make_issue):
    handler, model = make_issue("Handler"), make_issue("Model")
    _relate(handler, model, "blocked_by")
    assert _ids(b.open_blockers(handler)) == {model.id}
    assert _ids(b.dependents(model)) == {handler.id}
    # The relation doesn't make the blocker blocked, nor the dependent a blocker.
    assert b.open_blockers(model) == []
    assert b.dependents(handler) == []


@pytest.mark.unit
def test_relation_stored_from_the_blocker_side(make_issue):
    # A row under the reverse name ("model blocking handler") reads the same.
    handler, model = make_issue("Handler"), make_issue("Model")
    _relate(model, handler, "blocking")
    assert _ids(b.open_blockers(handler)) == {model.id}
    assert _ids(b.dependents(model)) == {handler.id}
    assert b.open_blockers(model) == []


@pytest.mark.unit
def test_completed_and_cancelled_resolve_but_review_stays_open(make_issue):
    handler = make_issue("Handler")
    done, cancelled, review = make_issue("Done", "done"), make_issue("Dropped", "cancelled"), make_issue("PR", "review")
    for blocker in (done, cancelled, review):
        _relate(handler, blocker)
    assert _ids(b.blockers(handler)) == {done.id, cancelled.id, review.id}
    assert _ids(b.open_blockers(handler)) == {review.id}
    assert b.has_open_blockers(handler) is True

    review.state = done.state
    review.save()
    assert b.open_blockers(handler) == []
    assert b.has_open_blockers(handler) is False


@pytest.mark.unit
def test_soft_deleted_relation_ignored(make_issue):
    handler, model = make_issue("Handler"), make_issue("Model")
    rel = _relate(handler, model)
    rel.deleted_at = timezone.now()
    rel.save()
    assert b.open_blockers(handler) == []
    assert b.dependents(model) == []
    assert not Issue.objects.filter(pk=handler.pk).filter(b.open_blockers_q()).exists()


@pytest.mark.unit
def test_deleted_or_archived_blocker_ignored(make_issue):
    handler, gone, archived = make_issue("Handler"), make_issue("Gone"), make_issue("Archived")
    _relate(handler, gone)
    _relate(handler, archived)
    Issue.objects.filter(pk=gone.pk).update(deleted_at=timezone.now())
    Issue.objects.filter(pk=archived.pk).update(archived_at=timezone.now().date())
    assert b.open_blockers(handler) == []
    assert b.has_open_blockers(handler) is False


@pytest.mark.unit
def test_non_blocking_relations_ignored(make_issue):
    handler, other = make_issue("Handler"), make_issue("Other")
    _relate(handler, other, "relates_to")
    _relate(other, handler, "start_before")
    assert b.blockers(handler) == [] and b.dependents(handler) == []


@pytest.mark.unit
def test_cross_project_blocker_counts(make_issue, workspace, create_user):
    other_project = Project.objects.create(name="Lib", identifier="LIB", workspace=workspace, created_by=create_user)
    lib_state = State.objects.create(name="Todo", project=other_project, group="unstarted")
    handler = make_issue("Handler")
    lib = make_issue("Library change", lib_state, project=other_project)
    _relate(handler, lib)
    assert _ids(b.open_blockers(handler)) == {lib.id}
    assert _ids(b.dependents(lib)) == {handler.id}


@pytest.mark.unit
def test_open_blockers_q_matches_row_form(make_issue):
    open_dep, resolved_dep, reversed_dep, free = (
        make_issue("Open dep"),
        make_issue("Resolved dep"),
        make_issue("Reversed dep"),
        make_issue("Free"),
    )
    _relate(open_dep, make_issue("Open blocker", "review"))
    _relate(resolved_dep, make_issue("Done blocker", "done"))
    _relate(make_issue("Stored backwards", "todo"), reversed_dep, "blocking")

    flagged = set(Issue.issue_objects.filter(b.open_blockers_q()).values_list("id", flat=True))
    assert {open_dep.id, reversed_dep.id} <= flagged
    assert resolved_dep.id not in flagged and free.id not in flagged
    for issue in (open_dep, resolved_dep, reversed_dep, free):
        assert (issue.id in flagged) == b.has_open_blockers(issue)


@pytest.mark.unit
def test_relations_summary_shape_and_open_first(make_issue):
    handler = make_issue("Handler")
    done, review = make_issue("Done", "done"), make_issue("PR", "review")
    downstream = make_issue("Downstream")
    _relate(handler, done)
    _relate(handler, review)
    _relate(downstream, handler)

    summary = b.relations_summary(handler)
    ident = lambda i: f"{i.project.identifier}-{i.sequence_id}"  # noqa: E731
    assert summary["has_open_blockers"] is True
    assert summary["relations_summary"]["blocked_by"] == [
        {"identifier": ident(review), "state": "In Review", "state_group": "review"},
        {"identifier": ident(done), "state": "Done", "state_group": "completed"},
    ]
    assert summary["relations_summary"]["blocking"] == [
        {"identifier": ident(downstream), "state": "Todo", "state_group": "unstarted"},
    ]


@pytest.mark.unit
def test_relations_summary_empty(make_issue):
    assert b.relations_summary(make_issue("Alone")) == {
        "relations_summary": {"blocked_by": [], "blocking": []},
        "has_open_blockers": False,
    }
