# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash import config
from pi_dash.config import accessor, registry
from pi_dash.license.models import InstanceConfiguration
from pi_dash.license.utils.encryption import encrypt_data


# --------------------------------------------------------------------------- #
# env-sourced keys
# --------------------------------------------------------------------------- #
@pytest.mark.unit
def test_env_key_reads_from_environment(monkeypatch):
    monkeypatch.setenv("POSTHOG_API_KEY", "ph_from_env")
    assert config.get_config("POSTHOG_API_KEY") == "ph_from_env"


@pytest.mark.unit
def test_env_key_falls_back_to_registry_default(monkeypatch):
    monkeypatch.delenv("POSTHOG_API_KEY", raising=False)
    # POSTHOG_API_KEY default is None
    assert config.get_config("POSTHOG_API_KEY") is None


@pytest.mark.unit
def test_env_key_resolves_without_database(monkeypatch):
    """env-tier keys must work even if the ORM is untouched (settings import)."""
    monkeypatch.setattr(registry, "CONFIG", {"FOO": {"source": "env", "default": "d"}})
    monkeypatch.setattr(accessor, "CONFIG", registry.CONFIG)
    monkeypatch.delenv("FOO", raising=False)
    assert config.get_config("FOO") == "d"


# --------------------------------------------------------------------------- #
# db-sourced keys
# --------------------------------------------------------------------------- #
@pytest.mark.unit
@pytest.mark.django_db
def test_db_key_reads_from_instance_configuration():
    InstanceConfiguration.objects.create(key="EMAIL_HOST", value="smtp.test", category="SMTP")
    assert config.get_config("EMAIL_HOST") == "smtp.test"


@pytest.mark.unit
@pytest.mark.django_db
def test_db_key_missing_row_returns_default():
    # EMAIL_PORT default is "587"; no row created
    assert config.get_config("EMAIL_PORT") == "587"


@pytest.mark.unit
@pytest.mark.django_db
def test_db_secret_key_is_decrypted():
    InstanceConfiguration.objects.create(
        key="EMAIL_HOST_PASSWORD",
        value=encrypt_data("s3cret"),
        category="SMTP",
        is_encrypted=True,
    )
    assert config.get_config("EMAIL_HOST_PASSWORD") == "s3cret"


@pytest.mark.unit
@pytest.mark.django_db
def test_get_many_batches_db_and_env(monkeypatch):
    monkeypatch.setenv("POSTHOG_HOST", "https://ph.test")
    InstanceConfiguration.objects.create(key="EMAIL_HOST", value="smtp.test", category="SMTP")
    result = config.get_many(["EMAIL_HOST", "EMAIL_PORT", "POSTHOG_HOST"])
    assert result == {
        "EMAIL_HOST": "smtp.test",
        "EMAIL_PORT": "587",  # default, no row
        "POSTHOG_HOST": "https://ph.test",
    }


# --------------------------------------------------------------------------- #
# typed helpers
# --------------------------------------------------------------------------- #
@pytest.mark.unit
@pytest.mark.django_db
def test_get_bool():
    InstanceConfiguration.objects.create(key="ENABLE_SIGNUP", value="1", category="AUTHENTICATION")
    InstanceConfiguration.objects.create(key="ENABLE_SMTP", value="0", category="SMTP")
    assert config.get_bool("ENABLE_SIGNUP") is True
    assert config.get_bool("ENABLE_SMTP") is False


@pytest.mark.unit
@pytest.mark.django_db
def test_get_int():
    assert config.get_int("EMAIL_PORT") == 587  # default "587"


@pytest.mark.unit
def test_get_int_invalid_returns_fallback(monkeypatch):
    monkeypatch.setenv("POSTHOG_HOST", "not-a-number")
    assert config.get_int("POSTHOG_HOST", default=42) == 42


# --------------------------------------------------------------------------- #
# unregistered keys
# --------------------------------------------------------------------------- #
@pytest.mark.unit
def test_unregistered_key_strict_raises(monkeypatch):
    # the test settings module ends with ".test" -> strict mode
    monkeypatch.setenv("WHATEVER_KEY", "x")
    with pytest.raises(config.ConfigError):
        config.get_config("WHATEVER_KEY")


@pytest.mark.unit
def test_unregistered_key_lenient_falls_back_to_env(monkeypatch):
    monkeypatch.setattr(accessor, "_strict", lambda: False)
    monkeypatch.setenv("WHATEVER_KEY", "from_env")
    assert config.get_config("WHATEVER_KEY") == "from_env"


# --------------------------------------------------------------------------- #
# registry integrity
# --------------------------------------------------------------------------- #
@pytest.mark.unit
def test_every_entry_has_valid_source():
    for key, entry in registry.CONFIG.items():
        assert entry["source"] in ("env", "db"), f"{key} has bad source {entry.get('source')!r}"


@pytest.mark.unit
def test_github_app_keys_classified_by_secret():
    """GitHub App identity is db-sourced (god-mode editable); the secrets are
    env-sourced (SSM in cloud, env locally) and flagged secret."""
    for key in ("GITHUB_APP_ID", "GITHUB_APP_SLUG", "GITHUB_APP_CLIENT_ID"):
        assert registry.CONFIG[key]["source"] == "db", key
    for key in ("GITHUB_APP_PRIVATE_KEY", "GITHUB_APP_WEBHOOK_SECRET", "GITHUB_APP_CLIENT_SECRET"):
        assert registry.CONFIG[key]["source"] == "env", key
        assert registry.CONFIG[key].get("secret") is True, key


@pytest.mark.unit
def test_source_overrides_are_applied(monkeypatch):
    monkeypatch.setattr(registry, "_RESOLVER_CONFIG", {"K": {"source": "db", "default": None}})
    monkeypatch.setattr(registry, "CONFIG_SOURCE_OVERRIDES", {"K": "env"})
    rebuilt = registry._build_config()
    assert rebuilt["K"]["source"] == "env"


@pytest.mark.unit
def test_env_var_override_flips_keys_to_env(monkeypatch):
    """The cloud's SSM-native seam: PIDASH_CONFIG_ENV_KEYS forces keys to env."""
    monkeypatch.setenv(registry.ENV_KEYS_OVERRIDE_VAR, "EMAIL_HOST, GOOGLE_CLIENT_SECRET ,")
    rebuilt = registry._build_config()
    assert rebuilt["EMAIL_HOST"]["source"] == "env"
    assert rebuilt["GOOGLE_CLIENT_SECRET"]["source"] == "env"
    # untouched key stays db
    assert rebuilt["ENABLE_SIGNUP"]["source"] == "db"
