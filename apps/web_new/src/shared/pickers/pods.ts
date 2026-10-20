// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution-target option shaping (parity/specs/issues.md §17/§21,
// ISS-218). Agent rows (cloud and managed) come from the project's
// executor options with availability carried as data; pods list with
// the default tagged. Picks map onto the issue execution-target
// patch: a pod pins the pod plus the local executor, an agent pins
// its executor and clears the pod. Availability reasons render from
// the server reason codes; desktop capability detection stays
// server-side.
import type { AgentExecutorOption, ExecutorKind, Pod, PodList } from "@pidash/api-client";

import { agentReasonMessage } from "../lib/agentReasons.js";

export const CLOUD_EXECUTOR: ExecutorKind = "cloud_agent";
export const MANAGED_EXECUTOR: ExecutorKind = "managed_runner";
export const LOCAL_EXECUTOR: ExecutorKind = "local_runner";

/** Agent rows the picker offers; pods cover the local path, so it has no row. */
export const AGENT_ROW_LABELS = {
  cloud_agent: "Pi Dash Cloud Agent",
  managed_runner: "Pi Dash Agent (desktop)",
} as const;
export type AgentRowKind = keyof typeof AGENT_ROW_LABELS;

export function isAgentRowKind(kind: string): kind is AgentRowKind {
  return kind === CLOUD_EXECUTOR || kind === MANAGED_EXECUTOR;
}

/**
 * Render one reason code. The shared agent vocabulary owns the sentences so
 * the picker and the desktop status notice explain a refusal the same way;
 * unknown codes surface as-is per the contract.
 */
export function formatAvailabilityReason(reasonCode: string): string {
  return agentReasonMessage(reasonCode);
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

/** Option-id prefixes, keeping pod and agent ids in one namespace. */
export const POD_OPTION_PREFIX = "pod:";
export const AGENT_OPTION_PREFIX = "agent:";

/** The option id for a selection, or null when nothing is picked. */
export function executionSelectionId(selection: ExecutionSelection | null): string | null {
  if (selection === null) return null;
  return selection.type === "pod"
    ? `${POD_OPTION_PREFIX}${selection.podId}`
    : `${AGENT_OPTION_PREFIX}${selection.executor}`;
}

/**
 * Read a selection back from an option id. Ids only ever come from
 * rendered options, so a non-pod id is always an agent pick.
 */
export function parseExecutionSelectionId(id: string): ExecutionSelection {
  if (id.startsWith(POD_OPTION_PREFIX)) return { type: "pod", podId: id.slice(POD_OPTION_PREFIX.length) };
  return { type: "agent", executor: id.slice(AGENT_OPTION_PREFIX.length) as ExecutorKind };
}
