# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Importing this package registers every tool onto the shared agent.

Each tool module decorates its functions with ``@assistant.tool`` at import
time, so importing the package (done in ``AssistantConfig.ready``) wires them up.
"""

from pi_dash.assistant.tools import comments, github, issues, projects, runs  # noqa: F401

__all__ = ["projects", "issues", "comments", "runs", "github"]
