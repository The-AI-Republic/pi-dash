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
            ),
            "pinned_runner": None,
        }
        if admission_error:
            fields["_cloud_admission_error"] = admission_error
        return fields
    if executor == AgentExecutorKind.MANAGED_RUNNER:
        return _managed_execution_fields(project=project, actor=actor, automatic=automatic)
    return {"executor_kind": executor, "tool_plan": {}}


def _managed_execution_fields(*, project, actor, automatic: bool):
    """Executor fields for a run on the creator's own desktop.

    Managed runs are always **pinned**: the run is created for one specific
    machine and must never drift to a teammate's bundled runner that happens to
    share the pod. ``matcher.next_queued_run_for_pod`` already excludes pinned
    runs, so delivery happens only through ``drain_for_runner`` when that
    machine heartbeats — which is exactly the wanted behaviour.

    User-triggered runs are refused outright when the desktop cannot take them:
    the click came *from* the desktop, so an immediate, specific error beats a
    row that waits. Automatic runs (ticker, scheduler) are created anyway when
    a runner is merely offline, carrying ``desktop_not_connected`` so the wait
    is visible; the periodic sweep fails them if the machine never returns.
    """
    from pi_dash.managed_runner.errors import ManagedRunnerReason, ManagedRunnerUnavailable
    from pi_dash.managed_runner.policy import (
        enrolled_managed_runners,
        managed_runner_availability,
        online_managed_runner,
    )

    available, reason = managed_runner_availability(project, actor)
    fields = {"executor_kind": AgentExecutorKind.MANAGED_RUNNER, "tool_plan": {}, "pinned_runner": None}
    if available:
        fields["pinned_runner"] = online_managed_runner(project, actor)
        return fields

    # "Offline right now" is the only transient failure; everything else is
    # structural and refused for automatic runs too, since waiting could not
    # fix it.
    transient = reason == ManagedRunnerReason.NOT_CONNECTED and enrolled_managed_runners(project, actor).exists()
    if automatic and transient:
        fields["pinned_runner"] = enrolled_managed_runners(project, actor).order_by("-last_heartbeat_at").first()
        # ``error_code`` is a real AgentRun field and every creation site
        # splats these fields into ``objects.create``, so the waiting reason
        # lands on the row itself and renders as "Waiting for your desktop"
        # without a bespoke channel. The sweep reads it back.
        fields["error_code"] = ManagedRunnerReason.NOT_CONNECTED
        return fields
    raise ManagedRunnerUnavailable(reason, _MANAGED_REFUSAL_DETAIL.get(reason, "Pi Dash Agent is not available"))


_MANAGED_REFUSAL_DETAIL = {
    "managed_runner_disabled": "Pi Dash Agent is not enabled on this instance.",
    "desktop_not_connected": "Open the Pi Dash desktop app on the machine you want this to run on.",
    "llm_config_missing": "The run creator has no AI provider configured. Configure one in Pi Dash AI settings.",
    "gateway_scopes_missing": "Sign in to Pi Dash again to refresh your AI access.",
    "byok_not_supported_on_desktop": (
        "Pi Dash Agent on desktop uses OpenHub. Switch your AI provider to OpenHub to run here; "
        "Pi Dash AI and the Cloud Agent keep using your own key."
    ),
    "no_managed_runner_for_project": "This project has no Pi Dash Agent on your desktop yet.",
}


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
