"""Single executor-aware seam shared by every AgentRun creation path."""

from django.conf import settings
from django.db import transaction

from pi_dash.cloud_agent.policy import build_tool_plan, resolve_executor_kind
from pi_dash.core.agent_execution import AgentExecutorKind
from pi_dash.runner.models import AgentRun, AgentRunStatus


def execution_fields(
    *,
    project,
    run_kind: str,
    has_issue: bool,
    required_capabilities=(),
    actor=None,
    automatic: bool = False,
    requested=None,
):
    """Resolve the executor-specific AgentRun fields for one run creation.

    ``requested`` is the per-issue execution-target override
    (``Issue.agent_executor``); ``None`` inherits the project default.
    """
    executor = resolve_executor_kind(project=project, requested=requested)
    if executor == AgentExecutorKind.CLOUD_AGENT:
        from pi_dash.cloud_agent.admission import CloudAgentAdmissionError, enforce_creation_rate
        from pi_dash.cloud_agent.policy import CloudAgentUnavailable
        from pi_dash.core.agent_execution import user_has_llm_config
        from pi_dash.db.models import Workspace

        # Cloud runs execute against the creator's BYOK LLM config (the same
        # per-user config Pi Dash AI uses); a run without a funded principal
        # can never start, so refuse it here with an actionable reason.
        if not user_has_llm_config(actor):
            raise CloudAgentUnavailable(
                "The run creator has no AI provider configured. Configure one in Pi Dash AI settings."
            )

        admission_error = None
        try:
            enforce_creation_rate(
                workspace_id=project.workspace_id,
                actor_id=getattr(actor, "id", None),
                automatic=automatic,
            )
        except CloudAgentAdmissionError as exc:
            if not automatic:
                raise
            admission_error = {"code": exc.code, "detail": str(exc)}
        # The caller's surrounding creation transaction keeps this lock until
        # insertion. The inner atomic also makes direct/test callers safe,
        # though only the shared creation paths provide race-free count+insert.
        with transaction.atomic():
            Workspace.objects.select_for_update().get(pk=project.workspace_id)
            queued = AgentRun.objects.filter(
                workspace_id=project.workspace_id,
                executor_kind=executor,
                status=AgentRunStatus.QUEUED,
            ).count()
            if queued >= settings.CLOUD_AGENT_MAX_QUEUED_PER_WORKSPACE:
                error = CloudAgentAdmissionError(
                    "run_quota_exceeded",
                    "Cloud Agent queue is full for this workspace",
                    retry_after_seconds=settings.CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS,
                )
                if not automatic:
                    raise error
                admission_error = {"code": error.code, "detail": str(error)}
        fields = {
            "executor_kind": executor,
            "tool_plan": build_tool_plan(
                run_kind=run_kind,
                has_issue=has_issue,
                required_capabilities=required_capabilities,
                project=project,
                # The run executes as its creator, so their preference is what
                # the snapshot records.
                creator=actor,
            ),
            "pinned_runner": None,
        }
        if admission_error:
            fields["_cloud_admission_error"] = admission_error
        return fields
    return {"executor_kind": executor, "tool_plan": {}}


def dispatch_after_commit(run_id):
    from django.db import transaction
    from pi_dash.cloud_agent.dispatch import dispatch_agent_run

    transaction.on_commit(lambda: dispatch_agent_run(run_id))


def lock_cloud_creation_capacity(*, project, executor_kind, automatic=False):
    """Repeat hard admission while holding the caller's insertion transaction."""
    if executor_kind != AgentExecutorKind.CLOUD_AGENT:
        return
    from pi_dash.cloud_agent.admission import CloudAgentAdmissionError
    from pi_dash.db.models import Workspace

    Workspace.objects.select_for_update().get(pk=project.workspace_id)
    queued = AgentRun.objects.filter(
        workspace_id=project.workspace_id,
        executor_kind=executor_kind,
        status=AgentRunStatus.QUEUED,
    ).count()
    if queued >= settings.CLOUD_AGENT_MAX_QUEUED_PER_WORKSPACE:
        error = CloudAgentAdmissionError(
            "run_quota_exceeded",
            "Cloud Agent queue is full for this workspace",
            retry_after_seconds=settings.CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS,
        )
        if automatic:
            return {"code": error.code, "detail": str(error)}
        raise error
    return None
