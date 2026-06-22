# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Global Settings"""

# Python imports
import os
from urllib.parse import urlparse
from urllib.parse import urljoin

# Third party imports
import dj_database_url

# Django imports
from django.core.management.utils import get_random_secret_key
from corsheaders.defaults import default_headers


# Module imports
from pi_dash.config import get_config
from pi_dash.utils.url import is_valid_url


BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Secret Key
SECRET_KEY = get_config("SECRET_KEY", get_random_secret_key())

# SECURITY WARNING: don't run with debug turned on in production!
DEBUG = int(get_config("DEBUG", "0"))

# Self-hosted mode
IS_SELF_MANAGED = True

# Allowed Hosts
ALLOWED_HOSTS = get_config("ALLOWED_HOSTS", "*").split(",")

# Application definition
INSTALLED_APPS = [
    "django.contrib.auth",
    "django.contrib.contenttypes",
    "django.contrib.sessions",
    "django.contrib.staticfiles",
    # Inhouse apps
    "pi_dash.analytics",
    "pi_dash.app",
    "pi_dash.space",
    "pi_dash.bgtasks",
    "pi_dash.db",
    "pi_dash.utils",
    "pi_dash.web",
    "pi_dash.middleware",
    "pi_dash.license",
    "pi_dash.api",
    "pi_dash.authentication",
    "pi_dash.runner",
    "pi_dash.prompting",
    "pi_dash.orchestration",
    "pi_dash.assistant",
    # Third-party things
    "channels",
    "rest_framework",
    "corsheaders",
    "django_celery_beat",
]

# Middlewares
MIDDLEWARE = [
    "corsheaders.middleware.CorsMiddleware",
    "django.middleware.security.SecurityMiddleware",
    "whitenoise.middleware.WhiteNoiseMiddleware",
    "pi_dash.authentication.middleware.session.SessionMiddleware",
    "django.middleware.common.CommonMiddleware",
    "django.middleware.csrf.CsrfViewMiddleware",
    "django.contrib.auth.middleware.AuthenticationMiddleware",
    "django.middleware.clickjacking.XFrameOptionsMiddleware",
    "crum.CurrentRequestUserMiddleware",
    "django.middleware.gzip.GZipMiddleware",
    "pi_dash.middleware.request_body_size.RequestBodySizeLimitMiddleware",
    "pi_dash.middleware.logger.APITokenLogMiddleware",
    "pi_dash.middleware.logger.RequestLoggerMiddleware",
]

# Rest Framework settings
REST_FRAMEWORK = {
    "DEFAULT_AUTHENTICATION_CLASSES": ("rest_framework.authentication.SessionAuthentication",),
    "DEFAULT_THROTTLE_CLASSES": ("rest_framework.throttling.AnonRateThrottle",),
    "DEFAULT_THROTTLE_RATES": {
        "anon": "30/minute",
        "asset_id": "5/minute",
        "user": "120/minute",
        "runner_chat_send": "60/minute",
        # `pidash auth login` device-code start. Bounded per IP — the
        # endpoint creates a DB row each call, so a flood would otherwise
        # be a free table-pollution vector.
        "auth_device_start": "20/minute",
        # AI assistant: the only platform-compute brake in the BYOK-only MVP.
        "assistant_message": "30/hour",
        "assistant_llm_test": "6/minute",
    },
    "DEFAULT_PERMISSION_CLASSES": ("rest_framework.permissions.IsAuthenticated",),
    "DEFAULT_RENDERER_CLASSES": ("rest_framework.renderers.JSONRenderer",),
    "DEFAULT_FILTER_BACKENDS": ("django_filters.rest_framework.DjangoFilterBackend",),
    "EXCEPTION_HANDLER": "pi_dash.authentication.adapter.exception.auth_exception_handler",
    # Preserve original Django URL parameter names (pk) instead of converting to 'id'
    "SCHEMA_COERCE_PATH_PK": False,
}

# Django Auth Backend
AUTHENTICATION_BACKENDS = ("django.contrib.auth.backends.ModelBackend",)  # default

# Root Urls
ROOT_URLCONF = "pi_dash.urls"

