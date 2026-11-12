"""Non-canonical ``<uuid:>`` spellings on webhook + git-account routes (PIDASHCONV-1463).

Django declares every segment below with ``<uuid:...>``
(``apps/api/pi_dash/app/urls/webhook.py``: ``webhooks/<uuid:pk>/``,
``webhooks/<uuid:pk>/regenerate/``, ``webhook-logs/<uuid:webhook_id>/``;
``apps/api/pi_dash/app/urls/integration.py``:
``integrations/git/accounts/<uuid:account_id>/`` and ``.../repos/``), whose
converter matches only the canonical lowercase hyphenated form — every
other spelling resolver-404s before the view runs. Rust parsed the
segment with ``Uuid::parse`` (``app_integrations::parse_id`` for the five
webhook sites, ``detail_or_proxy`` for the three git-account sites),
which also accepts uppercase, simple (no hyphens), braced and
``urn:uuid:`` spellings, so it served — and PATCH/DELETE/regenerate/
revoke wrote through — URLs Django never routes.

Fix: gate both parsers on ``classify_uuid_segment``; a non-canonical
spelling takes the route's existing unmatched path (the webhook 404 for
garbage ids, the Django proxy for git-account garbage ids).

Status-only assertions: the harness serves Django with
``pi_dash.settings.test`` (``DEBUG = True``), so its resolver 404 is the
technical HTML page, while Rust's webhook path answers its local JSON
404 (the pre-existing garbage-id body, unchanged by this fix) and its
git path proxies Django's own 404 bytes. All three are status 404; the
prod JSON resolver bytes (``{"error": "Page not found."}``) are pinned
by PIDASHCONV-1215.
"""

from __future__ import annotations

import pytest

from .conftest import ws_url

pytestmark = pytest.mark.contract

SPELLINGS = ["uppercase", "simple"]


def _spellings(row_id):
    """Spellings ``Uuid::parse`` accepts but Django's ``<uuid:>`` does not."""
    return {
        "uppercase": row_id.upper(),
        "simple": row_id.replace("-", ""),
    }


def _assert_not_found(response):
    assert response.status_code == 404, response.text


def _hooks(world, *parts):
    return ws_url(world, "webhooks", *parts)


def _accounts(world, *parts):
    return ws_url(world, "integrations", "git", "accounts", *parts)


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_webhook_detail_noncanonical_404s(admin, world, seeder, spelling):
    hook = seeder.create_webhook(world["workspace"]["id"])
    control = admin.get(_hooks(world, hook["id"]))
    assert control.status_code == 200, control.text
    response = admin.get(_hooks(world, _spellings(hook["id"])[spelling]))
    _assert_not_found(response)


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_webhook_patch_noncanonical_404s_and_writes_nothing(
    admin, world, seeder, spelling
):
    hook = seeder.create_webhook(world["workspace"]["id"])
    response = admin.patch(
        _hooks(world, _spellings(hook["id"])[spelling]),
        json={"url": f"https://example.com/hook/{seeder.tag}-mutated"},
    )
    _assert_not_found(response)
    control = admin.get(_hooks(world, hook["id"]))
    assert control.status_code == 200, control.text
    assert control.json()["url"] == hook["url"]


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_webhook_delete_noncanonical_404s_and_writes_nothing(
    admin, world, seeder, spelling
):
    hook = seeder.create_webhook(world["workspace"]["id"])
    response = admin.delete(_hooks(world, _spellings(hook["id"])[spelling]))
    _assert_not_found(response)
    control = admin.get(_hooks(world, hook["id"]))
    assert control.status_code == 200, control.text


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_webhook_regenerate_noncanonical_404s_and_writes_nothing(
    admin, world, seeder, spelling
):
    hook = seeder.create_webhook(world["workspace"]["id"])
    before = admin.get(_hooks(world, hook["id"]))
    assert before.status_code == 200, before.text
    response = admin.post(
        _hooks(world, _spellings(hook["id"])[spelling], "regenerate")
    )
    _assert_not_found(response)
    after = admin.get(_hooks(world, hook["id"]))
    assert after.status_code == 200, after.text
    assert after.json()["secret_key"] == before.json()["secret_key"]


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_webhook_logs_noncanonical_404s(admin, world, seeder, spelling):
    hook = seeder.create_webhook(world["workspace"]["id"])
    seeder.create_webhook_log(world["workspace"]["id"], hook["id"])
    control = admin.get(ws_url(world, "webhook-logs", hook["id"]))
    assert control.status_code == 200, control.text
    assert len(control.json()) == 1
    response = admin.get(
        ws_url(world, "webhook-logs", _spellings(hook["id"])[spelling])
    )
    _assert_not_found(response)


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_git_account_detail_noncanonical_404s(admin, world, seeder, spelling):
    account = seeder.create_git_provider_account(world["workspace"]["id"])
    control = admin.get(_accounts(world, account["id"]))
    assert control.status_code == 200, control.text
    response = admin.get(_accounts(world, _spellings(account["id"])[spelling]))
    _assert_not_found(response)


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_git_account_revoke_noncanonical_404s_and_writes_nothing(
    admin, world, seeder, spelling
):
    account = seeder.create_git_provider_account(world["workspace"]["id"])
    response = admin.delete(
        _accounts(world, _spellings(account["id"])[spelling])
    )
    _assert_not_found(response)
    control = admin.get(_accounts(world, account["id"]))
    assert control.status_code == 200, control.text
    assert control.json()["status"] == "connected"


@pytest.mark.parametrize("spelling", SPELLINGS)
def test_git_account_repos_noncanonical_404s(admin, world, seeder, spelling):
    account = seeder.create_git_provider_account(world["workspace"]["id"])
    response = admin.get(
        _accounts(world, _spellings(account["id"])[spelling], "repos")
    )
    _assert_not_found(response)
