# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest
from cryptography.fernet import Fernet

from pi_dash.assistant import crypto
from pi_dash.assistant.errors import AssistantNotConfigured


def test_roundtrip(settings):
    settings.ASSISTANT_ENCRYPTION_KEY = Fernet.generate_key().decode()
    token = crypto.encrypt("sk-secret")
    assert token != b"sk-secret"
    assert b"sk-secret" not in token  # not stored in plaintext
    assert crypto.decrypt(token) == "sk-secret"


def test_multifernet_rotation(settings):
    old_key = Fernet.generate_key().decode()
    new_key = Fernet.generate_key().decode()
    settings.ASSISTANT_ENCRYPTION_KEY = old_key
    token = crypto.encrypt("sk-rotate")

    # New primary key prepended; old key still present -> old token still decrypts.
    settings.ASSISTANT_ENCRYPTION_KEY = f"{new_key},{old_key}"
    assert crypto.decrypt(token) == "sk-rotate"

    rotated = crypto.rotate(token)
    assert crypto.decrypt(rotated) == "sk-rotate"


def test_missing_key_raises(settings):
    settings.ASSISTANT_ENCRYPTION_KEY = ""
    assert not crypto.is_configured()
    with pytest.raises(AssistantNotConfigured):
        crypto.encrypt("x")


def test_decrypt_with_wrong_keyset_raises(settings):
    settings.ASSISTANT_ENCRYPTION_KEY = Fernet.generate_key().decode()
    token = crypto.encrypt("sk-secret")
    settings.ASSISTANT_ENCRYPTION_KEY = Fernet.generate_key().decode()  # different key
    with pytest.raises(AssistantNotConfigured):
        crypto.decrypt(token)
