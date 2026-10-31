# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.urls import path

from pi_dash.authentication.views.cli import (
    DesktopHandoffCodeEndpoint,
    DesktopHandoffRedeemEndpoint,
    DeviceCodeApproveEndpoint,
    DeviceCodeRevokeEndpoint,
    DeviceCodeStartEndpoint,
    DeviceCodeTokenEndpoint,
    DeviceMachineTokenEndpoint,
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
        "auth/machine-token/",
        DeviceMachineTokenEndpoint.as_view(),
        name="auth-machine-token",
    ),
    path(
        "auth/desktop-handoff/",
        DesktopHandoffCodeEndpoint.as_view(),
        name="auth-desktop-handoff",
    ),
    # OSS default for ``settings.DESKTOP_HANDOFF_REDEEM_PATH``. Deployments
    # that override the setting serve their own redeem route.
    path(
        "auth/desktop-handoff/redeem/",
        DesktopHandoffRedeemEndpoint.as_view(),
        name="auth-desktop-handoff-redeem",
    ),
    path(
        "auth/workspaces/",
        WorkspaceListEndpoint.as_view(),
        name="auth-workspaces",
    ),
]
