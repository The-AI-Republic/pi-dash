# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""At-rest encryption for BYOK LLM API keys.

A thin, provider-agnostic seam (:class:`CipherBackend`) over a managed KMS, so
the call sites (``encrypt`` / ``decrypt`` / ``rotate`` / ``is_configured``) stay
the same regardless of which provider does the crypto. BYOK provider keys are
tiny, so backends encrypt them directly (no envelope); ``encrypt`` returns the
opaque ciphertext stored in ``UserLLMConfig.api_key_encrypted``.

Only **AWS KMS** is implemented today. To add another provider (GCP KMS, Azure
Key Vault, Vault Transit, …) implement :class:`CipherBackend`, register it in
``_BACKENDS``, and select it with ``ASSISTANT_CRYPTO_BACKEND`` — no call-site
changes. Whatever the provider, the goal is the same: the plaintext key
material never lives in app config, and decrypt is auditable + revocable.

Conventions for implementations:
  - "ciphertext I can't decrypt with this key" and "not configured" both raise
    :class:`AssistantNotConfigured`; operational failures (auth, network) are
    left to propagate.
"""

from __future__ import annotations

import abc

import boto3
from botocore.exceptions import BotoCoreError, ClientError
from django.conf import settings

from pi_dash.assistant.errors import AssistantNotConfigured


class CipherBackend(abc.ABC):
    """Encrypts/decrypts a single small secret (a BYOK provider key) at rest."""

    @abc.abstractmethod
    def is_configured(self) -> bool:
        """Whether the backend has everything it needs to encrypt/decrypt."""

    @abc.abstractmethod
    def encrypt(self, plaintext: str) -> bytes:
        """Encrypt ``plaintext``; returns the opaque ciphertext to store."""

    @abc.abstractmethod
    def decrypt(self, token: bytes) -> str:
        """Decrypt a stored ciphertext back to plaintext."""

    @abc.abstractmethod
    def rotate(self, token: bytes) -> bytes:
        """Re-encrypt a stored ciphertext under the current key material."""


class AwsKmsBackend(CipherBackend):
    """AWS KMS backend — direct Encrypt/Decrypt (BYOK keys are < KMS's 4KB
    limit, so no data-key envelope). ``api_key_encrypted`` holds the raw KMS
    ``CiphertextBlob``.

    Config: ``ASSISTANT_KMS_KEY_ID`` (CMK id/ARN/alias), ``AWS_REGION``, and an
    optional ``ASSISTANT_KMS_ENDPOINT_URL`` (e.g. LocalStack) for local /
    self-hosted setups without a real AWS account.
    """

    # KMS error codes meaning "this ciphertext can't be decrypted with this
    # key" (tampered/foreign blob or wrong CMK) — a data problem → reported as
    # AssistantNotConfigured. Everything else (AccessDenied, throttling,
    # endpoint down) is operational and propagates.
    _UNDECRYPTABLE_CODES = frozenset(
        {"InvalidCiphertextException", "IncorrectKeyException", "NotFoundException"}
    )

    def __init__(self, client=None):
        # ``client`` is injectable for tests; built lazily otherwise.
        self._client = client

    def _key_id(self) -> str:
        return (getattr(settings, "ASSISTANT_KMS_KEY_ID", "") or "").strip()

    def _require_key_id(self) -> str:
        key_id = self._key_id()
        if not key_id:
            raise AssistantNotConfigured(
                "ASSISTANT_KMS_KEY_ID is not set; BYOK keys cannot be stored."
            )
        return key_id

    def _kms(self):
        if self._client is None:
            kwargs = {}
            region = (getattr(settings, "AWS_REGION", "") or "").strip()
            if region:
                kwargs["region_name"] = region
            endpoint = (getattr(settings, "ASSISTANT_KMS_ENDPOINT_URL", "") or "").strip()
            if endpoint:
                kwargs["endpoint_url"] = endpoint
            self._client = boto3.client("kms", **kwargs)
        return self._client

    def is_configured(self) -> bool:
        return bool(self._key_id())

    def encrypt(self, plaintext: str) -> bytes:
        key_id = self._require_key_id()
        try:
            resp = self._kms().encrypt(KeyId=key_id, Plaintext=plaintext.encode("utf-8"))
        except (BotoCoreError, ClientError) as exc:
            raise AssistantNotConfigured(f"KMS encrypt failed: {exc}") from exc
        return resp["CiphertextBlob"]

    def decrypt(self, token: bytes) -> str:
        if not token:
            return ""
        # Pin KeyId so a ciphertext can only be decrypted by the CMK we expect.
        key_id = self._require_key_id()
        try:
            resp = self._kms().decrypt(CiphertextBlob=bytes(token), KeyId=key_id)
        except ClientError as exc:
            code = exc.response.get("Error", {}).get("Code", "")
            if code in self._UNDECRYPTABLE_CODES:
                raise AssistantNotConfigured(
                    "Stored BYOK key could not be decrypted with the configured KMS key."
                ) from exc
            raise
        return resp["Plaintext"].decode("utf-8")

    def rotate(self, token: bytes) -> bytes:
        key_id = self._require_key_id()
        try:
            resp = self._kms().re_encrypt(CiphertextBlob=bytes(token), DestinationKeyId=key_id)
        except (BotoCoreError, ClientError) as exc:
            raise AssistantNotConfigured(f"KMS re-encrypt failed: {exc}") from exc
        return resp["CiphertextBlob"]


# Registry of available backends, keyed by the ``ASSISTANT_CRYPTO_BACKEND``
# value. Add a provider here once its CipherBackend is implemented.
_BACKENDS: dict[str, type[CipherBackend]] = {
    "aws-kms": AwsKmsBackend,
}

# Process-wide cached backend instance (its KMS client is reused across calls).
_backend: CipherBackend | None = None


def get_backend() -> CipherBackend:
    global _backend
    if _backend is None:
        name = (getattr(settings, "ASSISTANT_CRYPTO_BACKEND", "") or "aws-kms").strip()
        backend_cls = _BACKENDS.get(name)
        if backend_cls is None:
            raise AssistantNotConfigured(
                f"unknown ASSISTANT_CRYPTO_BACKEND {name!r} "
                f"(available: {', '.join(sorted(_BACKENDS))})"
            )
        _backend = backend_cls()
    return _backend


# --- Public API: stable, provider-agnostic; delegates to the active backend. ---


def is_configured() -> bool:
    return get_backend().is_configured()


def encrypt(plaintext: str) -> bytes:
    return get_backend().encrypt(plaintext)


def decrypt(token: bytes) -> str:
    return get_backend().decrypt(token)


def rotate(token: bytes) -> bytes:
    return get_backend().rotate(token)
