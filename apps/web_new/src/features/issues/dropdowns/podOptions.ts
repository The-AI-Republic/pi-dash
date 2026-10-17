// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution-target reads and shaping behind the pod picker
// (parity/specs/issues.md §17/§21, ISS-218). Agent rows (cloud and
// managed) come from the project's executor options with
// availability carried as data; pods list with the default tagged.
// Picks map onto the issue execution-target patch: a pod pins the
// pod plus the local executor, an agent pins its executor and
// clears the pod. Availability reasons render from the server
// reason codes; desktop capability detection stays server-side.

import { queryOptions, useQuery } from "@tanstack/react-query";
import {
  getProject,
  listPods,
  type AgentExecutorOption,
  type ApiClient,
  type ExecutorKind,
  type Pod,
  type PodList,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";

export const CLOUD_EXECUTOR: ExecutorKind = "cloud_agent";
export const MANAGED_EXECUTOR: ExecutorKind = "managed_runner";
export const LOCAL_EXECUTOR: ExecutorKind = "local_runner";

/** Agent rows the picker offers; pods cover the local path, so it has no row. */
export const AGENT_ROW_LABELS = {
  cloud_agent: "Pi Dash Cloud Agent",
  managed_runner: "Pi Dash Agent (desktop)",
} as const;
export type AgentRowKind = keyof typeof AGENT_ROW_LABELS;

function isAgentRowKind(kind: string): kind is AgentRowKind {
  return kind === CLOUD_EXECUTOR || kind === MANAGED_EXECUTOR;
}

/** Human sentences for the server availability reason codes. */
const AVAILABILITY_REASONS: Record<string, string> = {
  cloud_agent_unavailable: "The Cloud Agent is disabled on this instance.",
  llm_config_missing: "No AI provider configured for your account.",
  no_local_runner: "No online runner on this project's pods.",
  managed_runner_disabled: "The desktop agent is disabled on this instance.",
  desktop_not_connected: "Your desktop app is not connected.",
  gateway_scopes_missing: "Your desktop session needs reconnecting.",
  byok_not_supported_on_desktop: "Your BYOK provider isn't supported on the desktop agent.",
  no_managed_runner_for_project: "No desktop runner enrolled for this project.",
};

/** Render one reason code; unknown codes surface as-is per the contract. */
export function formatAvailabilityReason(reasonCode: string): string {
  return AVAILABILITY_REASONS[reasonCode] ?? reasonCode;
}

/** One picker row: an agent executor or a pod. */
export type ExecutionTarget =
  | { kind: "agent"; executor: AgentRowKind; label: string; available: boolean; reason: string }
  | { kind: "pod"; podId: string; label: string; isDefault: boolean };

/**
 * Ordered picker rows: the offered agent rows first (cloud, then
 * managed — an option absent from the project config renders no
 * row), then the pods in server order (default first).
 */
export function buildExecutionTargets(pods: PodList, options: AgentExecutorOption[]): ExecutionTarget[] {
  const targets: ExecutionTarget[] = [];
  for (const option of options) {
    if (!isAgentRowKind(option.kind)) continue;
    targets.push({
      kind: "agent",
      executor: option.kind,
      label: AGENT_ROW_LABELS[option.kind],
      available: option.available,
      reason: option.available ? "" : formatAvailabilityReason(option.reason_code),
    });
  }
  for (const pod of pods) {
    targets.push({ kind: "pod", podId: pod.id, label: pod.name, isDefault: pod.is_default });
  }
  return targets;
}

/** The project's default pod, if one exists. */
export function selectDefaultPod(pods: PodList): Pod | null {
  return pods.find((pod) => pod.is_default) ?? null;
}

/** The row label: Runs on once a cloud agent is offered, else Pod. */
export function executionTargetLabel(options: AgentExecutorOption[]): "Runs on" | "Pod" {
  return options.some((option) => option.kind === CLOUD_EXECUTOR) ? "Runs on" : "Pod";
}

/** One picker pick: a pod or an agent executor. */
export type ExecutionSelection = { type: "pod"; podId: string } | { type: "agent"; executor: ExecutorKind };

/** The issue patch a pick writes. */
export interface ExecutionTargetPatch {
  assigned_pod_id: string | null;
  agent_executor: ExecutorKind;
}

/**
 * Map a pick onto the patch: a pod pins the pod plus the local
 * executor; an agent pins its executor and clears the pod, since
 * agent runs execute off-pod.
 */
export function executionTargetPatch(selection: ExecutionSelection): ExecutionTargetPatch {
  if (selection.type === "pod") return { assigned_pod_id: selection.podId, agent_executor: LOCAL_EXECUTOR };
  return { assigned_pod_id: null, agent_executor: selection.executor };
}

export function projectPodsQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: issueKeys.pods(workspaceSlug, projectId),
    queryFn: () => listPods(client, { projectId }),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export function projectExecutorQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: issueKeys.project(workspaceSlug, projectId),
    queryFn: () => getProject(client, workspaceSlug, projectId),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export interface ExecutionTargetsResult {
  pods: PodList | undefined;
  options: AgentExecutorOption[] | undefined;
  isPending: boolean;
  error: Error | null;
}

/** Pods plus executor options behind the picker. Either read may lag. */
export function useExecutionTargets(workspaceSlug: string, projectId: string): ExecutionTargetsResult {
  const podsQuery = useQuery(projectPodsQueryOptions(getAppClient(), workspaceSlug, projectId));
  const projectQuery = useQuery(projectExecutorQueryOptions(getAppClient(), workspaceSlug, projectId));
  return {
    pods: podsQuery.data,
    options: projectQuery.data?.agent_executor_options,
    isPending: podsQuery.isPending || projectQuery.isPending,
    error: podsQuery.error ?? projectQuery.error,
  };
}
