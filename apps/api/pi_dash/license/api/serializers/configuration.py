# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from .base import BaseSerializer
from pi_dash.config.registry import CONFIG
from pi_dash.license.models import InstanceConfiguration
from pi_dash.license.utils.encryption import decrypt_data


WRITE_ONLY_CONFIG_KEYS = {
    "GITHUB_APP_PRIVATE_KEY",
    "GITHUB_APP_WEBHOOK_SECRET",
    "GITHUB_APP_CLIENT_SECRET",
}


class InstanceConfigurationSerializer(BaseSerializer):
    class Meta:
        model = InstanceConfiguration
        fields = "__all__"

    def to_representation(self, instance):
        data = super().to_representation(instance)
        if instance.key in WRITE_ONLY_CONFIG_KEYS:
            data["value"] = "set" if instance.value else ""
            data["is_write_only"] = True
            return data

        # Decrypt secrets value
        if instance.is_encrypted and instance.value is not None:
            data["value"] = decrypt_data(instance.value)

        # Surface where this key is actually read from. When a key is sourced
        # from the environment (e.g. SSM-managed in the cloud), the DB row is
        # ignored at read time, so the admin UI should render it read-only.
        entry = CONFIG.get(instance.key)
        source = entry["source"] if entry else "db"
        data["source"] = source
        data["is_managed"] = source == "env"

        return data
