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
export type OwnRunnerInput = Pick<Runner, "id" | "pod" | "owner" | "revoked_at" | "provisioning">;

/**
 * The caller's runner "Run on my engine instead" pins the issue to: their
 * built-in engine in the project first, else any runner of theirs there.
 * Revoked runners never count. Null when the caller has none.
 */
export function selectOwnRunner<T extends OwnRunnerInput>(
  runners: ReadonlyArray<T> | undefined,
  userId: string | null | undefined
): T | null {
  if (runners === undefined || userId === null || userId === undefined || userId === "") return null;
  const mine = runners.filter((runner) => runner.owner === userId && runner.revoked_at === null);
  return mine.find((runner) => isEngineRunner(runner)) ?? mine[0] ?? null;
}

/** Action label for the not-owner refusal. */
export const RUN_ON_MY_ENGINE_LABEL = "Run on my engine instead";
/** Shown when the caller has no runner of their own in the project. */
export const NO_OWN_RUNNER_TEXT =
  "You have no runner in this project. Turn on the built-in engine for this project on your computer, or pick a pod.";
/** Action label for the offline refusal: the issue's other runners or its pod. */
export const PICK_ANOTHER_TARGET_LABEL = "Run on another runner or the pod";
