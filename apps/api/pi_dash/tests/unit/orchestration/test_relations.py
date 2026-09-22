# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for ``pi_dash.orchestration.relations`` (PDASHOSS01-199)."""

from unittest import mock

import pytest

from pi_dash.core.querysets import member_project_issues
from pi_dash.db.models import Issue, IssueRelation, Project, ProjectMember, State
from pi_dash.orchestration import relations as r


@pytest.fixture(autouse=True)
def _no_activity():
    # The activity task is a Celery ``.delay``; keep it off the broker.
    with mock.patch("pi_dash.bgtasks.issue_activities_task.issue_activity.delay") as delay:
        yield delay


@pytest.fixture
def states(project):
    return {
        "todo": State.objects.create(name="Todo", project=project, group="unstarted"),
        "done": State.objects.create(name="Done", project=project, group="completed"),
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


@pytest.mark.unit
def test_relate_blocked_by_creates_forward_row(make_issue, create_user, _no_activity):
    a, b = make_issue("A"), make_issue("B")
    out = r.relate(a, "blocked_by", [b], create_user)
    assert out == {
        "issue": r.identifier(a),
        "relation_type": "blocked_by",
        "created": [r.identifier(b)],
        "unchanged": [],
        "conflicts": [],
    }
    row = IssueRelation.objects.get(issue=a, related_issue=b)
    assert row.relation_type == "blocked_by"
    assert _no_activity.call_count == 1
    assert _no_activity.call_args.kwargs["type"] == "issue_relation.activity.created"


@pytest.mark.unit
def test_relate_blocking_stores_blocked_by_on_the_other_end(make_issue, create_user):
    a, b = make_issue("A"), make_issue("B")
    r.relate(a, "blocking", [b], create_user)
    row = IssueRelation.objects.get()
    assert (row.issue_id, row.related_issue_id, row.relation_type) == (b.id, a.id, "blocked_by")
    # Seen from B it is blocked_by A, and relating that way is a no-op.
    out = r.relate(b, "blocked_by", [a], create_user)
    assert out["unchanged"] == [r.identifier(a)] and out["created"] == []


@pytest.mark.unit
def test_relate_is_idempotent(make_issue, create_user, _no_activity):
    a, b, c = make_issue("A"), make_issue("B"), make_issue("C")
    r.relate(a, "blocked_by", [b], create_user)
    _no_activity.reset_mock()
    out = r.relate(a, "blocked_by", [b, c, b], create_user)
    assert out["created"] == [r.identifier(c)]
    assert out["unchanged"] == [r.identifier(b)]
    assert IssueRelation.objects.filter(issue=a).count() == 2
    # Only the newly created edge is logged.
    assert _no_activity.call_count == 1


@pytest.mark.unit
def test_relate_reports_conflict_and_does_not_overwrite(make_issue, create_user):
    a, b = make_issue("A"), make_issue("B")
    r.relate(a, "relates_to", [b], create_user)
    out = r.relate(a, "blocked_by", [b], create_user)
    assert out["created"] == [] and out["unchanged"] == []
    assert out["conflicts"] == [{"identifier": r.identifier(b), "existing_relation": "relates_to"}]
    assert IssueRelation.objects.get().relation_type == "relates_to"


@pytest.mark.unit
def test_relate_refuses_the_direct_two_cycle(make_issue, create_user):
    a, b = make_issue("A"), make_issue("B")
    r.relate(a, "blocked_by", [b], create_user)
    out = r.relate(b, "blocked_by", [a], create_user)
    assert out["conflicts"] == [{"identifier": r.identifier(a), "existing_relation": "blocking"}]
    assert IssueRelation.objects.count() == 1


@pytest.mark.unit
def test_relate_rejects_self_and_unknown_type(make_issue, create_user):
    a, b = make_issue("A"), make_issue("B")
    with pytest.raises(r.RelationError):
        r.relate(a, "blocked_by", [a], create_user)
    with pytest.raises(r.RelationError):
        r.relate(a, "depends_on", [b], create_user)
    assert not IssueRelation.objects.exists()


@pytest.mark.unit
def test_symmetric_relation_reads_the_same_from_both_ends(make_issue, create_user):
    a, b = make_issue("A"), make_issue("B")
    r.relate(a, "relates_to", [b], create_user)
    assert r.relate(b, "relates_to", [a], create_user)["unchanged"] == [r.identifier(a)]
    assert r.unrelate(b, "relates_to", [a], create_user)["removed"] == [r.identifier(a)]
    assert not IssueRelation.objects.exists()


@pytest.mark.unit
def test_unrelate_removes_only_that_type_and_is_idempotent(make_issue, create_user, _no_activity):
    a, b, c = make_issue("A"), make_issue("B"), make_issue("C")
    r.relate(a, "blocked_by", [b], create_user)
    r.relate(a, "relates_to", [c], create_user)
    _no_activity.reset_mock()

    out = r.unrelate(a, "blocked_by", [b, c], create_user)
    assert out == {
        "issue": r.identifier(a),
        "relation_type": "blocked_by",
        "removed": [r.identifier(b)],
        "not_related": [r.identifier(c)],
    }
    assert not IssueRelation.objects.filter(issue=a, related_issue=b).exists()
    assert IssueRelation.objects.filter(issue=a, related_issue=c).exists()
    assert _no_activity.call_args.kwargs["type"] == "issue_relation.activity.deleted"

    again = r.unrelate(a, "blocked_by", [b], create_user)
    assert again["removed"] == [] and again["not_related"] == [r.identifier(b)]


@pytest.mark.unit
def test_unrelate_then_relate_again(make_issue, create_user):
    # The unique constraint is on live rows only: a soft-deleted edge must not
    # stop the pair from being related again.
    a, b = make_issue("A"), make_issue("B")
    r.relate(a, "blocked_by", [b], create_user)
    r.unrelate(a, "blocked_by", [b], create_user)
    assert r.relate(a, "blocked_by", [b], create_user)["created"] == [r.identifier(b)]


@pytest.mark.unit
def test_unrelate_blocking_from_the_blocker_side(make_issue, create_user):
    a, b = make_issue("A"), make_issue("B")
    r.relate(a, "blocked_by", [b], create_user)
    assert r.unrelate(b, "blocking", [a], create_user)["removed"] == [r.identifier(a)]
    assert not IssueRelation.objects.exists()


@pytest.mark.unit
def test_grouped_relations_shape(make_issue, create_user):
    handler = make_issue("Handler")
    model = make_issue("Model", state="done")
    query = make_issue("Query")
    docs = make_issue("Docs")
    r.relate(handler, "blocked_by", [query, model], create_user)
    r.relate(handler, "relates_to", [docs], create_user)

    grouped = r.grouped_relations(handler)
    assert list(grouped) == list(r.RELATION_TYPES)
    assert [i["name"] for i in grouped["blocked_by"]] == ["Model", "Query"]
    assert grouped["blocked_by"][0] == {
        "id": str(model.id),
        "identifier": r.identifier(model),
        "name": "Model",
        "state": "Done",
        "state_group": "completed",
    }
    assert [i["name"] for i in grouped["relates_to"]] == ["Docs"]
    assert grouped["blocking"] == []

    # And from the blocker's side.
    assert [i["name"] for i in r.grouped_relations(query)["blocking"]] == ["Handler"]


@pytest.mark.unit
def test_grouped_relations_reads_rows_stored_under_the_reverse_name(make_issue):
    handler, model = make_issue("Handler"), make_issue("Model")
    IssueRelation.objects.create(
        issue=model, related_issue=handler, relation_type="blocking", project=model.project, workspace=model.workspace
    )
    assert [i["name"] for i in r.grouped_relations(handler)["blocked_by"]] == ["Model"]
    assert [i["name"] for i in r.grouped_relations(model)["blocking"]] == ["Handler"]


@pytest.mark.unit
def test_grouped_relations_hides_what_the_viewer_cannot_see(make_issue, workspace, project, create_user):
    ProjectMember.objects.get_or_create(project=project, member=create_user, defaults={"role": 20, "is_active": True})
    secret_project = Project.objects.create(
        name="Secret", identifier="SEC", workspace=workspace, created_by=create_user
    )
    ProjectMember.objects.filter(project=secret_project, member=create_user).delete()
    secret_state = State.objects.create(name="Todo", project=secret_project, group="unstarted")
    a = make_issue("A")
    hidden = make_issue("Hidden", state=secret_state, project=secret_project)
    r.relate(a, "blocked_by", [hidden], create_user)

    assert [i["name"] for i in r.grouped_relations(a)["blocked_by"]] == ["Hidden"]
    visible = member_project_issues(create_user, workspace.slug)
    assert r.grouped_relations(a, visible)["blocked_by"] == []


@pytest.mark.unit
def test_resolve_refs_accepts_identifiers_and_uuids(make_issue, workspace):
    a, b = make_issue("A"), make_issue("B")
    pool = Issue.issue_objects.filter(workspace=workspace)
    found, unresolved = r.resolve_refs(
        [r.identifier(a), str(b.id), r.identifier(a).lower(), "NOPE-999", "garbage", ""], pool
    )
    assert [i.id for i in found] == [a.id, b.id, a.id]
    assert unresolved == ["NOPE-999", "garbage", ""]
