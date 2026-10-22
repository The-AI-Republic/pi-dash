// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bulk selection model (parity/specs/issues.md §10, ISS-108/109/111).
// Selection is client state over a flattened list of same-project rows.
// Rows carry their group so a group header can report a tri-state and a
// picker can select the whole group. Range selection walks the flattened
// order, so it crosses group boundaries in both directions. The helpers
// stay pure; the hook owns the React state and window listeners.

/** One selectable row and the group bucket it renders inside. */
export interface SelectionItem {
  id: string;
  groupKey: string;
}

/** A group header's aggregate selection state. */
export type GroupSelectionState = "empty" | "partial" | "complete";

/** True when every id in the list is present in the selection. */
function containsAll(selected: ReadonlySet<string>, items: readonly SelectionItem[]): boolean {
  return items.length > 0 && items.every((item) => selected.has(item.id));
}

/** Toggle one row; shift replaces the walk with the range from the anchor. */
export function toggleItem(
  selected: ReadonlySet<string>,
  items: readonly SelectionItem[],
  id: string,
  anchorId: string | null,
  shiftKey: boolean
): Set<string> {
  if (!shiftKey || anchorId === null || anchorId === id) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    return next;
  }
  const from = items.findIndex((item) => item.id === anchorId);
  const to = items.findIndex((item) => item.id === id);
  if (from === -1 || to === -1) {
    const next = new Set(selected);
    next.add(id);
    return next;
  }
  const next = new Set(selected);
  const start = Math.min(from, to);
  const end = Math.max(from, to);
  for (let index = start; index <= end; index += 1) next.add(items[index]!.id);
  return next;
}

/** Aggregate state for one group's rows. */
export function groupSelectionState(
  selected: ReadonlySet<string>,
  items: readonly SelectionItem[],
  groupKey: string
): GroupSelectionState {
  const rows = items.filter((item) => item.groupKey === groupKey);
  if (rows.length === 0) return "empty";
  const count = rows.filter((item) => selected.has(item.id)).length;
  if (count === 0) return "empty";
  if (count === rows.length) return "complete";
  return "partial";
}

/** Select every row of a group, or clear it when it is already complete. */
export function toggleGroup(
  selected: ReadonlySet<string>,
  items: readonly SelectionItem[],
  groupKey: string
): Set<string> {
  const rows = items.filter((item) => item.groupKey === groupKey);
  const next = new Set(selected);
  if (containsAll(selected, rows)) {
    for (const row of rows) next.delete(row.id);
  } else {
    for (const row of rows) next.add(row.id);
  }
  return next;
}

/** Drop ids that are no longer present after a filter or regroup. */
export function pruneSelection(selected: ReadonlySet<string>, items: readonly SelectionItem[]): Set<string> {
  const present = new Set(items.map((item) => item.id));
  const next = new Set<string>();
  for (const id of selected) if (present.has(id)) next.add(id);
  return next;
}

/** Move the active row one step in the flattened order. */
export function moveActive(
  activeId: string | null,
  items: readonly SelectionItem[],
  direction: "up" | "down"
): string | null {
  if (items.length === 0) return null;
  if (activeId === null) return (direction === "down" ? items[0] : items[items.length - 1])!.id;
  const index = items.findIndex((item) => item.id === activeId);
  if (index === -1) return (direction === "down" ? items[0] : items[items.length - 1])!.id;
  const next = direction === "down" ? index + 1 : index - 1;
  return items[next]?.id ?? activeId;
}

/** Add the previous/next row to the selection, following the active row. */
export function extendSelection(
  selected: ReadonlySet<string>,
  activeId: string | null,
  items: readonly SelectionItem[],
  direction: "up" | "down"
): Set<string> {
  const target = moveActive(activeId, items, direction);
  if (target === null) return new Set(selected);
  const next = new Set(selected);
  next.add(target);
  return next;
}

/** The bulk-editable property keys, in toolbar order. */
export const BULK_PROPERTY_KEYS = [
  "state_id",
  "priority",
  "assignee_ids",
  "label_ids",
  "start_date",
  "target_date",
  "cycle_id",
  "module_ids",
  "estimate_point",
] as const;

export type BulkPropertyKey = (typeof BULK_PROPERTY_KEYS)[number];

/** The fields a project's feature flags leave visible in the toolbar. */
export function visibleBulkFields(flags: {
  hideCycle: boolean;
  hideModule: boolean;
  hideEstimate: boolean;
}): BulkPropertyKey[] {
  return BULK_PROPERTY_KEYS.filter((key) => {
    if (key === "cycle_id") return !flags.hideCycle;
    if (key === "module_ids") return !flags.hideModule;
    if (key === "estimate_point") return !flags.hideEstimate;
    return true;
  });
}
