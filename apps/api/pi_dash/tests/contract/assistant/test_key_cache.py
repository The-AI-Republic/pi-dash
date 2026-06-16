# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Tests for the short-lived in-process cache of decrypted BYOK keys.

Covers :func:`pi_dash.assistant.runtime.llm.get_decrypted_api_key` (the cache
itself) and its use from :func:`resolve_byok_model` (the hot path). The cache
must: serve repeat calls without a KMS round-trip, key on the ciphertext (so a
key change auto-invalidates and identical plaintexts never share a slot), evict
by TTL and by LRU at capacity, be disable-able, and stay correct under
concurrency.
"""

from __future__ import annotations

import threading
import types

import pytest
from cachetools import TTLCache

from pi_dash.assistant import crypto
from pi_dash.assistant.runtime import llm
from pi_dash.tests.contract.assistant.conftest import configure_llm


@pytest.fixture
def fresh_cache(monkeypatch):
    """Start every test with an uninitialised module-level cache."""
    monkeypatch.setattr(llm, "_key_cache", None)


def _decrypt_counter(monkeypatch):
    """Wrap crypto.decrypt with a call counter (the real decrypt still runs).

    Install this AFTER any ``crypto.encrypt`` setup so only the calls under test
    are counted.
    """
    calls = {"n": 0}
    real = crypto.decrypt

    def counting(token):
        calls["n"] += 1
        return real(token)

    monkeypatch.setattr(crypto, "decrypt", counting)
    return calls


def _cfg(plaintext: str):
    """Minimal stand-in for UserLLMConfig — only api_key_encrypted is read."""
    return types.SimpleNamespace(api_key_encrypted=crypto.encrypt(plaintext))


# --- hit / miss basics --------------------------------------------------------


def test_repeated_calls_hit_cache(kms_crypto, settings, fresh_cache, monkeypatch):
    settings.ASSISTANT_KEY_CACHE_TTL = 300
    cfg = _cfg("sk-secret")
    calls = _decrypt_counter(monkeypatch)
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"
    assert calls["n"] == 1  # only the first call hit KMS


def test_distinct_keys_cached_independently(kms_crypto, settings, fresh_cache, monkeypatch):
    settings.ASSISTANT_KEY_CACHE_TTL = 300
    cfg_a, cfg_b = _cfg("sk-a"), _cfg("sk-b")
    calls = _decrypt_counter(monkeypatch)
    assert llm.get_decrypted_api_key(cfg_a) == "sk-a"
    assert llm.get_decrypted_api_key(cfg_b) == "sk-b"
    assert llm.get_decrypted_api_key(cfg_a) == "sk-a"  # cached
    assert llm.get_decrypted_api_key(cfg_b) == "sk-b"  # cached
    assert calls["n"] == 2  # one decrypt per distinct key


# --- ciphertext keying / invalidation ----------------------------------------


def test_rotated_key_is_not_served_stale(kms_crypto, settings, fresh_cache, monkeypatch):
    """A new ciphertext (user changed their key) must re-decrypt to the new key."""
    settings.ASSISTANT_KEY_CACHE_TTL = 300
    calls = _decrypt_counter(monkeypatch)
    assert llm.get_decrypted_api_key(_cfg("sk-old")) == "sk-old"
    assert llm.get_decrypted_api_key(_cfg("sk-new")) == "sk-new"  # not "sk-old"
    assert calls["n"] == 2


def test_same_plaintext_different_ciphertext_not_shared(kms_crypto, settings, fresh_cache, monkeypatch):
    # KMS encryption is non-deterministic, so two rows with the SAME provider key
    # have distinct ciphertext and must not share a cache slot (keying is on the
    # ciphertext, never the plaintext).
    settings.ASSISTANT_KEY_CACHE_TTL = 300
    cfg1, cfg2 = _cfg("sk-same"), _cfg("sk-same")
    assert cfg1.api_key_encrypted != cfg2.api_key_encrypted
    calls = _decrypt_counter(monkeypatch)
    assert llm.get_decrypted_api_key(cfg1) == "sk-same"
    assert llm.get_decrypted_api_key(cfg2) == "sk-same"
    assert calls["n"] == 2


# --- eviction: capacity (LRU) and time (TTL) ---------------------------------


def test_lru_eviction_at_maxsize(kms_crypto, settings, fresh_cache, monkeypatch):
    settings.ASSISTANT_KEY_CACHE_TTL = 300
    settings.ASSISTANT_KEY_CACHE_MAXSIZE = 1  # room for one entry only
    cfg_a, cfg_b = _cfg("sk-a"), _cfg("sk-b")
    calls = _decrypt_counter(monkeypatch)
    assert llm.get_decrypted_api_key(cfg_a) == "sk-a"  # n=1, cache={a}
    assert llm.get_decrypted_api_key(cfg_b) == "sk-b"  # n=2, evicts a, cache={b}
    assert llm.get_decrypted_api_key(cfg_a) == "sk-a"  # a evicted -> n=3
    assert calls["n"] == 3


def test_ttl_expiry_triggers_redecrypt(kms_crypto, settings, monkeypatch):
    # Inject a cache with a controllable clock so TTL expiry is deterministic
    # (no sleeping).
    clock = {"t": 1000.0}
    monkeypatch.setattr(llm, "_key_cache", TTLCache(maxsize=10, ttl=300, timer=lambda: clock["t"]))
    settings.ASSISTANT_KEY_CACHE_TTL = 300  # keep caching enabled
    cfg = _cfg("sk-secret")
    calls = _decrypt_counter(monkeypatch)
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"  # n=1
    clock["t"] += 100  # still within TTL
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"  # cached, n=1
    clock["t"] += 250  # 350s elapsed > 300s TTL
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"  # expired -> n=2
    assert calls["n"] == 2


# --- disable / edge inputs ---------------------------------------------------


def test_ttl_zero_disables_cache(kms_crypto, settings, fresh_cache, monkeypatch):
    settings.ASSISTANT_KEY_CACHE_TTL = 0
    cfg = _cfg("sk-secret")
    calls = _decrypt_counter(monkeypatch)
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"
    assert calls["n"] == 2  # decrypt every call


def test_negative_ttl_disables_cache(kms_crypto, settings, fresh_cache, monkeypatch):
    settings.ASSISTANT_KEY_CACHE_TTL = -5
    cfg = _cfg("sk-secret")
    calls = _decrypt_counter(monkeypatch)
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"
    assert llm.get_decrypted_api_key(cfg) == "sk-secret"
    assert calls["n"] == 2


def test_empty_token_returns_empty(kms_crypto, settings, fresh_cache, monkeypatch):
    # Defensive: empty ciphertext bypasses the cache and decrypts to "" (never
    # reached in practice — has_api_key gates it — but must not raise on bytes()).
    settings.ASSISTANT_KEY_CACHE_TTL = 300
    cfg = types.SimpleNamespace(api_key_encrypted=b"")
    assert llm.get_decrypted_api_key(cfg) == ""


# --- concurrency -------------------------------------------------------------


def test_concurrent_access_is_thread_safe(kms_crypto, settings, fresh_cache, monkeypatch):
    # The resolve path runs under sync_to_async (a thread pool), so the cache
    # must tolerate concurrent get/set without corruption or error.
    settings.ASSISTANT_KEY_CACHE_TTL = 300
    settings.ASSISTANT_KEY_CACHE_MAXSIZE = 100
    cfg = _cfg("sk-secret")
    results: list[str] = []
    errors: list[Exception] = []

    def worker():
        try:
            results.append(llm.get_decrypted_api_key(cfg))
        except Exception as exc:  # noqa: BLE001
            errors.append(exc)

    threads = [threading.Thread(target=worker) for _ in range(20)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    assert not errors
    assert results == ["sk-secret"] * 20


# --- integration: the actual hot path ----------------------------------------


def test_resolve_byok_model_uses_cache(world, kms_crypto, settings, fresh_cache, monkeypatch):
    # Two turns for the same user must build a model with only one KMS Decrypt.
    settings.ASSISTANT_KEY_CACHE_TTL = 300
    configure_llm(world.admin)  # creates UserLLMConfig with an encrypted key
    calls = _decrypt_counter(monkeypatch)
    model_1 = llm.resolve_byok_model(world.admin)
    model_2 = llm.resolve_byok_model(world.admin)
    assert model_1 is not None
    assert model_2 is not None
    assert calls["n"] == 1  # second resolve served the key from cache
