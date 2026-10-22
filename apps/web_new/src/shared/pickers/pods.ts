// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution-target option shaping (parity/specs/issues.md §17/§21,
// ISS-218). Agent rows (cloud and managed) come from the project's
// executor options with availability carried as data; each pod offers
// an "any runner" row plus its runners, each with a status line, an
// owner name and an assignability answer. Picks map onto the issue
// execution-target patch: a pod pins the pod plus the local executor,
// a runner pins the runner inside its pod, an agent pins its executor
// and clears the pod. Availability reasons render from the server
// reason codes; desktop capability detection stays server-side.
import type {
  AgentExecutorOption,
  ExecutorKind,
  Pod,
  PodList,
  Runner,
  RunnerList,
  WorkspaceMemberList,
} from "@pidash/api-client";

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

/** One plain-language sentence per runner eligibility code (design §7.1). */
export const RUNNER_NOT_ASSIGNABLE_REASONS: Readonly<Record<string, string>> = {
  runner_not_found: "This runner is no longer available.",
  runner_wrong_project: "This runner belongs to another project.",
  runner_pod_mismatch: "This runner is not in this pod.",
  runner_revoked: "This runner was revoked.",
  runner_desktop_bundled: "Desktop runners can't take dedicated issues.",
  executor_not_local: "Dedicated runners need the local runner.",
  runner_owner_not_on_issue: "Add the runner's owner as an assignee to choose it.",
  active_run_in_progress: "This issue has a run in progress.",
};

/**
 * Render one runner eligibility code. Empty means assignable; unknown
 * codes surface as-is so a new rule never hides behind a blank row.
 */
export function runnerReasonMessage(code: string): string {
  if (code === "") return "";
  return RUNNER_NOT_ASSIGNABLE_REASONS[code] ?? code;
}

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/**
 * Compact age for a runner heartbeat ("2 h ago"). Null when the stamp
 * is missing or unparseable, so the status line degrades to bare
 * "offline" instead of showing a broken age.
 */