# Templates
TEMPLATES = [
    {
        "BACKEND": "django.template.backends.django.DjangoTemplates",
        "DIRS": ["templates"],
        "APP_DIRS": True,
        "OPTIONS": {
            "context_processors": [
                "django.template.context_processors.debug",
                "django.template.context_processors.request",
                "django.contrib.auth.context_processors.auth",
                "django.contrib.messages.context_processors.messages",
            ]
        },
    }
]


# CORS Settings
CORS_ALLOW_CREDENTIALS = True
cors_origins_raw = get_config("CORS_ALLOWED_ORIGINS", "")
# filter out empty strings
cors_allowed_origins = [origin.strip() for origin in cors_origins_raw.split(",") if origin.strip()]
if cors_allowed_origins:
    CORS_ALLOWED_ORIGINS = cors_allowed_origins
    secure_origins = False if [origin for origin in cors_allowed_origins if "http:" in origin] else True
else:
    CORS_ALLOW_ALL_ORIGINS = True
    secure_origins = False

CORS_ALLOW_HEADERS = [*default_headers, "X-API-Key"]

# Application Settings
WSGI_APPLICATION = "pi_dash.wsgi.application"
ASGI_APPLICATION = "pi_dash.asgi.application"

# Django Sites
SITE_ID = 1

# User Model
AUTH_USER_MODEL = "db.User"

# Database
if bool(get_config("DATABASE_URL")):
    # Parse database configuration from $DATABASE_URL
    DATABASES = {"default": dj_database_url.config()}
else:
    DATABASES = {
        "default": {
            "ENGINE": "django.db.backends.postgresql",
            "NAME": get_config("POSTGRES_DB"),
            "USER": get_config("POSTGRES_USER"),
            "PASSWORD": get_config("POSTGRES_PASSWORD"),
            "HOST": get_config("POSTGRES_HOST"),
            "PORT": get_config("POSTGRES_PORT", "5432"),
        }
    }

# Cap "idle in transaction" at 60s so a stalled in-txn await can't hold row
# locks indefinitely — Postgres only releases them at COMMIT/ROLLBACK or
# when the backend dies. Caps idle time only; statement time (incl.
# migrations) is unaffected.
DATABASES["default"].setdefault("OPTIONS", {})
DATABASES["default"]["OPTIONS"]["options"] = (
    DATABASES["default"]["OPTIONS"].get("options", "")
    + " -c idle_in_transaction_session_timeout=60000"
).strip()


if get_config("ENABLE_READ_REPLICA", "0") == "1":
    if bool(get_config("DATABASE_READ_REPLICA_URL")):
        # Parse database configuration from $DATABASE_URL
        DATABASES["replica"] = dj_database_url.parse(get_config("DATABASE_READ_REPLICA_URL"))
    else:
        DATABASES["replica"] = {
            "ENGINE": "django.db.backends.postgresql",
            "NAME": get_config("POSTGRES_READ_REPLICA_DB"),
            "USER": get_config("POSTGRES_READ_REPLICA_USER"),
            "PASSWORD": get_config("POSTGRES_READ_REPLICA_PASSWORD"),
            "HOST": get_config("POSTGRES_READ_REPLICA_HOST"),
            "PORT": get_config("POSTGRES_READ_REPLICA_PORT", "5432"),
        }

    # Database Routers
    DATABASE_ROUTERS = ["pi_dash.utils.core.dbrouters.ReadReplicaRouter"]
    # Add middleware at the end for read replica routing
    MIDDLEWARE.append("pi_dash.middleware.db_routing.ReadReplicaRoutingMiddleware")


# Redis Config
REDIS_URL = get_config("REDIS_URL")
REDIS_SSL = REDIS_URL and "rediss" in REDIS_URL
REDIS_SOCKET_CONNECT_TIMEOUT = get_config("REDIS_SOCKET_CONNECT_TIMEOUT", 2.0)
REDIS_SOCKET_TIMEOUT = get_config("REDIS_SOCKET_TIMEOUT", 5.0)
REDIS_HEALTH_CHECK_INTERVAL = get_config("REDIS_HEALTH_CHECK_INTERVAL", 30)
REDIS_MAX_CONNECTIONS = get_config("REDIS_MAX_CONNECTIONS")

