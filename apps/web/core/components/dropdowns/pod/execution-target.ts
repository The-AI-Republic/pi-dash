/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { IProject, TAgentExecutorKind } from "@pi-dash/types";

/**
 * Execution target = where an issue's agent runs execute.
 *
 * The Cloud Agent and the project's pods are mutually exclusive targets — the
 * Cloud Agent has no runner and no pod queue of its own — so the UI presents
 * them as one choice. This sentinel stands in for the Cloud Agent inside a
 * dropdown whose other values are pod UUIDs.
 */
export const CLOUD_AGENT_VALUE = "cloud_agent";
export const MANAGED_AGENT_VALUE = "managed_runner";

type TExecutionTargetIssue = {
  agent_executor?: TAgentExecutorKind | null;
  assigned_pod_id?: string | null;
};

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
 * Dropdown value for an issue: the sentinel when it is bound to an executor
 * still offered for choice, else its pod. A deprecated executor reads as the
 * issue's pod, which is where the server now sends its runs.
 */
export function executionTargetValue(
  issue: TExecutionTargetIssue | undefined | null,
  project: IProject | undefined | null
): string | null | undefined {
  const executor = effectiveExecutor(issue, project);
  const isAgent = executor === CLOUD_AGENT_VALUE || executor === MANAGED_AGENT_VALUE;
  return isAgent && offeredOption(project, executor) !== undefined ? executor : issue?.assigned_pod_id;
}

/**
 * Patch payload for a dropdown selection. Picking a pod also pins the executor
 * to `local_runner` — otherwise a project whose default is the Cloud Agent
 * would ignore the chosen pod on the next dispatch.
 */
export function executionTargetPatch(value: string): Partial<TExecutionTargetIssue> {
  return value === CLOUD_AGENT_VALUE || value === MANAGED_AGENT_VALUE
    ? { agent_executor: value }
    : { agent_executor: "local_runner", assigned_pod_id: value };
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
