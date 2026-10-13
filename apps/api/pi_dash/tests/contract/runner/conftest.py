# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Fixtures for runner contract tests.

Reuses the assistant contract ``world`` fixture (workspace + users at every
role) so the access-control matrix is identical.
"""

from __future__ import annotations

# Re-export the assistant fixture so it's discoverable in this package.
from pi_dash.tests.contract.assistant.conftest import world  # noqa: F401