# AI Assistant — see .ai_design/integrate_ai_agent/
# BYOK keys are encrypted at rest via AWS KMS (pi_dash.assistant.crypto).
# ASSISTANT_KMS_KEY_ID is the CMK (id / ARN / alias) used to encrypt+decrypt;
# region comes from AWS_REGION. When unset, BYOK keys cannot be stored (the
# config endpoint reports assistant_not_configured). ASSISTANT_KMS_ENDPOINT_URL
# optionally points the KMS client at a compatible endpoint (e.g. LocalStack)
# for local / self-hosted setups without a real AWS account.
# Which crypto backend encrypts BYOK keys (pi_dash.assistant.crypto). Only
# "aws-kms" ships today; the seam exists so other providers (GCP KMS, Azure
# Key Vault, Vault Transit) can be added without touching call sites.
ASSISTANT_CRYPTO_BACKEND = get_config("ASSISTANT_CRYPTO_BACKEND", "aws-kms")
ASSISTANT_KMS_KEY_ID = get_config("ASSISTANT_KMS_KEY_ID", "")
ASSISTANT_KMS_ENDPOINT_URL = get_config("ASSISTANT_KMS_ENDPOINT_URL", "")
# Short-lived in-process cache of decrypted BYOK keys, so the assistant doesn't
# call KMS Decrypt on every turn. In-memory per worker (plaintext never leaves
# the process — unlike a shared store); keyed by a hash of the ciphertext so a
# key change auto-invalidates. TTL also bounds how long a KMS-side revocation
# lags. Set TTL=0 to disable. Eviction (TTL or LRU at MAXSIZE) is always safe —
# a miss just costs one extra KMS Decrypt.
ASSISTANT_KEY_CACHE_TTL = int(get_config("ASSISTANT_KEY_CACHE_TTL", 300))
ASSISTANT_KEY_CACHE_MAXSIZE = int(get_config("ASSISTANT_KEY_CACHE_MAXSIZE", 1000))
# SSRF guard for BYOK base_url. Off in OSS (LAN vLLM/Ollama allowed); cloud sets True.
ASSISTANT_BLOCK_PRIVATE_URLS = get_config("ASSISTANT_BLOCK_PRIVATE_URLS", "false").lower() in (
    "1",
    "true",
    "yes",
)
ASSISTANT_TURN_SOFT_LIMIT = int(get_config("ASSISTANT_TURN_SOFT_LIMIT", 300))
ASSISTANT_TURN_HARD_LIMIT = int(get_config("ASSISTANT_TURN_HARD_LIMIT", 330))
# Max completed turns replayed to the model as history. Bounds per-turn token
# cost (and context-window use) on long threads; the durable transcript shown
# in the UI is unaffected — only what the model sees is truncated.
ASSISTANT_HISTORY_MAX_TURNS = int(get_config("ASSISTANT_HISTORY_MAX_TURNS", 40))

# Loop (Auto Project Management) — periodic assistant jobs.
# See .ai_design/loop_project_management/design.md §11.
# Instance kill switch: when false, the scanner and fire tasks short-circuit.
LOOP_ENABLED = get_config("LOOP_ENABLED", "true").lower() in ("1", "true", "yes")
# Deterministic per-edge fire offset window (minutes) so a daily job doesn't
# fire every membership edge in the same minute.
LOOP_STAGGER_WINDOW_MINUTES = int(get_config("LOOP_STAGGER_WINDOW_MINUTES", 60))
# Backpressure: at most this many targets dispatched per scanner tick; the rest
# stay due and drain on later ticks.
LOOP_MAX_DISPATCH_PER_TICK = int(get_config("LOOP_MAX_DISPATCH_PER_TICK", 100))
# Reconcile (create missing targets for new membership edges) runs only when
# minute % this == 0 — cheap throttle vs. the per-minute due scan.
LOOP_RECONCILE_EVERY_MINUTES = int(get_config("LOOP_RECONCILE_EVERY_MINUTES", 15))
# Rotate to a fresh hidden thread when within this many messages of the cap.
LOOP_ROTATION_HEADROOM = int(get_config("LOOP_ROTATION_HEADROOM", 30))
# Per-run blast-radius cap, enforced by instruction (hard backstop is the
# assistant's tool_calls_limit).
LOOP_MAX_WRITES = int(get_config("LOOP_MAX_WRITES", 10))
# Per-run cap on get_pull_request_status calls (protects the unauthenticated
# GitHub rate limit for the host IP).
LOOP_PR_LOOKUPS_PER_RUN = int(get_config("LOOP_PR_LOOKUPS_PER_RUN", 15))
# Completed loop turns replayed as history — tighter than chat (daily cost).
ASSISTANT_LOOP_HISTORY_MAX_TURNS = int(get_config("ASSISTANT_LOOP_HISTORY_MAX_TURNS", 5))

