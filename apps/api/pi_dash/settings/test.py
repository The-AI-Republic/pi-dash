# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Test Settings"""

import os

# Test defaults must be applied BEFORE common.py is imported, because common.py
# eagerly reads ``os.environ`` for several settings (WEB_URL, APP_BASE_URL,
# EMAIL_HOST, etc.) and never re-reads them. CI runners do not set these and
# they are never going to be set in unit-test environments, so seed safe
# defaults here so auth + redirect paths don't blow up at import time.
os.environ.setdefault("WEB_URL", "http://localhost")
os.environ.setdefault("APP_BASE_URL", "http://localhost")
os.environ.setdefault("EMAIL_HOST", "localhost")
os.environ.setdefault("API_KEY_RATE_LIMIT", "100000/minute")

from .common import *  # noqa: E402,F401,F403

DEBUG = True

# Send it in a dummy outbox
EMAIL_BACKEND = "django.core.mail.backends.locmem.EmailBackend"

INSTALLED_APPS.append(  # noqa: F405
    "pi_dash.tests"
)
