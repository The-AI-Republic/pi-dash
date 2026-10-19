// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Optimistic state rules for the upkeep tab (parity/specs/profile.md §11,
// PROF-024). Both switches flip the moment they are touched and the
// server's answer is the source of truth afterwards; a failed write
// restores the previous payload and reports.

import type { UpkeepSettings } from "@pidash/api-client";

import { UPKEEP_FAILED_FALLBACK } from "./copy.js";

/** The payload with the master switch replaced; jobs are untouched. */
export function withMasterPaused(settings: UpkeepSettings | undefined, paused: boolean): UpkeepSettings | undefined {
  if (settings === undefined) return undefined;
  return { ...settings, enabled: !paused };
}

/** The payload with one job's switch replaced; every other job stays. */
export function withJobToggled(
  settings: UpkeepSettings | undefined,
  slug: string,
  enabled: boolean
): UpkeepSettings | undefined {
  if (settings === undefined) return undefined;
  return {
    ...settings,
    jobs: settings.jobs.map((job) => (job.slug === slug ? { ...job, enabled } : job)),
  };
}

/** Individual switches stay usable only while the master is on. */
export function jobSwitchDisabled(settings: UpkeepSettings): boolean {
  return !settings.enabled;
}

/** The instance has nothing to switch when the read lists no jobs. */
export function showsEmptyExplanation(settings: UpkeepSettings | undefined): boolean {
  return settings !== undefined && settings.jobs.length === 0;
}

/** The server's error code or its detail when the body carries one. */
export function upkeepFailureMessage(error: unknown): string {
  const body = (error as { body?: unknown } | null)?.body;
  if (body !== null && typeof body === "object") {
    const record = body as Record<string, unknown>;
    for (const key of ["detail", "error"]) {
      const value = record[key];
      if (typeof value === "string" && value.trim() !== "") return value;
    }
  }
  return UPKEEP_FAILED_FALLBACK;
}
