# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Per-run tenant context injected into the stateless agent.

This object carries the requesting user's identity and workspace scope. Every
tool reads tenancy from ``ctx.deps`` — never from module globals or model
arguments — which is what makes one shared ``Agent`` safe across tenants.
See ``.ai_design/integrate_ai_agent/02-backend.md`` §2.
"""

from __future__ import annotations

import uuid
from dataclasses import dataclass


@dataclass(frozen=True)
class AssistantDeps:
    user_id: uuid.UUID
    user_display: str
    workspace_id: uuid.UUID
    workspace_slug: str
    workspace_name: str
    workspace_role: int  # 20 admin / 15 member / 5 guest
    thread_id: uuid.UUID
    turn_id: uuid.UUID
