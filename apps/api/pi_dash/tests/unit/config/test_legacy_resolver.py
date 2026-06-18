# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regression tests for the legacy get_configuration_value resolver after the
SKIP_ENV_VAR removal. Behavior must match the old resolver: db-sourced keys
read the InstanceConfiguration row (decrypting secrets), env-sourced keys read
the environment, and the caller-supplied ``default`` is the fallback in both
cases when the value is absent."""

import pytest

from pi_dash.config import registry
from pi_dash.license.models import InstanceConfiguration
from pi_dash.license.utils.encryption import encrypt_data
from pi_dash.license.utils.instance_value import get_configuration_value


@pytest.mark.unit
@pytest.mark.django_db
def test_db_key_reads_row():
    InstanceConfiguration.objects.create(key="EMAIL_HOST", value="smtp.test", category="SMTP")
    (value,) = get_configuration_value([{"key": "EMAIL_HOST", "default": "fallback"}])
    assert value == "smtp.test"


@pytest.mark.unit
@pytest.mark.django_db
def test_db_key_missing_row_uses_caller_default():
    (value,) = get_configuration_value([{"key": "EMAIL_HOST", "default": "fallback"}])
    assert value == "fallback"


@pytest.mark.unit
@pytest.mark.django_db
def test_db_secret_is_decrypted():
    InstanceConfiguration.objects.create(
        key="GOOGLE_CLIENT_SECRET", value=encrypt_data("shh"), category="GOOGLE", is_encrypted=True
    )
    (value,) = get_configuration_value([{"key": "GOOGLE_CLIENT_SECRET", "default": None}])
    assert value == "shh"


@pytest.mark.unit
@pytest.mark.django_db
def test_env_sourced_key_reads_environment(monkeypatch):
    # POSTHOG_API_KEY is source="env" in the registry
    monkeypatch.setenv("POSTHOG_API_KEY", "ph_env")
    (value,) = get_configuration_value([{"key": "POSTHOG_API_KEY", "default": None}])
    assert value == "ph_env"


@pytest.mark.unit
@pytest.mark.django_db
def test_env_sourced_key_uses_caller_default_when_unset(monkeypatch):
    monkeypatch.delenv("POSTHOG_HOST", raising=False)
    (value,) = get_configuration_value([{"key": "POSTHOG_HOST", "default": "https://default"}])
    assert value == "https://default"


@pytest.mark.unit
@pytest.mark.django_db
def test_mixed_keys_single_call(monkeypatch):
    monkeypatch.setenv("POSTHOG_API_KEY", "ph_env")
    InstanceConfiguration.objects.create(key="EMAIL_HOST", value="smtp.test", category="SMTP")
    result = get_configuration_value(
        [
            {"key": "EMAIL_HOST", "default": None},
            {"key": "POSTHOG_API_KEY", "default": None},
        ]
    )
    assert result == ("smtp.test", "ph_env")


@pytest.mark.unit
@pytest.mark.django_db
def test_cloud_style_override_routes_db_key_to_env(monkeypatch):
    """Simulate the cloud overlay flipping a db key to env: the resolver must
    then read it from the environment, not the DB row."""
    patched = dict(registry.CONFIG)
    patched["EMAIL_HOST"] = {"source": "env", "default": ""}
    monkeypatch.setattr(registry, "CONFIG", patched)
    monkeypatch.setattr("pi_dash.license.utils.instance_value.CONFIG", patched)
    InstanceConfiguration.objects.create(key="EMAIL_HOST", value="db_value", category="SMTP")
    monkeypatch.setenv("EMAIL_HOST", "env_value")
    (value,) = get_configuration_value([{"key": "EMAIL_HOST", "default": ""}])
    assert value == "env_value"
