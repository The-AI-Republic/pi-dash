# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Neutral, app-agnostic shared helpers (permissions, querysets).

``pi_dash.core`` is a plain Python package, NOT a Django app — it holds logic
that multiple apps (``app``, ``runner``, ``assistant``) must share without
coupling to each other. It MUST NOT import from ``pi_dash.runner`` or
``pi_dash.assistant``. See ``.ai_design/integrate_ai_agent/02-backend.md`` §5.
"""
