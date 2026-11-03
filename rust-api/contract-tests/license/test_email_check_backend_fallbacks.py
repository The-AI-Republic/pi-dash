# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""EmailCredentialCheckEndpoint: Django's localhost/port-25/DEFAULT_FROM_EMAIL fallbacks (PIDASHCONV-1366).

Django's `EmailBackend.__init__` resolves `host or settings.EMAIL_HOST` and
`port or settings.EMAIL_PORT` (`django/core/mail/backends/smtp.py:33-34`),
and `EmailMessage.__init__` resolves `from_email or settings.DEFAULT_FROM_EMAIL`
(`django/core/mail/message.py:239`). The project overrides none of the three
globals (`settings/common.py`), so an empty/missing `EMAIL_HOST` dials
`localhost`, `EMAIL_PORT="0"` dials port 25, and `EMAIL_FROM=""` sends
`MAIL FROM:<webmaster@localhost>`. Rust dialed `("", port)` / port 0 and sent
`MAIL FROM:<>` instead.

Profile note. The contract suite runs Django's `settings.test`, whose
`EMAIL_BACKEND` is `locmem` — Django never dials and answers 200 for every
shape below. What is pinned is therefore profile-tolerant (the PIDASHCONV-1133
pattern): each assertion holds on the Django leg always, fails on pre-fix Rust,
and passes on fixed Rust. The SMTP-profile-only 400 bodies were additionally
verified live against a `settings.local` + `EMAIL_BACKEND=smtp` Django, byte
for byte (see the issue workpad).
"""

import socket

from _harness.sinks import SmtpSink

URL = "/api/instances/email-credentials-check/"
SENT = {"message": "Email successfully sent."}
NETWORK = {"error": "Network connection error. Please check your internet connection."}
DEFAULT_FROM = "webmaster@localhost"


def _free_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _point_mail_at(db, host, port, from_addr="Team Pi Dash <team@example.com>"):
    db.make_config("EMAIL_HOST", host, category="email")
    db.make_config("EMAIL_PORT", str(port), category="email")
    db.make_config("EMAIL_USE_TLS", "0", category="email")
    db.make_config("EMAIL_USE_SSL", "0", category="email")
    db.make_config("EMAIL_FROM", from_addr, category="email")


def test_empty_host_unused_port_is_network_error(admin_api, world):
    # Django dials localhost:<unused>, gets ConnectionRefusedError, and
    # answers the Network 400; pre-fix Rust answers the generic 400.
    _point_mail_at(world["db"], "", _free_port())
    res = admin_api.post(URL, json={"receiver_email": "a@b.test"})
    assert res.json() in (SENT, NETWORK), res.text


def test_empty_host_local_sink_sends(admin_api, world):
    # Django dials localhost, reaches the sink, and answers 200; pre-fix
    # Rust fails to resolve "" and answers the generic 400.
    with SmtpSink() as sink:
        _point_mail_at(world["db"], "", sink.port)
        res = admin_api.post(URL, json={"receiver_email": "a@b.test"})
        assert res.status_code == 200, res.text
        assert res.json() == SENT


def test_empty_from_defaults_envelope_sender(admin_api, world):
    # Django sends MAIL FROM:<webmaster@localhost>; pre-fix Rust sends
    # MAIL FROM:<>. The sink assertion is vacuous on the Django leg
    # (locmem dials nowhere) and discriminating on the Rust leg.
    with SmtpSink() as sink:
        _point_mail_at(world["db"], sink.host, sink.port, from_addr="")
        res = admin_api.post(URL, json={"receiver_email": "a@b.test"})
        assert res.status_code == 200, res.text
        assert res.json() == SENT
        for message in sink.messages:
            assert message["mail_from"] == DEFAULT_FROM, message


def test_zero_port_dials_25(admin_api, world):
    # Django treats port 0 as falsy and dials 25 (refused on a bare
    # runner → Network 400); pre-fix Rust dials ("", 0) → generic 400.
    _point_mail_at(world["db"], "", 0)
    res = admin_api.post(URL, json={"receiver_email": "a@b.test"})
    assert res.json() in (SENT, NETWORK), res.text
