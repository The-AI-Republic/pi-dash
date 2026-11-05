// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assigned-runner refusals of Run AI (built-in engine design §7, §9).
// Pure: which failed dispatch is a refusal the issue surface answers
// itself, the sentence it shows, and which of the caller's own runners
// "Run on my engine instead" re-pins the issue to. A run on an issue
// assigned to someone else's runner is refused with
// `assigned_runner_not_owner`; a person's run to an offline runner with
// `assigned_runner_offline`. Both answer 409 with `{error, code,
// assigned_runner}`, where `error` is the server's sentence naming the
// machine.
import { isApiError, type Runner } from "@pidash/api-client";

import { agentReasonMessage } from "../../../shared/lib/agentReasons.js";
import { isEngineRunner } from "../../../shared/lib/runnerIdentity.js";

export const ASSIGNED_RUNNER_NOT_OWNER = "assigned_runner_not_owner";
export const ASSIGNED_RUNNER_OFFLINE = "assigned_runner_offline";

export type RunRefusalCode = typeof ASSIGNED_RUNNER_NOT_OWNER | typeof ASSIGNED_RUNNER_OFFLINE;

/** A refusal the issue surface explains with its own actions. */
export interface RunRefusal {
  code: RunRefusalCode;
  /** The server's sentence, else the shared reason sentence. */
  message: string;
  /** The runner the issue is assigned to, when the server names it. */
  assignedRunnerId: string | null;
}

function bodyRecord(body: unknown): Record<string, unknown> | null {
  return body !== null && typeof body === "object" ? (body as Record<string, unknown>) : null;
}

function isRefusalCode(value: unknown): value is RunRefusalCode {
  return value === ASSIGNED_RUNNER_NOT_OWNER || value === ASSIGNED_RUNNER_OFFLINE;
}

/** Read an assigned-runner refusal out of a failed dispatch, if it is one. */
export function parseRunRefusal(error: unknown): RunRefusal | null {
  if (!isApiError(error)) return null;
  const body = bodyRecord(error.body);
  const code = isRefusalCode(body?.["code"]) ? body["code"] : isRefusalCode(error.code) ? error.code : null;
  if (code === null) return null;
  const sentence = body?.["error"];
  const assigned = body?.["assigned_runner"];
  return {
    code,
    message: typeof sentence === "string" && sentence.trim() !== "" ? sentence : agentReasonMessage(code),
    assignedRunnerId: typeof assigned === "string" && assigned !== "" ? assigned : null,
  };
}

/** Runner fields the own-runner choice reads. */
export type OwnRunnerInput = Pick<Runner, "id" | "pod" | "owner" | "revoked_at" | "provisioning"> &
  Partial<Pick<Runner, "status" | "paused_at" | "assignable" | "last_heartbeat_at">>;

/** The caller's live runners in the project: owned and not revoked. */
function ownRunners<T extends OwnRunnerInput>(
  runners: ReadonlyArray<T> | undefined,
  userId: string | null | undefined
): T[] | null {
  if (runners === undefined || userId === null || userId === undefined || userId === "") return null;
  return runners.filter((runner) => runner.owner === userId && runner.revoked_at === null);
}

/**
 * A runner a re-pinned run can start on right now: connected, not
 * paused, and not marked unassignable for this issue. Fields an older
 * server omits do not count against the runner.
 */
export function runnerCanTakeRun(runner: OwnRunnerInput): boolean {
  const online = runner.status === undefined || runner.status === "online" || runner.status === "busy";
  const paused = (runner.paused_at ?? null) !== null;
  return online && !paused && runner.assignable !== false;
}

function heartbeatMs(runner: OwnRunnerInput): number {
  const at = runner.last_heartbeat_at ?? null;
  return at === null ? 0 : Date.parse(at) || 0;
}

/**
 * The caller's runner "Run on my engine instead" pins the issue to: one
 * that can take the run now (see `runnerCanTakeRun`), their built-in
 * engine first, then the most recently seen. Matches the server's
 * `callers_engine_runner`, except that an offline or paused runner is
 * never offered: re-pinning to it only gets the retry refused and moves
 * the issue's pin for nothing. Null when the caller has none that can.
 */
export function selectOwnRunner<T extends OwnRunnerInput>(
  runners: ReadonlyArray<T> | undefined,
  userId: string | null | undefined
): T | null {
  const mine = ownRunners(runners, userId);
  if (mine === null) return null;
  const ready = mine.filter(runnerCanTakeRun);
  const rank = (runner: T): number => (isEngineRunner(runner) ? 0 : 1);
  const sorted = [...ready].sort((a, b) => rank(a) - rank(b) || heartbeatMs(b) - heartbeatMs(a));
  return sorted[0] ?? null;
}

/** True when the caller has runners in the project but none can take the run now. */
export function ownRunnersUnavailable(
  runners: ReadonlyArray<OwnRunnerInput> | undefined,
  userId: string | null | undefined
): boolean {
  const mine = ownRunners(runners, userId);
  return mine !== null && mine.length > 0 && !mine.some(runnerCanTakeRun);
}

/** Action label for the not-owner refusal. */
export const RUN_ON_MY_ENGINE_LABEL = "Run on my engine instead";
/** Shown when the caller has no runner of their own in the project. */
export const NO_OWN_RUNNER_TEXT =
  "You have no runner in this project. Turn on the built-in engine for this project on your computer, or pick a pod.";
/** Shown when the caller's runners in the project are all offline or paused. */
export const OWN_RUNNER_UNAVAILABLE_TEXT =
  "Your runners in this project are offline or paused. Open Pi Dash on your computer or resume the engine, or pick a pod.";
/** Action label for the offline refusal: the issue's other runners or its pod. */
export const PICK_ANOTHER_TARGET_LABEL = "Run on another runner or the pod";
