# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Desktop session handoff contract tests (desktop_unified_signin §5, §9).

A CLI sign-in signs in the desktop app through one single-use code:

- the daemon asks for a code with a workspace credential and a challenge
  (base64url of SHA-256(verifier)); the server stores only the code's hash;
- the app redeems with the verifier — the code alone is useless;
- any mismatch consumes the code and creates no session;
- codes live 60 seconds and are single-use;
- the kill switch makes both endpoints refuse.

Acceptance scenarios 3, 10 (Django half) in section 9.
"""

from __future__ import annotations

import uuid
from datetime import timedelta

import pytest
from django.conf import settings
from django.test import Client, override_settings
from django.utils import timezone

from pi_dash.db.models.session import Session
from pi_dash.ee.authentication.desktop import DESKTOP_CLIENT, DESKTOP_SESSION_KEY
from pi_dash.runner.models import DesktopHandoffCode, DevMachine, MachineToken
from pi_dash.runner.services.desktop_handoff import challenge_for_verifier

from .conftest import _client

pytestmark = [pytest.mark.contract, pytest.mark.usefixtures("_isolate")]

ISSUE_URL = "/api/v1/auth/desktop-handoff/"
REDEEM_URL = "/api/v1/auth/desktop-handoff/redeem/"
VERIFY_CLIENT = "app-verifier-0123456789abcdef0123456789ab".encode()


def _issue(mt, verifier=VERIFY_CLIENT, **extra):
    body = {"challenge": challenge_for_verifier(verifier)}
    body.update(extra)
    return _client(api_key=mt).post(ISSUE_URL, body, format="json")


def _redeem(code, user_id, verifier=VERIFY_CLIENT, url=REDEEM_URL):
    # A plain Django client: the Set-Cookie must be readable, and DRF's
    # APIClient response does not surface it.
    import base64

    return Client().post(
        url,
        {
            "code": code,
            "verifier": base64.urlsafe_b64encode(verifier).decode(),
            "expected_user_id": str(user_id),
        },
        content_type="application/json",
    )


def test_issue_and_redeem_sign_in_the_app(db, create_user, workspace, machine, mt):
    issued = _issue(mt)
    assert issued.status_code == 200, issued.data
    code = issued.data["code"]
    assert issued.data["user_id"] == str(create_user.id)
    assert issued.data["email"] == create_user.email
    # Only the code's hash is stored; the raw code never is.
    row = DesktopHandoffCode.objects.get()
    assert code not in row.code_hash and row.code_hash

    redeemed = _redeem(code, create_user.id)
    assert redeemed.status_code == 200, redeemed.content
    assert redeemed.json()["user_id"] == str(create_user.id)
    # The Set-Cookie goes out on the response, for the native transport.
    cookie = redeemed.cookies.get(settings.SESSION_COOKIE_NAME)
    assert cookie is not None and cookie.value
    # The same session a desktop browser sign-in creates, marked desktop.
    decoded = Session.objects.get(session_key=cookie.value).get_decoded()
    assert decoded[DESKTOP_SESSION_KEY] == DESKTOP_CLIENT


def test_redeem_also_answers_on_the_auth_path(db, create_user, workspace, machine, mt):
    """The app redeems on /auth/desktop-handoff/redeem/ (design §5 step 5)."""
    code = _issue(mt).data["code"]
    resp = _redeem(code, create_user.id, url="/auth/desktop-handoff/redeem/")
    assert resp.status_code == 200, resp.content
    cookie = resp.cookies.get(settings.SESSION_COOKIE_NAME)
    assert cookie is not None and cookie.value


def test_issue_needs_a_daemon_credential(db, create_user, workspace, machine):
    resp = _issue("mt_not_a_token")
    assert resp.status_code in (401, 403), resp.data


def test_redeem_with_wrong_verifier_fails_consumes_and_makes_no_session(db, create_user, workspace, machine, mt):
    code = _issue(mt).data["code"]
    wrong = _redeem(code, create_user.id, verifier=b"\x00" * 32)
    assert wrong.status_code == 400, wrong.content
    # The first attempt uses the code up, pass or fail.
    again = _redeem(code, create_user.id)
    assert again.status_code == 400, again.content
    assert wrong.cookies.get(settings.SESSION_COOKIE_NAME) is None


def test_redeem_for_another_user_fails_and_consumes(db, create_user, workspace, machine, mt):
    code = _issue(mt).data["code"]
    resp = _redeem(code, uuid.uuid4())
    assert resp.status_code == 400, resp.content
    assert resp.json()["error"] == "invalid_grant"
    assert _redeem(code, create_user.id).status_code == 400


def test_malformed_expected_user_id_is_refused_without_consuming(db, create_user, workspace, machine, mt):
    """A non-UUID expected_user_id is a bad request; the code stays live."""
    code = _issue(mt).data["code"]
    resp = _redeem(code, "someone-else")
    assert resp.status_code == 400, resp.content
    assert resp.json()["error"] == "invalid_request"
    assert DesktopHandoffCode.objects.get().consumed_at is None
    assert _redeem(code, create_user.id).status_code == 200


def test_issue_records_the_daemon_credential(db, create_user, workspace, machine, mt):
    _issue(mt)
    row = DesktopHandoffCode.objects.get()
    assert row.machine_token_id == MachineToken.objects.get(dev_machine=machine, revoked_at__isnull=True).id


def test_code_for_a_revoked_daemon_credential_fails(db, create_user, workspace, machine, mt):
    """§5.1: a code is refused once the credential it was issued with is revoked."""
    code = _issue(mt).data["code"]
    MachineToken.objects.filter(dev_machine=machine).update(revoked_at=timezone.now())
    resp = _redeem(code, create_user.id)
    assert resp.status_code == 400, resp.content
    assert resp.json()["error"] == "invalid_grant"
    assert resp.cookies.get(settings.SESSION_COOKIE_NAME) is None


def test_code_for_an_expired_daemon_credential_fails(db, create_user, workspace, machine, mt):
    code = _issue(mt).data["code"]
    MachineToken.objects.filter(dev_machine=machine).update(expires_at=timezone.now() - timedelta(seconds=1))
    resp = _redeem(code, create_user.id)
    assert resp.status_code == 400, resp.content
    assert resp.json()["error"] == "invalid_grant"


def test_issue_prunes_old_dead_codes(db, create_user, workspace, machine, mt):
    """Expired or consumed codes older than the retention go on the next issue."""
    _issue(mt)
    _issue(mt)
    old = timezone.now() - timedelta(hours=2)
    first, second = DesktopHandoffCode.objects.order_by("created_at")
    DesktopHandoffCode.objects.filter(pk=first.pk).update(created_at=old, expires_at=old + timedelta(seconds=60))
    DesktopHandoffCode.objects.filter(pk=second.pk).update(created_at=old, consumed_at=old)
    _issue(mt)
    remaining = list(DesktopHandoffCode.objects.all())
    assert len(remaining) == 1
    assert remaining[0].pk not in (first.pk, second.pk)


def test_redeem_with_no_verifier_or_code_fails(db, create_user, workspace, machine, mt):
    resp = _client().post(REDEEM_URL, {"code": "dh_x", "expected_user_id": str(create_user.id)}, format="json")
    assert resp.status_code == 400, resp.data
    resp = _client().post(REDEEM_URL, {"verifier": "AAAA", "expected_user_id": str(create_user.id)}, format="json")
    assert resp.status_code == 400, resp.data


def test_redeemed_code_cannot_be_replayed(db, create_user, workspace, machine, mt):
    code = _issue(mt).data["code"]
    assert _redeem(code, create_user.id).status_code == 200
    assert _redeem(code, create_user.id).status_code == 400


def test_expired_code_fails(db, create_user, workspace, machine, mt):
    code = _issue(mt).data["code"]
    DesktopHandoffCode.objects.update(expires_at=timezone.now())
    resp = _redeem(code, create_user.id)
    assert resp.status_code == 410, resp.content


def test_code_for_a_revoked_machine_fails(db, create_user, workspace, machine, mt):
    code = _issue(mt).data["code"]
    DevMachine.objects.filter(pk=machine.pk).update(revoked_at=timezone.now())
    resp = _redeem(code, create_user.id)
    assert resp.status_code == 400, resp.content


@override_settings(DESKTOP_SESSION_HANDOFF_ENABLED=False)
def test_kill_switch_off_refuses_both_endpoints(db, create_user, workspace, machine, mt):
    assert _issue(mt).status_code == 410
    resp = _redeem("dh_x", create_user.id)
    assert resp.status_code == 410, resp.content


def test_challenge_is_bound_to_the_issuer(db, create_user, workspace, machine, mt):
    """A code issued for verifier A cannot be redeemed with verifier B."""
    code = _issue(mt, verifier=b"a" * 32).data["code"]
    resp = _redeem(code, create_user.id, verifier=b"b" * 32)
    assert resp.status_code == 400, resp.content
    assert _redeem(code, create_user.id, verifier=b"a" * 32).status_code == 400
