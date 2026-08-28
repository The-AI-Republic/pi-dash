"""Executor policy shared by projects, orchestration, and runner models."""

from django.conf import settings
from django.db import models


class AgentExecutorKind(models.TextChoices):
    LOCAL_RUNNER = "local_runner", "Local Runner"
    CLOUD_AGENT = "cloud_agent", "Pi Dash Cloud Agent"


def get_default_agent_executor() -> str:
    """Return the validated instance default for newly-created projects."""
    value = getattr(settings, "DEFAULT_AGENT_EXECUTOR", AgentExecutorKind.LOCAL_RUNNER)
    if value not in AgentExecutorKind.values:
        return AgentExecutorKind.LOCAL_RUNNER
    return value


def cloud_agent_is_configured() -> bool:
    """Whether the managed executor can accept new work.

    The Cloud Agent has no platform model of its own: each run executes
    against its creator's BYOK LLM config — the same per-user config Pi
    Dash AI uses. Instance readiness is therefore just the operator kill
    switch; whether a *specific* run can execute is a per-creator question
    answered by :func:`user_has_llm_config` at creation and execution time.
    """
    return bool(getattr(settings, "CLOUD_AGENT_ENABLED", False))


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

    When ``user`` is provided, cloud availability also reflects whether that
    user has a BYOK LLM config — the viewer-specific "you need to configure
    your AI provider" signal. Without a user, only the instance switch is
    reported (a project-level policy question, not a per-viewer one).
    """
    from pi_dash.runner.models import Runner, RunnerStatus

    cloud_available = cloud_agent_is_configured()
    cloud_reason = "" if cloud_available else "cloud_agent_unavailable"
    if cloud_available and user is not None and not user_has_llm_config(user):
        cloud_available = False
        cloud_reason = "llm_config_missing"
    local_available = Runner.objects.filter(
        pod__project_id=project.id,
        workspace_id=project.workspace_id,
        status=RunnerStatus.ONLINE,
    ).exists()
    return [
        {
            "kind": AgentExecutorKind.CLOUD_AGENT,
            "available": cloud_available,
            "reason_code": cloud_reason,
        },
        {
            "kind": AgentExecutorKind.LOCAL_RUNNER,
            "available": local_available,
            "reason_code": "" if local_available else "no_local_runner",
        },
    ]
