# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""The extra-toolsets seam.

Two properties matter and neither is visible from one call site:

1. Tool names from this seam never enter ``tools`` or ``required_tools``. If
   they did, an external change — a user uninstalling something — would make
   ``resolve_current_tool_names`` raise and fail runs on unrelated work items.
2. The opt-in is *snapshotted* at plan time. A run executes under the policy it
   was admitted with, the same contract ``executor_kind`` has.
"""

from __future__ import annotations

import pytest

from pi_dash.cloud_agent import policy
from pi_dash.ee.cloud_agent import toolsets

pytestmark = pytest.mark.unit


class _User:
    pass


def _plan(**over):
    kwargs = {"run_kind": "issue", "has_issue": True}
    kwargs.update(over)
    return policy.build_tool_plan(**kwargs)


# --------------------------------------------------------------------------- #
# CE defaults
# --------------------------------------------------------------------------- #


def test_ce_grants_no_extra_toolsets():
    assert toolsets.extra_toolsets_enabled_for(_User()) is False
    assert toolsets.resolve_extra_toolsets_for_run(object()) == []


def test_a_plan_without_a_creator_does_not_enable_them():
    # Scheduler and system-initiated runs may have no actor; absent an explicit
    # opt-in the answer is no.
    assert _plan()["extra_toolsets"] is False


def test_ce_never_enables_them_even_with_a_creator():
    assert _plan(creator=_User())["extra_toolsets"] is False


# --------------------------------------------------------------------------- #
# The snapshot
# --------------------------------------------------------------------------- #


def test_an_opted_in_creator_is_recorded_on_the_plan(monkeypatch):
    monkeypatch.setattr(toolsets, "extra_toolsets_enabled_for", lambda user: True)
    assert _plan(creator=_User())["extra_toolsets"] is True


def test_the_flag_is_a_sibling_of_tools_never_a_member(monkeypatch):
    # The whole point: these tool names are unknowable at plan time, so the
    # plan records *permission*, not names. A name here would eventually reach
    # required_tools and make an unrelated external change fail runs.
    monkeypatch.setattr(toolsets, "extra_toolsets_enabled_for", lambda user: True)
    plan = _plan(creator=_User())

    assert "extra_toolsets" in plan
    assert isinstance(plan["extra_toolsets"], bool)
    assert "extra_toolsets" not in plan["tools"]
    assert "extra_toolsets" not in plan["required_tools"]


def test_enabling_extra_toolsets_does_not_change_the_tool_catalog(monkeypatch):
    before = _plan(creator=_User())
    monkeypatch.setattr(toolsets, "extra_toolsets_enabled_for", lambda user: True)
    after = _plan(creator=_User())

    assert after["tools"] == before["tools"]
    assert after["required_tools"] == before["required_tools"]


def test_resolve_current_tool_names_is_unaffected_by_the_flag(monkeypatch):
    """The dispatch-time re-intersection must not see this flag at all.

    ``resolve_current_tool_names`` raises when a *required* tool disappears.
    Keeping the flag out of the name sets is what stops an uninstall elsewhere
    from failing an unrelated run.
    """
    monkeypatch.setattr(toolsets, "extra_toolsets_enabled_for", lambda user: True)
    plan = _plan(creator=_User())

    names = set(plan["tools"]) | set(plan["required_tools"])
    assert not any("extra" in n for n in names)


# --------------------------------------------------------------------------- #
# The runtime honours the seam
# --------------------------------------------------------------------------- #


def test_the_runtime_appends_whatever_the_seam_returns(monkeypatch):
    """A seam nothing calls is the failure mode worth guarding.

    Asserted by inspecting the toolsets handed to ``Agent``, because a seam
    that is imported but never reached would still import cleanly and pass
    every other test here.
    """
    import asyncio
    from types import SimpleNamespace
    from unittest.mock import patch

    from pi_dash.cloud_agent import runtime

    sentinel = object()
    captured = {}

    class _Agent:
        def __init__(self, **kwargs):
            captured.update(kwargs)

        async def run(self, *a, **kw):  # pragma: no cover - not the assertion
            raise RuntimeError("stop after construction")

    run = SimpleNamespace(
        id="r1",
        prompt="do the thing",
        tool_plan={"tools": [], "limits": {}, "extra_toolsets": True},
        created_by=_User(),
    )

    with (
        patch("pydantic_ai.Agent", _Agent),
        patch("pi_dash.ee.cloud_agent.model_provider.resolve_model_for_run", return_value=object()),
        patch(
            "pi_dash.ee.cloud_agent.toolsets.resolve_extra_toolsets_for_run",
            return_value=[sentinel],
        ),
        patch("pi_dash.cloud_agent.events.append"),
    ):
        with pytest.raises(RuntimeError, match="stop after construction"):
            asyncio.run(runtime.execute(run))

    assert sentinel in captured["toolsets"]