export function formatRunnerSeenAgo(stamp: string, nowMs: number = Date.now()): string | null {
  const thenMs = Date.parse(stamp);
  if (Number.isNaN(thenMs)) return null;
  const elapsedMs = Math.max(nowMs - thenMs, 0);
  if (elapsedMs < MINUTE_MS) return "just now";
  const minutes = Math.floor(elapsedMs / MINUTE_MS);
  if (minutes < 60) return `${minutes} m ago`;
  const hours = Math.floor(elapsedMs / HOUR_MS);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.floor(elapsedMs / DAY_MS);
  if (days < 7) return `${days} d ago`;
  const weeks = Math.floor(days / 7);
  if (weeks < 5) return `${weeks} w ago`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months} mo ago`;
  return `${Math.floor(months / 12)} y ago`;
}

/** Runner fields the status line reads. */
export type RunnerStatusInput = Pick<
  Runner,
  "status" | "enrolled_at" | "is_busy" | "queue_depth" | "last_heartbeat_at"
>;

/**
 * One status line per runner (design §10.1): online reads idle unless a
 * queue is waiting, busy names the queue depth, offline names the last
 * heartbeat. A never-enrolled runner reads offline, as on the runners
 * overview; absent queue answers degrade to the bare state.
 */
export function runnerStatusLine(runner: RunnerStatusInput, nowMs: number = Date.now()): string {
  const status = runner.enrolled_at === null ? "offline" : runner.status;
  if (status === "revoked") return "revoked";
  const busy = status === "busy" || runner.is_busy === true;
  const depth = typeof runner.queue_depth === "number" ? runner.queue_depth : null;
  if (status === "offline") {
    const stamp = runner.last_heartbeat_at;
    const ago = stamp === null ? null : formatRunnerSeenAgo(stamp, nowMs);
    return ago === null ? "offline" : `offline · last seen ${ago}`;
  }
  if (busy) return depth !== null && depth > 0 ? `busy · ${depth} waiting` : "busy";
  return depth !== null && depth > 0 ? `online · ${depth} waiting` : "online · idle";
}

/** Runner fields the wait note reads. */
export type RunnerWaitInput = Pick<Runner, "name" | "status" | "is_busy" | "queue_depth">;

/**
 * The inline note a busy or queued runner carries (design §10.1): it
 * warns, never blocks the save. Null for a free runner.
 */
export function runnerWaitNote(runner: RunnerWaitInput): string | null {
  const busy = runner.status === "busy" || runner.is_busy === true;
  const depth = typeof runner.queue_depth === "number" ? runner.queue_depth : 0;
  if (!busy && depth <= 0) return null;
  if (depth > 0) {
    const others = depth === 1 ? "1 other" : `${depth} others`;
    return `${runner.name} runs one issue at a time. This issue will wait behind ${others}.`;
  }
  return `${runner.name} is busy. This issue will wait until it is free.`;
}

/**
 * Display name for a runner owner id from the workspace directory, or
 * null while the directory is missing or holds no such member.
 */
export function selectRunnerOwnerName(directory: WorkspaceMemberList | undefined, ownerId: string): string | null {
  const entry = directory?.find((row) => row.member.id === ownerId);
  return entry?.member.display_name ?? null;
}

/** The pod row's option text: the squad choice, as opposed to one runner. */
export function podOptionLabel(podName: string): string {
  return `Any runner in ${podName}`;
}

/** One picker row: an agent executor, a pod squad, or one runner. */
export type ExecutionTarget =
  | { kind: "agent"; executor: AgentRowKind; label: string; available: boolean; reason: string }
  | { kind: "pod"; podId: string; label: string; isDefault: boolean }
  | {
      kind: "runner";
      runnerId: string;
      podId: string;
      label: string;
      statusLine: string;
      /** Owner display name, or "" while unknown. */
      ownerName: string;
      assignable: boolean;
      /** Human reason when not assignable, else "". */
      reason: string;
    };

/**
 * Ordered picker rows: the offered agent rows first (cloud, then
 * managed — an option absent from the project config renders no
 * row), then each pod in server order with its squad row followed by
 * that pod's runners in server order. Runners read assignable unless
 * the server says otherwise, so plain list rows without eligibility
 * answers stay pickable.
 */
export function buildExecutionTargets(
  pods: PodList,
  options: AgentExecutorOption[],
  runners: RunnerList = [],
  resolveOwnerName: (ownerId: string) => string | null = () => null
): ExecutionTarget[] {
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
    for (const runner of runners) {
      if (runner.pod !== pod.id) continue;
      const assignable = runner.assignable ?? true;
      const code = assignable ? "" : (runner.not_assignable_code ?? "");
      targets.push({
        kind: "runner",
        runnerId: runner.id,
        podId: pod.id,
        label: runner.name,
        statusLine: runnerStatusLine(runner),
        ownerName: resolveOwnerName(runner.owner) ?? "",
        assignable,
        reason: runnerReasonMessage(code),
      });
    }
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

/** One picker pick: a pod squad, one runner, or an agent executor. */
export type ExecutionSelection =
  | { type: "pod"; podId: string }
  | { type: "runner"; runnerId: string; podId: string }
  | { type: "agent"; executor: ExecutorKind };

/** The issue patch a pick writes. */
export interface ExecutionTargetPatch {
  assigned_pod_id: string | null;
  assigned_runner_id: string | null;
  agent_executor: ExecutorKind;
}

/**
 * Map a pick onto the patch: a pod pins the pod plus the local
 * executor and clears any runner; a runner pins the runner inside its
 * pod plus the local executor; an agent pins its executor and clears
 * the pod and the runner, since agent runs execute off-pod.
 */
export function executionTargetPatch(selection: ExecutionSelection): ExecutionTargetPatch {
  if (selection.type === "pod")
    return { assigned_pod_id: selection.podId, assigned_runner_id: null, agent_executor: LOCAL_EXECUTOR };
  if (selection.type === "runner")
    return {
      assigned_pod_id: selection.podId,
      assigned_runner_id: selection.runnerId,
      agent_executor: LOCAL_EXECUTOR,
    };
  return { assigned_pod_id: null, assigned_runner_id: null, agent_executor: selection.executor };
}

/** Option-id prefixes, keeping pod, runner and agent ids in one namespace. */
export const POD_OPTION_PREFIX = "pod:";
export const RUNNER_OPTION_PREFIX = "runner:";
export const AGENT_OPTION_PREFIX = "agent:";

/** The option id for a selection, or null when nothing is picked. */
export function executionSelectionId(selection: ExecutionSelection | null): string | null {
  if (selection === null) return null;
  if (selection.type === "pod") return `${POD_OPTION_PREFIX}${selection.podId}`;
  if (selection.type === "runner") return `${RUNNER_OPTION_PREFIX}${selection.runnerId}`;
  return `${AGENT_OPTION_PREFIX}${selection.executor}`;
}

/**
 * Read a selection back from an option id. Ids only ever come from
 * rendered options, so a runner id always resolves against the loaded
 * list; a stale id keeps the runner with an empty pod rather than
 * throwing, and the save then fails visibly at the API.
 */
export function parseExecutionSelectionId(
  id: string,
  runners: ReadonlyArray<Pick<Runner, "id" | "pod">> = []
): ExecutionSelection {
  if (id.startsWith(POD_OPTION_PREFIX)) return { type: "pod", podId: id.slice(POD_OPTION_PREFIX.length) };
  if (id.startsWith(RUNNER_OPTION_PREFIX)) {
    const runnerId = id.slice(RUNNER_OPTION_PREFIX.length);
    const runner = runners.find((entry) => entry.id === runnerId);
    return { type: "runner", runnerId, podId: runner?.pod ?? "" };
  }
  return { type: "agent", executor: id.slice(AGENT_OPTION_PREFIX.length) as ExecutorKind };
}
