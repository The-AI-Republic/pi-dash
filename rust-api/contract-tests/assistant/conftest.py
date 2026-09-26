# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Assistant + SSE contract suite (PIDASHCONV-20).

Black-box coverage of ``apps/api/pi_dash/assistant/urls.py`` (15 paths):
threads, messages/turns, the SSE event stream, cancel, generate-title, BYOK
LLM config + test, STT config + test, transcribe, the desktop agent-profile /
token pair, and MCP servers.

Conventions:

- Every test seeds a fresh ``world`` (unique emails/slugs); nothing is torn
  down, so the suite is re-runnable and parallel-safe.
- ``PUBLIC_BASE`` is a literal public IP, not a hostname: the contract run
  enables the SSRF guard (cloud parity), under which unresolvable hostnames
  are rejected without any DNS lookup. Numeric IPs resolve without DNS, which
  keeps the suite hermetic.
- Outbound-provider behaviour (title generation content, connection-test
  success, transcription text) cannot be pinned without network access, so
  those endpoints are covered through their deterministic gates and error
  shapes. The cases that need a live provider are the Rust proxy's job, not
  this suite's.
"""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _harness.client import anonymous_client, login_client  # noqa: E402
from _harness.worlds import World, build_world  # noqa: E402

PUBLIC_BASE = "https://8.8.8.8/v1"


@pytest.fixture
def world() -> World:
    return build_world()


@pytest.fixture
def member(world):
    return login_client(world.member)


@pytest.fixture
def admin(world):
    return login_client(world.admin)


@pytest.fixture
def guest(world):
    return login_client(world.guest)


@pytest.fixture
def outsider_client(world):
    return login_client(world.other_user)


@pytest.fixture
def project_outsider(world):
    # Workspace member with no project role: passes the workspace-scoped
    # endpoint gate (project roles are a tool concern, not an endpoint one).
    return login_client(world.outsider)


@pytest.fixture
def anon():
    return anonymous_client()


def threads_url(world: World) -> str:
    return f"/api/workspaces/{world.ws.slug}/ai-assistant"
