# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Python imports
import os

# Module imports
from pi_dash.config.registry import CONFIG
from pi_dash.license.models import InstanceConfiguration
from pi_dash.license.utils.encryption import decrypt_data


def _source(key):
    """Where to read ``key`` from. Unregistered keys default to env, matching
    the centralized accessor's policy (see pi_dash.config)."""
    entry = CONFIG.get(key)
    return entry["source"] if entry else "env"


# Helper function to return value from the passed key.
#
# This is the legacy resolver, kept as a thin compatibility shim over the
# per-key source registry. It honors the caller-supplied ``default`` exactly as
# before; the only change from the old behavior is that the source (db vs env)
# is now decided per key via the registry instead of by the global SKIP_ENV_VAR
# flag. New code should prefer ``pi_dash.config.get_config`` directly.
def get_configuration_value(keys):
    db_keys = [key.get("key") for key in keys if _source(key.get("key")) == "db"]
    rows = {}
    if db_keys:
        rows = {
            row["key"]: row
            for row in InstanceConfiguration.objects.filter(key__in=db_keys).values(
                "key", "value", "is_encrypted"
            )
        }

    environment_list = []
    for key in keys:
        name = key.get("key")
        default = key.get("default")
        if _source(name) == "db":
            row = rows.get(name)
            if row is None:
                environment_list.append(default)
            elif row.get("is_encrypted", False):
                environment_list.append(decrypt_data(row.get("value")))
            else:
                environment_list.append(row.get("value"))
        else:
            environment_list.append(os.environ.get(name, default))

    return tuple(environment_list)


def get_email_configuration():
    return get_configuration_value(
        [
            {"key": "EMAIL_HOST", "default": os.environ.get("EMAIL_HOST")},
            {"key": "EMAIL_HOST_USER", "default": os.environ.get("EMAIL_HOST_USER")},
            {
                "key": "EMAIL_HOST_PASSWORD",
                "default": os.environ.get("EMAIL_HOST_PASSWORD"),
            },
            {"key": "EMAIL_PORT", "default": os.environ.get("EMAIL_PORT", 587)},
            {"key": "EMAIL_USE_TLS", "default": os.environ.get("EMAIL_USE_TLS", "1")},
            {"key": "EMAIL_USE_SSL", "default": os.environ.get("EMAIL_USE_SSL", "0")},
            {
                "key": "EMAIL_FROM",
                "default": os.environ.get("EMAIL_FROM", "Team Pi Dash <team@airepublic.com>"),
            },
        ]
    )
