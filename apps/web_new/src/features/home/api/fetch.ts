// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Logged home fetches (SHELL-023; written from parity/specs/shell.md §2).
// Home list fetches never surface a retry affordance: a failure is logged
// for diagnostics, the error propagates so the query stays failed, and the
// UI keeps its prior state.

export async function logHomeFetch<T>(label: string, task: () => Promise<T>): Promise<T> {
  try {
    return await task();
  } catch (error) {
    console.error(`[web_new] home fetch failed: ${label}`, error);
    throw error;
  }
}
