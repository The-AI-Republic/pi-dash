# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The one logger every ``managed_runner.*`` product event is written to.

Design §15.1 lists a set of structured events — ``enrolled`` / ``removed``,
``run_pinned``, ``queued_waiting`` / ``queued_expired``, ``token_issued`` /
``token_refresh_failed``, ``engine_version`` — that support and operations read
out of the service log. They are emitted from four different packages, so the
name they share is what makes them greppable as one stream.

Deliberately **not** ``getLogger(__name__)`` at each call site: the project's
``LOGGING`` sets ``disable_existing_loggers: True`` and declares no ``root``
logger, so a module logger such as ``pi_dash.runner.views.desktop`` resolves to
level WARNING with zero handlers and its INFO records are dropped before they
are formatted. An event that reaches no sink is not observability, so these are
emitted on a name ``settings.LOGGING`` actually configures — see the
``pi_dash.managed_runner`` entry in ``pi_dash/settings/local.py`` and
``production.py``; ``tests/unit/managed_runner/test_observability.py`` guards
both the declaration and the delivery.

The repo-wide version of this gap — every other ``getLogger(__name__)`` module
in the codebase has the same problem — is tracked in PDASHOSS01-208.

Payload rule (design §17): ids, counts and timestamps only. Never a token,
never a path under the user's home directory.
"""

from __future__ import annotations

import logging

event_logger = logging.getLogger("pi_dash.managed_runner")
