// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop job detail pure helpers (ADM-058/059/060/061, from
// parity/specs/admin.md §7.8–7.11). The detail page reads its pinned
// copy, skip-reason options, and target-cell formatting from here.
// Strings marked oracle-pinned are asserted verbatim by
// e2e/parity/admin/loop.spec.ts and must stay exact.
import { LOOP_SKIP_REASONS, type LoopTargetLastRun } from "@pidash/api-client";

/** Placeholder header while the job loads (oracle-pinned: loop.spec.ts ADM-061). */
export const LOOP_JOB_DETAIL_SKELETON_TITLE = "Loop job";

/** Skeleton bar heights, top to bottom (oracle-pinned: loop.spec.ts ADM-061). */
export const LOOP_JOB_DETAIL_SKELETON_HEIGHTS: readonly string[] = ["48px", "120px"];

/** Header description when the job carries no user-facing description. */
export const LOOP_JOB_DESCRIPTION_FALLBACK = "Loop job detail";

/**
 * Targets table column headers, left to right (oracle-pinned:
 * loop.spec.ts ADM-058).
 */
export const LOOP_TARGETS_HEADERS: readonly string[] = [
  "Workspace",
  "User",
  "Next run",
  "Last run",
  "Tokens",
  "Last skip",
];

/** Placeholder row when the targets read resolves empty (oracle-pinned: loop.spec.ts ADM-058). */
export const LOOP_TARGETS_EMPTY_TEXT = "No targets yet.";

/** Cell text for a missing timestamp, status, or reason (spec §7.8). */
export const LOOP_TARGET_MISSING_TEXT = "—";

/** Skip-reason filter label. */
export const LOOP_SKIP_FILTER_LABEL = "Skip reason";

/** Skip-reason option label for the blank (unfiltered) value (spec §7.9). */
export const LOOP_SKIP_FILTER_ALL_LABEL = "all";

/**
 * Skip-reason filter option values, blank first for all (oracle-pinned:
 * loop.spec.ts ADM-059 reads the values). The fixed set rides the
 * api-client contract; the blank is the page's unfiltered state.
 */
export const LOOP_SKIP_FILTER_OPTIONS: readonly string[] = ["", ...LOOP_SKIP_REASONS];

/** Delete-success notice (oracle-pinned: loop.spec.ts ADM-060). */
export const LOOP_JOB_DELETED_TITLE = "Deleted";
export const LOOP_JOB_DELETED_MESSAGE = "Loop job deleted.";

/**
 * Delete-failure notice (oracle-pinned: loop.spec.ts ADM-060). Fixed
 * copy, not the server message: the scenario fails the write with a
 * probe error and still expects this text.
 */
export const LOOP_JOB_DELETE_FAILED_TITLE = "Delete failed";
export const LOOP_JOB_DELETE_FAILED_MESSAGE = "Could not delete the job.";

/** List path the detail returns to after a successful delete (oracle-pinned: loop.spec.ts ADM-060). */
export const LOOP_JOBS_LIST_PATH = "/god-mode/loop/";

/**
 * Stat tile labels (oracle-pinned: loop.spec.ts ADM-058 reads the
 * tiles by label). The targets tile shows the job's target count;
 * the rest show the 24h rollup.
 */
export const LOOP_JOB_STAT_LABELS = {
  targets: "Targets",
  completed: "Completed (24h)",
  failed: "Failed (24h)",
  skipped: "Skipped (24h)",
} as const;

/** Header description with the fallback for a blank user-facing description (spec §7.8). */
export function resolveLoopJobDescription(publicDescription: string): string {
  return publicDescription || LOOP_JOB_DESCRIPTION_FALLBACK;
}

/** Skip-reason option label: the blank value reads as all (spec §7.9). */
export function formatSkipOptionLabel(reason: string): string {
  return reason === "" ? LOOP_SKIP_FILTER_ALL_LABEL : reason;
}

/** Next-run cell: the localized timestamp, or the missing marker when unset (spec §7.8). */
export function formatTargetNextRun(nextRunAt: string | null): string {
  return nextRunAt ? new Date(nextRunAt).toLocaleString() : LOOP_TARGET_MISSING_TEXT;
}

/** Last-run status cell: the run status, or the missing marker when the target never ran. */
export function formatTargetLastRunStatus(lastRun: LoopTargetLastRun | null): string {
  return lastRun ? lastRun.status : LOOP_TARGET_MISSING_TEXT;
}

/**
 * Last-run tokens cell: the token count, or the missing marker when
 * the target never ran or reported none. A zero count still renders
 * as zero.
 */
export function formatTargetTokens(lastRun: LoopTargetLastRun | null): string {
  return String(lastRun?.total_tokens ?? LOOP_TARGET_MISSING_TEXT);
}

/** Last skip reason cell: the reason, or the missing marker when blank. */
export function formatTargetSkipReason(lastSkipReason: string): string {
  return lastSkipReason || LOOP_TARGET_MISSING_TEXT;
}
