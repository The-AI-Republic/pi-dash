"""Cloud Agent relation tools and their place in the tool plan (PDASHOSS01-199)."""

from unittest.mock import patch

import pytest
from django.test import override_settings

from pi_dash.cloud_agent.policy import build_tool_plan
from pi_dash.cloud_agent.tools import build_tools
from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.db.models import Issue, IssueRelation, Project, ProjectMember, State
from pi_dash.runner.models import AgentRun

RELATION_TOOLS = {"pidash_list_issue_relations", "pidash_relate_issues", "pidash_unrelate_issues"}


@pytest.fixture(autouse=True)
def _no_activity():
    with patch("pi_dash.bgtasks.issue_activities_task.issue_activity.delay"):
        yield


@pytest.fixture
def todo(project, workspace, create_user):
    ProjectMember.objects.get_or_create(workspace=workspace, project=project, member=create_user, defaults={"role": 20})
    return State.objects.create(name="Todo", group="unstarted", project=project)


@pytest.fixture
def make_issue(project, workspace, create_user, todo):
    def _make(name, project_=None, state=None):
        return Issue.objects.create(
            workspace=workspace,
            project=project_ or project,
            state=state or todo,
            name=name,
            created_by=create_user,
        )

    return _make


def _ident(issue):
    return f"{issue.project.identifier}-{issue.sequence_id}"


def _run(issue, create_user):
    return AgentRun.objects.create(
        workspace=issue.workspace,
        created_by=create_user,
        pod=issue.project.pods.get(is_default=True),
        work_item=issue,
        executor_kind=AgentExecutorKind.CLOUD_AGENT,
        tool_plan=build_tool_plan(run_kind="issue", has_issue=True),
        prompt="Bound task",
    )


def _tools(run):
    return {tool.__name__: tool for tool in build_tools(run.id, run.tool_plan["tools"])}


@pytest.mark.unit
@override_settings(CLOUD_AGENT_ENABLED=True, CLOUD_AGENT_WRITES_ENABLED=True)
@pytest.mark.parametrize("run_kind,has_issue", [("issue", True), ("scheduler", False)])
def test_relation_tools_are_in_the_plan_for_issue_and_scheduler_runs(run_kind, has_issue):
    tools = set(build_tool_plan(run_kind=run_kind, has_issue=has_issue)["tools"])
    assert RELATION_TOOLS <= tools
    if not has_issue:
        # Current-issue writes still drop out without a bound issue.
        assert "pidash_add_current_issue_comment" not in tools
        assert "pidash_create_project_issue" in tools


@pytest.mark.unit
@override_settings(CLOUD_AGENT_ENABLED=True, CLOUD_AGENT_WRITES_ENABLED=False)
def test_relation_writes_follow_the_write_switch():
    tools = set(build_tool_plan(run_kind="issue", has_issue=True)["tools"])
    assert "pidash_list_issue_relations" in tools
    assert not {"pidash_relate_issues", "pidash_unrelate_issues"} & tools


@pytest.mark.unit
@override_settings(CLOUD_AGENT_ENABLED=True, CLOUD_AGENT_WRITES_ENABLED=True)
def test_relate_list_unrelate_and_repeat_calls(make_issue, create_user):
    handler, model, query = make_issue("Handler"), make_issue("Model"), make_issue("Query")
    run = _run(handler, create_user)
    tools = _tools(run)

    res = tools["pidash_relate_issues"](_ident(handler), "blocked_by", [_ident(model)])
    assert res["created"] == [_ident(model)]
    assert [i["identifier"] for i in res["relations"]["blocked_by"]] == [_ident(model)]
    # Relate is repeatable within one run (a split wires several children) and
    # idempotent.
    res = tools["pidash_relate_issues"](_ident(handler), "blocked_by", [_ident(model), _ident(query)])
    assert res["created"] == [_ident(query)] and res["unchanged"] == [_ident(model)]

    listed = tools["pidash_list_issue_relations"]()
    assert listed["issue"] == _ident(handler)
    assert [i["name"] for i in listed["relations"]["blocked_by"]] == ["Model", "Query"]
    assert [i["identifier"] for i in tools["pidash_list_issue_relations"](_ident(model))["relations"]["blocking"]] == [
        _ident(handler)
    ]

    res = tools["pidash_unrelate_issues"](_ident(handler), "blocked_by", [_ident(model)])
    assert res["removed"] == [_ident(model)]
    assert list(IssueRelation.objects.values_list("related_issue_id", flat=True)) == [query.id]
    assert run.tool_calls.filter(tool_name="pidash_relate_issues", status="succeeded").count() == 2


@pytest.mark.unit
@override_settings(CLOUD_AGENT_ENABLED=True, CLOUD_AGENT_WRITES_ENABLED=True)
def test_relate_is_scoped_to_what_the_creator_can_see(make_issue, workspace, create_user):
    handler = make_issue("Handler")
    secret = Project.objects.create(name="Secret", identifier="SEC", workspace=workspace, created_by=create_user)
    ProjectMember.objects.filter(project=secret, member=create_user).delete()
    hidden = make_issue("Hidden", project_=secret, state=State.objects.create(name="Todo", project=secret))
    tools = _tools(_run(handler, create_user))

    with pytest.raises(ValueError, match="not found or not accessible"):
        tools["pidash_relate_issues"](_ident(handler), "blocked_by", [_ident(hidden)])
    assert not IssueRelation.objects.exists()


@pytest.mark.unit
@override_settings(CLOUD_AGENT_ENABLED=True, CLOUD_AGENT_WRITES_ENABLED=True)
def test_relate_source_must_be_in_the_runs_project(make_issue, workspace, create_user):
    handler = make_issue("Handler")
    other = Project.objects.create(name="Other", identifier="OTH", workspace=workspace, created_by=create_user)
    ProjectMember.objects.get_or_create(project=other, member=create_user, defaults={"role": 20, "is_active": True})
    elsewhere = make_issue("Elsewhere", project_=other, state=State.objects.create(name="Todo", project=other))
    tools = _tools(_run(handler, create_user))

    with pytest.raises(ValueError, match="not found or not accessible"):
        tools["pidash_relate_issues"](_ident(elsewhere), "blocked_by", [_ident(handler)])
    # ...but a visible issue in another project is a fine target.
    res = tools["pidash_relate_issues"](_ident(handler), "blocked_by", [_ident(elsewhere)])
    assert res["created"] == [_ident(elsewhere)]


@pytest.mark.unit
@override_settings(CLOUD_AGENT_ENABLED=True, CLOUD_AGENT_WRITES_ENABLED=True)
def test_invalid_relation_type_is_rejected_before_any_write(make_issue, create_user):
    handler, model = make_issue("Handler"), make_issue("Model")
    run = _run(handler, create_user)
    with pytest.raises(ValueError):
        _tools(run)["pidash_relate_issues"](_ident(handler), "depends_on", [_ident(model)])
    assert not run.tool_calls.exists()
