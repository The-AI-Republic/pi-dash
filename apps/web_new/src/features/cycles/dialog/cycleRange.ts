// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle date-range rules for the create/edit dialog (parity/specs/cycles.md
// §6, §7 and §23, CYC-016, CYC-017). Pure: payload days, the overlap-check
// gate, and the refusal copy. The sidebar inline date editing (slice G)
// reuses the check gate and the refusal copy so both surfaces refuse the
// same ranges with the same guidance.

import type { RangeValue } from "../../issues/index.js";

import { instantToProjectDate } from "../lib/cycleDates.js";

/** Overlap refusal toast title (spec §6, pinned for the oracle). */
export const CYCLE_OVERLAP_TITLE = "Error!";

/** Stored calendar days for a create/update payload; unset ends are null. */
export interface CyclePayloadDays {
  start_date: string | null;
  end_date: string | null;
}

/** Blank picker ends read as unset, like nulls. */
function normalizeDay(day: string | null): string | null {
  return day === null || day === "" ? null : day;
}

/** The picked range as payload days (one-sided ranges stay one-sided). */
export function cycleFormDays(range: RangeValue): CyclePayloadDays {
  return { start_date: normalizeDay(range.start), end_date: normalizeDay(range.end) };
}

/** True when both payload days are set (the check never runs otherwise). */
export function cycleDaysComplete(days: CyclePayloadDays): days is { start_date: string; end_date: string } {
  return days.start_date !== null && days.end_date !== null;
}

export interface CycleStoredDays {
  start_date: string | null;
  end_date: string | null;
}

/**
 * True when the picked days match the stored instants read in the project
 * timezone. A stored null matches only an unset pick, so clearing or
 * setting a side counts as a change.
 */
export function cycleDaysUnchanged(
  original: CycleStoredDays,
  next: CyclePayloadDays,
  timeZone: string | null | undefined
): boolean {
  return (
    instantToProjectDateOrNull(original.start_date, timeZone) === next.start_date &&
    instantToProjectDateOrNull(original.end_date, timeZone) === next.end_date
  );
}

function instantToProjectDateOrNull(instant: string | null, timeZone: string | null | undefined): string | null {
  if (instant === null) return null;
  return instantToProjectDate(instant, timeZone);
}

/**
 * Whether the submit must ask the overlap check (spec §6/§7): on create
 * (no original) whenever both days are set; on edit only when both days
 * are set and the range changed. Undated and one-sided ranges always
 * save without a check.
 */
export function needsCycleDateCheck(
  original: CycleStoredDays | null,
  range: RangeValue,
  timeZone: string | null | undefined
): range is RangeValue & { start: string; end: string } {
  const days = cycleFormDays(range);
  if (!cycleDaysComplete(days)) return false;
  if (original === null) return true;
  return !cycleDaysUnchanged(original, days, timeZone);
}

/**
 * Overlap refusal message (spec §6, wording from the spec, not the old
 * app). Names the pinned phrase and the draft workaround: the new range
 * can stay undated, or the conflicting draft can drop its dates.
 */
export function cycleOverlapMessage(mode: "create" | "update"): string {
  const keep =
    mode === "create"
      ? "Leave the new cycle undated to save it as a draft"
      : "Clear this cycle's dates to save it as a draft";
  return (
    "These dates overlap another cycle: you already have a cycle in this window. " +
    `${keep}, or remove the dates from the conflicting draft to free the window.`
  );
}

/** Server detail when the body carries one; null otherwise. */
function serverDetail(error: unknown): string | null {
  const body = (error as { body?: unknown }).body ?? null;
  if (typeof body === "object" && body !== null && !Array.isArray(body)) {
    const detail = (body as Record<string, unknown>)["detail"];
    if (typeof detail === "string" && detail.length > 0) return detail;
  }
  return null;
}

/**
 * Inline dialog error for a failed write (spec §6: shown in the dialog,
 * never a silent close). Prefers the server's detail; the fixed copy
 * otherwise, phrased per mode.
 */
export function cycleFailureMessage(error: unknown, mode: "create" | "update"): string {
  const detail = serverDetail(error);
  if (detail !== null) return detail;
  return mode === "create"
    ? "Nothing was created — check the details and try again."
    : "Nothing was changed — check the details and try again.";
}
