// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending calendar creates (parity/specs/issues.md §4, ISS-025/026). A
// quick-add overlays a pulsing temp block on its day until the create
// settles, then the overlay is removed and the normal refetch takes over.

import type { IssueListItem } from "@pidash/api-client";

export type PendingRowsByDay = Record<string, IssueListItem[]>;

/** Add one temp block to a day without dropping other pending creates. */
export function addPendingRow(current: PendingRowsByDay, day: string, row: IssueListItem): PendingRowsByDay {
  const existing = current[day] ?? [];
  if (existing.some((entry) => entry.id === row.id)) return current;
  return { ...current, [day]: [...existing, row] };
}

/** Remove a settled temp block from every day it was overlaid on. */
export function removePendingRow(current: PendingRowsByDay, tempId: string): PendingRowsByDay {
  let changed = false;
  const next: PendingRowsByDay = {};
  for (const [day, rows] of Object.entries(current)) {
    const remaining = rows.filter((row) => row.id !== tempId);
    if (remaining.length !== rows.length) changed = true;
    if (remaining.length > 0) next[day] = remaining;
  }
  return changed ? next : current;
}
