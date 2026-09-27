# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Production settings"""

import os

from .common import *  # noqa
from pi_dash.config import get_config

# SECURITY WARNING: don't run with debug turned on in production!
DEBUG = int(get_config("DEBUG", 0)) == 1

# Honor the 'X-Forwarded-Proto' header for request.is_secure()
SECURE_PROXY_SSL_HEADER = ("HTTP_X_FORWARDED_PROTO", "https")

INSTALLED_APPS += ("scout_apm.django",)  # noqa


# Scout Settings
SCOUT_MONITOR = get_config("SCOUT_MONITOR", False)
SCOUT_KEY = get_config("SCOUT_KEY", "")
SCOUT_NAME = "Pi Dash"

LOG_DIR = os.path.join(BASE_DIR, "logs")  # noqa

if not os.path.exists(LOG_DIR):
    os.makedirs(LOG_DIR)

# Logging configuration
LOGGING = {
    "version": 1,
    # False so loggers created before dictConfig runs (e.g. in modules the
    # settings import) aren't hard-disabled; they follow the config below
    # like everything else.
    "disable_existing_loggers": False,
    "formatters": {
        "verbose": {"format": "%(asctime)s [%(process)d] %(levelname)s %(name)s: %(message)s"},
        "json": {
            "()": "pythonjsonlogger.jsonlogger.JsonFormatter",
            "fmt": "%(levelname)s %(asctime)s %(module)s %(name)s %(message)s",
        },
    },
    "handlers": {
        "console": {
            "class": "logging.StreamHandler",
            "formatter": "json",
            "level": "INFO",
        },
        "file": {
            "class": "pi_dash.utils.logging.SizedTimedRotatingFileHandler",
            "filename": (
                os.path.join(BASE_DIR, "logs", "pi-dash-debug.log")  # noqa
                if DEBUG
                else os.path.join(BASE_DIR, "logs", "pi-dash-error.log")  # noqa
            ),
            "when": "s",
            "maxBytes": 1024 * 1024 * 1,
            "interval": 1,
            "backupCount": 5,
            "formatter": "json",
            "level": "DEBUG" if DEBUG else "ERROR",
        },
    },
    "loggers": {
        "pi_dash.api.request": {
            "level": "DEBUG" if DEBUG else "INFO",
            "handlers": ["console"],
            "propagate": False,
        },
        "pi_dash.api": {
            "level": "DEBUG" if DEBUG else "INFO",
            "handlers": ["console"],
            "propagate": False,
        },
        "pi_dash.worker": {
            "level": "DEBUG" if DEBUG else "INFO",
            "handlers": ["console"],
            "propagate": False,
        },
        "pi_dash.exception": {
            "level": "DEBUG" if DEBUG else "ERROR",
            "handlers": ["console", "file"],
            "propagate": False,
        },
        "pi_dash.external": {
            "level": "INFO",
            "handlers": ["console"],
            "propagate": False,
        },
        "pi_dash.mongo": {
            "level": "INFO",
            "handlers": ["console"],
            "propagate": False,
        },
        "pi_dash.authentication": {
            "level": "DEBUG" if DEBUG else "INFO",
            "handlers": ["console"],
            "propagate": False,
        },
        "pi_dash.migrations": {
            "level": "DEBUG" if DEBUG else "INFO",
            "handlers": ["console"],
            "propagate": False,
        },
        # Catch-all for every pi_dash.* module logger created with
        # logging.getLogger(__name__) — runner services/views, managed_runner,
        # cloud_agent, … Without this, anything not named above inherits the
        # root default (WARNING, no handlers) and its events — including the
        # structured observability events and logger.exception() calls — are
        # silently dropped. The named loggers above keep propagate=False, so
        # nothing is emitted twice.
        "pi_dash": {
            "level": "DEBUG" if DEBUG else "INFO",
            "handlers": ["console"],
            "propagate": False,
        },
    },
    # Backstop for everything outside pi_dash.* (django.request errors,
    # third-party warnings). WARNING keeps third-party INFO/DEBUG noise out.
    "root": {
        "level": "WARNING",
        "handlers": ["console"],
    },
}
