"""Model resolution seam for the Cloud Agent.

The Cloud Agent has no platform model config of its own: every run
executes against its creator's BYOK LLM config — the same per-user
``UserLLMConfig`` (and the same pydantic-ai model construction, key
decryption cache, and SSRF guard) that Pi Dash AI uses. One config, one
resolution path, two runtime instances.
"""


def resolve_model_for_creator(run):
    """Return a pydantic-ai model built from ``run.created_by``'s BYOK config.

    Raises :class:`pi_dash.assistant.errors.LLMConfigMissing` when the
    creator has no usable config; callers surface that as a run failure
    with the ``llm_config_missing`` code.
    """
    from pi_dash.assistant.runtime.llm import resolve_byok_model

    return resolve_byok_model(run.created_by)
