"""Instance-owned model provider seam for the Cloud Agent."""

from django.conf import settings


def resolve_cloud_model():
    provider = settings.CLOUD_AGENT_MODEL_PROVIDER.lower()
    model_name = settings.CLOUD_AGENT_MODEL
    api_key = settings.CLOUD_AGENT_MODEL_API_KEY
    if provider == "anthropic":
        from pydantic_ai.models.anthropic import AnthropicModel
        from pydantic_ai.providers.anthropic import AnthropicProvider

        return AnthropicModel(model_name, provider=AnthropicProvider(api_key=api_key))
    if provider == "openai":
        from pydantic_ai.models.openai import OpenAIChatModel
        from pydantic_ai.providers.openai import OpenAIProvider

        kwargs = {"api_key": api_key}
        if settings.CLOUD_AGENT_MODEL_BASE_URL:
            kwargs["base_url"] = settings.CLOUD_AGENT_MODEL_BASE_URL
        return OpenAIChatModel(model_name, provider=OpenAIProvider(**kwargs))
    raise ValueError("CLOUD_AGENT_MODEL_PROVIDER must be openai or anthropic")
