# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Centralized configuration access for the Pi Dash backend.

Read every config value through this package instead of touching
``os.environ`` or the ``InstanceConfiguration`` table directly::

    from pi_dash import config

    host = config.get_config("EMAIL_HOST")
    signup_on = config.get_bool("ENABLE_SIGNUP")

Where each key is read from (env vs. database) is declared once in
``pi_dash.config.registry``.
"""

from .accessor import (
    ConfigError,
    get_bool,
    get_config,
    get_int,
    get_many,
)
from .registry import CONFIG, all_keys, is_registered

__all__ = [
    "CONFIG",
    "ConfigError",
    "all_keys",
    "get_bool",
    "get_config",
    "get_int",
    "get_many",
    "is_registered",
]
