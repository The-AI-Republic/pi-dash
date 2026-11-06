// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution-target option shaping (parity/specs/issues.md §17/§21,
// ISS-218; built-in engine design §6, §7, §11). Runner is the only
// executor, so the picker offers no agent rows: each pod offers an "any
// runner" row plus its runners, each with the machine it runs on, a
// status line, an owner name and an assignability answer. Built-in
// engine runners are never offered: an engine takes issue work only as
// a pod member, through the pod row (dedicating an issue to one engine
// belongs to the dedicated-runner design, HA §23). A pod pick pins the
// pod and clears the runner; a runner pick pins the runner inside its
// pod. Both keep the local executor.
import type { ExecutorKind, Pod, PodList, Runner, RunnerList, WorkspaceMemberList } from "@pidash/api-client";

import { isEngineRunner, runnerMachineName } from "../lib/runnerIdentity.js";

/** The one executor issues and projects run on. */
export const LOCAL_EXECUTOR: ExecutorKind = "local_runner";

/** The execution-target property name, on every surface. */
export const EXECUTION_TARGET_LABEL = "Runs on";

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

/** One picker row: a pod squad, or one runner. */
export type ExecutionTarget =
  | { kind: "pod"; podId: string; label: string; isDefault: boolean }
  | {
      kind: "runner";
      runnerId: string;
      podId: string;
      label: string;
      /** The machine the runner runs on, or "" when it reports none. */
      machineName: string;
      statusLine: string;
      /** Owner display name, or "" while unknown. */
      ownerName: string;
      assignable: boolean;
      /** Human reason when not assignable, else "". */
      reason: string;
    };

/**
 * Ordered picker rows: each pod in server order with its squad row
 * followed by that pod's runners in server order. Built-in engine
 * runners are skipped (design §7, §11): servers list them now that the
 * `include_bundled` filter is gone, but an engine is reached only
 * through its pod. Runners read
 * assignable unless the server says otherwise, so plain list rows
 * without eligibility answers stay pickable.
 */
export function buildExecutionTargets(
  pods: PodList,
  runners: RunnerList = [],
  resolveOwnerName: (ownerId: string) => string | null = () => null
): ExecutionTarget[] {
  const targets: ExecutionTarget[] = [];
  for (const pod of pods) {
    targets.push({ kind: "pod", podId: pod.id, label: pod.name, isDefault: pod.is_default });
    for (const runner of runners) {
      if (runner.pod !== pod.id || isEngineRunner(runner)) continue;
      const assignable = runner.assignable ?? true;
      const code = assignable ? "" : (runner.not_assignable_code ?? "");
      targets.push({
        kind: "runner",
        runnerId: runner.id,
        podId: pod.id,
        label: runner.name,
        machineName: runnerMachineName(runner) ?? "",
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

/** One picker pick: a pod squad, or one runner. */
export type ExecutionSelection = { type: "pod"; podId: string } | { type: "runner"; runnerId: string; podId: string };

/** The issue patch a pick writes. */
export interface ExecutionTargetPatch {
  assigned_pod_id: string | null;
  assigned_runner_id: string | null;
  agent_executor: ExecutorKind;
}

/**
 * Map a pick onto the patch: a pod pins the pod plus the local
 * executor and clears any runner; a runner pins the runner inside its
 * pod plus the local executor (the server moves the pod to the
 * runner's own pod either way).
 */
export function executionTargetPatch(selection: ExecutionSelection): ExecutionTargetPatch {
  if (selection.type === "pod")
    return { assigned_pod_id: selection.podId, assigned_runner_id: null, agent_executor: LOCAL_EXECUTOR };
  return {
    assigned_pod_id: selection.podId,
    assigned_runner_id: selection.runnerId,
    agent_executor: LOCAL_EXECUTOR,
  };
}

/** Option-id prefixes, keeping pod and runner ids in one namespace. */
export const POD_OPTION_PREFIX = "pod:";
export const RUNNER_OPTION_PREFIX = "runner:";

/** The option id for a selection, or null when nothing is picked. */
export function executionSelectionId(selection: ExecutionSelection | null): string | null {
  if (selection === null) return null;
  if (selection.type === "pod") return `${POD_OPTION_PREFIX}${selection.podId}`;
  return `${RUNNER_OPTION_PREFIX}${selection.runnerId}`;
}

/**
 * Trigger text for a selection: the pod or runner name. A set
 * pod or runner renders the prompt until the lists load; once loaded,
 * a missing row reads as unknown (every single picker shares the
 * prompt fallback).
 */
export function selectTriggerText(
  selection: ExecutionSelection | null,
  targets: ExecutionTarget[],
  listsLoaded: boolean,
  placeholder: string
): string {
  if (selection === null) return placeholder;
  if (!listsLoaded) return placeholder;
  if (selection.type === "pod") {
    const pod = targets.find((target) => target.kind === "pod" && target.podId === selection.podId);
    return pod?.label ?? "Unknown pod";
  }
  const runner = targets.find((target) => target.kind === "runner" && target.runnerId === selection.runnerId);
  return runner?.label ?? "Unknown runner";
}

/** Pod name behind a runner selection, for the trigger's secondary text. */
export function selectTriggerPodName(selection: ExecutionSelection | null, targets: ExecutionTarget[]): string | null {
  if (selection === null || selection.type !== "runner") return null;
  const runner = targets.find((target) => target.kind === "runner" && target.runnerId === selection.runnerId);
  if (runner?.kind !== "runner") return null;
  const pod = targets.find((target) => target.kind === "pod" && target.podId === runner.podId);
  return pod?.label ?? null;
}

/** The wait note for the selected runner, or null when none applies. */
export function selectRunnerWaitNote(runners: RunnerList | undefined, runnerId: string): string | null {
  const runner = runners?.find((entry) => entry.id === runnerId);
  if (runner === undefined) return null;
  return runnerWaitNote(runner);
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
  const runnerId = id.startsWith(RUNNER_OPTION_PREFIX) ? id.slice(RUNNER_OPTION_PREFIX.length) : id;
  const runner = runners.find((entry) => entry.id === runnerId);
  return { type: "runner", runnerId, podId: runner?.pod ?? "" };
}
