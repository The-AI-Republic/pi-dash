// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook detail address state (parity/specs/workspace-settings.md §36,
// WSET-036; intended behavior of NEWFRONT-417). The shell resolves the
// webhook id; a missing read is explained as missing/deleted instead of
// spinning forever.

export type WorkspaceWebhookDetailStatus = "loading" | "ready" | "missing" | "error";

export interface WorkspaceWebhookReadState<T> {
  status: "pending" | "success" | "error";
  data?: T;
  error?: unknown;
  fetchStatus?: "fetching" | "paused" | "idle";
}

/** True for a 404-shaped API error (the deleted/unknown webhook case). */
export function isMissingWebhookError(error: unknown): boolean {
  if (error === null || typeof error !== "object") return false;
  const status = (error as { status?: unknown }).status;
  return status === 404;
}

/**
 * Map a webhook detail read onto the page state. Pending reads keep the
 * spinner, a 404 becomes the missing explanation, and other failures
 * become a retryable error. Cached data wins over a transient refetch.
 */
export function resolveWebhookDetailStatus<T>(state: WorkspaceWebhookReadState<T>): WorkspaceWebhookDetailStatus {
  if (state.data !== undefined) return "ready";
  if (state.status === "pending") return "loading";
  if (state.status === "error") return isMissingWebhookError(state.error) ? "missing" : "error";
  return "loading";
}
