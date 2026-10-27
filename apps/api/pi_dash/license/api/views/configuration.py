# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Python imports
from smtplib import (
    SMTPAuthenticationError,
    SMTPConnectError,
    SMTPRecipientsRefused,
    SMTPSenderRefused,
    SMTPServerDisconnected,
)

# Django imports
from django.core.mail import BadHeaderError, EmailMultiAlternatives, get_connection
from django.db.models import Q, Case, When, Value

# Third party imports
from rest_framework import status
from rest_framework.response import Response

# Module imports
from .base import BaseAPIView
from pi_dash.license.api.permissions import InstanceAdminPermission
from pi_dash.license.models import InstanceConfiguration
from pi_dash.license.api.serializers import InstanceConfigurationSerializer
from pi_dash.license.utils.encryption import encrypt_data
from pi_dash.utils.cache import cache_response, invalidate_cache
from pi_dash.license.utils.instance_value import get_email_configuration


class InstanceConfigurationEndpoint(BaseAPIView):
    permission_classes = [InstanceAdminPermission]

    @cache_response(60 * 60 * 2, user=False)
    def get(self, request):
        instance_configurations = InstanceConfiguration.objects.all()
        serializer = InstanceConfigurationSerializer(instance_configurations, many=True)
        return Response(serializer.data, status=status.HTTP_200_OK)

    @invalidate_cache(path="/api/instances/configurations/", user=False)
    @invalidate_cache(path="/api/instances/", user=False)
    def patch(self, request):
        configurations = InstanceConfiguration.objects.filter(key__in=request.data.keys())

        bulk_configurations = []
        for configuration in configurations:
            raw_value = request.data.get(configuration.key, configuration.value)
            value = "" if raw_value is None else str(raw_value).strip()
            if configuration.is_encrypted:
                configuration.value = encrypt_data(value)
            else:
                configuration.value = value
            bulk_configurations.append(configuration)

        if bulk_configurations:
            InstanceConfiguration.objects.bulk_update(bulk_configurations, ["value"], batch_size=100)

        serializer = InstanceConfigurationSerializer(configurations, many=True)
        return Response(serializer.data, status=status.HTTP_200_OK)


class DisableEmailFeatureEndpoint(BaseAPIView):
    permission_classes = [InstanceAdminPermission]

    @invalidate_cache(path="/api/instances/", user=False)
    def delete(self, request):
        try:
            InstanceConfiguration.objects.filter(
                Q(
                    key__in=[
                        "EMAIL_HOST",
                        "EMAIL_HOST_USER",
                        "EMAIL_HOST_PASSWORD",
                        "ENABLE_SMTP",
                        "EMAIL_PORT",
                        "EMAIL_FROM",
                    ]
                )
            ).update(value=Case(When(key="ENABLE_SMTP", then=Value("0")), default=Value("")))
            return Response(status=status.HTTP_200_OK)
        except Exception:
            return Response(
                {"error": "Failed to disable email configuration"},
                status=status.HTTP_400_BAD_REQUEST,
            )


def _flag_is_enabled(value):
    """Normalize a transport-security flag to bool.

    Stored flags are "1"/"0" strings; candidates may also arrive as JSON
    booleans or numbers.
    """
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return bool(value)
    return str(value).strip().lower() in ("1", "true", "t", "yes", "y", "on")


class EmailCredentialCheckEndpoint(BaseAPIView):
    """Grade SMTP settings by sending a real message to ``receiver_email``.

    Besides ``receiver_email`` the body accepts the optional candidate
    settings ``EMAIL_HOST``, ``EMAIL_PORT``, ``EMAIL_HOST_USER``,
    ``EMAIL_HOST_PASSWORD``, ``EMAIL_USE_TLS``, ``EMAIL_USE_SSL`` and
    ``EMAIL_FROM`` (same keys the configurations endpoint stores), so an
    admin can grade unsaved form values before saving them. Omitted or null
    fields fall back to the stored configuration; a blank password also
    falls back to the stored one, since stored secrets read back blank and
    blank therefore means "unchanged", not "empty".
    """

    def post(self, request):
        receiver_email = request.data.get("receiver_email", False)
        if not receiver_email:
            return Response(
                {"error": "Receiver email is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        (
            stored_host,
            stored_user,
            stored_password,
            stored_port,
            stored_use_tls,
            stored_use_ssl,
            stored_from,
        ) = get_email_configuration()

        def candidate(key, stored, *, blank_means_stored=False):
            if key not in request.data:
                return stored
            value = request.data.get(key)
            if value is None:
                return stored
            if isinstance(value, str):
                value = value.strip()
                if blank_means_stored and not value:
                    return stored
            return value

        EMAIL_HOST = candidate("EMAIL_HOST", stored_host)
        EMAIL_HOST_USER = candidate("EMAIL_HOST_USER", stored_user)
        EMAIL_HOST_PASSWORD = candidate(
            "EMAIL_HOST_PASSWORD", stored_password, blank_means_stored=True
        )
        EMAIL_FROM = candidate("EMAIL_FROM", stored_from)

        try:
            port = int(str(candidate("EMAIL_PORT", stored_port)).strip())
        except (TypeError, ValueError):
            return Response(
                {"error": "Could not send email. Please check your configuration"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        # Configure all the connections
        connection = get_connection(
            host=EMAIL_HOST,
            port=port,
            username=EMAIL_HOST_USER,
            password=EMAIL_HOST_PASSWORD,
            use_tls=_flag_is_enabled(candidate("EMAIL_USE_TLS", stored_use_tls)),
            use_ssl=_flag_is_enabled(candidate("EMAIL_USE_SSL", stored_use_ssl)),
        )
        # Prepare email details
        subject = "Email Notification from Pi Dash"
        message = "This is a sample email notification sent from Pi Dash application."
        # Send the email
        try:
            msg = EmailMultiAlternatives(
                subject=subject,
                body=message,
                from_email=EMAIL_FROM,
                to=[receiver_email],
                connection=connection,
            )
            msg.send(fail_silently=False)
            return Response({"message": "Email successfully sent."}, status=status.HTTP_200_OK)
        except BadHeaderError:
            return Response({"error": "Invalid email header."}, status=status.HTTP_400_BAD_REQUEST)
        except SMTPAuthenticationError:
            return Response(
                {"error": "Invalid credentials provided"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        except SMTPConnectError:
            return Response(
                {"error": "Could not connect with the SMTP server."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        except SMTPSenderRefused:
            return Response(
                {"error": "From address is invalid."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        except SMTPServerDisconnected:
            return Response(
                {"error": "SMTP server disconnected unexpectedly."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        except SMTPRecipientsRefused:
            return Response(
                {"error": "All recipient addresses were refused."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        except TimeoutError:
            return Response(
                {"error": "Timeout error while trying to connect to the SMTP server."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        except ConnectionError:
            return Response(
                {"error": "Network connection error. Please check your internet connection."},
                status=status.HTTP_400_BAD_REQUEST,
            )
        except Exception:
            return Response(
                {"error": "Could not send email. Please check your configuration"},
                status=status.HTTP_400_BAD_REQUEST,
            )
