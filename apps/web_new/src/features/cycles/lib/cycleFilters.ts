// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles list filter model (parity/specs/cycles.md §4, CYC-006–008).
// Three dimensions narrow the loaded rows client-side: lifecycle states
// combine as OR, every date entry is ANDed with the others and with the
// other window, and all of it ANDs with the search term. A preset keeps
// a cycle whose own date sits at least that far ahead of today in the
// project's zone; a custom entry bounds the day from one side. A cycle
// without a date on the filtered side never matches, so drafts drop out
// of every dated filter. Nothing here issues a request.

import type { CycleRow as CycleRowData } from "@pidash/api-client";

import { instantToProjectDate, projectToday } from "./cycleDates.js";

/** The filter dimensions on the live list; the archived list omits status. */
export type CycleFilterDimension = "status" | "startDate" | "endDate";

/** The two date windows. */
export type CycleDateFilterDimension = "startDate" | "endDate";

/** Applied values per dimension; empty reads as no constraint. */
export interface CycleListFilters {
  /** Lifecycle markers, lowercase (`current`, `upcoming`, `completed`, `draft`). */
  status: readonly string[];
  /** Start-window entries: presets plus custom `day;after|before` bounds. */
  startDate: readonly string[];
  /** Due-window entries, same grammar as the start window. */
  endDate: readonly string[];
}

/** No values anywhere; the predicate below keeps every row. */
export const EMPTY_CYCLE_FILTERS: CycleListFilters = { status: [], startDate: [], endDate: [] };

/** Lifecycle values in spec §4 menu order. */
export const CYCLE_FILTER_STATUS_VALUES: readonly string[] = ["current", "upcoming", "completed", "draft"];

/** Menu section headings; the oracle picks sections by these labels. */
export const CYCLE_FILTER_SECTION_TITLES: Record<CycleFilterDimension, string> = {
  status: "Status of the cycle",
  startDate: "Start date",
  endDate: "Due date",
};

/** Short dimension names for the chips row. */
export const CYCLE_FILTER_CHIP_LABELS: Record<CycleFilterDimension, string> = {
  status: "Status",
  startDate: "Start date",
  endDate: "Due date",
};

/** Whether any dimension narrows the list. */
export function hasCycleFilters(filters: CycleListFilters): boolean {
  return filters.status.length > 0 || filters.startDate.length > 0 || filters.endDate.length > 0;
}

/** Applied values across every dimension, for the trigger badge. */
export function countCycleFilters(filters: CycleListFilters): number {
  return filters.status.length + filters.startDate.length + filters.endDate.length;
}

/** One date entry read into the check it performs. */
export type CycleDateEntry =
  | { kind: "preset"; days: number }
  | { kind: "custom"; day: string; direction: "after" | "before" }
  | { kind: "unknown" };

const PRESET_ENTRY = /^(\d+)_(weeks?|months?);after;fromnow$/;
const CUSTOM_ENTRY = /^(\d{4}-\d{2}-\d{2});(after|before)$/;

/**
 * Read one stored date entry. Presets resolve against today
 * (`<n>_<unit>s;after;fromnow`, months counting thirty days); a custom
 * entry bounds the cycle day from one side. Anything else never matches.
 */
export function parseCycleDateEntry(entry: string): CycleDateEntry {
  const preset = PRESET_ENTRY.exec(entry);
  if (preset !== null) {
    // Both groups are mandatory, so they always participate in a match.
    const amount = Number(preset[1]!);
    const unit = preset[2]!;
    return { kind: "preset", days: unit.startsWith("month") ? amount * 30 : amount * 7 };
  }
  const custom = CUSTOM_ENTRY.exec(entry);
  if (custom !== null && (custom[2] === "after" || custom[2] === "before")) {
    return { kind: "custom", day: custom[1]!, direction: custom[2] };
  }
  return { kind: "unknown" };
}

const DAY_MS = 86_400_000;

/** Whole calendar days from `today` to `day`, both `yyyy-MM-dd`. */
function dayLead(day: string, today: string): number {
  const at = Date.UTC(Number(day.slice(0, 4)), Number(day.slice(5, 7)) - 1, Number(day.slice(8, 10)));
  const from = Date.UTC(Number(today.slice(0, 4)), Number(today.slice(5, 7)) - 1, Number(today.slice(8, 10)));
  return Math.round((at - from) / DAY_MS);
}

/** Whether one project-zone day satisfies one parsed entry. */
function entryKeepsDay(entry: CycleDateEntry, day: string, today: string): boolean {
  if (entry.kind === "preset") return dayLead(day, today) >= entry.days;
  if (entry.kind === "custom") return entry.direction === "after" ? day >= entry.day : day <= entry.day;
  return false;
}

/**
 * The row predicate for the applied filters. Lifecycle markers compare
 * case-insensitively; each date side converts its instant to a
 * project-zone day first, so the list, the predicate and the spans all
 * agree on what "today" means. `now` is injectable for tests.
 */
export function makeCycleFilter(
  filters: CycleListFilters,
  projectTimeZone: string | null | undefined,
  now: Date = new Date()
): (cycle: CycleRowData) => boolean {
  const today = projectToday(projectTimeZone, now);
  const statuses = filters.status.map((status) => status.toLowerCase());
  const startEntries = filters.startDate.map(parseCycleDateEntry);
  const endEntries = filters.endDate.map(parseCycleDateEntry);
  const keepStart = (cycle: CycleRowData): boolean => {
    if (startEntries.length === 0) return true;
    if (cycle.start_date === null) return false;
    const day = instantToProjectDate(cycle.start_date, projectTimeZone);
    if (day === null) return false;
    return startEntries.every((entry) => entryKeepsDay(entry, day, today));
  };
  const keepEnd = (cycle: CycleRowData): boolean => {
    if (endEntries.length === 0) return true;
    if (cycle.end_date === null) return false;
    const day = instantToProjectDate(cycle.end_date, projectTimeZone);
    if (day === null) return false;
    return endEntries.every((entry) => entryKeepsDay(entry, day, today));
  };
  return (cycle: CycleRowData): boolean => {
    if (statuses.length > 0 && !statuses.includes((cycle.status ?? "").toLowerCase())) return false;
    if (!keepStart(cycle)) return false;
    return keepEnd(cycle);
  };
}
