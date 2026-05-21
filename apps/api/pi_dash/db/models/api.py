# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Python imports
import secrets
from uuid import uuid4

# Django imports
from django.db import models
from django.conf import settings

from .base import BaseModel


def generate_label_token():
    return uuid4().hex


def generate_token():
    return "pi_dash_api_" + uuid4().hex


def generate_device_code():
    return secrets.token_urlsafe(32)


def generate_user_code():
    # 8-char, ambiguity-free alphabet, hyphenated for readability ("WXYZ-1234").
    alphabet = "BCDFGHJKLMNPQRSTVWXZ23456789"
    raw = "".join(secrets.choice(alphabet) for _ in range(8))
    return f"{raw[:4]}-{raw[4:]}"


class APIToken(BaseModel):
    # Meta information
    label = models.CharField(max_length=255, default=generate_label_token)
    description = models.TextField(blank=True)
    is_active = models.BooleanField(default=True)
    last_used = models.DateTimeField(null=True)

    # Token
    token = models.CharField(max_length=255, unique=True, default=generate_token, db_index=True)

    # User Information
    user = models.ForeignKey(settings.AUTH_USER_MODEL, on_delete=models.CASCADE, related_name="bot_tokens")
    user_type = models.PositiveSmallIntegerField(choices=((0, "Human"), (1, "Bot")), default=0)
    workspace = models.ForeignKey("db.Workspace", related_name="api_tokens", on_delete=models.CASCADE, null=True)
    expired_at = models.DateTimeField(blank=True, null=True)
    is_service = models.BooleanField(default=False)
    allowed_rate_limit = models.CharField(max_length=255, default="60/min")

    class Meta:
        verbose_name = "API Token"
        verbose_name_plural = "API Tokems"
        db_table = "api_tokens"
        ordering = ("-created_at",)

    def __str__(self):
        return str(self.user.id)


class CLIDeviceCode(BaseModel):
    """RFC 8628-shaped device-authorization grant for `pidash auth login`.

    Lifecycle: ``pidash auth login`` POSTs to start, gets back
    ``device_code`` (opaque, returned to CLI only) + ``user_code``
    (short, shown to the human). User opens the verification URL in a
    browser, signs in, enters ``user_code``, approves — server stamps
    ``user`` + ``approved``. CLI polls the token endpoint with
    ``device_code`` and trades the approved row for a fresh
    :class:`APIToken`. Row is then marked ``consumed`` to prevent reuse.
    """

    device_code = models.CharField(max_length=64, unique=True, default=generate_device_code, db_index=True)
    user_code = models.CharField(max_length=16, unique=True, default=generate_user_code, db_index=True)
    user = models.ForeignKey(settings.AUTH_USER_MODEL, on_delete=models.CASCADE, null=True, related_name="device_codes")
    workspace = models.ForeignKey(
        "db.Workspace", related_name="device_codes", on_delete=models.CASCADE, null=True, blank=True
    )
    approved = models.BooleanField(default=False)
    denied = models.BooleanField(default=False)
    consumed = models.BooleanField(default=False)
    expires_at = models.DateTimeField()
    last_polled_at = models.DateTimeField(null=True, blank=True)

    class Meta:
        verbose_name = "CLI Device Code"
        verbose_name_plural = "CLI Device Codes"
        db_table = "cli_device_codes"
        ordering = ("-created_at",)

    def __str__(self):
        return self.user_code


class APIActivityLog(BaseModel):
    token_identifier = models.CharField(max_length=255)

    # Request Info
    path = models.CharField(max_length=255)
    method = models.CharField(max_length=10)
    query_params = models.TextField(null=True, blank=True)
    headers = models.TextField(null=True, blank=True)
    body = models.TextField(null=True, blank=True)

    # Response info
    response_code = models.PositiveIntegerField()
    response_body = models.TextField(null=True, blank=True)

    # Meta information
    ip_address = models.GenericIPAddressField(null=True, blank=True)
    user_agent = models.CharField(max_length=512, null=True, blank=True)

    class Meta:
        verbose_name = "API Activity Log"
        verbose_name_plural = "API Activity Logs"
        db_table = "api_activity_logs"
        ordering = ("-created_at",)

    def __str__(self):
        return str(self.token_identifier)
