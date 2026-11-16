// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer rules for completed cycles (parity/specs/issues.md §18,
// ISS-224). A completed cycle offers the transfer prompt, and only a
// completed cycle does: the wording talks about active cycles, but the
// server derives completed from the dates and the oracle pins the
// prompt to that state. The button appears while the cycle's progress
// read still reports backlog, unstarted or started work. After a move the
// server freezes the source's counts into its progress snapshot and the
// progress endpoint keeps serving them, so the button stays up exactly as
// the oracle observes on the old app.

/** The counts the gate reads. */
export interface TransferableCycleFields {
  status?: string | undefined;
  backlog_issues?: number | undefined;
  unstarted_issues?: number | undefined;
  started_issues?: number | undefined;
}

/** A cycle counts as completed when its status names it, any casing. */
export function isCompletedCycleStatus(status: string | undefined): boolean {
  return status !== undefined && status.toLowerCase().includes("complet");
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
  return transferableIssueCount(cycle) > 0;
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
