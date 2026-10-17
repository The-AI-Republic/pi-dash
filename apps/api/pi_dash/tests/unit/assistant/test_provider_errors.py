# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""What the user is told when the model provider refuses a request.

One row per provider response, checked for both connection kinds: a key the
user supplied (BYOK) and a metered gateway that mints the credential for them.
The same status must not read the same way on both — a gateway user has no
API key to fix.
"""

from __future__ import annotations

import pytest
from pydantic_ai.exceptions import ModelHTTPError

from pi_dash.assistant.runtime.provider_errors import (
    BYOK,
    GATEWAY,
    classify_provider_error,
    http_status,
    provider_detail,
)

pytestmark = pytest.mark.unit

MODEL = "acme/gone-1"
CREDIT_TEXT = "Daily free quota used and LLM credit balance is insufficient; add credit or upgrade your plan."


def _http(status: int, message: str) -> ModelHTTPError:
    return ModelHTTPError(status, MODEL, {"message": message, "type": "invalid_request_error"})


OUT_OF_CREDIT = _http(402, CREDIT_TEXT)
BAD_CREDENTIAL = _http(401, "Invalid JWT")
UNSUPPORTED_MODEL = _http(400, "Unsupported model")
RATE_LIMITED = _http(429, "Rate limit exceeded: 60 requests per minute")


def _classify(exc, connection):
    error = classify_provider_error(exc, connection=connection)
    assert error is not None
    return error.code, error.detail


# --- 402: no credit --------------------------------------------------------


def test_byok_402_is_a_credit_problem_with_the_providers_text():
    code, detail = _classify(OUT_OF_CREDIT, BYOK)
    assert code == "provider_out_of_credit"
    assert detail == f"The provider rejected the request for lack of credit. The provider said: {CREDIT_TEXT}"


def test_gateway_402_tells_the_user_to_add_credit_and_keeps_the_providers_text():
    code, detail = _classify(OUT_OF_CREDIT, GATEWAY)
    assert code == "provider_out_of_credit"
    assert detail.startswith("You are out of credit for the model provider. Add credit to keep going.")
    assert CREDIT_TEXT in detail


# --- 401: credential rejected ----------------------------------------------


def test_byok_401_is_an_api_key_problem():
    assert _classify(BAD_CREDENTIAL, BYOK) == (
        "provider_auth_failed",
        "Your API key was rejected by the provider.",
    )


def test_gateway_401_asks_the_user_to_sign_in_again():
    code, detail = _classify(BAD_CREDENTIAL, GATEWAY)
    assert code == "provider_session_expired"
    assert "Sign out and sign back in" in detail
    assert "API key" not in detail


# --- 400: unsupported model ------------------------------------------------


def test_byok_unsupported_model_names_the_model_and_points_at_the_setting():
    code, detail = _classify(UNSUPPORTED_MODEL, BYOK)
    assert code == "model_invalid"
    assert detail == f'The model "{MODEL}" was not accepted by the provider. Check the model name in Settings.'


def test_gateway_unsupported_model_names_the_model_and_says_to_pick_another():
    code, detail = _classify(UNSUPPORTED_MODEL, GATEWAY)
    assert code == "model_unavailable"
    assert detail == f'The model "{MODEL}" is not available right now. Pick another model in Settings.'


# --- 429: rate limit -------------------------------------------------------


@pytest.mark.parametrize("connection", [BYOK, GATEWAY])
def test_429_asks_the_user_to_retry(connection):
    code, detail = _classify(RATE_LIMITED, connection)
    assert code == "provider_rate_limited"
    assert detail == "Too many requests to the model provider. Wait a moment and try again."


# --- The status is read from the exception, not found in its text ----------


@pytest.mark.parametrize("connection", [BYOK, GATEWAY])
@pytest.mark.parametrize("request_id", ["req_401abc", "req_402abc", "req_429abc"])
def test_a_status_like_number_in_the_text_is_not_a_status(connection, request_id):
    """A request id containing "401" used to turn any failure into "API key rejected"."""
    exc = ModelHTTPError(500, MODEL, {"message": "upstream failed", "request_id": request_id})
    assert classify_provider_error(exc, connection=connection) is None


def test_the_status_is_found_on_a_wrapped_exception():
    try:
        try:
            raise BAD_CREDENTIAL
        except ModelHTTPError as inner:
            raise RuntimeError("model request failed") from inner
    except RuntimeError as wrapped:
        assert http_status(wrapped) == 401
        assert _classify(wrapped, GATEWAY)[0] == "provider_session_expired"


def test_a_400_that_is_not_about_the_model_is_left_unclassified():
    exc = _http(400, "This model's maximum context length is 128000 tokens.")
    assert classify_provider_error(exc, connection=GATEWAY) is None


def test_an_exhausted_quota_reported_as_429_is_a_credit_problem_not_a_retry():
    exc = ModelHTTPError(429, MODEL, {"message": "You exceeded your current quota.", "code": "insufficient_quota"})
    assert _classify(exc, BYOK)[0] == "provider_out_of_credit"


def test_a_request_quota_reported_as_429_is_a_retry_not_a_credit_problem():
    exc = _http(429, "Rate limit quota exceeded, retry in 30s")
    for connection in (BYOK, GATEWAY):
        assert _classify(exc, connection)[0] == "provider_rate_limited"


@pytest.mark.parametrize(
    ("status", "message"),
    [(400, "API key not valid. Please pass a valid API key."), (403, "Invalid API key")],
)
def test_a_rejected_key_answered_with_400_or_403_is_still_a_key_problem(status, message):
    assert _classify(_http(status, message), BYOK) == (
        "provider_auth_failed",
        "Your API key was rejected by the provider.",
    )
    assert _classify(_http(status, message), GATEWAY)[0] == "provider_session_expired"


def test_a_server_error_that_mentions_authentication_is_not_a_key_problem():
    assert classify_provider_error(_http(500, "authentication service returned an error")) is None


def test_the_model_name_falls_back_to_the_callers_when_the_exception_has_none():
    error = classify_provider_error(Exception("model_not_found"), connection=GATEWAY, model="picked/model")
    assert error is not None
    assert error.detail == 'The model "picked/model" is not available right now. Pick another model in Settings.'


# --- Failures with no HTTP status still fall back to text ------------------


def test_statusless_failures_are_still_classified_by_text():
    assert _classify(Exception("Connection timed out"), BYOK)[0] == "provider_unreachable"
    assert _classify(Exception("Incorrect API key provided"), BYOK)[0] == "provider_auth_failed"
    assert classify_provider_error(Exception("something else entirely")) is None


def test_a_gateway_timeout_status_is_still_unreachable():
    assert _classify(_http(504, "Gateway timeout"), GATEWAY)[0] == "provider_unreachable"


# --- Provider text shown to the user ---------------------------------------


def test_provider_detail_reads_openai_style_nested_errors():
    exc = ModelHTTPError(402, MODEL, {"error": {"message": "  Insufficient\n credits  ", "code": 402}})
    assert provider_detail(exc) == "Insufficient credits"


def test_provider_detail_is_empty_without_a_structured_body():
    assert provider_detail(ModelHTTPError(402, MODEL, "<html>Payment Required</html>")) == ""
    code, detail = _classify(ModelHTTPError(402, MODEL, None), BYOK)
    assert detail == "The provider rejected the request for lack of credit."


def test_provider_detail_is_capped():
    exc = ModelHTTPError(402, MODEL, {"message": "x" * 5000})
    assert len(provider_detail(exc)) == 300
