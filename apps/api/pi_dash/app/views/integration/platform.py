# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

import json
import uuid

from rest_framework import status
from rest_framework.permissions import AllowAny
from rest_framework.response import Response

from pi_dash.app.views.base import BaseAPIView
from pi_dash.core.platform_federation import (
    PlatformConfigurationError,
    PlatformFederationError,
    apply_platform_event,
    platform_federation_enabled,
    verify_ios_webhook_signature,
)
from pi_dash.db.models import PlatformWebhookDelivery
from pi_dash.utils.exception_logger import log_exception


def _safe_uuid(value):
    if not value:
        return None
    try:
        return uuid.UUID(str(value))
    except (TypeError, ValueError):
        return None


class PlatformIosWebhookEndpoint(BaseAPIView):
    """POST /integrations/platform/ios/webhook/"""

    authentication_classes = []
    permission_classes = [AllowAny]
    throttle_classes = []

    def post(self, request):
        if not platform_federation_enabled():
            return Response({"error": "Platform federation is disabled"}, status=status.HTTP_404_NOT_FOUND)

        try:
            if not verify_ios_webhook_signature(
                request.body,
                request.headers.get("X-IOS-Timestamp"),
                request.headers.get("X-IOS-Signature-256"),
            ):
                return Response({"error": "Invalid signature"}, status=status.HTTP_401_UNAUTHORIZED)
        except PlatformConfigurationError as exc:
            return Response({"error": str(exc)}, status=status.HTTP_409_CONFLICT)
        except PlatformFederationError:
            return Response({"error": "Invalid signature timestamp"}, status=status.HTTP_401_UNAUTHORIZED)

        delivery_header = request.headers.get("X-IOS-Delivery")
        if not delivery_header:
            return Response({"error": "Missing X-IOS-Delivery"}, status=status.HTTP_400_BAD_REQUEST)
        try:
            delivery_id = uuid.UUID(delivery_header)
        except ValueError:
            return Response({"error": "Invalid X-IOS-Delivery"}, status=status.HTTP_400_BAD_REQUEST)

        try:
            payload = json.loads(request.body.decode("utf-8") or "{}")
        except json.JSONDecodeError:
            return Response({"error": "Invalid JSON"}, status=status.HTTP_400_BAD_REQUEST)

        event_type = payload.get("event_type") or request.headers.get("X-IOS-Event") or ""
        if request.headers.get("X-IOS-Event") and request.headers.get("X-IOS-Event") != event_type:
            return Response({"error": "X-IOS-Event mismatch"}, status=status.HTTP_400_BAD_REQUEST)
        try:
            event_id = uuid.UUID(str(payload.get("event_id")))
        except (TypeError, ValueError):
            return Response({"error": "Invalid event_id"}, status=status.HTTP_400_BAD_REQUEST)

        org = payload.get("org") or {}
        subject = payload.get("subject") or {}
        delivery, created = PlatformWebhookDelivery.objects.get_or_create(
            event_id=event_id,
            defaults={
                "delivery_id": delivery_id,
                "event_type": event_type,
                "platform_org_id": _safe_uuid(org.get("org_id")),
                "platform_user_id": _safe_uuid(subject.get("user_id")),
                "payload": payload,
                "status": PlatformWebhookDelivery.Status.RECEIVED,
            },
        )
        if not created and delivery.status != PlatformWebhookDelivery.Status.FAILED:
            return Response({"status": delivery.status}, status=status.HTTP_202_ACCEPTED)

        delivery.delivery_id = delivery_id
        delivery.event_type = event_type
        delivery.payload = payload
        delivery.platform_org_id = _safe_uuid(org.get("org_id"))
        delivery.platform_user_id = _safe_uuid(subject.get("user_id"))
        delivery.attempt_count += 1
        try:
            result = apply_platform_event(payload)
            delivery.status = result
            delivery.error = ""
            delivery.processed_at = None
            if result in {PlatformWebhookDelivery.Status.PROCESSED, PlatformWebhookDelivery.Status.SKIPPED}:
                from django.utils import timezone

                delivery.processed_at = timezone.now()
            delivery.save(
                update_fields=[
                    "delivery_id",
                    "event_type",
                    "payload",
                    "platform_org_id",
                    "platform_user_id",
                    "attempt_count",
                    "status",
                    "error",
                    "processed_at",
                    "updated_at",
                ]
            )
            return Response({"status": delivery.status}, status=status.HTTP_202_ACCEPTED)
        except PlatformFederationError as exc:
            from django.utils import timezone

            delivery.status = PlatformWebhookDelivery.Status.DEAD_LETTERED
            delivery.error = str(exc)[:2000]
            delivery.processed_at = timezone.now()
            delivery.save(update_fields=["attempt_count", "status", "error", "processed_at", "updated_at"])
            return Response({"error": str(exc)}, status=status.HTTP_400_BAD_REQUEST)
        except Exception as exc:
            from django.utils import timezone

            log_exception(exc)
            delivery.status = PlatformWebhookDelivery.Status.FAILED
            delivery.error = str(exc)[:2000]
            delivery.processed_at = timezone.now()
            delivery.save(update_fields=["attempt_count", "status", "error", "processed_at", "updated_at"])
            return Response({"error": "Platform event processing failed"}, status=status.HTTP_500_INTERNAL_SERVER_ERROR)
