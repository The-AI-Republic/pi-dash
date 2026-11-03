// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Value-list shaping for the legacy filter dimensions
// (parity/specs/issues.md §9, ISS-082–092). Member dimensions keep the
// applied rows on top, then the signed-in member, then display-name
// order; labels, cycles, modules and projects keep applied rows on top
// and sort the rest by name.
import type { Label } from "@pidash/api-client";

import type { MemberOption } from "../../../../shared/pickers/index.js";

/** Selected ids read as a string array whatever the stored shape. */
export function selectedIds(value: unknown): string[] {
  if (Array.isArray(value)) return value.filter((entry): entry is string => typeof entry === "string");
  return typeof value === "string" && value.length > 0 ? [value] : [];
}

/**
 * Applied rows first, then the signed-in member, then alphabetical by
 * display name. Ties keep the incoming directory order.
 */
export function orderMemberOptions(
  options: readonly MemberOption[],
  chosen: readonly string[],
  meId: string | null
): MemberOption[] {
  const chosenSet = new Set(chosen);
  return [...options].sort((left, right) => {
    const leftApplied = chosenSet.has(left.userId);
    const rightApplied = chosenSet.has(right.userId);
    if (leftApplied !== rightApplied) return leftApplied ? -1 : 1;
    const leftMe = meId !== null && left.userId === meId;
    const rightMe = meId !== null && right.userId === meId;
    if (leftMe !== rightMe) return leftMe ? -1 : 1;
    return left.searchText.toLowerCase().localeCompare(right.searchText.toLowerCase());
  });
}

/** A row with a stable id and a display name. */
export interface NamedRow {
  id: string;
  name: string;
}

/**
 * Applied rows first, then alphabetical by name, case-insensitively.
 * Ties keep the incoming order, so an already sorted source stays stable.
 * Cycles, modules and projects all read this way (ISS-089–091).
 */
export function orderAppliedByName<T extends NamedRow>(rows: readonly T[], chosen: readonly string[]): T[] {
  const chosenSet = new Set(chosen);
  // Each name lowers once and compares once. Lowering both sides of every
  // comparison makes the comparator contradictory for names that fold to
  // the same key (both directions read as "greater"), which leaves the
  // order engine-dependent.
  const keyed = rows.map((row) => ({ row, key: row.name.toLocaleLowerCase() }));
  return keyed
    .sort((left, right) => {
      const leftApplied = chosenSet.has(left.row.id);
      const rightApplied = chosenSet.has(right.row.id);
      if (leftApplied !== rightApplied) return leftApplied ? -1 : 1;
      return left.key.localeCompare(right.key);
    })
    .map(({ row }) => row);
}

/**
 * The cycle filter's rows (ISS-089): every project cycle the server lists,
 * finished ones included, because the dimension marks each lifecycle and
 * the completed glyph has to be reachable. This differs from the shared
 * cycle picker, which hides completed cycles, so callers build the rows
 * here rather than through `selectCycleOptions`.
 */
export function cycleFilterRows(cycles: readonly { id: string; name: string }[]): { id: string; name: string }[] {
  return cycles.map((cycle) => ({ id: cycle.id, name: cycle.name }));
}

/** Applied labels first, then alphabetical by name. */
export function orderLabelOptions(labels: readonly Label[], chosen: readonly string[]): Label[] {
  const chosenSet = new Set(chosen);
  return [...labels].sort((left, right) => {
    const leftApplied = chosenSet.has(left.id);
    const rightApplied = chosenSet.has(right.id);
    if (leftApplied !== rightApplied) return leftApplied ? -1 : 1;
    return left.name.toLowerCase().localeCompare(right.name.toLowerCase());
  });
}
