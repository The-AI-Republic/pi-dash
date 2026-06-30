# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Human-facing diagnostics for terminal agent runs.

These helpers intentionally derive from the persisted ``AgentRun.error`` text
instead of adding new columns. That lets old failed runs become more legible as
soon as the API ships.
"""

from __future__ import annotations

from typing import Any

_ENRICHED_RAW_ERROR_MARKER = "Raw agent error:"
_AGENT_AUTH_HEADER = "401 authentication_failed"


def _first_non_empty_line(value: str) -> str:
    for line in value.splitlines():
        line = line.strip()
        if line:
            return line
    return ""


def _iter_text_values(value: Any):
    if isinstance(value, str):
        yield value
        return
    if isinstance(value, dict):
        for key, item in value.items():
            yield from _iter_text_values(key)
            yield from _iter_text_values(item)
        return
    if isinstance(value, (list, tuple, set)):
        for item in value:
            yield from _iter_text_values(item)


def _match_agent_label(value: str) -> str:
    lowered = value.lower()
    if "claude_code" in lowered or "claude-code" in lowered or "claude code" in lowered or "claude" in lowered:
        return "Claude Code"
    if "codex" in lowered:
        return "Codex"
    if "cursor_agent" in lowered or "cursor-agent" in lowered or "cursor" in lowered:
        return "Cursor"
    if "openclaw" in lowered or "open-claw" in lowered or "acpx" in lowered:
        return "OpenClaw"
    return ""


def _agent_label_from_enriched_error(detail: str) -> str:
    for line in detail.splitlines():
        line = line.strip()
        if not line.lower().startswith("ai agent:"):
            continue
        body = line.split(":", 1)[1].strip()
        marker = " auth "
        idx = body.lower().find(marker)
        if idx > 0:
            return body[:idx].strip()
    return ""


def infer_agent_label(*, runner: Any = None, error: str = "", model: Any = None) -> str:
    """Best-effort display name for the local agent behind a run."""

    for value in [model, error]:
        label = _match_agent_label(str(value or ""))
        if label:
            return label

    capabilities = getattr(runner, "capabilities", None)
    for value in _iter_text_values(capabilities):
        label = _match_agent_label(value)
        if label:
            return label

    for attr in ("name", "host_label"):
        label = _match_agent_label(str(getattr(runner, attr, "") or ""))
        if label:
            return label

    return ""


def _runner_location(runner: Any = None) -> str:
    runner_name = str(getattr(runner, "name", "") or "").strip()
    dev_machine = getattr(runner, "dev_machine", None)
    machine_label = ""
    if dev_machine is not None:
        machine_label = str(getattr(dev_machine, "label", "") or getattr(dev_machine, "host_label", "") or "").strip()
    if not machine_label:
        machine_label = str(getattr(runner, "host_label", "") or "").strip()

    if machine_label and runner_name:
        return f'dev machine "{machine_label}" for runner "{runner_name}"'
    if runner_name:
        return f'dev machine for runner "{runner_name}"'
    if machine_label:
        return f'dev machine "{machine_label}"'
    return "dev machine"


def enrich_run_error(error: str, *, runner: Any = None, model: Any = None) -> str:
    """Add operator guidance before persisting known agent failures."""

    detail = (error or "").strip()
    if not detail or _ENRICHED_RAW_ERROR_MARKER in detail:
        return detail

    diagnostic = classify_run_error(detail)
    if diagnostic is None or diagnostic["kind"] != "agent_authentication":
        return detail

    agent_label = infer_agent_label(runner=runner, error=detail, model=model)
    auth_subject = f"{agent_label} auth" if agent_label else "AI agent auth"
    agent_command = agent_label or "the AI agent"
    return (
        f"{_AGENT_AUTH_HEADER}\n"
        f"AI agent: {auth_subject} appears expired or invalid. "
        f"Go to the {_runner_location(runner)} and re-authenticate {agent_command}, "
        "then restart the Pi Dash runner.\n\n"
        f"{_ENRICHED_RAW_ERROR_MARKER}\n"
        f"{detail}"
    )


def classify_run_error(error: str) -> dict[str, Any] | None:
    """Return a compact diagnostic for a stored run error.

    ``source`` answers the operator question "did Pi Dash fail, or did the
    spawned agent fail?" while ``kind`` is a stable-ish category the UI can
    render without brittle text matching.
    """

    detail = (error or "").strip()
    if not detail:
        return None

    lowered = detail.lower()
    summary = _first_non_empty_line(detail)
    agent_label = _agent_label_from_enriched_error(detail)

    if (
        "invalid authentication credentials" in lowered
        or "authentication_failed" in lowered
        or "failed to authenticate" in lowered
    ):
        action_agent = agent_label or "the agent CLI"
        return {
            "source": "agent",
            "source_label": agent_label or "Agent CLI",
            "kind": "agent_authentication",
            "summary": summary,
            "action": f"Re-authenticate {action_agent} on the runner machine, then restart the Pi Dash runner.",
        }

    if "selected model" in lowered and ("may not exist" in lowered or "may not have access" in lowered):
        return {
            "source": "agent",
            "source_label": "Agent CLI",
            "kind": "agent_model_access",
            "summary": summary,
            "action": "Choose a model the agent account can access, then retry the run.",
        }

    if "runner_not_found" in lowered or "run_not_owned_by_runner" in lowered:
        return {
            "source": "pidash_cloud",
            "source_label": "Pi Dash cloud",
            "kind": "runner_registration",
            "summary": summary,
            "action": "Remove or re-add the stale local runner registration.",
        }

    if "daemon shutdown requested" in lowered or "runner revoked" in lowered or "session_evicted" in lowered:
        return {
            "source": "pidash_runner",
            "source_label": "Pi Dash runner",
            "kind": "runner_lifecycle",
            "summary": summary,
            "action": "Check runner service status and restart the runner if it should still accept work.",
        }

    if "agent stalled" in lowered or "without new agent events" in lowered:
        return {
            "source": "agent",
            "source_label": "Agent CLI",
            "kind": "agent_stalled",
            "summary": summary,
            "action": "Inspect the runner machine for a stuck agent process or long-running tool call.",
        }

    return {
        "source": "unknown",
        "source_label": "Unknown",
        "kind": "unknown",
        "summary": summary,
        "action": "",
    }
