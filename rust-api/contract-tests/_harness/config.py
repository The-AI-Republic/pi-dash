"""Environment configuration for contract suites.

Only two variables are required; both point at live infrastructure:

- ``BASE_URL`` — e.g. ``http://127.0.0.1:18013`` (no trailing slash).
- ``DATABASE_URL`` — psycopg-connectable Postgres URL for the same backend,
  used only for seeding/cleanup SQL, never for assertions about internals.

Suites that mint auth sessions additionally use ``SECRET_KEY`` (see
``secret_key`` below).
"""
import os


def base_url() -> str:
    try:
        return os.environ["BASE_URL"].rstrip("/")
    except KeyError:
        raise RuntimeError("BASE_URL is not set (e.g. http://127.0.0.1:18013)") from None


def database_url() -> str:
    try:
        return os.environ["DATABASE_URL"]
    except KeyError:
        raise RuntimeError("DATABASE_URL is not set (psycopg URL for the backend DB)") from None

def secret_key() -> str:
    """Django SECRET_KEY of the backend under test (for session minting)."""
    return os.environ["SECRET_KEY"]


from dataclasses import dataclass


@dataclass(frozen=True)
class Settings:
    base_url: str
    database_url: str
    # Django SECRET_KEY of the backend under test. Suites that seed token
    # hashes (runner enrollment / machine tokens, PIDASHCONV-97) recompute
    # the server-side hashes, so this must equal the server's key.
    secret_key: str = ""


def get_settings() -> Settings:
    base_url_value = os.environ.get("BASE_URL", "http://localhost:8000").rstrip("/")
    database_url_value = os.environ.get("DATABASE_URL", "")
    if not database_url_value:
        raise RuntimeError(
            "DATABASE_URL is required: point it at the Postgres database of "
            "the server BASE_URL serves, e.g. "
            "DATABASE_URL=postgres://user:pass@localhost:5432/pidash"
        )
    return Settings(
        base_url=base_url_value,
        database_url=database_url_value,
        secret_key=os.environ.get("SECRET_KEY", ""),
    )


# --- PIDASHCONV-83 (app project/state/estimate oracle) ---
# Union with the baseline above.
def required(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(
            f"contract-tests require {name} to be set "
            "(see rust-api/contract-tests/README.md)"
        )
    return value


def optional(name: str, default: str) -> str:
    return os.environ.get(name, default)


DATABASE_URL = "DATABASE_URL"
CELERY_BROKER_URL = "CELERY_BROKER_URL"
BASE_URL = "BASE_URL"
PI_DASH_SOURCE_DIR = "PI_DASH_SOURCE_DIR"
# Fixed SECRET_KEY the contract Django stack runs with. HTTP suites forge
# session rows with it (see _harness/http.py); it must match the server.
CONTRACT_SECRET_KEY = "CONTRACT_SECRET_KEY"

# How long to wait for the Django worker to execute a published job.
TASK_TIMEOUT_SECONDS = float(optional("CONTRACT_TASK_TIMEOUT", "60"))

# Poll interval while waiting for worker effects.
POLL_INTERVAL_SECONDS = float(optional("CONTRACT_POLL_INTERVAL", "0.5"))

# SMTP sink the harness starts; the Django side must deliver to it via
# EMAIL_HOST / EMAIL_PORT / EMAIL_USE_TLS=0 / EMAIL_USE_SSL=0.
SMTP_SINK_HOST = optional("SMTP_SINK_HOST", "127.0.0.1")
SMTP_SINK_PORT = int(optional("SMTP_SINK_PORT", "1025"))

# Base URL of the webhook sink the harness starts. Tests seed Webhook rows
# pointing at <base>/hook/<token>.
WEBHOOK_SINK_BASE = optional("WEBHOOK_SINK_BASE", "http://127.0.0.1:18099")
