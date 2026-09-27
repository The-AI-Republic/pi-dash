/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { RRule, Weekday } from "rrule";
import type { Options } from "rrule";

/**
 * Pure conversion layer between an RFC 5545 RRULE string (the bare
 * `FREQ=…` form the API stores) and the recurrence-builder widget state.
 *
 * The widget deliberately covers only the common shapes — day / week /
 * month / year cadence, weekday chips, monthly by-day vs nth-weekday, and
 * an Ends group. `parseRrule` returns `null` for anything it cannot
 * express **exactly** (BYSETPOS, BYHOUR lists, WKST, sub-daily FREQs, …)
 * so the UI falls back to raw-textarea mode and never silently rewrites a
 * hand-written rule.
 */

/** "NONE" = does not repeat (empty rrule; single-shot at dtstart). */
export type RecurrenceUnit = "NONE" | "DAILY" | "WEEKLY" | "MONTHLY" | "YEARLY";

export type MonthlyMode = "monthday" | "weekday";

export type EndsMode = "never" | "until" | "count";

export interface RecurrenceDraft {
  unit: RecurrenceUnit;
  /** ≥ 1. The server 400s on INTERVAL=0. */
  interval: number;
  /** Weekly only: rrule weekday numbers, 0=MO … 6=SU. Never empty for unit=WEEKLY. */
  weekdays: number[];
  /** Monthly only: repeat on a fixed day-of-month, or on the nth weekday. */
  monthlyMode: MonthlyMode;
  /** 1..31 — monthlyMode=monthday. */
  monthday: number;
  /** 1..4, or -1 for "last" — monthlyMode=weekday. */
  nth: number;
  /** rrule weekday number, 0=MO … 6=SU — monthlyMode=weekday. */
  nthWeekday: number;
  ends: EndsMode;
  /** "YYYY-MM-DD", interpreted as end-of-day UTC — ends=until. */
  untilDate: string;
  /** ≥ 1 — ends=count. */
  count: number;
}

const FREQ_TO_UNIT: Partial<Record<number, RecurrenceUnit>> = {
  [RRule.DAILY]: "DAILY",
  [RRule.WEEKLY]: "WEEKLY",
  [RRule.MONTHLY]: "MONTHLY",
  [RRule.YEARLY]: "YEARLY",
};

const BYDAY_CODES = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

/** rrule weekday numbers in canonical MO..SU emission order. */
const ALL_WEEKDAYS = [0, 1, 2, 3, 4, 5, 6];

/** Option keys the widget can represent; anything else forces raw mode. */
const SUPPORTED_KEYS = new Set(["freq", "interval", "byweekday", "bymonthday", "until", "count"]);

/** JS Date#getDay (0=Sun) → rrule weekday number (0=MO … 6=SU). */
function jsDayToRruleWeekday(jsDay: number): number {
  return (jsDay + 6) % 7;
}

/**
 * Widget defaults derived from the binding's dtstart (today when absent).
 * Reads the anchor's *local* components: the dtstart form value is a
 * `datetime-local` wall-time string, so the derived weekday/monthday match
 * what the user sees in the picker.
 */
export function defaultDraft(dtstart?: Date | string | null): RecurrenceDraft {
  const anchorRaw = dtstart ? new Date(dtstart) : new Date();
  const anchor = Number.isNaN(anchorRaw.getTime()) ? new Date() : anchorRaw;
  const weekday = jsDayToRruleWeekday(anchor.getDay());
  const monthday = anchor.getDate();
  const nth = Math.ceil(monthday / 7);
  return {
    unit: "DAILY",
    interval: 1,
    weekdays: [weekday],
    monthlyMode: "monthday",
    monthday,
    nth: nth >= 5 ? -1 : nth,
    nthWeekday: weekday,
    ends: "never",
    untilDate: "",
    count: 13,
  };
}

function normalizeWeekdayEntry(entry: Weekday | number | string): { weekday: number; n: number | null } | null {
  if (typeof entry === "number")
    return Number.isInteger(entry) && entry >= 0 && entry <= 6 ? { weekday: entry, n: null } : null;
  if (entry instanceof Weekday) return { weekday: entry.weekday, n: entry.n ?? null };
  if (typeof entry === "string") {
    const idx = BYDAY_CODES.indexOf(entry);
    return idx >= 0 ? { weekday: idx, n: null } : null;
  }
  return null;
}

/**
 * Parse a stored RRULE string into widget state, or `null` when the rule
 * uses anything the widget cannot express (→ the UI stays in raw mode).
 * An empty string is the single-shot state (`unit: "NONE"`).
 */
