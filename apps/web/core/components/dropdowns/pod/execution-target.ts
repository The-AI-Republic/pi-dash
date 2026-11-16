/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { IProject, IRunner, TAgentExecutorKind } from "@pi-dash/types";

/**
 * Execution target = where an issue's agent runs execute.
 *
 * Two modes (§14.4, §23.6): pod-assigned ("Any runner in <pod>") or
 * dedicated to one runner inside a pod. The dropdown value is a tagged
 * value `pod:<id>` or `runner:<id>`. Engines are listed as
 * "Built-in engine · <machine>" and there are no executor choices —
 * `local_runner` is the only executor since #2903.
 */
export const POD_PREFIX = "pod:";
export const RUNNER_PREFIX = "runner:";

export const CLOUD_AGENT_VALUE = "cloud_agent";
export const MANAGED_AGENT_VALUE = "managed_runner";

export type ExecutionSelection = { type: "pod"; podId: string } | { type: "runner"; runnerId: string; podId: string };

type TExecutionTargetIssue = {
  agent_executor?: TAgentExecutorKind | null;
  assigned_pod_id?: string | null;
  assigned_runner_id?: string | null;
};

export function parseExecutionSelectionId(
  value: string | null | undefined,
  runners: Pick<IRunner, "id" | "pod">[] = []
): ExecutionSelection | null {
  if (!value) return null;
  if (value.startsWith(RUNNER_PREFIX)) {
    const runnerId = value.slice(RUNNER_PREFIX.length);
    const runner = runners.find((r) => r.id === runnerId);
    return { type: "runner", runnerId, podId: runner?.pod ?? "" };
  }
  if (value.startsWith(POD_PREFIX)) {
    return { type: "pod", podId: value.slice(POD_PREFIX.length) };
  }
  // Legacy bare pod id (before tagged values) — treat as pod selection.
  return { type: "pod", podId: value };
}

export function executionSelectionId(selection: ExecutionSelection | null): string | null {
  if (!selection) return null;
  return selection.type === "pod" ? `${POD_PREFIX}${selection.podId}` : `${RUNNER_PREFIX}${selection.runnerId}`;
}

/** The executor actually in force: the issue override, else the project default. */
export function effectiveExecutor(
  issue: TExecutionTargetIssue | undefined | null,
  project: IProject | undefined | null
): TAgentExecutorKind {
  return issue?.agent_executor ?? project?.default_agent_executor ?? "local_runner";
}

/**
 * An executor entry the project still offers for choice. Runner is the only
 * executor now (built-in engine design §6): the server keeps returning the
 * Cloud Agent and built-in engine entries for older clients, marked
 * `deprecated: true`, and this build hides them.
 */
function offeredOption(project: IProject | undefined | null, kind: TAgentExecutorKind) {
  const option = project?.agent_executor_options?.find((o) => o.kind === kind);
  return option === undefined || option.deprecated === true ? undefined : option;
}

/**
 * Selection for an issue: runner when assigned_runner_id is set (pod is
 * the runner's pod), else pod. Legacy executor sentinels no longer appear
 * since §23.6 removed executor choices.
 */
export function executionTargetSelection(
  issue: TExecutionTargetIssue | undefined | null,
  _project: IProject | undefined | null
): ExecutionSelection | null {
  const runnerId = issue?.assigned_runner_id ?? null;
  const podId = issue?.assigned_pod_id ?? null;
  if (runnerId) {
    // Pod id comes from the caller; for legacy callers with only runner id
    // we still return podId as known (may be empty until resolved).
    return { type: "runner", runnerId, podId: podId ?? "" };
  }
  if (podId) return { type: "pod", podId };
  return null;
}

/**
 * Dropdown value for an issue: tagged `pod:<id>` or `runner:<id>`.
 * Falls back to legacy bare pod id handling for older callers.
 */
export function executionTargetValue(
  issue: TExecutionTargetIssue | undefined | null,
  project: IProject | undefined | null
): string | null | undefined {
  const sel = executionTargetSelection(issue, project);
  return executionSelectionId(sel);
}

