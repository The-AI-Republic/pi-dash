"""Model resolution seam for the Cloud Agent.

The Cloud Agent has no platform model config of its own: every run
executes against its creator's BYOK LLM config — the same per-user
``UserLLMConfig`` (and the same pydantic-ai model construction, key
decryption cache, and SSRF guard) that Pi Dash AI uses. One config, one
resolution path, two runtime instances.
"""


def resolve_model_for_creator(run):
    """Return a pydantic-ai model for ``run.created_by``'s LLM config.

    Delegates to the assistant's EE-overlayable ``resolve_model_for_user``
    seam (CE: BYOK; the hosted build overlays platform credentials), so the
    Cloud Agent and Pi Dash AI always resolve models the same way. Raises
    :class:`pi_dash.assistant.errors.LLMConfigMissing` when the creator has
    no usable config; callers surface that as ``llm_config_missing``.
    """
    from pi_dash.ee.assistant.model_provider import resolve_model_for_user

    return resolve_model_for_user(run.created_by)