if REDIS_SSL:
    CACHES = {
        "default": {
            "BACKEND": "django_redis.cache.RedisCache",
            "LOCATION": REDIS_URL,
            "OPTIONS": {
                "CLIENT_CLASS": "django_redis.client.DefaultClient",
                "CONNECTION_POOL_KWARGS": {"ssl_cert_reqs": False},
            },
        }
    }
else:
    CACHES = {
        "default": {
            "BACKEND": "django_redis.cache.RedisCache",
            "LOCATION": REDIS_URL,
            "OPTIONS": {"CLIENT_CLASS": "django_redis.client.DefaultClient"},
        }
    }

# Channels channel layer — used by pi_dash.runner to fan messages to
# connected runner WebSockets. Falls back to in-memory when Redis is not
# available (dev/test); single-process only in that mode.
if REDIS_URL:
    CHANNEL_LAYERS = {
        "default": {
            "BACKEND": "channels_redis.core.RedisChannelLayer",
            "CONFIG": {"hosts": [REDIS_URL]},
        }
    }
else:
    CHANNEL_LAYERS = {
        "default": {"BACKEND": "channels.layers.InMemoryChannelLayer"},
    }

# Password validations
AUTH_PASSWORD_VALIDATORS = [
    {"NAME": "django.contrib.auth.password_validation.UserAttributeSimilarityValidator"},
    {"NAME": "django.contrib.auth.password_validation.MinimumLengthValidator"},
    {"NAME": "django.contrib.auth.password_validation.CommonPasswordValidator"},
    {"NAME": "django.contrib.auth.password_validation.NumericPasswordValidator"},
]

# Password reset time the number of seconds the uniquely generated uid will be valid
PASSWORD_RESET_TIMEOUT = 3600

# Static files (CSS, JavaScript, Images)
STATIC_URL = "/static/"
STATIC_ROOT = os.path.join(BASE_DIR, "static-assets", "collected-static")
STATICFILES_DIRS = (os.path.join(BASE_DIR, "static"),)

# Media Settings
MEDIA_ROOT = "mediafiles"
MEDIA_URL = "/media/"

# Internationalization
LANGUAGE_CODE = "en-us"
USE_I18N = True
USE_L10N = True

# Timezones
USE_TZ = True
TIME_ZONE = "UTC"

# Default Auto Field
DEFAULT_AUTO_FIELD = "django.db.models.BigAutoField"

# Email settings
EMAIL_BACKEND = "django.core.mail.backends.smtp.EmailBackend"

# Storage Settings
# Use Minio settings
USE_MINIO = int(get_config("USE_MINIO", 0)) == 1

STORAGES = {"staticfiles": {"BACKEND": "whitenoise.storage.CompressedManifestStaticFilesStorage"}}
STORAGES["default"] = {"BACKEND": "pi_dash.settings.storage.S3Storage"}
AWS_ACCESS_KEY_ID = get_config("AWS_ACCESS_KEY_ID", "access-key")
AWS_SECRET_ACCESS_KEY = get_config("AWS_SECRET_ACCESS_KEY", "secret-key")
AWS_STORAGE_BUCKET_NAME = get_config("AWS_S3_BUCKET_NAME", "uploads")
AWS_REGION = get_config("AWS_REGION", "")
AWS_DEFAULT_ACL = "public-read"
AWS_QUERYSTRING_AUTH = False
AWS_S3_FILE_OVERWRITE = False
AWS_S3_ENDPOINT_URL = get_config("AWS_S3_ENDPOINT_URL", None) or get_config("MINIO_ENDPOINT_URL", None)
if AWS_S3_ENDPOINT_URL and USE_MINIO:
    parsed_url = urlparse(get_config("WEB_URL", "http://localhost"))
    AWS_S3_CUSTOM_DOMAIN = f"{parsed_url.netloc}/{AWS_STORAGE_BUCKET_NAME}"
    AWS_S3_URL_PROTOCOL = f"{parsed_url.scheme}:"

