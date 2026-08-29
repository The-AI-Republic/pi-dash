/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { IProject } from "@pi-dash/types";

/**
 * Execution target = where an issue's agent runs execute.
 *
 * The Cloud Agent and the project's pods are mutually exclusive targets — the
 * Cloud Agent has no runner and no pod queue of its own — so the UI presents
 * them as one choice. This sentinel stands in for the Cloud Agent inside a
 * dropdown whose other values are pod UUIDs.
 */
export const CLOUD_AGENT_VALUE = "cloud_agent";

type TExecutionTargetIssue = {
  agent_executor?: "local_runner" | "cloud_agent" | null;
  assigned_pod_id?: string | null;
};

/** The executor actually in force: the issue override, else the project default. */
export function effectiveExecutor(
  issue: TExecutionTargetIssue | undefined | null,
  project: IProject | undefined | null
): "local_runner" | "cloud_agent" {
  return issue?.agent_executor ?? project?.default_agent_executor ?? "local_runner";
}

/** Dropdown value for an issue: the sentinel when cloud-bound, else its pod. */
export function executionTargetValue(
  issue: TExecutionTargetIssue | undefined | null,
  project: IProject | undefined | null
): string | null | undefined {
  return effectiveExecutor(issue, project) === CLOUD_AGENT_VALUE ? CLOUD_AGENT_VALUE : issue?.assigned_pod_id;
}

/**
 * Patch payload for a dropdown selection. Picking a pod also pins the executor
 * to `local_runner` — otherwise a project whose default is the Cloud Agent
 * would ignore the chosen pod on the next dispatch.
 */
export function executionTargetPatch(value: string): Partial<TExecutionTargetIssue> {
  return value === CLOUD_AGENT_VALUE
    ? { agent_executor: CLOUD_AGENT_VALUE }
    : { agent_executor: "local_runner", assigned_pod_id: value };
}

/** Cloud-agent availability for this project, as the dropdown needs it. */
export function cloudAgentOption(project: IProject | undefined | null) {
  const option = project?.agent_executor_options?.find((o) => o.kind === CLOUD_AGENT_VALUE);
  return { available: option?.available ?? false, reasonCode: option?.reason_code ?? "" };
}
