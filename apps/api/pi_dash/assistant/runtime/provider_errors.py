# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Turn a failed model call into the error the user is shown.

The same HTTP status means different things depending on how the user reaches
the model. With their own key (BYOK) a 401 is a bad API key; on a metered
gateway connection there is no key to fix and the answer is signing in again.
:func:`classify_provider_error` therefore takes the connection kind, and the
``ee`` model-provider seam (``turn_error_for_user``) is what picks it.
"""

from __future__ import annotations

from pi_dash.assistant.errors import (
    AssistantError,
    ModelInvalid,
    ModelUnavailable,
    ProviderAuthFailed,
    ProviderOutOfCredit,
    ProviderRateLimited,
    ProviderSessionExpired,
    ProviderUnreachable,
)

# The user supplied the credential (an API key in Settings).
BYOK = "byok"
# The credential is minted for the user's session by a metered gateway.
GATEWAY = "gateway"

# Provider text is shown to the user, so it is kept to one short line.
_MAX_PROVIDER_DETAIL = 300

# Provider error *codes* and phrases specific enough to trust without a status.
_CREDIT_MARKERS = (
    "payment required",
    "insufficient_credits",
    "insufficient credits",
    "insufficient_quota",
    "no_credit_account",
    "quota exceeded",
)
# A 429 is a rate limit unless it carries one of these: "quota exceeded" on
# its own also describes a per-minute request quota.
_CREDIT_CODES_ON_429 = ("insufficient_quota", "insufficient_credits", "insufficient credits", "no_credit_account")
_AUTH_MARKERS = ("unauthorized", "api key", "authentication", "invalid_api_key")
_RATE_LIMIT_MARKERS = ("rate limit", "rate_limit", "too many requests")
_MODEL_MARKERS = (
    "unsupported model",
    "model_not_found",
    "does not exist",
    "unknown model",
    "no such model",
)
_UNREACHABLE_MARKERS = (
    "connection",
    "timeout",
    "timed out",
    "unreachable",
    "could not connect",
    "name resolution",
)


def _chain(exc: BaseException):
    seen: set[int] = set()
    cause: BaseException | None = exc
    while cause is not None and id(cause) not in seen:
        seen.add(id(cause))
        yield cause
        cause = cause.__cause__


def http_status(exc: BaseException) -> int | None:
    """The provider's HTTP status, from the exception or whatever it wraps."""
    for cause in _chain(exc):
        status = getattr(cause, "status_code", None)
        if isinstance(status, int) and not isinstance(status, bool):
            return status
    return None


def provider_detail(exc: BaseException) -> str:
    """The provider's own one-line explanation, or ``""`` when it gave none.

    Read from the structured response body only. The exception's ``str()`` is
    never used: it carries the status line, the model name and often a request
    id or URL, none of which belong in front of the user.
    """
    for cause in _chain(exc):
        body = getattr(cause, "body", None)
        if not isinstance(body, dict):
            continue
        nested = body.get("error")
        for candidate in (body.get("message"), nested.get("message") if isinstance(nested, dict) else nested):
            if isinstance(candidate, str) and candidate.strip():
                text = " ".join(candidate.split())
                if len(text) > _MAX_PROVIDER_DETAIL:
                    text = text[: _MAX_PROVIDER_DETAIL - 1].rstrip() + "…"
                return text
    return ""


def _model_name(exc: BaseException, fallback: str) -> str:
    for cause in _chain(exc):
        name = getattr(cause, "model_name", None)
        if isinstance(name, str) and name:
            return name
    return fallback


def _with_detail(message: str, detail: str) -> str:
    return f"{message} The provider said: {detail}" if detail else message


def classify_provider_error(exc: BaseException, *, connection: str = BYOK, model: str = "") -> AssistantError | None:
    """Map a provider failure onto a typed error, or ``None`` if unrecognised.

    ``connection`` is :data:`BYOK` or :data:`GATEWAY` and decides the wording;
    ``model`` names the model for the message when the exception does not.

    The status code is read from the exception, never substring-matched:
    "401" and "402" appear in request ids and token counts, so matching them
    in the message text misclassifies unrelated errors. Text markers are a
    fallback for failures that carry no status at all.
    """
    gateway = connection == GATEWAY
    status = http_status(exc)
    text = str(exc).lower()
    detail = provider_detail(exc)

    def has(markers) -> bool:
        return any(m in text for m in markers)

    # Credit exhaustion comes first: routed providers answer 402 for it, and
    # OpenAI reports an empty balance as a 429 with ``insufficient_quota``,
    # which must not read as "retry shortly".
    if status == 402 or has(_CREDIT_CODES_ON_429 if status == 429 else _CREDIT_MARKERS):
        if gateway:
            return ProviderOutOfCredit(
                _with_detail("You are out of credit for the model provider. Add credit to keep going.", detail)
            )
        return ProviderOutOfCredit(_with_detail("The provider rejected the request for lack of credit.", detail))

    # Not every provider answers 401 for a bad key: Gemini's OpenAI-compatible
    # endpoint says 400 "API key not valid", others 403. The text decides for
    # those, but never for a 5xx.
    if status == 401 or (status in (None, 400, 403) and has(_AUTH_MARKERS)):
        if gateway:
            return ProviderSessionExpired(
                "Your session with the model provider has expired. Sign out and sign back in to continue."
            )
        return ProviderAuthFailed("Your API key was rejected by the provider.")

    if status == 429 or (status is None and has(_RATE_LIMIT_MARKERS)):
        return ProviderRateLimited("Too many requests to the model provider. Wait a moment and try again.")

    if (status in (400, 404) or status is None) and has(_MODEL_MARKERS):
        name = _model_name(exc, model)
        quoted = f'The model "{name}"' if name else "The selected model"
        if gateway:
            return ModelUnavailable(f"{quoted} is not available right now. Pick another model in Settings.")
        return ModelInvalid(f"{quoted} was not accepted by the provider. Check the model name in Settings.")

    # A status means the endpoint answered, so only a timeout or a server-side
    # failure can still be a reachability problem.
    if (status is None or status == 408 or status >= 500) and has(_UNREACHABLE_MARKERS):
        return ProviderUnreachable("Could not reach the configured provider endpoint.")

    return None
