# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.assistant import crypto
from pi_dash.assistant.errors import AssistantNotConfigured
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
    monkeypatch.setattr(crypto, "_client", FakeKMS())
    assert not crypto.is_configured()
    with pytest.raises(AssistantNotConfigured):
        crypto.encrypt("x")


def test_decrypt_with_wrong_key_raises(settings, kms_crypto):
    token = crypto.encrypt("sk-secret")
    # A different CMK cannot decrypt a ciphertext minted under another key.
    settings.ASSISTANT_KMS_KEY_ID = "arn:aws:kms:us-west-2:000000000000:key/other-cmk"
    with pytest.raises(AssistantNotConfigured):
        crypto.decrypt(token)


def test_decrypt_empty_returns_empty(kms_crypto):
    assert crypto.decrypt(b"") == ""
