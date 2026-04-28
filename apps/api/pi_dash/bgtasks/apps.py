# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.apps import AppConfig


class BgtasksConfig(AppConfig):
    name = "pi_dash.bgtasks"

    def ready(self) -> None:
        # Wire the GitHub completion-comment-back signal handlers.
        from pi_dash.bgtasks import github_signals  # noqa: F401
