# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Development settings"""

import os

from .common import *  # noqa
from pi_dash.config import get_config

DEBUG = True

# Debug Toolbar settings
INSTALLED_APPS += ("debug_toolbar",)  # noqa
MIDDLEWARE += ("debug_toolbar.middleware.DebugToolbarMiddleware",)  # noqa

DEBUG_TOOLBAR_PATCH_SETTINGS = False

# Only show emails in console don't send it to smtp
EMAIL_BACKEND = get_config("EMAIL_BACKEND", "django.core.mail.backends.console.EmailBackend")

CACHES = {
    "default": {
        "BACKEND": "django_redis.cache.RedisCache",
        "LOCATION": REDIS_URL,  # noqa
        "OPTIONS": {"CLIENT_CLASS": "django_redis.client.DefaultClient"},
    }
}

INTERNAL_IPS = ("127.0.0.1",)

MEDIA_URL = "/uploads/"
MEDIA_ROOT = os.path.join(BASE_DIR, "uploads")  # noqa

LOG_DIR = os.path.join(BASE_DIR, "logs")  # noqa

if not os.path.exists(LOG_DIR):
    os.makedirs(LOG_DIR)

LOGGING = {
    "version": 1,
    # False so loggers created before dictConfig runs (e.g. in modules the
    # settings import) aren't hard-disabled; they follow the config below
    # like everything else.
    "disable_existing_loggers": False,
    "formatters": {
        "verbose": {
            "format": "{levelname} {asctime} {module} {process:d} {thread:d} {message}",
            "style": "{",
        },
        "json": {
            "()": "pythonjsonlogger.jsonlogger.JsonFormatter",
            "fmt": "%(levelname)s %(asctime)s %(module)s %(name)s %(message)s",
        },
    },
    "handlers": {
        "console": {
            "level": "DEBUG",
            "class": "logging.StreamHandler",
            "formatter": "json",
        }
    },
    "loggers": {
        "pi_dash.api.request": {
            "level": "INFO",
            "handlers": ["console"],
            "propagate": False,
        },
        "pi_dash.api": {"level": "INFO", "handlers": ["console"], "propagate": False},
        "pi_dash.worker": {"level": "INFO", "handlers": ["console"], "propagate": False},
        "pi_dash.exception": {
            "level": "ERROR",
            "handlers": ["console"],
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
            "level": "INFO",
            "handlers": ["console"],
            "propagate": False,
        },
        "pi_dash.migrations": {
            "level": "INFO",
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
            "level": "INFO",
            "handlers": ["console"],
            "propagate": False,
        },
        # Django's DEFAULT_LOGGING runs before this dict (django.setup applies
        # both) and configures "django" at INFO with its own console handler
        # and propagate=True. Without an explicit entry here, django.* warnings
        # would be emitted twice in DEBUG (Django's handler + root), and django
        # INFO would leak through root's handler (a logger's level doesn't
        # filter records propagated from descendants). WARNING matches the
        # root backstop's noise choice.
        "django": {
            "level": "WARNING",
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
