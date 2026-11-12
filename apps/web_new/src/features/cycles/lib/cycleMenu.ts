// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row-menu entries per role and cycle state (parity/specs/cycles.md
// §10, CYC-028). Titles are the exact strings the parity driver
// filters on; the array order is the menu order. Finished cycles
// lose edit and delete (CYC-023); archiving stays visible but gated
// on unfinished rows so the restriction is discoverable (CYC-025);
// guests keep the link actions only.

import type { CycleRow } from "@pidash/api-client";

/** Menu entry kinds in spec order; archive yields to restore on archived rows. */
export type CycleMenuEntryId = "edit" | "open" | "copy" | "archive" | "restore" | "delete";

export interface CycleMenuEntry {
  id: CycleMenuEntryId;
  /** Exact title; the parity driver matches menu items on it. */
  title: string;
  /** Explanatory line under a gated entry; null when the entry needs none. */
  hint: string | null;
  disabled: boolean;
}

/** The archive gate's explanatory line on unfinished rows. */
export const CYCLE_ARCHIVE_GATE_HINT = "Only completed cycles can be archived";

const LINK_ENTRIES: CycleMenuEntry[] = [
  { id: "open", title: "Open in new tab", hint: null, disabled: false },
  { id: "copy", title: "Copy link", hint: null, disabled: false },
];

/** Finished means the completed marker, compared case-insensitively. */
export function isCycleFinished(cycle: Pick<CycleRow, "status">): boolean {
  return (cycle.status ?? "").toLowerCase() === "completed";
}

/** Archived means the server stamped the row, whatever the marker reads. */
export function isCycleArchived(cycle: Pick<CycleRow, "archived_at">): boolean {
  return cycle.archived_at !== null && cycle.archived_at !== undefined;
}

/**
 * The row menu for one cycle: guests see the link actions on every
 * state; members and admins see the state matrix — edit plus the
 * gated archive and delete on unfinished rows, archive alone on
 * finished rows, restore alone on archived rows.
 */
export function cycleMenuEntries(
  cycle: Pick<CycleRow, "status" | "archived_at">,
  canMutate: boolean
): CycleMenuEntry[] {
  if (!canMutate) return [...LINK_ENTRIES];
  if (isCycleArchived(cycle)) {
    return [...LINK_ENTRIES, { id: "restore", title: "Restore", hint: null, disabled: false }];
  }
  if (isCycleFinished(cycle)) {
    return [...LINK_ENTRIES, { id: "archive", title: "Archive", hint: null, disabled: false }];
  }
  return [
    { id: "edit", title: "Edit", hint: null, disabled: false },
    ...LINK_ENTRIES,
    { id: "archive", title: "Archive", hint: CYCLE_ARCHIVE_GATE_HINT, disabled: true },
    { id: "delete", title: "Delete", hint: null, disabled: false },
  ];
}
