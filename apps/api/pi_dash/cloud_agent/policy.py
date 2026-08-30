"""Creation/admission policy and immutable tool-plan construction."""

from django.conf import settings

from pi_dash.core.agent_execution import AgentExecutorKind, cloud_agent_is_configured

READ_TOOLS = (
    "pidash_get_current_issue",
    "pidash_list_current_issue_comments",
    "pidash_list_project_states",
    "pidash_search_project_issues",
    "pidash_get_project_issue",
    "pidash_list_linked_code_reviews",
    "github_get_file",
    "github_get_linked_pull_request",
)
WRITE_TOOLS = (
    "pidash_add_current_issue_comment",
    "pidash_update_current_issue_workpad",
    "pidash_transition_current_issue",
    "pidash_create_project_issue",
)


class CloudAgentUnavailable(ValueError):
    code = "cloud_agent_unavailable"


class CloudCapabilityUnavailable(ValueError):
    code = "cloud_capability_unavailable"


class RequiredToolUnavailable(RuntimeError):
    code = "required_tool_unavailable"


def resolve_executor_kind(*, project, requested=None) -> str:
    value = requested or project.default_agent_executor
    if value not in AgentExecutorKind.values:
        raise ValueError("unknown agent executor")
    if value == AgentExecutorKind.CLOUD_AGENT and not cloud_agent_is_configured():
        raise CloudAgentUnavailable("Pi Dash Cloud Agent is not currently available")
    return value


def github_available_for_project(project) -> bool:
    if not getattr(settings, "CLOUD_AGENT_GITHUB_TOOLS_ENABLED", True):
        return False
    from pi_dash.db.models import GitRepositoryBinding

    return GitRepositoryBinding.objects.filter(
        project_id=project.id,
        workspace_id=project.workspace_id,
        deleted_at__isnull=True,
        repository__provider="github",
        repository__host_url="https://github.com",
        provider_account__workspace_id=project.workspace_id,
        provider_account__provider="github",
        provider_account__host_url="https://github.com",
        provider_account__auth_type="github_app",
        provider_account__status="connected",
        provider_account__verified_at__isnull=False,
        provider_account__workspace_integration__github_app_installation__verified_at__isnull=False,
        provider_account__workspace_integration__github_app_installation__suspended_at__isnull=True,
    ).exists()


def build_tool_plan(
    *, run_kind: str, has_issue: bool, required_capabilities=(), project=None, creator=None
) -> dict:
    # Local import: the ee seam is overlayable, and importing it at module
    # scope would bind CE's version before an overlay could replace it.
    from pi_dash.ee.cloud_agent.toolsets import extra_toolsets_enabled_for

    disabled = set(getattr(settings, "CLOUD_AGENT_DISABLED_TOOLS", ()))
    tools = set(READ_TOOLS)
    if not has_issue:
        tools -= {
            "pidash_get_current_issue",
            "pidash_list_current_issue_comments",
            "pidash_list_linked_code_reviews",
            "github_get_linked_pull_request",
            "pidash_add_current_issue_comment",
            "pidash_update_current_issue_workpad",
            "pidash_transition_current_issue",
        }
    if run_kind != "scheduler":
        tools.discard("pidash_get_project_issue")
        tools.discard("pidash_create_project_issue")
    if not getattr(settings, "CLOUD_AGENT_GITHUB_TOOLS_ENABLED", True) or (
        project is not None and not github_available_for_project(project)
    ):
        tools -= {"github_get_file", "github_get_linked_pull_request"}
    if getattr(settings, "CLOUD_AGENT_WRITES_ENABLED", False):
        tools.update(WRITE_TOOLS)
        if run_kind != "scheduler":
            tools.discard("pidash_create_project_issue")
        if not has_issue:
            tools -= set(WRITE_TOOLS) - {"pidash_create_project_issue"}
    tools -= disabled
    requested = set(required_capabilities)
    unavailable = sorted(requested - tools)
    if unavailable:
        raise CloudCapabilityUnavailable("Cloud Agent cannot provide required capabilities: " + ", ".join(unavailable))
    required = sorted(requested)
    return {
        "v": 1,
        "catalog_version": 1,
        "tools": sorted(tools),
        "required_tools": required,
        "limits": {
            "model_requests": settings.CLOUD_AGENT_MODEL_REQUEST_LIMIT,
            "tool_calls": settings.CLOUD_AGENT_TOOL_CALL_LIMIT,
            "writes": settings.CLOUD_AGENT_WRITE_CALL_LIMIT,
            "input_tokens": settings.CLOUD_AGENT_INPUT_TOKEN_LIMIT,
            "output_tokens": settings.CLOUD_AGENT_OUTPUT_TOKEN_LIMIT,
            "total_tokens": settings.CLOUD_AGENT_TOTAL_TOKEN_LIMIT,
            "wall_seconds": settings.CLOUD_AGENT_EXECUTION_TIMEOUT_SECONDS,
        },
        "unavailable_capabilities": ["filesystem", "shell", "worktree"],
        # Whether this run may use deployment-provided toolsets whose tool
        # names are not knowable at plan time (see
        # ``pi_dash.ee.cloud_agent.toolsets``). A sibling of ``tools``, never a
        # member of it: those names must never reach ``tools`` or
        # ``required_tools``, or an external change — a user uninstalling
        # something — would start failing runs on unrelated work items.
        # Snapshotted here so the run executes under the policy it was
        # admitted with. CE always False.
        "extra_toolsets": bool(creator is not None and extra_toolsets_enabled_for(creator)),
    }


def resolve_current_tool_names(run) -> list[str]:
    """Re-intersect the immutable plan with current grants and kill switches."""
    planned = set(run.tool_plan.get("tools", ()))
    current = planned - set(getattr(settings, "CLOUD_AGENT_DISABLED_TOOLS", ()))
    if not settings.CLOUD_AGENT_WRITES_ENABLED:
        current -= set(WRITE_TOOLS)
    project = (
        run.work_item.project
        if run.work_item_id
        else run.scheduler_binding.project
        if run.scheduler_binding_id
        else run.pod.project
    )
    if not github_available_for_project(project):
        current -= {"github_get_file", "github_get_linked_pull_request"}
    elif run.work_item_id:
        from pi_dash.db.models import GitCodeReviewLink, GitRepositoryBinding

        binding = GitRepositoryBinding.objects.select_related("repository").get(
            project_id=project.id, deleted_at__isnull=True
        )
        if not GitCodeReviewLink.objects.filter(
            issue_id=run.work_item_id,
            provider="github",
            host_url="https://github.com",
            namespace=binding.repository.namespace,
            repo_name=binding.repository.name,
            deleted_at__isnull=True,
        ).exists():
            current.discard("github_get_linked_pull_request")
    required = set(run.tool_plan.get("required_tools", ()))
    missing = sorted(required - current)
    if missing:
        raise RequiredToolUnavailable("Required Cloud Agent tools are no longer available: " + ", ".join(missing))
    return sorted(current)
