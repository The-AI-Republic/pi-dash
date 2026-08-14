from django.conf import settings
from django.core.checks import Error, register
from urllib.parse import urlparse

from pi_dash.core.agent_execution import AgentExecutorKind


@register()
def cloud_agent_configuration_check(app_configs, **kwargs):
    from pi_dash.cloud_agent.policy import READ_TOOLS, WRITE_TOOLS

    errors = []
    default = settings.DEFAULT_AGENT_EXECUTOR
    if default not in AgentExecutorKind.values:
        errors.append(Error("DEFAULT_AGENT_EXECUTOR is invalid", id="cloud_agent.E001"))
    configured = bool(
        settings.CLOUD_AGENT_MODEL_PROVIDER in {"openai", "anthropic"}
        and settings.CLOUD_AGENT_MODEL
        and settings.CLOUD_AGENT_MODEL_API_KEY
    )
    if default == AgentExecutorKind.CLOUD_AGENT and not configured:
        errors.append(Error("Cloud default requires a configured platform model provider", id="cloud_agent.E002"))
    if settings.CLOUD_AGENT_ENABLED and not configured:
        errors.append(Error("CLOUD_AGENT_ENABLED requires provider, model, and API key", id="cloud_agent.E003"))
    base_url = settings.CLOUD_AGENT_MODEL_BASE_URL
    if base_url:
        parsed = urlparse(base_url)
        if parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password:
            errors.append(
                Error(
                    "CLOUD_AGENT_MODEL_BASE_URL must be an HTTPS URL without embedded credentials",
                    id="cloud_agent.E004",
                )
            )
    unknown_tools = sorted(set(settings.CLOUD_AGENT_DISABLED_TOOLS) - set(READ_TOOLS) - set(WRITE_TOOLS))
    if unknown_tools:
        errors.append(
            Error(
                "CLOUD_AGENT_DISABLED_TOOLS contains unknown names: " + ", ".join(unknown_tools),
                id="cloud_agent.E005",
            )
        )
    if len(settings.CLOUD_AGENT_MODEL) > 128:
        errors.append(Error("CLOUD_AGENT_MODEL must not exceed 128 characters", id="cloud_agent.E006"))
    if not (
        0
        < settings.CLOUD_AGENT_EXECUTION_TIMEOUT_SECONDS
        < settings.CLOUD_AGENT_RUN_SOFT_LIMIT_SECONDS
        < settings.CLOUD_AGENT_RUN_HARD_LIMIT_SECONDS
    ):
        errors.append(
            Error(
                "Cloud Agent execution, soft, and hard timeouts must be positive and strictly increasing",
                id="cloud_agent.E007",
            )
        )
    positive_settings = (
        "CLOUD_AGENT_MODEL_REQUEST_TIMEOUT_SECONDS",
        "CLOUD_AGENT_STALE_GRACE_SECONDS",
        "CLOUD_AGENT_DISPATCH_LEASE_SECONDS",
        "CLOUD_AGENT_DISPATCH_BACKOFF_SECONDS",
        "CLOUD_AGENT_DISPATCH_SCAN_INTERVAL_SECONDS",
        "CLOUD_AGENT_SWEEP_INTERVAL_SECONDS",
        "CLOUD_AGENT_DISPATCH_SCAN_BATCH",
        "CLOUD_AGENT_MAX_QUEUE_AGE_SECONDS",
        "CLOUD_AGENT_MODEL_REQUEST_LIMIT",
        "CLOUD_AGENT_TOOL_CALL_LIMIT",
        "CLOUD_AGENT_WRITE_CALL_LIMIT",
        "CLOUD_AGENT_INPUT_TOKEN_LIMIT",
        "CLOUD_AGENT_OUTPUT_TOKEN_LIMIT",
        "CLOUD_AGENT_TOTAL_TOKEN_LIMIT",
        "CLOUD_AGENT_MAX_OUTPUT_TOKENS_PER_REQUEST",
        "CLOUD_AGENT_MAX_QUEUED_PER_WORKSPACE",
        "CLOUD_AGENT_MAX_RUNNING_PER_WORKSPACE",
        "CLOUD_AGENT_USER_CREATION_RATE_PER_MINUTE",
        "CLOUD_AGENT_WORKSPACE_CREATION_RATE_PER_MINUTE",
        "CLOUD_AGENT_TOOL_TIMEOUT_SECONDS",
        "CLOUD_AGENT_MAX_TOOL_RESULT_BYTES",
        "CLOUD_AGENT_MAX_PROMPT_BYTES",
        "CLOUD_AGENT_MAX_FINAL_RESULT_BYTES",
        "CLOUD_AGENT_MAX_EVENTS",
    )
    invalid_positive = [name for name in positive_settings if getattr(settings, name) <= 0]
    if invalid_positive:
        errors.append(
            Error(
                "Cloud Agent limits must be positive: " + ", ".join(invalid_positive),
                id="cloud_agent.E008",
            )
        )
    return errors
