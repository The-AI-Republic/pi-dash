# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Single source of truth for where each config value is read from.

Every config key managed by ``pi_dash.config`` is declared here with a
``source``:

* ``"env"`` — read from the process environment (populated by a ``.env`` file
  locally, or by SSM-injected env vars in the cloud). The code never
  distinguishes ``.env`` from SSM; both just become ``os.environ``.
* ``"db"`` — read from the ``InstanceConfiguration`` table, i.e. values an
  instance admin can edit at runtime through the admin UI.

A key belongs to exactly one source. The *system* is hybrid; no single key is.

Entry fields:
    source   : "env" | "db"          (required)
    default  : value when unset/missing (optional, defaults to None)
    secret   : True for values stored encrypted in the DB / sensitive in env

Back-compat note (OSS): every key that the legacy ``get_configuration_value``
resolver served from the DB under the default ``SKIP_ENV_VAR=1`` is classified
``"db"`` here, so removing ``SKIP_ENV_VAR`` does not change self-hosted
behavior. The cloud deployment reclassifies the secret/identity keys to
``"env"`` (so they are sourced from SSM) via the ``PIDASH_CONFIG_ENV_KEYS``
environment variable — see ``_load_env_overrides`` below.
"""

import os

# Keys the legacy resolver managed. Classified for back-compat: the values
# that configure_instance seeds and serves from the DB stay "db"; analytics
# keys that were never seeded (always fell through to env) are "env".
_RESOLVER_CONFIG = {
    # --- Authentication toggles (runtime, admin-editable) -----------------
    "ENABLE_SIGNUP": {"source": "db", "default": "1"},
    "ENABLE_EMAIL_PASSWORD": {"source": "db", "default": "1"},
    "ENABLE_MAGIC_LINK_LOGIN": {"source": "db", "default": "0"},
    "DISABLE_WORKSPACE_CREATION": {"source": "db", "default": "0"},
    # --- Google OAuth -----------------------------------------------------
    "GOOGLE_CLIENT_ID": {"source": "db", "default": None},
    "GOOGLE_CLIENT_SECRET": {"source": "db", "default": None, "secret": True},
    "ENABLE_GOOGLE_SYNC": {"source": "db", "default": "0"},
    # IS_*_ENABLED flags are derived + seeded into the DB by configure_instance
    # and read back by the instances endpoint, so they are db-sourced.
    "IS_GOOGLE_ENABLED": {"source": "db", "default": "0"},
    # --- GitHub OAuth -----------------------------------------------------
    "GITHUB_CLIENT_ID": {"source": "db", "default": None},
    "GITHUB_CLIENT_SECRET": {"source": "db", "default": None, "secret": True},
    "GITHUB_ORGANIZATION_ID": {"source": "db", "default": None},
    "ENABLE_GITHUB_SYNC": {"source": "db", "default": "0"},
    "IS_GITHUB_ENABLED": {"source": "db", "default": "0"},
    # GITHUB_APP_NAME is read by the instances endpoint from the env only
    # (never seeded into the DB), so it is env-sourced.
    "GITHUB_APP_NAME": {"source": "env", "default": None},
    # --- GitHub App -------------------------------------------------------
    # Non-secret identity is db-sourced (admin-editable in god-mode, seeded
    # from env by configure_instance); the secrets are env-sourced (SSM in
    # cloud, env locally) and never touch the DB.
    "GITHUB_APP_ID": {"source": "db", "default": None},
    "GITHUB_APP_SLUG": {"source": "db", "default": None},
    "GITHUB_APP_CLIENT_ID": {"source": "db", "default": None},
    "GITHUB_APP_PRIVATE_KEY": {"source": "env", "default": None, "secret": True},
    "GITHUB_APP_WEBHOOK_SECRET": {"source": "env", "default": None, "secret": True},
    "GITHUB_APP_CLIENT_SECRET": {"source": "env", "default": None, "secret": True},
    # --- GitLab OAuth -----------------------------------------------------
    "GITLAB_HOST": {"source": "db", "default": None},
    "GITLAB_CLIENT_ID": {"source": "db", "default": None},
    "GITLAB_CLIENT_SECRET": {"source": "db", "default": None, "secret": True},
    "ENABLE_GITLAB_SYNC": {"source": "db", "default": "0"},
    "IS_GITLAB_ENABLED": {"source": "db", "default": "0"},
    # --- Gitea OAuth ------------------------------------------------------
    "IS_GITEA_ENABLED": {"source": "db", "default": "0"},
    "GITEA_HOST": {"source": "db", "default": None},
    "GITEA_CLIENT_ID": {"source": "db", "default": None},
    "GITEA_CLIENT_SECRET": {"source": "db", "default": None, "secret": True},
    "ENABLE_GITEA_SYNC": {"source": "db", "default": "0"},
    # --- SMTP / email -----------------------------------------------------
    "ENABLE_SMTP": {"source": "db", "default": "0"},
    "EMAIL_HOST": {"source": "db", "default": ""},
    "EMAIL_HOST_USER": {"source": "db", "default": ""},
    "EMAIL_HOST_PASSWORD": {"source": "db", "default": "", "secret": True},
    "EMAIL_PORT": {"source": "db", "default": "587"},
    "EMAIL_FROM": {"source": "db", "default": ""},
    "EMAIL_USE_TLS": {"source": "db", "default": "1"},
    "EMAIL_USE_SSL": {"source": "db", "default": "0"},
    # --- LLM --------------------------------------------------------------
    "LLM_API_KEY": {"source": "db", "default": None, "secret": True},
    "LLM_PROVIDER": {"source": "db", "default": "openai"},
    "LLM_MODEL": {"source": "db", "default": "gpt-4o-mini"},
    "GPT_ENGINE": {"source": "db", "default": "gpt-3.5-turbo"},  # deprecated, use LLM_MODEL
    # --- Misc -------------------------------------------------------------
    "UNSPLASH_ACCESS_KEY": {"source": "db", "default": "", "secret": True},
    # Read by the instances endpoint from the env only (never seeded to DB).
    "SLACK_CLIENT_ID": {"source": "env", "default": None},
    # --- Analytics (never seeded to DB; always env) -----------------------
    "POSTHOG_API_KEY": {"source": "env", "default": None},
    "POSTHOG_HOST": {"source": "env", "default": None},
}

# Infrastructure / framework config read at boot in the settings modules. These
# are always "env" (they must exist before the DB is reachable — see the tier
# note in accessor.py). Declared here so the registry is the single catalog of
# every config key; settings modules still pass their own inline defaults to
# get_config, so the values below are documentation of the effective default.
_ENV_INFRA = {
    # Core / security
    "SECRET_KEY": None,
    "DEBUG": "0",
    "ALLOWED_HOSTS": "*",
    "CORS_ALLOWED_ORIGINS": "",
    # Database
    "DATABASE_URL": None,
    "POSTGRES_DB": None,
    "POSTGRES_USER": None,
    "POSTGRES_PASSWORD": None,
    "POSTGRES_HOST": None,
    "POSTGRES_PORT": "5432",
    "ENABLE_READ_REPLICA": "0",
    "DATABASE_READ_REPLICA_URL": None,
    "POSTGRES_READ_REPLICA_DB": None,
    "POSTGRES_READ_REPLICA_USER": None,
    "POSTGRES_READ_REPLICA_PASSWORD": None,
    "POSTGRES_READ_REPLICA_HOST": None,
    "POSTGRES_READ_REPLICA_PORT": "5432",
    # Redis
    "REDIS_URL": None,
    "REDIS_SOCKET_CONNECT_TIMEOUT": 2.0,
    "REDIS_SOCKET_TIMEOUT": 5.0,
    "REDIS_HEALTH_CHECK_INTERVAL": 30,
    "REDIS_MAX_CONNECTIONS": None,
    # AI assistant / KMS
    "ASSISTANT_CRYPTO_BACKEND": "aws-kms",
    "ASSISTANT_KMS_KEY_ID": "",
    "ASSISTANT_KMS_ENDPOINT_URL": "",
    "ASSISTANT_ENCRYPTION_KEY": "",
    "ASSISTANT_KEY_CACHE_TTL": 300,
    "ASSISTANT_KEY_CACHE_MAXSIZE": 1000,
    "ASSISTANT_BLOCK_PRIVATE_URLS": "false",
    "ASSISTANT_TURN_SOFT_LIMIT": 300,
    "ASSISTANT_TURN_HARD_LIMIT": 330,
    "ASSISTANT_HISTORY_MAX_TURNS": 40,
    "ASSISTANT_LOOP_HISTORY_MAX_TURNS": 5,
    # Git provider outbound targets. gitlab.com is always allowed by the
    # adapter; self-managed GitLab hosts must be explicitly configured here.
    "GITLAB_ALLOWED_HOSTS": "",
    # Loop (auto project management)
    "LOOP_ENABLED": "true",
    "LOOP_STAGGER_WINDOW_MINUTES": 60,
    "LOOP_MAX_DISPATCH_PER_TICK": 100,
    "LOOP_RECONCILE_EVERY_MINUTES": 15,
    "LOOP_ROTATION_HEADROOM": 30,
    "LOOP_MAX_WRITES": 10,
    "LOOP_PR_LOOKUPS_PER_RUN": 15,
    # Storage / S3 / MinIO
    "USE_MINIO": 0,
    "AWS_ACCESS_KEY_ID": "access-key",
    "AWS_SECRET_ACCESS_KEY": "secret-key",
    "AWS_S3_BUCKET_NAME": "uploads",
    "AWS_REGION": "",
    "AWS_S3_ENDPOINT_URL": None,
    "MINIO_ENDPOINT_URL": None,
    "MINIO_ENDPOINT_SSL": None,
    "SIGNED_URL_EXPIRATION": "3600",
    "WEB_URL": None,
    # RabbitMQ / Celery
    "RABBITMQ_HOST": "localhost",
    "RABBITMQ_PORT": "5672",
    "RABBITMQ_USER": "guest",
    "RABBITMQ_PASSWORD": "guest",
    "RABBITMQ_VHOST": "/",
    "AMQP_URL": None,
    # Misc product config
    "FILE_SIZE_LIMIT": 5242880,
    "GITHUB_ACCESS_TOKEN": False,
    "GITHUB_SYNC_ENABLED": "true",
    "SCHEDULER_ENABLED": "true",
    "ANALYTICS_SECRET_KEY": False,
    "ANALYTICS_BASE_API": False,
    # Runner transport / lifecycle tunables
    "LONG_POLL_INTERVAL_SECS": 25,
    "ACCESS_TOKEN_TTL_SECS": 3600,
    "RUNNER_OFFLINE_THRESHOLD_SECS": 50,
    "OFFLINE_STREAM_TTL_SECS": 86400,
    "OFFLINE_STREAM_MAXLEN": 1000,
    "RUNNER_STREAM_MIN_RETENTION_SECS": 3600,
    "EVENT_BATCH_MAX_AGE_MS": 250,
    "EVENT_BATCH_MAX_BYTES": 65536,
    "RUN_MESSAGE_DEDUPE_TTL_SECS": 604800,
    "LATEST_RUNNER_VERSION": None,
    "MIN_RUNNER_VERSION": None,
    "RUNNER_AGENT_STALL_THRESHOLD_SECS": 360,
    "RUNNER_AGENT_OBSERVABILITY_STALE_SECS": 90,
    # Sessions / cookies
    "SESSION_COOKIE_AGE": 604800,
    "SESSION_COOKIE_NAME": "session-id",
    "COOKIE_DOMAIN": None,
    "SESSION_SAVE_EVERY_REQUEST": "0",
    "ADMIN_SESSION_COOKIE_AGE": 3600,
    # Base URLs
    "ADMIN_BASE_URL": None,
    "ADMIN_BASE_PATH": "/god-mode/",
    "SPACE_BASE_URL": None,
    "SPACE_BASE_PATH": "/spaces/",
    "APP_BASE_URL": None,
    "APP_BASE_PATH": "/",
    "LIVE_BASE_URL": None,
    "LIVE_BASE_PATH": "/live/",
    "HARD_DELETE_AFTER_DAYS": 60,
    "INSTANCE_CHANGELOG_URL": "",
    "ENABLE_DRF_SPECTACULAR": "0",
    # Mongo (legacy / optional)
    "MONGO_DB_URL": False,
    "MONGO_DB_DATABASE": False,
    # Production (Scout APM) + local
    "SCOUT_MONITOR": False,
    "SCOUT_KEY": "",
    "EMAIL_BACKEND": "django.core.mail.backends.smtp.EmailBackend",
}

# Per-key source overrides applied on top of the base registry. Empty in OSS.
# The cloud deployment flips secret/identity keys to "env" (so they are sourced
# from SSM) via the ``PIDASH_CONFIG_ENV_KEYS`` environment variable — a
# comma-separated list of keys to force to "env". This is the SSM-native seam:
# the cloud already injects config through SSM → env, so the classification
# travels the same path as the values it governs, with no file-overlay or
# settings coupling. The in-module dict below is a secondary hook for tests.
CONFIG_SOURCE_OVERRIDES: dict[str, str] = {}

# Name of the env var that lists keys to force to "env" (see above).
ENV_KEYS_OVERRIDE_VAR = "PIDASH_CONFIG_ENV_KEYS"


def _load_env_overrides() -> dict[str, str]:
    raw = os.environ.get(ENV_KEYS_OVERRIDE_VAR, "")
    return {key.strip(): "env" for key in raw.split(",") if key.strip()}


def _build_config() -> dict[str, dict]:
    config = {key: dict(entry) for key, entry in _RESOLVER_CONFIG.items()}
    # Infra keys are env-sourced; never override an already-declared resolver
    # key (e.g. UNSPLASH_ACCESS_KEY stays db-managed for the admin UI).
    for key, default in _ENV_INFRA.items():
        config.setdefault(key, {"source": "env", "default": default})
    overrides = {**_load_env_overrides(), **CONFIG_SOURCE_OVERRIDES}
    for key, source in overrides.items():
        if key in config:
            config[key]["source"] = source
        else:
            config[key] = {"source": source, "default": None}
    return config


CONFIG = _build_config()


def all_keys() -> frozenset[str]:
    """Every registered config key (used by the registration guard)."""
    return frozenset(CONFIG)


def is_registered(key: str) -> bool:
    return key in CONFIG
