# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""EmailCredentialCheckEndpoint: `to=[receiver_email]` parity (PIDASHCONV-1133).

Django builds `to=[receiver_email]` verbatim (`configuration.py:120-127`).
Its SMTP backend parses each recipient with `sanitize_address`
(`django/core/mail/backends/smtp.py`): a string or a scalar is `force_str`ed
into one mailbox and delivered, while a non-empty array or object repr
(`"['a@x.test', 'b@x.test']"`, `"{'email': 'a@x.test'}"`) never parses, so
`send()` raises inside the view's `try` and the generic 400 is answered with
nothing delivered (`configuration.py:168-172`). Rust used to flatten an array
into one recipient per element and deliver real mail to every address.

Profile note. The contract suite runs Django's `settings.test`, whose
`EMAIL_BACKEND` is `locmem` — no address ever reaches `sanitize_address`, so
the suite's Django leg answers 200 for every shape and dials nowhere. What is
pinned below is therefore the side effect, which is profile-independent at the
Django end and load-bearing at Rust's: an array/object receiver must deliver
nothing, while a string receiver must still succeed. The container 400 body is
SMTP-profile-only; it is pinned by the Rust unit test
`email_recipient_shapes_match_django` and was verified live against a
`settings.local` + `EMAIL_BACKEND=smtp` Django, byte for byte:
`400 {"error":"Could not send email. Please check your configuration"}`
(65 bytes) with nothing delivered.
"""

from _harness.sinks import SmtpSink

URL = "/api/instances/email-credentials-check/"
SENT = {"message": "Email successfully sent."}

# Shapes Django's SMTP `sanitize_address` cannot parse into a mailbox.
UNPARSABLE_SHAPES = [["a@x.test", "b@x.test"], {"email": "a@x.test"}]


def _point_mail_at(db, sink):
    db.make_config("EMAIL_HOST", sink.host, category="email")
    db.make_config("EMAIL_PORT", str(sink.port), category="email")
    db.make_config("EMAIL_USE_TLS", "0", category="email")
    db.make_config("EMAIL_USE_SSL", "0", category="email")
    db.make_config("EMAIL_FROM", "Team Pi Dash <team@example.com>", category="email")


def test_array_and_object_receivers_deliver_nothing(admin_api, world):
    db = world["db"]
    with SmtpSink() as sink:
        _point_mail_at(db, sink)
        for value in UNPARSABLE_SHAPES:
            res = admin_api.post(URL, json={"receiver_email": value})
            assert res.status_code in (200, 400), (value, res.status_code, res.text)
        assert sink.messages == []


def test_string_receiver_still_sends(admin_api, world):
    db = world["db"]
    with SmtpSink() as sink:
        _point_mail_at(db, sink)
        res = admin_api.post(URL, json={"receiver_email": "only@x.test"})
        assert res.status_code == 200, res.text
        assert res.json() == SENT