export function parseRrule(rrule: string, dtstart?: Date | string | null): RecurrenceDraft | null {
  const draft = defaultDraft(dtstart);
  const trimmed = (rrule ?? "").trim();
  if (!trimmed) return { ...draft, unit: "NONE" };
  // Multi-line inputs (embedded DTSTART line etc.) are out of widget scope.
  if (/[\r\n]/.test(trimmed)) return null;

  let parsed: Partial<Options>;
  try {
    parsed = RRule.parseString(trimmed.replace(/^RRULE:/i, ""));
  } catch {
    return null;
  }

  for (const [key, value] of Object.entries(parsed)) {
    if (value === undefined || value === null) continue;
    if (!SUPPORTED_KEYS.has(key)) return null;
  }

  const unit = parsed.freq != null ? FREQ_TO_UNIT[parsed.freq] : undefined;
  if (!unit) return null; // missing FREQ, or HOURLY/MINUTELY/SECONDLY → raw mode
  draft.unit = unit;

  if (parsed.interval != null) {
    if (!Number.isInteger(parsed.interval) || parsed.interval < 1) return null;
    draft.interval = parsed.interval;
  }

  if (parsed.until != null && parsed.count != null) return null; // RFC 5545 forbids both
  if (parsed.until != null) {
    if (!(parsed.until instanceof Date) || Number.isNaN(parsed.until.getTime())) return null;
    draft.ends = "until";
    draft.untilDate = parsed.until.toISOString().slice(0, 10);
  }
  if (parsed.count != null) {
    if (!Number.isInteger(parsed.count) || parsed.count < 1) return null;
    draft.ends = "count";
    draft.count = parsed.count;
  }

  const byweekdayRaw =
    parsed.byweekday == null ? [] : Array.isArray(parsed.byweekday) ? parsed.byweekday : [parsed.byweekday];
  const byweekday: { weekday: number; n: number | null }[] = [];
  for (const entry of byweekdayRaw) {
    const norm = entry == null ? null : normalizeWeekdayEntry(entry);
    if (!norm) return null;
    byweekday.push(norm);
  }
  const bymonthdayRaw = (
    parsed.bymonthday == null ? [] : Array.isArray(parsed.bymonthday) ? parsed.bymonthday : [parsed.bymonthday]
  ).filter((d): d is number => d != null);

  switch (unit) {
    case "WEEKLY": {
      if (bymonthdayRaw.length > 0) return null;
      if (byweekday.some((d) => d.n !== null)) return null; // nth-weekday only makes sense monthly
      if (byweekday.length > 0) {
        const present = new Set(byweekday.map((d) => d.weekday));
        draft.weekdays = ALL_WEEKDAYS.filter((d) => present.has(d));
      }
      // else: plain FREQ=WEEKLY repeats on dtstart's weekday — the default already matches.
      break;
    }
    case "MONTHLY": {
      if (byweekday.length > 0 && bymonthdayRaw.length > 0) return null;
      if (bymonthdayRaw.length > 1 || byweekday.length > 1) return null;
      if (bymonthdayRaw.length === 1) {
        const day = bymonthdayRaw[0];
        if (!Number.isInteger(day) || day < 1 || day > 31) return null; // negative monthdays → raw
        draft.monthlyMode = "monthday";
        draft.monthday = day;
      } else if (byweekday.length === 1) {
        const { weekday, n } = byweekday[0];
        if (n === null || n === 0 || n > 4 || n < -1) return null;
        draft.monthlyMode = "weekday";
        draft.nth = n;
        draft.nthWeekday = weekday;
      }
      // else: plain FREQ=MONTHLY repeats on dtstart's day-of-month — default matches.
      break;
    }
    default: {
      // DAILY / YEARLY carry no BY* parts in widget form.
      if (byweekday.length > 0 || bymonthdayRaw.length > 0) return null;
    }
  }

  return draft;
}

/**
 * Serialize widget state to the bare `FREQ=…` form the API expects: no
 * `RRULE:` prefix, no embedded DTSTART, INTERVAL omitted when 1. Returns
 * `""` for the single-shot state.
 */
export function serializeRrule(draft: RecurrenceDraft): string {
  if (draft.unit === "NONE") return "";
  const parts: string[] = [`FREQ=${draft.unit}`];
  const interval = Math.max(1, Math.floor(draft.interval) || 1);
  if (interval > 1) parts.push(`INTERVAL=${interval}`);

  if (draft.unit === "WEEKLY" && draft.weekdays.length > 0) {
    const present = new Set(draft.weekdays);
    const days = ALL_WEEKDAYS.filter((d) => present.has(d));
    parts.push(`BYDAY=${days.map((d) => BYDAY_CODES[d]).join(",")}`);
  }
  if (draft.unit === "MONTHLY") {
    if (draft.monthlyMode === "monthday") parts.push(`BYMONTHDAY=${draft.monthday}`);
    else parts.push(`BYDAY=${draft.nth}${BYDAY_CODES[draft.nthWeekday]}`);
  }

  if (draft.ends === "until" && draft.untilDate) {
    // The picked calendar date is interpreted as end-of-day UTC — expansion
    // runs in UTC server-side, and RFC 5545 requires UNTIL in UTC anyway.
    parts.push(`UNTIL=${draft.untilDate.replaceAll("-", "")}T235959Z`);
  } else if (draft.ends === "count") {
    parts.push(`COUNT=${Math.max(1, Math.floor(draft.count) || 1)}`);
  }

  return parts.join(";");
}
