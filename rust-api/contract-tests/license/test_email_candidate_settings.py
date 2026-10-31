# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""EmailCredentialCheckEndpoint: candidate SMTP settings from the body (PIDASHCONV-1687).

Django main (NEWFRONT-524, `configuration.py:102-224`) grades unsaved
candidate settings — `EMAIL_HOST`, `EMAIL_PORT`, `EMAIL_HOST_USER`,
`EMAIL_HOST_PASSWORD`, `EMAIL_USE_TLS`, `EMAIL_USE_SSL`, `EMAIL_FROM` —
with per-field fallback to the stored configuration, and answers the
fallthrough 400 for an unparseable port. Ported from
`apps/api/pi_dash/tests/contract/license/test_email_credentials_check.py`,
which mocks `get_connection`; here the graded host is observed through
a live `SmtpSink` instead. The mock-only behaviors (blank password,
flag spellings, null fallback) are pinned by the Rust unit tests
`candidate_strings_fall_back_per_field`, `candidate_flags_match_python`
and `candidate_port_coercions_match_python`.

Profile note (the PIDASHCONV-1133 pattern). The contract suite runs
Django's `settings.test`, whose `EMAIL_BACKEND` is `locmem` — Django
never dials, so the sink assertions below are vacuous on the Django
leg and discriminating on the Rust leg under the SMTP backend. The
status pins hold on every leg.
"""

import socket

from _harness.sinks import SmtpSink

URL = "/api/instances/email-credentials-check/"
SENT = {"message": "Email successfully sent."}
FAILED = {"error": "Could not send email. Please check your configuration"}
NETWORK = {"error": "Network connection error. Please check your internet connection."}


def _free_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _point_mail_at(db, host, port):
    db.make_config("EMAIL_HOST", host, category="email")
    db.make_config("EMAIL_PORT", str(port), category="email")
    db.make_config("EMAIL_USE_TLS", "0", category="email")
    db.make_config("EMAIL_USE_SSL", "0", category="email")
    db.make_config("EMAIL_FROM", "Team Pi Dash <team@example.com>", category="email")


def test_candidate_host_and_port_override_stored(admin_api, world):
    # Stored points at a dead port; the candidate points at a live
    # sink. A backend that honors the candidate delivers and 200s;
    # pre-fix Rust dials the dead stored host and 400s.
    with SmtpSink() as sink:
        _point_mail_at(world["db"], "127.0.0.1", _free_port())
        res = admin_api.post(
            URL,
            json={
                "receiver_email": "to@example.com",
                "EMAIL_HOST": sink.host,
                "EMAIL_PORT": str(sink.port),
            },
        )
        assert res.status_code == 200, res.text
        assert res.json() == SENT


def test_candidate_dead_host_leaves_stored_sink_empty(admin_api, world):
    # Stored points at a live sink; the candidate host is dead. A
    # backend that honors the candidate 400s without a live dial;
    # pre-fix Rust delivers to the stored sink (200 + 1 message).
    # The status pin is profile-tolerant: locmem backends 200.
    with SmtpSink() as sink:
        _point_mail_at(world["db"], sink.host, sink.port)
        res = admin_api.post(
            URL,
            json={
                "receiver_email": "to@example.com",
                "EMAIL_HOST": "127.0.0.1",
                "EMAIL_PORT": str(_free_port()),
            },
        )
        assert res.status_code in (200, 400), res.text
        assert res.json() in (SENT, NETWORK), res.text
        assert sink.messages == []


def test_candidate_tls_flag_grades_as_given(admin_api, world):
    # Stored flags are off; the candidate turns TLS on against a
    # plain sink. A backend that grades the candidate attempts
    # STARTTLS and 400s with nothing delivered; pre-fix Rust ignores
    # the candidate and delivers (200 + 1 message).
    # The status pin is profile-tolerant: locmem backends 200.
    with SmtpSink() as sink:
        _point_mail_at(world["db"], sink.host, sink.port)
        res = admin_api.post(
            URL,
            json={"receiver_email": "to@example.com", "EMAIL_USE_TLS": "1"},
        )
        assert res.status_code in (200, 400), res.text
        assert res.json() in (SENT, FAILED), res.text
        assert sink.messages == []


def test_candidate_bad_port_sends_nothing(admin_api, world):
    # Stored points at a live sink; the candidate port is garbage. A
    # backend that grades the candidate 400s without dialing; pre-fix
    # Rust ignores the candidate and delivers (200 + 1 message).
    # The status pin is profile-tolerant: locmem backends 200.
    with SmtpSink() as sink:
        _point_mail_at(world["db"], sink.host, sink.port)
        res = admin_api.post(
            URL,
            json={"receiver_email": "to@example.com", "EMAIL_PORT": "not-a-port"},
        )
        assert res.status_code in (200, 400), res.text
        assert res.json() in (SENT, FAILED), res.text
        assert sink.messages == []
