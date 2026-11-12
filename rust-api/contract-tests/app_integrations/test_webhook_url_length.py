"""Contract tests: webhook URL length limits (PIDASHCONV-1201).

``WebhookSerializer`` declares ``url`` explicitly
(``app/serializers/webhook.py:20``), so DRF never maps the model's
``max_length=1024`` (``db/models/webhook.py:36``) onto it: the serializer
chain is ``validate_schema``, ``validate_domain``,
``ProhibitNullCharactersValidator`` and ``URLValidator`` (2048-char cap).
There is **no** 1024-character field error. The ``varchar(1024)`` column is
enforced by Postgres alone, and a 1025-2048-char URL therefore reaches the
INSERT/UPDATE and fails with ``SQLSTATE 22001`` (Django ``DataError`` →
generic 500), while ``WebhookEndpoint.post`` only catches ``IntegrityError``.

Probed on Django main (2026-10-18, d656243d) and pinned here byte for byte;
the Rust port must answer identically.
"""

from __future__ import annotations

import json

import pytest

from .conftest import ws_url

pytestmark = pytest.mark.contract

SERVER_ERROR = {"error": "Something went wrong please try again later"}
INVALID_URL = {"url": ["Enter a valid URL."]}
BAD_SCHEMA = {"url": ["Invalid schema. Only HTTP and HTTPS are allowed."]}

# ``example.com`` resolves to a public IP, so ``create()``/``update()`` pass
# their DNS/SSRF guards and the write reaches the column.
HTTPS_1050 = "https://example.com/" + "a" * 1030
HTTPS_2060 = "https://example.com/" + "a" * 2040
FTP_1103 = "ftp://example.com/" + "a" * 1085


def hooks(world):
    return ws_url(world, "webhooks")


def assert_exact(response, status, body):
    """Pin the status and the exact bytes (compact JSON, no spaces)."""
    assert response.status_code == status
    assert response.json() == body
    assert response.content == json.dumps(body, separators=(",", ":")).encode()


def test_post_url_1025_to_2048_is_a_database_error(admin, world):
    """A valid 1025-2048-char URL passes the serializer and dies on INSERT."""
    assert len(HTTPS_1050) == 1050
    response = admin.post(hooks(world), json={"url": HTTPS_1050})
    assert_exact(response, 500, SERVER_ERROR)


def test_post_url_1025_to_2048_writes_nothing(admin, world):
    admin.post(hooks(world), json={"url": HTTPS_1050})
    assert admin.get(hooks(world)).json() == []


def test_patch_url_1025_to_2048_is_a_database_error(admin, world, seeder):
    """A PATCH reaches the UPDATE and dies there (``save()`` → ``DataError``)."""
    hook = seeder.create_webhook(world["workspace"]["id"])
    response = admin.patch(hooks(world) + f"{hook['id']}/", json={"url": HTTPS_1050})
    assert_exact(response, 500, SERVER_ERROR)
    row = seeder.db.fetchone("SELECT url FROM webhooks WHERE id = %s", (hook["id"],))
    assert row["url"] == hook["url"]


def test_post_url_over_2048_is_a_single_invalid_url_error(admin, world):
    """Over the ``URLValidator`` 2048 cap: one message, no max-length element."""
    assert len(HTTPS_2060) == 2060
    response = admin.post(hooks(world), json={"url": HTTPS_2060})
    assert_exact(response, 400, INVALID_URL)


def test_post_long_ftp_url_has_no_max_length_element(admin, world):
    """Long ``ftp:`` URL: schema message only — DRF has no 1024 check."""
    assert len(FTP_1103) == 1103
    response = admin.post(hooks(world), json={"url": FTP_1103})
    assert_exact(response, 400, BAD_SCHEMA)
