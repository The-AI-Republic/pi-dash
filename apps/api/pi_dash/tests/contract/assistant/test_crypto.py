# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import types

import pytest

from pi_dash.assistant import crypto
from pi_dash.assistant.errors import AssistantNotConfigured
from pi_dash.assistant.runtime import llm
from pi_dash.tests.contract.assistant.conftest import FakeKMS


def test_roundtrip(kms_crypto):
    token = crypto.encrypt("sk-secret")
    assert token != b"sk-secret"
    assert b"sk-secret" not in token  # ciphertext is opaque, no plaintext
    assert crypto.decrypt(token) == "sk-secret"


def test_reencrypt_roundtrips(kms_crypto):
    token = crypto.encrypt("sk-rotate")
    rotated = crypto.rotate(token)
    assert rotated != token
    assert crypto.decrypt(rotated) == "sk-rotate"


def test_missing_key_raises(settings, monkeypatch):
    settings.ASSISTANT_KMS_KEY_ID = ""
    monkeypatch.setattr(crypto, "_backend", crypto.AwsKmsBackend(client=FakeKMS()))
    assert not crypto.is_configured()
    with pytest.raises(AssistantNotConfigured):
        crypto.encrypt("x")


def test_unknown_backend_raises(settings, monkeypatch):
    settings.ASSISTANT_CRYPTO_BACKEND = "gcp-kms"  # not implemented
    monkeypatch.setattr(crypto, "_backend", None)  # force re-selection
    with pytest.raises(AssistantNotConfigured):
        crypto.get_backend()


def test_decrypt_with_wrong_key_raises(settings, kms_crypto):
    token = crypto.encrypt("sk-secret")
    # A different CMK cannot decrypt a ciphertext minted under another key.
    settings.ASSISTANT_KMS_KEY_ID = "arn:aws:kms:us-west-2:000000000000:key/other-cmk"
    with pytest.raises(AssistantNotConfigured):
        crypto.decrypt(token)


def test_decrypt_empty_returns_empty(kms_crypto):
    assert crypto.decrypt(b"") == ""


def _count_decrypts(monkeypatch):
    """Wrap crypto.decrypt with a call counter (real decrypt still runs)."""
    calls = {"n": 0}
    real = crypto.decrypt

    def counting(token):
        calls["n"] += 1
        return real(token)

    monkeypatch.setattr(crypto, "decrypt", counting)
    return calls


def test_decrypted_key_is_cached(kms_crypto, settings, monkeypatch):
    settings.ASSISTANT_KEY_CACHE_TTL = 300
    settings.ASSISTANT_KEY_CACHE_MAXSIZE = 100
    monkeypatch.setattr(llm, "_key_cache", None)  # fresh cache
    cfg = types.SimpleNamespace(api_key_encrypted=crypto.encrypt("sk-secret"))
    calls = _count_decrypts(monkeypatch)
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"
    assert calls["n"] == 1  # second call served from cache, no KMS round-trip


def test_cache_is_keyed_by_ciphertext(kms_crypto, settings, monkeypatch):
    # A different stored key (different ciphertext) must not collide; changing a
    # user's key naturally invalidates because the ciphertext changes.
    settings.ASSISTANT_KEY_CACHE_TTL = 300
    monkeypatch.setattr(llm, "_key_cache", None)
    cfg_a = types.SimpleNamespace(api_key_encrypted=crypto.encrypt("sk-a"))
    cfg_b = types.SimpleNamespace(api_key_encrypted=crypto.encrypt("sk-b"))
    calls = _count_decrypts(monkeypatch)
    assert llm.get_decrypted_api_key(cfg_a) == "sk-a"
    assert llm.get_decrypted_api_key(cfg_b) == "sk-b"
    assert llm.get_decrypted_api_key(cfg_a) == "sk-a"
    assert llm.get_decrypted_api_key(cfg_b) == "sk-b"
    assert calls["n"] == 2  # one decrypt per distinct key, rest cached


def test_cache_disabled_when_ttl_zero(kms_crypto, settings, monkeypatch):
    settings.ASSISTANT_KEY_CACHE_TTL = 0
    monkeypatch.setattr(llm, "_key_cache", None)
    cfg = types.SimpleNamespace(api_key_encrypted=crypto.encrypt("sk-secret"))
    calls = _count_decrypts(monkeypatch)
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"
    assert calls["n"] == 2  # caching off -> decrypt every call
