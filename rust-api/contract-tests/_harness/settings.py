# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Suite configuration, always from the environment."""

import os


def _required(name):
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"contract suite requires env {name}")
    return value


def base_url():
    return _required("BASE_URL").rstrip("/")


def database_url():
    return _required("DATABASE_URL")


def secret_key():
    # Must equal the live server's SECRET_KEY: sessions are minted locally
    # with the same signing key the server verifies.
    return _required("SECRET_KEY")


def redis_url():
    return os.environ.get("REDIS_URL", "")
