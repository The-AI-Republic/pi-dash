// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Waiting-run reads for the issue detail (hybrid assignment, design
// §10.3 and §14.3). Pure: which active run waits for its runner, the
// banner sentence that names the runner and the reason, and the
// compact badge the execution-target trigger carries while waiting.
import type { AgentRunSummary, DispatchWait, Issue } from "@pidash/api-client";

import { formatRunnerSeenAgo } from "../../../../shared/pickers/index.js";
import { formatCountdown } from "../agent/agentStatus.js";

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
export type WaitingReasonInput = Pick<
  DispatchWait,
  "reason" | "runner_last_seen_at" | "runs_ahead" | "pause_reason" | "expires_at" | "pin_kind"
>;

/**
 * Why the run waits, in one clause: offline names the last heartbeat,
 * busy names the runs ahead, paused names the pause reason, other
 * reasons read plainly. Unknown reasons surface as-is per the contract.
 */
export function waitingReasonText(wait: WaitingReasonInput, nowMs: number = Date.now()): string {
  switch (wait.reason) {
    case "runner_offline": {
      const stamp = wait.runner_last_seen_at;
      const ago = stamp === null ? null : formatRunnerSeenAgo(stamp, nowMs);
      return ago === null ? "offline" : `offline, last seen ${ago}`;
    }
    case "runner_paused": {
      const reason = wait.pause_reason ?? null;
      if (reason === "dispatch_paused" || reason === "owner_paused") return "paused by its owner";
      if (reason === "wallet_empty") return "the wallet is empty";
      if (reason === "engine_sign_in_required") return "the engine's sign-in expired";
      if (reason === "gateway_session_revoked") return "the gateway session was revoked";
      if (reason === "profile_unavailable") return "the profile is unavailable";
      if (reason === "engine_disabled") return "the engine is disabled";
      return reason ? `paused: ${reason}` : "paused";
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
  const reason = waitingReasonText(wait, nowMs);
  const base = `Waiting for runner ${runnerName} — ${reason}.`;
  // expires_at: dedicated runs fail after 12h while the runner is unavailable
  // (§11.3); locality pins carry no bound (null).
  const countdown = formatCountdown(wait.expires_at, nowMs);
  if (wait.pin_kind !== "dedicated" || countdown === null) return base;
  if (wait.reason !== "runner_busy" && wait.reason !== "dispatching")
    return `${base} Fails at ${countdown} if it does not reconnect.`;
  return `${base} Fails in ${countdown}.`;
}

/** Compact waiting marker for the execution-target trigger. */
export function waitingBadgeLabel(wait: WaitingReasonInput): string {
  switch (wait.reason) {
    case "runner_offline":
      return "Offline";
    case "runner_paused":
      return "Paused";
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

/** The trigger badge for an issue, or undefined while nothing waits. */
export function selectWaitingBadge(issue: Pick<Issue, "agent_status">): string | undefined {
  const waiting = selectWaitingRun(issue);
  return waiting === null ? undefined : waitingBadgeLabel(waiting.wait);
}