# RabbitMQ connection settings
RABBITMQ_HOST = get_config("RABBITMQ_HOST", "localhost")
RABBITMQ_PORT = get_config("RABBITMQ_PORT", "5672")
RABBITMQ_USER = get_config("RABBITMQ_USER", "guest")
RABBITMQ_PASSWORD = get_config("RABBITMQ_PASSWORD", "guest")
RABBITMQ_VHOST = get_config("RABBITMQ_VHOST", "/")
AMQP_URL = get_config("AMQP_URL")

# Celery Configuration
if AMQP_URL:
    CELERY_BROKER_URL = AMQP_URL
else:
    CELERY_BROKER_URL = f"amqp://{RABBITMQ_USER}:{RABBITMQ_PASSWORD}@{RABBITMQ_HOST}:{RABBITMQ_PORT}/{RABBITMQ_VHOST}"

CELERY_TIMEZONE = TIME_ZONE
CELERY_TASK_SERIALIZER = "json"
CELERY_RESULT_SERIALIZER = "json"
CELERY_ACCEPT_CONTENT = ["application/json"]


CELERY_IMPORTS = (
    # scheduled tasks
    "pi_dash.bgtasks.issue_automation_task",
    "pi_dash.bgtasks.exporter_expired_task",
    "pi_dash.bgtasks.file_asset_task",
    "pi_dash.bgtasks.email_notification_task",
    "pi_dash.bgtasks.cleanup_task",
    "pi_dash.license.bgtasks.tracer",
    # management tasks
    "pi_dash.bgtasks.dummy_data_task",
    # issue version tasks
    "pi_dash.bgtasks.issue_version_sync",
    "pi_dash.bgtasks.issue_description_version_sync",
    # runner lifecycle tasks
    "pi_dash.runner.tasks",
    # platform federation
    "pi_dash.bgtasks.platform_federation_task",
)

FILE_SIZE_LIMIT = int(get_config("FILE_SIZE_LIMIT", 5242880))

# Unsplash Access key. Intentionally read straight from the env here: the same
# key is also admin-managed (db-sourced) via the InstanceConfiguration resolver
# for the in-app Unsplash feature, so it cannot route through get_config (which
# would resolve it from the DB at settings-import time).
UNSPLASH_ACCESS_KEY = os.environ.get("UNSPLASH_ACCESS_KEY")  # noqa: config-env-read
# Github Access Token
GITHUB_ACCESS_TOKEN = get_config("GITHUB_ACCESS_TOKEN", False)

# GitHub Issue Sync feature gate. Default on; self-hosters who don't want
# the integration set GITHUB_SYNC_ENABLED=false. See .ai_design/github_sync/
# design.md §9 Rollout.
GITHUB_SYNC_ENABLED = get_config("GITHUB_SYNC_ENABLED", "true").lower() == "true"

# Platform federation / enterprise identity bridge. Disabled by default so
# standalone/self-managed installs keep Pi Dash's existing local identity model.
PLATFORM_FEDERATION_ENABLED = get_config("PLATFORM_FEDERATION_ENABLED", "false").lower() == "true"
PLATFORM_IOS_ISSUER = get_config("PLATFORM_IOS_ISSUER", "")
PLATFORM_IOS_JWKS_URL = get_config("PLATFORM_IOS_JWKS_URL", "")
PLATFORM_IOS_AUDIENCE = get_config("PLATFORM_IOS_AUDIENCE", "pi-dash")
PLATFORM_IOS_WEBHOOK_SECRET = get_config("PLATFORM_IOS_WEBHOOK_SECRET", "")
PLATFORM_IOS_INTERNAL_API_BASE_URL = get_config("PLATFORM_IOS_INTERNAL_API_BASE_URL", "")
PLATFORM_IOS_INTERNAL_API_TOKEN = get_config("PLATFORM_IOS_INTERNAL_API_TOKEN", "")
PLATFORM_IOS_HTTP_TIMEOUT_SECONDS = int(get_config("PLATFORM_IOS_HTTP_TIMEOUT_SECONDS", "5"))
PLATFORM_IOS_JWKS_CACHE_SECONDS = int(get_config("PLATFORM_IOS_JWKS_CACHE_SECONDS", "300"))

