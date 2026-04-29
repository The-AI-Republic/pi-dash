# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.apps import AppConfig


class BgtasksConfig(AppConfig):
    name = "pi_dash.bgtasks"

    def ready(self) -> None:
        # Wire the GitHub completion-comment-back signal handlers.
        from pi_dash.bgtasks import github_signals  # noqa: F401
        # Eagerly import task modules that no other startup code imports,
        # so their @shared_task decorators register with Celery at worker boot.
        from pi_dash.bgtasks import agent_ticker  # noqa: F401
        from pi_dash.bgtasks import scheduler  # noqa: F401
        # Wire the project-scheduler seed-on-workspace-create signal.
        from pi_dash.scheduler import signals as _scheduler_signals  # noqa: F401
