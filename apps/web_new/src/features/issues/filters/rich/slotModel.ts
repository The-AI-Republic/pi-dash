// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Value-slot model (parity/specs/issues.md §9, ISS-100–103). One place
// decides which editor a field kind renders, which read backs its list,
// how a committed value is shaped, and what the trigger reads while
// nothing or only part of a value is set. Pure functions only.

import type { RichFilterField } from "./expression.js";

/** The editor a slot kind renders. */
export type SlotEditor = "choice" | "pick" | "date" | "range";

/** Which read backs one field's option list. */
export type SlotSource = "static" | "states" | "members" | "labels" | "cycles" | "modules" | "projects";

/** Placeholder for an unset value. */
export const EMPTY_SLOT_LABEL = "--";

const SOURCES: Readonly<Record<string, SlotSource>> = {
  state_id: "states",
  assignee_id: "members",
  created_by_id: "members",
  mention_id: "members",
  label_id: "labels",
  cycle_id: "cycles",
  module_id: "modules",
  project_id: "projects",
};

/** Which read backs one field's option list. */
export function slotSource(field: RichFilterField): SlotSource {
  return SOURCES[field.stem] ?? "static";
}

/**
 * The editor for a field at one operator: date kinds get calendars and
 * static fields a list. A date with an `exact` lookup edits a single day
 * and its `range` lookup edits two ends, so one field can offer both
 * forms the way the old filter set does. Every other lookup uses the
 * kind's own editor.
 */
export function slotEditor(field: RichFilterField, operator: string): SlotEditor {
  if (operator === "exact" && (field.kind === "date" || field.kind === "date_range")) return "date";
  if (operator === "range") return "range";
  if (field.kind === "date") return "date";
  if (field.kind === "date_range") return "range";
  return slotSource(field) === "static" ? "choice" : "pick";
}

/** The slot opens by itself whenever it holds no complete value. */
export function opensWhenIncomplete(
  field: RichFilterField,
  operator: string,
  value: unknown,
  complete: boolean
): boolean {
  return slotEditor(field, operator) !== "choice" && !complete;
}

/** Both ends of a range for either an array or the stored `"start,end"` string. */
export function rangeEndsFromString(value: unknown): { start: string | null; end: string | null } {
  if (typeof value === "string") {
    const [start, end] = value.split(",").map((entry) => entry.trim());
    return {
      start: start === undefined || start === "" ? null : start,
      end: end === undefined || end === "" ? null : end,
    };
  }
  return rangeEnds(value);
}

/**
 * The value shape one operator expects. Switching `exact` to `range`
 * keeps a stored day as the first end so the pair starts from what the
 * user already picked; switching `range` back to `exact` keeps the first
 * end. Incompatible shapes drop to no value rather than store a pair
 * under a single-day lookup.
 */
export function valueForOperator(previousOperator: string, nextOperator: string, value: unknown): unknown {
  const previous = slotEditorKind(previousOperator);
  const next = slotEditorKind(nextOperator);
  if (previous === next) return value;
  if (next === "date") {
    if (previous === "range") return rangeEndsFromString(value).start;
    return typeof value === "string" && value.length > 0 ? value : null;
  }
  if (next === "range") {
    if (previous === "date") return typeof value === "string" && value.length > 0 ? [value, null] : null;
    return rangeEnds(value).start === null && rangeEnds(value).end === null ? null : value;
  }
  return value;
}

function slotEditorKind(operator: string): "date" | "range" | "other" {
  if (operator === "exact") return "date";
  if (operator === "range") return "range";
  return "other";
}

/** Selected values as strings; a lone stored value reads as one pick. */
export function selectedIds(value: unknown): string[] {
  if (Array.isArray(value)) return value.filter((entry): entry is string => typeof entry === "string");
  return typeof value === "string" && value.length > 0 ? [value] : [];
}

/** Both ends of a stored range; a missing end reads as null. */
export function rangeEnds(value: unknown): { start: string | null; end: string | null } {
  if (!Array.isArray(value)) return { start: null, end: null };
  return {
    start: typeof value[0] === "string" && value[0] !== "" ? value[0] : null,
    end: typeof value[1] === "string" && value[1] !== "" ? value[1] : null,
  };
}

/** Store both ends; nothing set reads as no value. */
export function rangeValue(start: string | null, end: string | null): unknown {
  return start === null && end === null ? null : [start, end];
}

/** Trigger text for a range: both ends, the open end, or the empty dash. */
export function rangeLabel(ends: { start: string | null; end: string | null }): string {
  if (ends.start === null && ends.end === null) return EMPTY_SLOT_LABEL;
  if (ends.end === null) return `${ends.start} → …`;
  // An end without a start stays the empty dash; the warning, not the text,
  // tells the user the pair is unfinished.
  if (ends.start === null) return EMPTY_SLOT_LABEL;
  return `${ends.start} → ${ends.end}`;
}

/** A range holding exactly one end is incomplete: warned and reopened. */
export function isRangeIncomplete(value: unknown): boolean {
  const ends =
    typeof value === "object" && value !== null && !Array.isArray(value)
      ? (value as { start?: string | null; end?: string | null })
      : rangeEndsFromString(value);
  const start = ends.start ?? null;
  const end = ends.end ?? null;
  return (start === null) !== (end === null);
}

/** Add or remove one pick; a single field keeps at most one. */
export function togglePick(field: RichFilterField, value: unknown, id: string): unknown {
  const current = selectedIds(value);
  if (current.includes(id)) {
    const rest = current.filter((entry) => entry !== id);
    return field.kind === "multi" && rest.length > 0 ? rest : null;
  }
  return field.kind === "multi" ? [...current, id] : id;
}

/** Trigger text for a pick: the first two labels plus an overflow count. */
export function pickTriggerLabel(labels: readonly string[]): string {
  if (labels.length === 0) return EMPTY_SLOT_LABEL;
  if (labels.length <= 2) return labels.join(", ");
  return `${labels.slice(0, 2).join(", ")} +${labels.length - 2} more`;
}
