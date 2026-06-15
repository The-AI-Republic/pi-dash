# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""At-rest encryption for BYOK LLM API keys via AWS KMS.

BYOK provider keys are tiny (well under KMS's 4KB Encrypt limit), so we call
KMS Encrypt/Decrypt directly — no envelope/data-key dance. The plaintext key
material never leaves KMS; ``UserLLMConfig.api_key_encrypted`` stores the raw
KMS ``CiphertextBlob``. Compared with an app-held symmetric key this removes
the single stealable master key, gives per-decrypt CloudTrail audit, and makes
access revocable via the key policy / IAM.

Configuration:
  - ``ASSISTANT_KMS_KEY_ID`` — the CMK (key id, ARN, or alias) used to encrypt
    and decrypt. Unset → the assistant reports not-configured and BYOK keys
    cannot be stored.
  - ``AWS_REGION`` — region for the KMS client (falls back to boto3's default
    resolution when blank).
  - ``ASSISTANT_KMS_ENDPOINT_URL`` — optional endpoint override so a
    KMS-compatible service (e.g. LocalStack) can back local / self-hosted
    setups that have no real AWS account.
"""

from __future__ import annotations

import boto3
from botocore.exceptions import BotoCoreError, ClientError
from django.conf import settings

from pi_dash.assistant.errors import AssistantNotConfigured

# KMS error codes that mean "this ciphertext can't be decrypted with this key"
# (tampered/foreign ciphertext or the wrong CMK) — a data problem, not an
# operational one. These map to AssistantNotConfigured like Fernet's
# InvalidToken did; everything else (AccessDenied, throttling, endpoint down)
# is operational and propagates unchanged.
_UNDECRYPTABLE_CODES = frozenset(
    {"InvalidCiphertextException", "IncorrectKeyException", "NotFoundException"}
)

# Lazily-built, process-wide KMS client. Cached because boto3 client creation
# is relatively expensive; the client is safe to reuse across these calls.
_client = None


def _key_id() -> str:
    return (getattr(settings, "ASSISTANT_KMS_KEY_ID", "") or "").strip()


def _require_key_id() -> str:
    key_id = _key_id()
    if not key_id:
        raise AssistantNotConfigured(
            "ASSISTANT_KMS_KEY_ID is not set; BYOK keys cannot be stored."
        )
    return key_id


def _kms():
    global _client
    if _client is None:
        kwargs = {}
        region = (getattr(settings, "AWS_REGION", "") or "").strip()
        if region:
            kwargs["region_name"] = region
        endpoint = (getattr(settings, "ASSISTANT_KMS_ENDPOINT_URL", "") or "").strip()
        if endpoint:
            kwargs["endpoint_url"] = endpoint
        _client = boto3.client("kms", **kwargs)
    return _client


def is_configured() -> bool:
    return bool(_key_id())


def encrypt(plaintext: str) -> bytes:
    """Encrypt a BYOK key under the configured CMK. Returns the raw KMS
    CiphertextBlob (stored as-is in ``api_key_encrypted``)."""
    key_id = _require_key_id()
    try:
        resp = _kms().encrypt(KeyId=key_id, Plaintext=plaintext.encode("utf-8"))
    except (BotoCoreError, ClientError) as exc:
        raise AssistantNotConfigured(f"KMS encrypt failed: {exc}") from exc
    return resp["CiphertextBlob"]


def decrypt(token: bytes) -> str:
    if not token:
        return ""
    # Pin KeyId so a ciphertext can only be decrypted by the CMK we expect
    # (defence in depth against a swapped/foreign blob).
    key_id = _require_key_id()
    try:
        resp = _kms().decrypt(CiphertextBlob=bytes(token), KeyId=key_id)
    except ClientError as exc:
        code = exc.response.get("Error", {}).get("Code", "")
        if code in _UNDECRYPTABLE_CODES:
            raise AssistantNotConfigured(
                "Stored BYOK key could not be decrypted with the configured KMS key."
            ) from exc
        raise
    return resp["Plaintext"].decode("utf-8")


def rotate(token: bytes) -> bytes:
    """Re-encrypt a stored ciphertext under the current CMK.

    KMS rotates the backing key material automatically and old ciphertext keeps
    decrypting, so this is only needed to refresh a blob (e.g. after switching
    CMKs). ``ReEncrypt`` does it inside KMS without exposing the plaintext.
    """
    key_id = _require_key_id()
    try:
        resp = _kms().re_encrypt(CiphertextBlob=bytes(token), DestinationKeyId=key_id)
    except (BotoCoreError, ClientError) as exc:
        raise AssistantNotConfigured(f"KMS re-encrypt failed: {exc}") from exc
    return resp["CiphertextBlob"]
