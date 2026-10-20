// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer rules for completed cycles (parity/specs/issues.md §18,
// ISS-224). A completed cycle offers the transfer prompt, and only a
// completed cycle does: the wording talks about active cycles, but the
// server derives completed from the dates and the oracle pins the
// prompt to that state. The button appears while the cycle's progress
// snapshot is still empty and it still holds backlog, unstarted or
// started work. Once a transfer freezes the snapshot the button goes
// away, so a cycle can transfer once.

import type { CycleDetail } from "@pidash/api-client";

/** The counts and snapshot the gate reads. */
export interface TransferableCycleFields {
  status?: string | undefined;
  progress_snapshot?: Record<string, unknown> | null | undefined;
  backlog_issues?: number | undefined;
  unstarted_issues?: number | undefined;
  started_issues?: number | undefined;
}

/** A cycle counts as completed when its status names it, any casing. */
export function isCompletedCycleStatus(status: string | undefined): boolean {
  return status !== undefined && status.toLowerCase().includes("complet");
}

/**
 * True when the progress snapshot is absent, null or an empty object.
 * The server sends `{}` before a transfer and a populated snapshot
 * after one, so an unknown shape reads as present (fail closed).
 */
export function hasEmptyProgressSnapshot(snapshot: Record<string, unknown> | null | undefined): boolean {
  return snapshot === null || snapshot === undefined || Object.keys(snapshot).length === 0;
}

/** Backlog, unstarted and started work: the rows a transfer would move. */
export function transferableIssueCount(cycle: TransferableCycleFields | null | undefined): number {
  if (cycle === null || cycle === undefined) return 0;
  return (cycle.backlog_issues ?? 0) + (cycle.unstarted_issues ?? 0) + (cycle.started_issues ?? 0);
}

/** Whether the cycle page shows the transfer button. */
export function canTransferCycleIssues(cycle: TransferableCycleFields | null | undefined): boolean {
  if (cycle === null || cycle === undefined) return false;
  if (!isCompletedCycleStatus(cycle.status)) return false;
  return hasEmptyProgressSnapshot(cycle.progress_snapshot) && transferableIssueCount(cycle) > 0;
}

/** Whether a cycle page renders the completed-cycle note at all. */
export function showsCompletedCycleNote(cycle: TransferableCycleFields | null | undefined): boolean {
  return cycle !== null && cycle !== undefined && isCompletedCycleStatus(cycle.status);
}

/**
 * The cycles the transfer dialog offers: every cycle the picker list
 * returned except completed ones and the source itself, narrowed by the
 * search text (case-insensitive substring on the name).
 */
export function transferTargetOptions<T extends { id: string; name: string; status?: string | undefined }>(
  cycles: readonly T[],
  sourceCycleId: string,
  query: string
): T[] {
  const needle = query.trim().toLowerCase();
  return cycles.filter((cycle) => {
    if (cycle.id === sourceCycleId) return false;
    if (isCompletedCycleStatus(cycle.status)) return false;
    if (needle === "") return true;
    return cycle.name.toLowerCase().includes(needle);
  });
}

/** Narrow a detail read to the fields the gate needs. */
export function transferFieldsOf(cycle: CycleDetail | undefined): TransferableCycleFields | null {
  if (cycle === undefined) return null;
  return {
    status: cycle.status,
    progress_snapshot: cycle.progress_snapshot,
    backlog_issues: cycle.backlog_issues,
    unstarted_issues: cycle.unstarted_issues,
    started_issues: cycle.started_issues,
  };
}
