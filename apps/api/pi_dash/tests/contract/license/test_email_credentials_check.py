# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests for grading candidate SMTP settings (NEWFRONT-524).

POST /api/instances/email-credentials-check/ accepts optional candidate
SMTP fields alongside ``receiver_email`` and grades those; omitted fields
fall back to the stored configuration so receiver-only requests behave
exactly as before. A blank password also falls back to the stored one:
stored secrets read back blank, so blank means "unchanged", not "empty".
"""

from smtplib import SMTPAuthenticationError
from unittest.mock import patch

import pytest
from django.utils import timezone
from rest_framework.test import APIClient

from pi_dash.db.models import User
from pi_dash.license.models import Instance, InstanceAdmin

pytestmark = [pytest.mark.contract, pytest.mark.django_db]

URL = "/api/instances/email-credentials-check/"

# (host, user, password, port, use_tls, use_ssl, from) as
# get_email_configuration() returns it.
STORED = (
    "stored.smtp.example",
    "stored-user",
    "stored-secret",
    "2525",
    "1",
    "0",
    "stored@example.com",
)


@pytest.fixture
def admin_user(db):
    user = User.objects.create(
        email="godmode@example.com", username="godmode", first_name="Root"
    )
    user.set_password("x")
    user.save()
    instance = Instance.objects.create(
        instance_name="test",
        instance_id="i1",
        current_version="1.0.0",
        last_checked_at=timezone.now(),
    )
    InstanceAdmin.objects.create(instance=instance, user=user, role=20, is_verified=True)
    return user


@pytest.fixture
def plain_user(db):
    user = User.objects.create(
        email="member@example.com", username="member", first_name="Member"
    )
    user.set_password("x")
    user.save()
    return user


@pytest.fixture
def admin_client(admin_user):
    client = APIClient()
    client.force_authenticate(user=admin_user)
    return client


@pytest.fixture
def smtp_mocks():
    with (
        patch(
            "pi_dash.license.api.views.configuration.get_email_configuration",
            return_value=STORED,
        ),
        patch(
            "pi_dash.license.api.views.configuration.get_connection"
        ) as get_connection,
        patch(
            "pi_dash.license.api.views.configuration.EmailMultiAlternatives"
        ) as message_class,
    ):
        message_class.return_value.send.return_value = 1
        yield get_connection, message_class


def _connection_kwargs(get_connection):
    assert get_connection.call_count == 1
    return get_connection.call_args.kwargs


def _message_kwargs(message_class):
    assert message_class.call_count == 1
    return message_class.call_args.kwargs


def test_requires_receiver_email(admin_client, smtp_mocks):
    get_connection, message_class = smtp_mocks
    response = admin_client.post(URL, {"EMAIL_HOST": "smtp.example.com"}, format="json")
    assert response.status_code == 400
    assert response.data == {"error": "Receiver email is required"}
    get_connection.assert_not_called()
    message_class.assert_not_called()


def test_non_instance_admin_refused(plain_user, smtp_mocks):
    client = APIClient()
    client.force_authenticate(user=plain_user)
    response = client.post(URL, {"receiver_email": "to@example.com"}, format="json")
    assert response.status_code == 403


def test_receiver_only_grades_stored_configuration(admin_client, smtp_mocks):
    get_connection, message_class = smtp_mocks
    response = admin_client.post(
        URL, {"receiver_email": "to@example.com"}, format="json"
    )
    assert response.status_code == 200
    assert response.data == {"message": "Email successfully sent."}
    assert _connection_kwargs(get_connection) == {
        "host": "stored.smtp.example",
        "port": 2525,
        "username": "stored-user",
        "password": "stored-secret",
        "use_tls": True,
        "use_ssl": False,
    }
    message_kwargs = _message_kwargs(message_class)
    assert message_kwargs["from_email"] == "stored@example.com"
    assert message_kwargs["to"] == ["to@example.com"]


def test_noncanonical_stored_flags_grade_as_runtime_would(admin_client):
    """Stored non-"1" spellings (e.g. env EMAIL_USE_TLS=true with no DB row).

    A receiver-only check must agree with both the old `== "1"` check and
    production sends, so stored "true"/"True" grade as off.
    """
    stored = (
        "stored.smtp.example",
        "stored-user",
        "stored-secret",
        "2525",
        "true",
        "True",
        "stored@example.com",
    )
    with (
        patch(
            "pi_dash.license.api.views.configuration.get_email_configuration",
            return_value=stored,
        ),
        patch(
            "pi_dash.license.api.views.configuration.get_connection"
        ) as get_connection,
        patch(
            "pi_dash.license.api.views.configuration.EmailMultiAlternatives"
        ) as message_class,
    ):
        message_class.return_value.send.return_value = 1
        response = admin_client.post(
            URL, {"receiver_email": "to@example.com"}, format="json"
        )
    assert response.status_code == 200
    kwargs = _connection_kwargs(get_connection)
    assert (kwargs["use_tls"], kwargs["use_ssl"]) == (False, False)


def test_candidate_settings_override_stored(admin_client, smtp_mocks):
    get_connection, message_class = smtp_mocks
    response = admin_client.post(
        URL,
        {
            "receiver_email": "to@example.com",
            "EMAIL_HOST": "candidate.smtp.example",
            "EMAIL_PORT": "1025",
            "EMAIL_HOST_USER": "candidate-user",
            "EMAIL_HOST_PASSWORD": "candidate-secret",
            "EMAIL_USE_TLS": "0",
            "EMAIL_USE_SSL": "1",
            "EMAIL_FROM": "candidate@example.com",
        },
        format="json",
    )
    assert response.status_code == 200
    assert _connection_kwargs(get_connection) == {
        "host": "candidate.smtp.example",
        "port": 1025,
        "username": "candidate-user",
        "password": "candidate-secret",
        "use_tls": False,
        "use_ssl": True,
    }
    assert _message_kwargs(message_class)["from_email"] == "candidate@example.com"


def test_omitted_fields_fall_back_to_stored(admin_client, smtp_mocks):
    get_connection, message_class = smtp_mocks
    response = admin_client.post(
        URL,
        {"receiver_email": "to@example.com", "EMAIL_HOST": "candidate.smtp.example"},
        format="json",
    )
    assert response.status_code == 200
    assert _connection_kwargs(get_connection) == {
        "host": "candidate.smtp.example",
        "port": 2525,
        "username": "stored-user",
        "password": "stored-secret",
        "use_tls": True,
        "use_ssl": False,
    }
    assert _message_kwargs(message_class)["from_email"] == "stored@example.com"


@pytest.mark.parametrize("blank", ["", "   ", None])
def test_blank_password_falls_back_to_stored(admin_client, smtp_mocks, blank):
    get_connection, _ = smtp_mocks
    response = admin_client.post(
        URL,
        {"receiver_email": "to@example.com", "EMAIL_HOST_PASSWORD": blank},
        format="json",
    )
    assert response.status_code == 200
    assert _connection_kwargs(get_connection)["password"] == "stored-secret"


@pytest.mark.parametrize(
    "port, use_tls, use_ssl, expected",
    [
        (1025, True, False, (1025, True, False)),
        ("1026", 1, 0, (1026, True, False)),
        # Non-canonical spellings grade as runtime would treat them after
        # saving (off): save stores verbatim, runtime compares == "1".
        (" 1027 ", "true", "True", (1027, False, False)),
        ("1028", "1", "0", (1028, True, False)),
    ],
)
def test_flag_and_port_coercions(admin_client, smtp_mocks, port, use_tls, use_ssl, expected):
    get_connection, _ = smtp_mocks
    response = admin_client.post(
        URL,
        {
            "receiver_email": "to@example.com",
            "EMAIL_PORT": port,
            "EMAIL_USE_TLS": use_tls,
            "EMAIL_USE_SSL": use_ssl,
        },
        format="json",
    )
    assert response.status_code == 200
    kwargs = _connection_kwargs(get_connection)
    assert (kwargs["port"], kwargs["use_tls"], kwargs["use_ssl"]) == expected


def test_empty_nonpassword_graded_as_given_and_null_falls_back(
    admin_client, smtp_mocks
):
    """Only the password treats blank as stored; other present fields grade
    as given, while null falls back to stored per field."""
    get_connection, _ = smtp_mocks
    response = admin_client.post(
        URL,
        {"receiver_email": "to@example.com", "EMAIL_HOST": "", "EMAIL_PORT": None},
        format="json",
    )
    assert response.status_code == 200
    kwargs = _connection_kwargs(get_connection)
    assert kwargs["host"] == ""
    assert kwargs["port"] == 2525


def test_candidate_auth_failure_mapping_preserved(admin_client, smtp_mocks):
    _, message_class = smtp_mocks
    message_class.return_value.send.side_effect = SMTPAuthenticationError(
        535, b"authentication failed"
    )
    response = admin_client.post(
        URL,
        {
            "receiver_email": "to@example.com",
            "EMAIL_HOST": "candidate.smtp.example",
            "EMAIL_HOST_PASSWORD": "wrong-secret",
        },
        format="json",
    )
    assert response.status_code == 400
    assert response.data == {"error": "Invalid credentials provided"}


def test_invalid_candidate_port_rejected(admin_client, smtp_mocks):
    get_connection, _ = smtp_mocks
    response = admin_client.post(
        URL,
        {"receiver_email": "to@example.com", "EMAIL_PORT": "not-a-port"},
        format="json",
    )
    assert response.status_code == 400
    assert response.data == {
        "error": "Could not send email. Please check your configuration"
    }
    get_connection.assert_not_called()


def test_unknown_keys_ignored(admin_client, smtp_mocks):
    get_connection, _ = smtp_mocks
    response = admin_client.post(
        URL,
        {"receiver_email": "to@example.com", "ENABLE_SMTP": "1", "whatever": "x"},
        format="json",
    )
    assert response.status_code == 200
    assert _connection_kwargs(get_connection)["host"] == "stored.smtp.example"