/**
 * Patch payload for a selection. Pod clears the runner; runner pins the
 * runner inside its pod. Both keep the local executor which the server
 * normalizes independently (§8).
 */
export function executionTargetPatchForSelection(selection: ExecutionSelection): {
  agent_executor: "local_runner";
  assigned_pod_id: string;
  assigned_runner_id: string | null;
} {
  if (selection.type === "pod") {
    return { agent_executor: "local_runner", assigned_pod_id: selection.podId, assigned_runner_id: null };
  }
  return {
    agent_executor: "local_runner",
    assigned_pod_id: selection.podId,
    assigned_runner_id: selection.runnerId,
  };
}

/**
 * Patch payload for a dropdown selection (tagged or legacy bare value).
 * Kept for callers that pass the raw dropdown string.
 */
export function executionTargetPatch(value: string): Partial<TExecutionTargetIssue> {
  const sel = parseExecutionSelectionId(value);
  if (!sel) return { agent_executor: "local_runner" };
  if (sel.type === "pod") {
    return { agent_executor: "local_runner", assigned_pod_id: sel.podId, assigned_runner_id: null };
  }
  return { agent_executor: "local_runner", assigned_pod_id: sel.podId || undefined, assigned_runner_id: sel.runnerId };
}

/** Cloud-agent row for this project, or undefined when it is not offered. */
export function cloudAgentOption(project: IProject | undefined | null) {
  const option = offeredOption(project, CLOUD_AGENT_VALUE);
  return option === undefined ? undefined : { available: option.available, reasonCode: option.reason_code };
}

/** Built-in agent row for this project, or undefined when it is not offered. */
export function managedAgentOption(project: IProject | undefined | null) {
  const option = offeredOption(project, MANAGED_AGENT_VALUE);
  return option === undefined
    ? undefined
    : { available: option.available, reasonCode: option.reason_code || "desktop_not_connected" };
}

/** Display label for a runner (§23.6): engines read "Built-in engine · <machine>". */
export function runnerDisplayLabel(
  runner: Pick<IRunner, "name" | "provisioning" | "dev_machine_detail" | "machine_display_name">
): string {
  if (runner.provisioning === "desktop_bundled") {
    const machine = runner.machine_display_name ?? runner.dev_machine_detail?.label ?? runner.name;
    return `Built-in engine · ${machine}`;
  }
  return runner.name;
}

/** One status line per runner (§10.1). */
export function runnerStatusLine(
  runner: Pick<IRunner, "status" | "is_busy" | "queue_depth" | "last_heartbeat_at">
): string {
  if (runner.status === "revoked") return "revoked";
  if (runner.status === "offline") {
    const stamp = runner.last_heartbeat_at;
    if (!stamp) return "offline";
    const ms = Date.parse(stamp);
    if (Number.isNaN(ms)) return "offline";
    const elapsed = Date.now() - ms;
    if (elapsed < 60_000) return "offline · just now";
    if (elapsed < 3_600_000) return `offline · ${Math.floor(elapsed / 60_000)} m ago`;
    if (elapsed < 86_400_000) return `offline · ${Math.floor(elapsed / 3_600_000)} h ago`;
    return `offline · ${Math.floor(elapsed / 86_400_000)} d ago`;
  }
  const busy = runner.status === "busy" || runner.is_busy === true;
  const depth = typeof runner.queue_depth === "number" ? runner.queue_depth : null;
  if (busy) return depth !== null && depth > 0 ? `busy · ${depth} waiting` : "busy";
  return depth !== null && depth > 0 ? `online · ${depth} waiting` : "online · idle";
}

export const RUNNER_NOT_ASSIGNABLE_REASONS: Readonly<Record<string, string>> = {
  runner_not_found: "This runner is no longer available.",
  runner_wrong_project: "This runner belongs to another project.",
  runner_pod_mismatch: "This runner is not in this pod.",
  runner_revoked: "This runner was revoked.",
  runner_owner_not_on_issue: "Add the runner's owner as an assignee to choose it.",
  active_run_in_progress: "This issue has a run in progress.",
};

export function runnerReasonMessage(code: string): string {
  if (!code) return "";
  return RUNNER_NOT_ASSIGNABLE_REASONS[code] ?? code;
}
