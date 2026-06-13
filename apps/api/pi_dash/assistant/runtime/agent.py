# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The single, stateless, module-level agent shared by all tenants.

Holds zero tenant data: a model is supplied per run, and tenant scope arrives
via ``deps``. Concurrency-safe because all run state is call-local. See
``.ai_design/integrate_ai_agent/02-backend.md`` §2.
"""

from __future__ import annotations

from pydantic_ai import Agent

from pi_dash.assistant.runtime.deps import AssistantDeps
from pi_dash.assistant.runtime.instructions import BASE_INSTRUCTIONS, dynamic_instructions

# ``retries`` here are pydantic-ai output/tool validation retries — distinct
# from Celery task retries (which are disabled; see tasks.py).
assistant = Agent(
    deps_type=AssistantDeps,
    instructions=BASE_INSTRUCTIONS,
    retries=2,
)

# Register the per-run dynamic context (workspace/user/date) as instructions so
# it is re-sent fresh each turn and not duplicated from replayed history.
assistant.instructions(dynamic_instructions)
