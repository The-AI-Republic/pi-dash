# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.authentication.views.cli import (
    DeviceCodeApproveEndpoint,
    DeviceCodeRevokeEndpoint,
    DeviceCodeStartEndpoint,
    DeviceCodeTokenEndpoint,
    WorkspaceListEndpoint,
)

urlpatterns = [
    path(
        "auth/device/start/",
        DeviceCodeStartEndpoint.as_view(),
        name="auth-device-start",
    ),
    path(
        "auth/device/approve/",
        DeviceCodeApproveEndpoint.as_view(),
        name="auth-device-approve",
    ),
    path(
        "auth/device/token/",
        DeviceCodeTokenEndpoint.as_view(),
        name="auth-device-token",
    ),
    path(
        "auth/revoke/",
        DeviceCodeRevokeEndpoint.as_view(),
        name="auth-revoke",
    ),
    path(
        "auth/workspaces/",
        WorkspaceListEndpoint.as_view(),
        name="auth-workspaces",
    ),
]