# Project Scheduler feature gate. Default on; self-hosters who don't want
# periodic agent ticks against projects set SCHEDULER_ENABLED=false. See
# .ai_design/project_scheduler/design.md §10 Rollout.
SCHEDULER_ENABLED = get_config("SCHEDULER_ENABLED", "true").lower() == "true"

# Analytics
ANALYTICS_SECRET_KEY = get_config("ANALYTICS_SECRET_KEY", False)
ANALYTICS_BASE_API = get_config("ANALYTICS_BASE_API", False)

# Posthog settings
POSTHOG_API_KEY = get_config("POSTHOG_API_KEY", False)
POSTHOG_HOST = get_config("POSTHOG_HOST", False)

# Per-runner HTTPS transport tunables (see ``.ai_design/move_to_https/design.md`` §9).
# Clamped to [1, 55] so the server-side block always finishes strictly
# before the daemon's per-request timeout (capped at 55 + 5 buffer in
# `runner/src/cloud/http.rs::MAX_LONG_POLL_INTERVAL_SECS`) and a
# misconfigured 0 doesn't tight-loop the daemon. Out-of-range values are
# logged at WARNING so operators see the override during boot instead
# of silently getting a value they didn't ask for.
_LONG_POLL_MIN_SECS = 1
_LONG_POLL_MAX_SECS = 55
_raw_long_poll_secs = int(get_config("LONG_POLL_INTERVAL_SECS", 25))
if _raw_long_poll_secs < _LONG_POLL_MIN_SECS or _raw_long_poll_secs > _LONG_POLL_MAX_SECS:
    import logging

    logging.getLogger(__name__).warning(
        "LONG_POLL_INTERVAL_SECS=%s out of allowed range [%s, %s]; clamping. "
        "Raising the upper bound requires also raising "
        "MAX_LONG_POLL_INTERVAL_SECS in runner/src/cloud/http.rs and the "
        "shared reqwest Client::timeout so daemon timeouts don't fire "
        "before the server's block_ms completes.",
        _raw_long_poll_secs,
        _LONG_POLL_MIN_SECS,
        _LONG_POLL_MAX_SECS,
    )
LONG_POLL_INTERVAL_SECS = max(
    _LONG_POLL_MIN_SECS, min(_raw_long_poll_secs, _LONG_POLL_MAX_SECS)
)
ACCESS_TOKEN_TTL_SECS = int(get_config("ACCESS_TOKEN_TTL_SECS", 3600))
RUNNER_OFFLINE_THRESHOLD_SECS = int(get_config("RUNNER_OFFLINE_THRESHOLD_SECS", 50))
OFFLINE_STREAM_TTL_SECS = int(get_config("OFFLINE_STREAM_TTL_SECS", 86400))
OFFLINE_STREAM_MAXLEN = int(get_config("OFFLINE_STREAM_MAXLEN", 1000))
RUNNER_STREAM_MIN_RETENTION_SECS = int(
    get_config("RUNNER_STREAM_MIN_RETENTION_SECS", 3600)
)
EVENT_BATCH_MAX_AGE_MS = int(get_config("EVENT_BATCH_MAX_AGE_MS", 250))
EVENT_BATCH_MAX_BYTES = int(get_config("EVENT_BATCH_MAX_BYTES", 65536))
RUN_MESSAGE_DEDUPE_TTL_SECS = int(get_config("RUN_MESSAGE_DEDUPE_TTL_SECS", 604800))
RUNNER_PROTOCOL_VERSION = 4

