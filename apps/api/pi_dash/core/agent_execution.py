"""Executor policy shared by projects, orchestration, and runner models."""

from django.conf import settings
from django.db import models


class AgentExecutorKind(models.TextChoices):
    """Who executes an agent run.

    Runner (``LOCAL_RUNNER``) is the only executor issues and projects can be
    set to (``.ai_design/built_engine_as_runner/design.md`` §6); which machine
    runs the work is a dispatch question answered by pods and
    ``Issue.assigned_runner``. The other two values stay readable for history:
    ``AgentRun.executor_kind`` rows keep them, and queued or running
    ``cloud_agent`` runs finish on the Cloud Agent. Writes of either from
    older clients are translated by :func:`normalize_executor_write`.
    """

    LOCAL_RUNNER = "local_runner", "Local Runner"
    # Legacy for issue/project work. The Cloud Agent run pipeline is kept
    # (removing it is separate cleanup) and still serves cloud chat.
    CLOUD_AGENT = "cloud_agent", "Pi Dash Cloud Agent"
    # Legacy: the built-in engine is now a runner (``provisioning =
    # desktop_bundled``) reached through a pod or an assigned runner.
    MANAGED_RUNNER = "managed_runner", "Pi Dash Agent"


#: Executors whose runs execute on a machine through the ``pidash`` daemon.
#: Every call site that asks "is this local work" must use this rather than
#: comparing against ``LOCAL_RUNNER`` alone, or legacy managed runs silently
#: take the Cloud Agent path.
MACHINE_EXECUTORS = (AgentExecutorKind.LOCAL_RUNNER, AgentExecutorKind.MANAGED_RUNNER)

#: Executor values no longer offered for issues and projects. Still accepted on
#: write from older clients and translated to ``local_runner``.
DEPRECATED_EXECUTORS = (AgentExecutorKind.CLOUD_AGENT, AgentExecutorKind.MANAGED_RUNNER)


def normalize_executor_write(value):
    """The stored value for an executor written by a client: deprecated
    choices become ``local_runner``; ``None`` (inherit) and unknown values are
    returned unchanged for the caller's own validation."""
    if value in DEPRECATED_EXECUTORS:
        return AgentExecutorKind.LOCAL_RUNNER
    return value


def get_default_agent_executor() -> str:
    """Return the instance default for newly-created projects.

    ``DEFAULT_AGENT_EXECUTOR`` accepts only ``local_runner`` now (design §6);
    any other value is ignored here and reported by the ``managed.W001``
    system check.
    """
    return AgentExecutorKind.LOCAL_RUNNER


def cloud_agent_is_configured() -> bool:
    """Whether the managed executor can accept new work.

    The Cloud Agent has no platform model of its own: each run executes
    against its creator's BYOK LLM config — the same per-user config Pi
    Dash AI uses. Instance readiness is therefore just the operator kill
    switch; whether a *specific* run can execute is a per-creator question
    answered by :func:`user_has_llm_config` at creation and execution time.
    """
    return bool(getattr(settings, "CLOUD_AGENT_ENABLED", False))


def managed_runner_is_enabled() -> bool:
    """Operator kill switch for the desktop-bundled managed runner.

    Mirrors :func:`cloud_agent_is_configured`: this answers only "may this
    instance accept managed work at all". Whether a *specific* viewer can run
    on their desktop right now is answered by
    :func:`pi_dash.managed_runner.policy.managed_runner_availability`.
    """
    return bool(getattr(settings, "MANAGED_RUNNER_ENABLED", False))


def effective_executor_for_issue(issue) -> str:
    """The executor actually in force for ``issue``.

    The per-issue ``agent_executor`` override wins; ``None`` inherits the
    project's ``default_agent_executor``. Every caller that branches on "is
    this cloud or local work" must use this rather than reading the project
    default directly, or a cloud-pinned issue on a local-default project gets
    routed down the pod/runner path (and bounced for having no runner).
    """
    return getattr(issue, "agent_executor", None) or issue.project.default_agent_executor


def user_has_llm_config(user) -> bool:
    """Whether ``user`` has a usable LLM config (CE: BYOK; cloud: overlay).

    Delegates to the EE-overlayable ``has_usable_llm_config`` seam — the same
    single switch point Pi Dash AI uses — so the hosted build's
    platform-credential overlay applies to Cloud Agent runs too.
    """
    if user is None or not getattr(user, "is_active", False) or getattr(user, "is_bot", False):
        return False
    from pi_dash.ee.assistant.model_provider import has_usable_llm_config

    return has_usable_llm_config(user)


def agent_executor_options(project, user=None) -> list[dict[str, object]]:
    """Availability is explicit API data, never inferred from UI heartbeats.

    Runner is the only selectable executor (``built_engine_as_runner`` design
    §6). The server cannot tell older desktop builds from new clients on the
    shared project serializer, so the Cloud Agent and built-in engine entries
    are still returned, each marked ``deprecated: true``: new web and desktop
    builds hide deprecated entries, older ones (which ignore the flag) keep
    showing them, and their writes are translated to ``local_runner``. Their
    availability is still computed so those older builds keep working.

    When ``user`` is provided, cloud availability also reflects whether that
    user has a BYOK LLM config. Without a user, only the instance switch is
    reported (a project-level policy question, not a per-viewer one).
    """
    from pi_dash.runner.models import Runner, RunnerPodDispatch, RunnerStatus, engine_paused_q
    from pi_dash.managed_runner.policy import managed_runner_availability

    cloud_available = cloud_agent_is_configured()
    cloud_reason = "" if cloud_available else "cloud_agent_unavailable"
    if cloud_available and user is not None and not user_has_llm_config(user):
        cloud_available = False
        cloud_reason = "llm_config_missing"
    # Only runners that take pod work count as "a runner is available":
    # ``pinned_only`` runners (engines by default) serve only issues assigned
    # to them, and a paused engine takes nothing.
    online_local = (
        Runner.objects.filter(
            pod__project_id=project.id,
            workspace_id=project.workspace_id,
            status=RunnerStatus.ONLINE,
        )
        .exclude(pod_dispatch=RunnerPodDispatch.PINNED_ONLY)
        .exclude(revoked_at__isnull=False)
    )
    # A paused runner is connected but takes no new work. Runs still queue for
    # it (the preflight counts it as registered), so report "paused" rather
    # than "no runner" — the fix is to resume, not to add one.
    local_available = online_local.filter(paused_at__isnull=True).exclude(engine_paused_q()).exists()
    if local_available:
        local_reason = ""
    elif online_local.exists():
        local_reason = "local_runner_paused"
    else:
        local_reason = "no_local_runner"
    managed_available, managed_reason = managed_runner_availability(project, user)
    return [
        {
            "kind": AgentExecutorKind.CLOUD_AGENT,
            "available": cloud_available,
            "reason_code": cloud_reason,
            "deprecated": True,
        },
        {
            "kind": AgentExecutorKind.LOCAL_RUNNER,
            "available": local_available,
            "reason_code": local_reason,
            "deprecated": False,
        },
        {
            "kind": AgentExecutorKind.MANAGED_RUNNER,
            "available": managed_available,
            "reason_code": managed_reason,
            "deprecated": True,
        },
    ]
