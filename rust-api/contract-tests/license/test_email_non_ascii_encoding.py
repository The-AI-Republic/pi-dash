# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""EmailCredentialCheckEndpoint: non-ASCII From / IDN recipient encoding (PIDASHCONV-1445).

Django's SMTP send (`EmailMultiAlternatives.send`, `configuration.py:120-130`)
RFC 2047-encodes a non-ASCII `From` display name, punycodes IDN domains in the
`To` header, and sanitizes the envelope the same way (`sanitize_address` +
`smtplib.quoteaddr`, `smtp.py:144-149`). Rust formatted headers and envelope
as raw UTF-8; it now reuses the shared tasks_mail encoder (PIDASHCONV-1286).

Profile note (the PIDASHCONV-1133 pattern). The contract suite runs Django's
`settings.test`, whose `EMAIL_BACKEND` is `locmem` — Django never dials, so
the sink assertions below are vacuous on the Django leg and discriminating on
the Rust leg under the SMTP backend. The 200 pin holds on every leg.
"""

from _harness.sinks import SmtpSink

URL = "/api/instances/email-credentials-check/"
SENT = {"message": "Email successfully sent."}

NONASCII_FROM = "\u00c9quipe Pi Dash <team@x.com>"
IDN_RCPT = "invite@m\u00fcnchen.de"
PUNY_RCPT = "invite@xn--mnchen-3ya.de"
ENCODED_FROM = "From: =?utf-8?q?=C3=89quipe_Pi_Dash?= <team@x.com>"
SUBJECT = "Subject: Email Notification from Pi Dash"


def _point_mail_at(db, sink, from_addr):
    db.make_config("EMAIL_HOST", sink.host, category="email")
    db.make_config("EMAIL_PORT", str(sink.port), category="email")
    db.make_config("EMAIL_USE_TLS", "0", category="email")
    db.make_config("EMAIL_USE_SSL", "0", category="email")
    db.make_config("EMAIL_FROM", from_addr, category="email")


def test_non_ascii_from_and_idn_recipient_encoded(admin_api, world):
    db = world["db"]
    with SmtpSink() as sink:
        _point_mail_at(db, sink, NONASCII_FROM)
        res = admin_api.post(URL, json={"receiver_email": IDN_RCPT})
        assert res.status_code == 200, res.text
        assert res.json() == SENT
        for message in sink.messages:
            data = message["data"]
            assert ENCODED_FROM in data, data
            assert f"To: {PUNY_RCPT}" in data, data
            assert SUBJECT in data, data
            assert message["mail_from"] == "team@x.com", message
            assert message["rcpt_tos"] == [PUNY_RCPT], message
