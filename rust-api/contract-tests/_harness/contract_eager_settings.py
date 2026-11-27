# Local-boot settings shim for contract-test runs without a broker.
#
# Imports the standard test settings, then forces Celery tasks to execute
# eagerly in-process so ``.delay()`` calls in the request path (model /
# issue activity) work with no RabbitMQ running. CI uses real services
# instead (see .github/workflows/rust-api-contract.yml) — the HTTP
# behaviour under test is identical either way because the tasks only
# write activity rows / fan out webhooks after the response is built.
#
# Not part of the product. Used only to boot a local Django server:
#   PYTHONPATH=apps/api:rust-api/contract-tests/_harness \
#   DJANGO_SETTINGS_MODULE=contract_eager_settings \
#   DATABASE_URL=... python apps/api/manage.py runserver <port>

from pi_dash.settings.test import *  # noqa: E402,F401,F403

CELERY_TASK_ALWAYS_EAGER = True
CELERY_TASK_EAGER_PROPAGATES = False

# No Redis runs on a contract-test laptop boot: keep Django's cache (used by
# the API-key rate throttle) in process memory. Single-process runserver only.
CACHES = {
    "default": {
        "BACKEND": "django.core.cache.backends.locmem.LocMemCache",
    }
}
