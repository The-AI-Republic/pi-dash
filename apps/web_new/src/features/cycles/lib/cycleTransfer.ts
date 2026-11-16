// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer eligibility and destination options (parity/specs/cycles.md
// §12, CYC-031). The finished detail banner and the completed list row
// share the transferable count and the open-only destination rule; only
// the snapshot clause differs between the two gates.

import type { CycleListItem } from "@pidash/api-client";

/** The row fields the transfer gates read. */
export interface TransferableCycle {
  status?: string | undefined;
  archived_at?: string | null | undefined;
  total_issues?: number | undefined;
  cancelled_issues?: number | undefined;
  completed_issues?: number | undefined;
  progress_snapshot?: Record<string, unknown> | null | undefined;
}

/** Finished means the completed marker, compared case-insensitively. */
export function isTransferableStatus(status: string | undefined): boolean {
  return (status ?? "").toLowerCase() === "completed";
}

/** Archived rows never transfer, whatever their marker reads. */
export function isTransferArchived(cycle: Pick<TransferableCycle, "archived_at">): boolean {
  return cycle.archived_at !== null && cycle.archived_at !== undefined;
}

/**
 * Work that could move: total minus cancelled and completed, read live
 * (not from the frozen snapshot). Missing counters read as zero; a
 * negative residue reads as none.
 */
export function transferableWorkCount(cycle: TransferableCycle): number {
  const total = cycle.total_issues ?? 0;
  const done = (cycle.cancelled_issues ?? 0) + (cycle.completed_issues ?? 0);
  return Math.max(0, total - done);
}

/** Empty snapshot: absent, null, or carrying no entries. */
export function isTransferSnapshotEmpty(snapshot: Record<string, unknown> | null | undefined): boolean {
  if (snapshot === null || snapshot === undefined) return true;
  return Object.keys(snapshot).length === 0;
}

/**
 * List-row gate: completed, live, and holding transferable work. It
 * does not look at the snapshot.
 */
export function canTransferFromRow(cycle: TransferableCycle): boolean {
  if (!isTransferableStatus(cycle.status)) return false;
  if (isTransferArchived(cycle)) return false;
  return transferableWorkCount(cycle) > 0;
}

/**
 * Detail-banner gate: the row gate plus an empty snapshot. Once the
 * server writes the snapshot the banner action leaves even with work
 * left, while the row action follows the count alone.
 */
export function canTransferFromBanner(cycle: TransferableCycle): boolean {
  if (!canTransferFromRow(cycle)) return false;
  return isTransferSnapshotEmpty(cycle.progress_snapshot);
}

/**
 * Destination options: the project's open cycles except the source —
 * draft, current, upcoming — never finished or archived, narrowed by
 * a case-insensitive name substring. Server order is kept.
 */
export function transferTargetOptions<T extends Pick<CycleListItem, "id" | "name" | "status">>(
  cycles: readonly T[],
  sourceCycleId: string,
  query: string
): T[] {
  const needle = query.trim().toLowerCase();
  return cycles.filter((cycle) => {
    if (cycle.id === sourceCycleId) return false;
    if (isTransferableStatus(cycle.status)) return false;
    if (needle === "") return true;
    return cycle.name.toLowerCase().includes(needle);
  });
}

/** Capitalised lifecycle pill for a destination option; missing reads as draft. */
export function transferStatusPill(status: string | undefined): string {
  const marker = (status ?? "").toLowerCase();
  if (marker === "current") return "Current";
  if (marker === "upcoming") return "Upcoming";
  return "Draft";
}
