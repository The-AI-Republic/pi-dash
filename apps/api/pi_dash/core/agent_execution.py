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
    """Whether the managed executor can safely accept new work."""
    return bool(
        getattr(settings, "CLOUD_AGENT_ENABLED", False)
        and getattr(settings, "CLOUD_AGENT_MODEL_PROVIDER", "")
        and getattr(settings, "CLOUD_AGENT_MODEL", "")
        and getattr(settings, "CLOUD_AGENT_MODEL_API_KEY", "")
    )


def agent_executor_options(project) -> list[dict[str, object]]:
    """Availability is explicit API data, never inferred from UI heartbeats."""
    from pi_dash.runner.models import Runner, RunnerStatus

    cloud_available = cloud_agent_is_configured()
    local_available = Runner.objects.filter(
        pod__project_id=project.id,
        workspace_id=project.workspace_id,
        status=RunnerStatus.ONLINE,
    ).exists()
    return [
        {
            "kind": AgentExecutorKind.CLOUD_AGENT,
            "available": cloud_available,
            "reason_code": "" if cloud_available else "cloud_agent_unavailable",
        },
        {
            "kind": AgentExecutorKind.LOCAL_RUNNER,
            "available": local_available,
            "reason_code": "" if local_available else "no_local_runner",
        },
    ]
