# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import models

from pi_dash.db.models.base import BaseModel


class PlatformWebhookDelivery(BaseModel):
    class Status(models.TextChoices):
        RECEIVED = "received", "Received"
        PROCESSED = "processed", "Processed"
        FAILED = "failed", "Failed"
        SKIPPED = "skipped", "Skipped"
        DEAD_LETTERED = "dead_lettered", "Dead lettered"

    delivery_id = models.UUIDField(unique=True, db_index=True)
    event_id = models.UUIDField(unique=True, db_index=True)
    event_type = models.CharField(max_length=100, db_index=True)
    platform_org_id = models.UUIDField(null=True, blank=True, db_index=True)
    platform_user_id = models.UUIDField(null=True, blank=True, db_index=True)
    payload = models.JSONField(default=dict)
    status = models.CharField(max_length=16, choices=Status.choices, default=Status.RECEIVED)
    attempt_count = models.PositiveIntegerField(default=0)
    received_at = models.DateTimeField(auto_now_add=True)
    processed_at = models.DateTimeField(null=True, blank=True)
    error = models.TextField(blank=True, default="")

    def __str__(self):
        return f"{self.event_type}:{self.event_id}"

    class Meta:
        verbose_name = "Platform Webhook Delivery"
        verbose_name_plural = "Platform Webhook Deliveries"
        db_table = "platform_webhook_deliveries"
        ordering = ("-received_at",)
        indexes = [
            models.Index(fields=["platform_org_id", "event_type"], name="platform_wh_org_event_idx"),
            models.Index(fields=["platform_user_id", "event_type"], name="platform_wh_user_event_idx"),
            models.Index(fields=["status", "received_at"], name="platform_wh_status_recv_idx"),
        ]


class PlatformFederationState(BaseModel):
    class Status(models.TextChoices):
        ACTIVE = "active", "Active"
        DISABLED = "disabled", "Disabled"
        ERROR = "error", "Error"

    workspace = models.OneToOneField(
        "db.Workspace",
        related_name="platform_federation_state",
        on_delete=models.CASCADE,
    )
    status = models.CharField(max_length=16, choices=Status.choices, default=Status.ACTIVE, db_index=True)
    last_event_id = models.UUIDField(null=True, blank=True)
    last_reconciled_at = models.DateTimeField(null=True, blank=True)
    last_error = models.TextField(blank=True, default="")

    def __str__(self):
        return f"{self.workspace_id}:{self.status}"

    class Meta:
        verbose_name = "Platform Federation State"
        verbose_name_plural = "Platform Federation States"
        db_table = "platform_federation_states"
        ordering = ("-updated_at",)
