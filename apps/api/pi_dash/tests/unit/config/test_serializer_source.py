# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.config import registry
from pi_dash.license.api.serializers import InstanceConfigurationSerializer
from pi_dash.license.models import InstanceConfiguration


@pytest.mark.unit
@pytest.mark.django_db
def test_serializer_exposes_source_and_is_managed(monkeypatch):
    # Simulate the cloud flipping EMAIL_HOST to env-sourced.
    patched = dict(registry.CONFIG)
    patched["EMAIL_HOST"] = {"source": "env", "default": ""}
    monkeypatch.setattr("pi_dash.license.api.serializers.configuration.CONFIG", patched)

    InstanceConfiguration.objects.create(key="EMAIL_HOST", value="smtp.x", category="SMTP")
    InstanceConfiguration.objects.create(key="ENABLE_SIGNUP", value="1", category="AUTHENTICATION")

    rows = {
        r["key"]: r
        for r in InstanceConfigurationSerializer(InstanceConfiguration.objects.all(), many=True).data
    }
    assert rows["EMAIL_HOST"]["source"] == "env"
    assert rows["EMAIL_HOST"]["is_managed"] is True
    assert rows["ENABLE_SIGNUP"]["source"] == "db"
    assert rows["ENABLE_SIGNUP"]["is_managed"] is False
