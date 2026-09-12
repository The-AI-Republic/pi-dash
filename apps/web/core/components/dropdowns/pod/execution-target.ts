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

/** Dropdown value for an issue: the sentinel when cloud-bound, else its pod. */
export function executionTargetValue(
  issue: TExecutionTargetIssue | undefined | null,
  project: IProject | undefined | null
): string | null | undefined {
  const executor = effectiveExecutor(issue, project);
  return executor === CLOUD_AGENT_VALUE || executor === MANAGED_AGENT_VALUE ? executor : issue?.assigned_pod_id;
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

/** Cloud-agent availability for this project, as the dropdown needs it. */
export function cloudAgentOption(project: IProject | undefined | null) {
  const option = project?.agent_executor_options?.find((o) => o.kind === CLOUD_AGENT_VALUE);
  return { available: option?.available ?? false, reasonCode: option?.reason_code ?? "" };
}

export function managedAgentOption(project: IProject | undefined | null) {
  const option = project?.agent_executor_options?.find((o) => o.kind === MANAGED_AGENT_VALUE);
  return { available: option?.available ?? false, reasonCode: option?.reason_code ?? "desktop_not_connected" };
}

type TranslateFn = (key: string) => string;

/**
 * Why the managed runner ("Pi Dash Agent") is unavailable, keyed by the
 * `reason_code` the server returned. `null` means render nothing: the desktop
 * fixes `no_managed_runner_for_project` silently the moment it opens the
 * project, so nagging the viewer about it would be noise. Copy mirrors the
 * server-side detail map (`app/serializers/issue.py`) so the picker and a
 * refused pin never explain one situation two different ways. Overlay-friendly:
 * the desktop overlay imports this rather than re-deriving the copy.
 */
export function managedRunnerReasonCopy(reasonCode: string, t: TranslateFn): string | null {
  switch (reasonCode) {
    case "no_managed_runner_for_project":
      return null;
    case "byok_not_supported_on_desktop":
      return t(
        "Pi Dash Agent on desktop uses OpenHub; switch your AI provider to OpenHub to use it here. Pi Dash AI and the Cloud Agent keep using your own key."
      );
    case "managed_runner_disabled":
      return t("Pi Dash Agent is not enabled on this server.");
    case "llm_config_missing":
      return t("Configure your AI provider in Pi Dash AI settings to run on your desktop.");
    case "gateway_scopes_missing":
      return t("Sign in to Pi Dash again to refresh your AI access.");
    case "desktop_not_connected":
    default:
      return t("Open Pi Dash Desktop to run here.");
  }
}

/** Human label for an executor kind, shared by the run list and issue panels. */
export function executorKindLabel(kind: TAgentExecutorKind, t: TranslateFn): string {
  switch (kind) {
    case CLOUD_AGENT_VALUE:
      return t("Pi Dash Cloud Agent");
    case MANAGED_AGENT_VALUE:
      return t("Pi Dash Agent");
    default:
      return t("Local Runner");
  }
}
