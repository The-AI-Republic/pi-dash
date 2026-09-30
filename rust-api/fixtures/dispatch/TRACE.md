# TRACE — D-11 cloud_agent + managed_runner dispatch fixtures

Every line maps one fixture file to the exact Python source lines it records.
Base: `apps/api/pi_dash/`. EE seams: `apps/api/pi_dash/ee/cloud_agent/`.
Ported-from: `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

Method per fixture is recorded inside the file under `method`. "Executed
offline" means the project venv (`pi-dash/apps/api/.venv`, system
site-packages + pydantic 2.13.5), `settings.configure()` + sqlite
`:memory:` for `on_commit`, no DB rows touched; generators live outside the
repo (`/tmp/gen*.py`, not committed). pydantic_ai is NOT installed, so the
`_is_usage_limit` type branch records `False` (the `ImportError` path).
ORM shapes for claim/dispatch/sweep paths are compiler-form sketches with
shape-derived rows (no live Postgres on this runner) — same precedent as the
D-06 assistant fixtures. `runner/models.py` is READ-ONLY for this domain
(owned by D-13…D-15); only read shapes are recorded.

## FX-DISP-01 types

- `fx-disp-01-types.golden.json` — `cloud_agent/errors.py:7-44`
  (secret patterns, `sanitize_error`, `_is_usage_limit`, `classify_error`);
  `cloud_agent/output.py:1-13` (`CloudAgentOutput`); `managed_runner/errors.py:15-45`
  (`ManagedRunnerReason`, `ManagedRunnerUnavailable`); `core/agent_execution.py:7-31`
  (`AgentExecutorKind`, `MACHINE_EXECUTORS`, `get_default_agent_executor`).

## FX-DISP-02 models (reads only)

- `fx-disp-02-models.golden.json` — `runner/models.py:207-235` (`AgentRunStatus`,
  13 values); `runner/models.py:237-250` (`AgentRunTrigger`, 6 values);
  `runner/models.py:872+` (`AgentRun`, dispatch-touched subset);
  `runner/models.py:1162-1185` (`AgentRunEvent`); `runner/models.py:1187-1230`
  (`AgentRunToolCall`).

## FX-DISP-03 executor policy

- `fx-disp-03-policy.golden.json` — `core/agent_execution.py:57-128`
  (`effective_executor_for_issue`, `user_has_llm_config`, `agent_executor_options`);
  `cloud_agent/api.py:5-8` (`CloudAgentUnavailableAPI` 409 body);
  `cloud_agent/policy.py:57-196` (`resolve_executor_kind`, `github_available_for_project`,
  `build_tool_plan`, `resolve_current_tool_names`);
  `managed_runner/policy.py:76-96` (availability gate order).

## FX-DISP-04 admission / permissions / checks

- `fx-disp-04-admission-checks.golden.json` — `cloud_agent/admission.py:15-76`
  (error, `_consume`, `_take`, `enforce_creation_rate`);
  `managed_runner/permissions.py:12-28` (`IsDesktopSession`);
  `cloud_agent/checks.py:8-74` (E001/E002/E005/E007/E008);
  `managed_runner/checks.py:20-50` (E001..E004).

## FX-DISP-05 tools

- `fx-disp-05-tools.golden.json` — `cloud_agent/policy.py:11-42` (catalogs);
  `cloud_agent/github_mcp.py:10-77` (`GITHUB_TOOL_NAMES`, MCP adapter);
  `cloud_agent/tools.py:24-554` (denied, canonical/fingerprint/bounded, scope,
  audit, issue data, tool catalog, guards, github context);
  `cloud_agent/model.py:11-22` + `ee/cloud_agent/model_provider.py:1-7` (model routing);
  `ee/cloud_agent/toolsets.py:23-50` (CE seam defaults).

## FX-DISP-06 dispatch

- `fx-disp-06-dispatch.golden.json` — `cloud_agent/creation.py:11-177`
  (`execution_fields`, `_managed_execution_fields`, `dispatch_after_commit`,
  `lock_cloud_creation_capacity`); `cloud_agent/dispatch.py:20-70`
  (`dispatch_waiting`, `_publish`, `dispatch_agent_run`); `cloud_agent/events.py:9-25`
  (`append`).

## FX-DISP-07 execute

- `fx-disp-07-execute.golden.json` — `cloud_agent/tasks.py:31-260` (`_claim`,
  `_fail`, `run_cloud_agent` outcomes, `scan_queued_runs`, `sweep_stale_runs`);
  `managed_runner/tasks.py:31-61` (`expire_waiting_runs`); `cloud_agent/runtime.py:18-88`
  (`execute`, `_usage_report`); `celery.py:156-186` (3 owned beat entries);
  `settings/common.py:531-590` (interval/limit defaults).

## Ported quirks (translate as-is)

- `_cloud_admission_error` pseudo-key is not an `AgentRun` column (`creation.py:84-85`).
- `events.append` silently truncates `kind` to 64 chars (`events.py:24`).
- `sanitize_error` truncates to 16000 chars before classification (`errors.py:20,35`).
- Per-minute admission may overshoot under burst by design (`admission.py:40-42`).
