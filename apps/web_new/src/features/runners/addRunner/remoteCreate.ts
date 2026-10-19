// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud-driven runner creation (RUN-007). One create request, then a bounded
// status poll. Transient status-read failures never end the poll early; a
// close aborts it without writing state. Dependencies are injected so the
// loop's timing and cancellation are unit-testable without a real clock.

import type { CreateRunnerStatus } from "@pidash/api-client";

import { REMOTE_POLL_MS, REMOTE_TIMEOUT_MS, remoteFailureText, remoteStatusError, type RemoteState } from "./model.js";

export interface RemoteCreateDeps {
  createRunner: (machineId: string) => Promise<{ request_id: string }>;
  readStatus: (machineId: string, requestId: string) => Promise<CreateRunnerStatus>;
  /** Resolve after `ms` (or immediately once aborted). */
  sleep: (ms: number) => Promise<void>;
  /** Current epoch milliseconds. */
  now: () => number;
  /** Flip true when the modal closes; the loop checks it after each sleep. */
  isCancelled: () => boolean;
  /** Render a phase. */
  update: (state: RemoteState) => void;
  timeoutMs?: number;
  pollMs?: number;
}

export interface RemoteCreateInput {
  machineId: string;
  machineLabel: string;
}

/**
 * Run one cloud-driven creation to completion. Resolves with the terminal
 * state (ok/error/timeout), or null when the caller cancelled.
 */
export async function runRemoteCreate(input: RemoteCreateInput, deps: RemoteCreateDeps): Promise<RemoteState | null> {
  const { machineId, machineLabel } = input;
  const timeoutMs = deps.timeoutMs ?? REMOTE_TIMEOUT_MS;
  const pollMs = deps.pollMs ?? REMOTE_POLL_MS;
  deps.update({ phase: "creating", machineLabel });

  let requestId: string;
  try {
    const accepted = await deps.createRunner(machineId);
    requestId = accepted.request_id;
  } catch (error) {
    const code = (error as { code?: string } | undefined)?.code;
    const cancelled = code === "aborted";
    if (cancelled) return null;
    const state: RemoteState = { phase: "error", machineLabel, error: remoteFailureText(code) };
    deps.update(state);
    return state;
  }

  const deadline = deps.now() + timeoutMs;
  while (deps.now() < deadline) {
    if (deps.isCancelled()) return null;
    let status: CreateRunnerStatus;
    try {
      status = await deps.readStatus(machineId, requestId);
    } catch {
      await deps.sleep(pollMs);
      continue;
    }
    if (status.status === "ok") {
      const state: RemoteState = {
        phase: "ok",
        machineLabel,
        ...(status.runner_name === undefined ? {} : { runnerName: status.runner_name }),
      };
      deps.update(state);
      return state;
    }
    if (status.status === "error") {
      const state: RemoteState = {
        phase: "error",
        machineLabel,
        ...(status.error === undefined ? {} : { error: remoteStatusError(status.error) }),
      };
      deps.update(state);
      return state;
    }
    await deps.sleep(pollMs);
  }
  if (deps.isCancelled()) return null;
  const state: RemoteState = { phase: "timeout", machineLabel };
  deps.update(state);
  return state;
}
