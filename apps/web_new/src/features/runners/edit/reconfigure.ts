// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Apply a runner edit on its machine: one reconfigure request, then a
// bounded status poll until the daemon answers. Transient status-read
// failures never end the poll early. Dependencies are injected so timing
// and cancellation are unit-testable without a real clock.

import { isApiError, type RunnerReconfigure, type RunnerReconfigureStatus } from "@pidash/api-client";

import { RECONFIGURE_POLL_MS, RECONFIGURE_TIMEOUT_MS } from "./model.js";

export type ReconfigureOutcome =
  | { kind: "ok" }
  | { kind: "error"; reason: string | undefined }
  | { kind: "timeout" }
  | { kind: "cancelled" };

export interface ReconfigureDeps {
  send: (runnerId: string, changes: RunnerReconfigure) => Promise<{ request_id: string }>;
  readStatus: (runnerId: string, requestId: string) => Promise<RunnerReconfigureStatus>;
  sleep: (ms: number) => Promise<void>;
  now: () => number;
  isCancelled: () => boolean;
  timeoutMs?: number;
  pollMs?: number;
}

/** The server's refusal reason (`{"error": "<reason>"}`), or the client's code. */
export function refusalReason(error: unknown): string | undefined {
  if (!isApiError(error)) return undefined;
  const body = (error.body ?? {}) as Record<string, unknown>;
  if (typeof body["error"] === "string") return body["error"];
  return typeof error.code === "string" ? error.code : undefined;
}

export async function runReconfigure(
  runnerId: string,
  changes: RunnerReconfigure,
  deps: ReconfigureDeps
): Promise<ReconfigureOutcome> {
  const timeoutMs = deps.timeoutMs ?? RECONFIGURE_TIMEOUT_MS;
  const pollMs = deps.pollMs ?? RECONFIGURE_POLL_MS;
  let requestId: string;
  try {
    requestId = (await deps.send(runnerId, changes)).request_id;
  } catch (error) {
    return { kind: "error", reason: refusalReason(error) };
  }
  const deadline = deps.now() + timeoutMs;
  while (deps.now() < deadline) {
    if (deps.isCancelled()) return { kind: "cancelled" };
    let status: RunnerReconfigureStatus | null = null;
    try {
      status = await deps.readStatus(runnerId, requestId);
    } catch {
      status = null;
    }
    if (status?.status === "ok") return { kind: "ok" };
    if (status?.status === "error") return { kind: "error", reason: status.error };
    await deps.sleep(pollMs);
  }
  return deps.isCancelled() ? { kind: "cancelled" } : { kind: "timeout" };
}