# Runner auto-update advisory. Cloud announces these in the welcome frame;
# runners with `auto_update` enabled swap their on-disk binary to match
# LATEST_RUNNER_VERSION. MIN_RUNNER_VERSION surfaces a red banner in the
# runner TUI/status (advisory only — does not block task claims).
# Leave unset (empty string) to skip the announcement. Values must match
# the SemVer shape `MAJOR.MINOR.PATCH[-prerelease]`; the runner's
# `version_lt` ignores values it can't parse, so a typo silently disables
# the advisory. We log a warning at startup so operator mistakes are
# noisy rather than invisible.
import re as _re_runner_version
import logging as _logging_runner_version

_RUNNER_VERSION_RE = _re_runner_version.compile(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$")


def _validated_runner_version(name):
    raw = get_config(name, "") or None
    if raw is not None and not _RUNNER_VERSION_RE.match(raw):
        _logging_runner_version.getLogger(__name__).warning(
            "%s=%r does not match MAJOR.MINOR.PATCH[-pre]; "
            "runners will treat the advisory as malformed and skip it",
            name,
            raw,
        )
    return raw


LATEST_RUNNER_VERSION = _validated_runner_version("LATEST_RUNNER_VERSION")
MIN_RUNNER_VERSION = _validated_runner_version("MIN_RUNNER_VERSION")

# Per-active-run agent observability watchdog tunables — see
# ``.ai_design/runner_agent_bridge/design.md`` §4.5.3.
# 360s is slightly longer than the runner's own 5-minute internal stall
# watchdog, so the cloud acts as a backstop rather than racing the runner.
RUNNER_AGENT_STALL_THRESHOLD_SECS = int(
    get_config("RUNNER_AGENT_STALL_THRESHOLD_SECS", 360)
)
# Snapshot-row freshness guard. The watchdog only acts on runners whose
# poll-driven `RunnerLiveState.updated_at` is newer than this. Covers
# roughly three missed 25s polls; stale rows from disabled / downgraded
# runners age out instead of failing active runs.
RUNNER_AGENT_OBSERVABILITY_STALE_SECS = int(
    get_config("RUNNER_AGENT_OBSERVABILITY_STALE_SECS", 90)
)
# Access-token signing key ring. Each entry: {kid, secret, status} where
# status ∈ {"active", "verify_only"}. Exactly one key is active.
# Default to a deterministic per-instance key derived from SECRET_KEY so
# dev/test setups Just Work; production should override via env / settings.
RUNNER_ACCESS_TOKEN_KEYS = []

DATA_UPLOAD_MAX_MEMORY_SIZE = int(get_config("FILE_SIZE_LIMIT", 5242880))

# Cookie Settings
SESSION_COOKIE_SECURE = secure_origins
SESSION_COOKIE_HTTPONLY = True
SESSION_ENGINE = "pi_dash.db.models.session"
SESSION_COOKIE_AGE = int(get_config("SESSION_COOKIE_AGE", 604800))
SESSION_COOKIE_NAME = get_config("SESSION_COOKIE_NAME", "session-id")
SESSION_COOKIE_DOMAIN = get_config("COOKIE_DOMAIN", None)
SESSION_SAVE_EVERY_REQUEST = get_config("SESSION_SAVE_EVERY_REQUEST", "0") == "1"

# Admin Cookie
ADMIN_SESSION_COOKIE_NAME = "admin-session-id"
ADMIN_SESSION_COOKIE_AGE = int(get_config("ADMIN_SESSION_COOKIE_AGE", 3600))

# CSRF cookies
CSRF_COOKIE_SECURE = secure_origins
CSRF_COOKIE_HTTPONLY = True
CSRF_TRUSTED_ORIGINS = cors_allowed_origins
CSRF_COOKIE_DOMAIN = get_config("COOKIE_DOMAIN", None)
CSRF_FAILURE_VIEW = "pi_dash.authentication.views.common.csrf_failure"

######  Base URLs ######

# Admin Base URL
ADMIN_BASE_URL = get_config("ADMIN_BASE_URL", None)
if ADMIN_BASE_URL and not is_valid_url(ADMIN_BASE_URL):
    ADMIN_BASE_URL = None
ADMIN_BASE_PATH = get_config("ADMIN_BASE_PATH", "/god-mode/")

# Space Base URL
SPACE_BASE_URL = get_config("SPACE_BASE_URL", None)
if SPACE_BASE_URL and not is_valid_url(SPACE_BASE_URL):
    SPACE_BASE_URL = None
SPACE_BASE_PATH = get_config("SPACE_BASE_PATH", "/spaces/")

# App Base URL
APP_BASE_URL = get_config("APP_BASE_URL", None)
if APP_BASE_URL and not is_valid_url(APP_BASE_URL):
    APP_BASE_URL = None
APP_BASE_PATH = get_config("APP_BASE_PATH", "/")

# Live Base URL
LIVE_BASE_URL = get_config("LIVE_BASE_URL", None)
if LIVE_BASE_URL and not is_valid_url(LIVE_BASE_URL):
    LIVE_BASE_URL = None
LIVE_BASE_PATH = get_config("LIVE_BASE_PATH", "/live/")

LIVE_URL = urljoin(LIVE_BASE_URL, LIVE_BASE_PATH) if LIVE_BASE_URL else None

# WEB URL
WEB_URL = get_config("WEB_URL")

HARD_DELETE_AFTER_DAYS = int(get_config("HARD_DELETE_AFTER_DAYS", 60))

# Instance Changelog URL
INSTANCE_CHANGELOG_URL = get_config("INSTANCE_CHANGELOG_URL", "")

ATTACHMENT_MIME_TYPES = [
    # Images
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/svg+xml",
    "image/webp",
    "image/tiff",
    "image/bmp",
    # Documents
    "application/pdf",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-powerpoint",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "text/plain",
    "text/markdown",
    "application/rtf",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.oasis.opendocument.text",
    "application/vnd.oasis.opendocument.presentation",
    "application/vnd.oasis.opendocument.graphics",
    # Microsoft Visio
    "application/vnd.visio",
    # Netpbm format
    "image/x-portable-graymap",
    "image/x-portable-bitmap",
    "image/x-portable-pixmap",
    # Open Office Bae
    "application/vnd.oasis.opendocument.database",
    # Audio
    "audio/mpeg",
    "audio/wav",
    "audio/ogg",
    "audio/midi",
    "audio/x-midi",
    "audio/aac",
    "audio/flac",
    "audio/x-m4a",
    # Video
    "video/mp4",
    "video/mpeg",
    "video/ogg",
    "video/webm",
    "video/quicktime",
    "video/x-msvideo",
    "video/x-ms-wmv",
    # Archives
    "application/zip",
    "application/x-rar",
    "application/x-rar-compressed",
    "application/x-tar",
    "application/gzip",
    "application/x-zip",
    "application/x-zip-compressed",
    "application/x-7z-compressed",
    "application/x-compressed",
    "application/x-compressed-tar",
    "application/x-compressed-tar-gz",
    "application/x-compressed-tar-bz2",
    "application/x-compressed-tar-zip",
    "application/x-compressed-tar-7z",
    "application/x-compressed-tar-rar",
    "application/x-compressed-tar-zip",
    # 3D Models
    "model/gltf-binary",
    "model/gltf+json",
    "application/octet-stream",  # for .obj files, but be cautious
    # Fonts
    "font/ttf",
    "font/otf",
    "font/woff",
    "font/woff2",
    # Other
    "text/css",
    "text/javascript",
    "application/json",
    "text/xml",
    "text/csv",
    "application/xml",
    # SQL
    "application/x-sql",
    # Gzip
    "application/x-gzip",
    # Markdown
    "text/markdown",
]

# Seed directory path
SEED_DIR = os.path.join(BASE_DIR, "seeds")

ENABLE_DRF_SPECTACULAR = get_config("ENABLE_DRF_SPECTACULAR", "0") == "1"

if ENABLE_DRF_SPECTACULAR:
    REST_FRAMEWORK["DEFAULT_SCHEMA_CLASS"] = "drf_spectacular.openapi.AutoSchema"
    INSTALLED_APPS.append("drf_spectacular")
    from .openapi import SPECTACULAR_SETTINGS  # noqa: F401

# MongoDB Settings
MONGO_DB_URL = get_config("MONGO_DB_URL", False)
MONGO_DB_DATABASE = get_config("MONGO_DB_DATABASE", False)
