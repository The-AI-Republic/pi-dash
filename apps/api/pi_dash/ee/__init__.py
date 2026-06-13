# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Community-edition (CE) seams that the enterprise/cloud build overlays.

Files under ``pi_dash/ee/`` are CE stubs. The private cloud repo replaces
matching paths from its ``ee-overlay/`` tree at Docker build time. OSS callers
import from here and get the open-source behaviour; cloud callers get the
overlaid implementation. See ``.ai_design/integrate_ai_agent/04-cloud.md`` §3.
"""
