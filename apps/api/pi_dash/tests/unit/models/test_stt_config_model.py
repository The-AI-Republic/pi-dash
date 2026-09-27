# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Unit tests for the ``UserSTTConfig`` model (issue PDASHOSS01-149).

The model mirrors ``UserLLMConfig``: a per-user, workspace-global row that
stores the OpenAI-compatible transcription endpoint plus an encrypted API key.
The key is stored as opaque ciphertext produced by ``pi_dash.assistant.crypto``
— there is no second key-handling path — so the round-trip test below encrypts
through that module and decrypts back out.
"""

import pytest
from cryptography.fernet import Fernet

from pi_dash.assistant import crypto
from pi_dash.assistant.models import UserSTTConfig


@pytest.mark.unit
class TestUserSTTConfigModel:
    """Test the UserSTTConfig model."""

    @pytest.mark.django_db
    def test_defaults_and_has_api_key_false(self, create_user):
        """A freshly created config has empty fields and no key."""
        config = UserSTTConfig.objects.create(user=create_user)

        assert config.pk is not None
        assert config.user == create_user
        assert config.base_url == ""
        assert config.model_name == ""
        assert config.api_key_encrypted is None
        assert config.last_verified_at is None
        assert config.created_at is not None
        assert config.updated_at is not None
        assert config.has_api_key is False

    @pytest.mark.django_db
    def test_one_to_one_per_user(self, create_user):
        """The user relation is one-to-one and reachable via the reverse accessor."""
        config = UserSTTConfig.objects.create(user=create_user)

        assert create_user.assistant_stt_config == config

    @pytest.mark.django_db
    def test_str(self, create_user):
        config = UserSTTConfig.objects.create(user=create_user)

        assert str(config) == f"UserSTTConfig({create_user.id})"

    @pytest.mark.django_db
    def test_encrypted_key_round_trips_through_crypto(self, create_user, settings, monkeypatch):
        """Storing a key encrypted by ``crypto`` and reading it back decrypts cleanly.

        Proves the model reuses the existing BYOK crypto seam rather than a
        second key-handling path, and that ``has_api_key`` reflects the stored
        ciphertext.
        """
        settings.ASSISTANT_CRYPTO_BACKEND = "fernet"
        settings.ASSISTANT_ENCRYPTION_KEY = Fernet.generate_key().decode()
        monkeypatch.setattr(crypto, "_backend", None)  # force backend re-selection
        assert crypto.is_configured()

        plaintext = "sk-test-transcription-key"
        config = UserSTTConfig.objects.create(
            user=create_user,
            base_url="https://stt.example.com/v1",
            model_name="whisper-1",
            api_key_encrypted=crypto.encrypt(plaintext),
        )
        config.refresh_from_db()

        assert config.has_api_key is True
        # The stored value is opaque ciphertext, never the plaintext key.
        assert bytes(config.api_key_encrypted) != plaintext.encode()
        assert crypto.decrypt(bytes(config.api_key_encrypted)) == plaintext
