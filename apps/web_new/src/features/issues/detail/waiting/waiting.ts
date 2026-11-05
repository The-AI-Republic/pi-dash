// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Waiting-run reads for the issue detail (hybrid assignment, design
// §10.3 and §14.3). Pure: which active run waits for its runner, the
// banner sentence that names the runner and the reason, and the
// compact badge the execution-target trigger carries while waiting.
import type { AgentRunSummary, DispatchWait, Issue, Runner } from "@pidash/api-client";

import { machineOfflineMessage } from "../../../../shared/lib/agentReasons.js";
import { runnerDisplayName, runnerMachineName, type RunnerIdentity } from "../../../../shared/lib/runnerIdentity.js";
import { formatRunnerSeenAgo } from "../../../../shared/pickers/index.js";

/** A queued run pinned to a runner, with its computed wait. */
export interface WaitingRun {
  run: AgentRunSummary;
  wait: DispatchWait;
  runnerName: string;
}

/**
 * The issue's waiting run, if its active run is queued behind a pin.
 * Anything else — no active run, a started run, an unpinned queue —
 * reads as not waiting.
 */
export function selectWaitingRun(issue: Pick<Issue, "agent_status">): WaitingRun | null {
  const run = issue.agent_status?.active_run ?? null;
  if (run === null || run.status !== "queued") return null;
  const wait = run.dispatch_wait ?? null;
  if (wait === null) return null;
  const runnerName = run.pinned_runner_name ?? wait.runner_name;
  return { run, wait, runnerName };
}

/** Wait fields the reason sentence reads. */
export type WaitingReasonInput = Pick<DispatchWait, "reason" | "runner_last_seen_at" | "runs_ahead">;

/**
 * Why the run waits, in one clause: offline names the last heartbeat,
 * busy names the runs ahead, other reasons read plainly. Unknown
 * reasons surface as-is per the contract.
 */
export function waitingReasonText(wait: WaitingReasonInput, nowMs: number = Date.now()): string {
  switch (wait.reason) {
    case "runner_offline": {
      const stamp = wait.runner_last_seen_at;
      const ago = stamp === null ? null : formatRunnerSeenAgo(stamp, nowMs);
      return ago === null ? "offline" : `offline, last seen ${ago}`;
    }
    case "runner_busy":
      if (wait.runs_ahead <= 0) return "busy";
      return `busy, ${wait.runs_ahead} ${wait.runs_ahead === 1 ? "run" : "runs"} ahead`;
    case "runner_not_permitted":
      return "the runner can't take this issue";
    case "dispatching":
      return "dispatching";
    default:
      return wait.reason;
  }
}

/** The banner sentence: which runner, and why it waits. */
export function waitingBannerText(runnerName: string, wait: WaitingReasonInput, nowMs: number = Date.now()): string {
  return `Waiting for runner ${runnerName} — ${waitingReasonText(wait, nowMs)}.`;
}

/** Compact waiting marker for the execution-target trigger. */
export function waitingBadgeLabel(wait: WaitingReasonInput): string {
  switch (wait.reason) {
    case "runner_offline":
      return "Offline";
    case "runner_busy":
      return wait.runs_ahead > 0 ? `${wait.runs_ahead} ahead` : "Busy";
    case "runner_not_permitted":
      return "Not permitted";
    case "dispatching":
      return "Dispatching";
    default:
      return wait.reason;
  }
}

/**
 * Error code a queued run strictly pinned to an offline runner carries
 * (built-in engine design §9). The value predates the design; it now
 * means "waiting for that runner's computer", whatever the runner.
 */
export const WAITING_FOR_MACHINE_CODE = "desktop_not_connected";

/** Fallback name when the waited-for runner is not in the loaded list. */
export const UNKNOWN_MACHINE_NAME = "this runner";

/** A queued run waiting for its runner's computer to come back. */
export interface MachineWait {
  run: AgentRunSummary;
  /** The runner the run is pinned to, when known. */
  runnerId: string | null;
}

/**
 * The issue's run waiting for an offline computer: the active run is
 * queued with the waiting code. Servers that compute a `dispatch_wait`
 * use the banner above instead, so a run carrying one never reads here.
 */
export function selectMachineWait(issue: Pick<Issue, "agent_status" | "assigned_runner_id">): MachineWait | null {
  const run = issue.agent_status?.active_run ?? null;
  if (run === null || run.status !== "queued") return null;
  if ((run.dispatch_wait ?? null) !== null) return null;
  if (run.error_code !== WAITING_FOR_MACHINE_CODE) return null;
  const runnerId = issue.assigned_runner_id ?? run.pinned_runner ?? null;
  return { run, runnerId };
}

/** The runner fields the machine name is derived from. */
type MachineRunner = Pick<Runner, "id"> & RunnerIdentity;

/**
 * The computer a machine wait is for: the pinned runner's machine,
 * else the runner's own name, else a neutral fallback while the
 * runner list is missing it.
 */
export function machineWaitName(wait: MachineWait, runners: ReadonlyArray<MachineRunner> | undefined): string {
  const runner = wait.runnerId === null ? undefined : runners?.find((row) => row.id === wait.runnerId);
  if (runner === undefined) return UNKNOWN_MACHINE_NAME;
  return runnerMachineName(runner) ?? runnerDisplayName(runner);
}

/** "Waiting for <machine>": the label the issue panel shows (design §9). */
export function machineWaitLabel(machineName: string): string {
  return `Waiting for ${machineName}`;
}

/** The banner sentence: what the run waits for, and why. */
export function machineWaitBannerText(machineName: string): string {
  return machineName === UNKNOWN_MACHINE_NAME
    ? `${machineWaitLabel(machineName)} — its computer is offline. The run starts when it reconnects.`
    : `${machineWaitLabel(machineName)} — ${machineOfflineMessage(machineName)}. The run starts when it reconnects.`;
}

/** Compact marker the trigger shows while the computer is away. */
export const MACHINE_WAIT_BADGE = "Offline";

/** The trigger badge for an issue, or undefined while nothing waits. */
export function selectWaitingBadge(issue: Pick<Issue, "agent_status" | "assigned_runner_id">): string | undefined {
  const waiting = selectWaitingRun(issue);
  if (waiting !== null) return waitingBadgeLabel(waiting.wait);
  return selectMachineWait(issue) === null ? undefined : MACHINE_WAIT_BADGE;
}
