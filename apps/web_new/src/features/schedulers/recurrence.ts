// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Recurrence builder model for parity/specs/agents.md §3 and §14 (AGT-065).
// The builder reads and writes the same bare RRULE string the API stores.
// It never rewrites a stored rule on open: parsing yields a widget state
// and an expressibility flag, and only a widget interaction serializes.
// Anything the widget cannot express keeps the raw textarea as the only
// editor, so an untouched stored rule is submitted exactly as it arrived.

/** Weekday codes in the order the chips show them (Sunday first). */
export const WEEKDAY_CODES = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"] as const;
export type WeekdayCode = (typeof WEEKDAY_CODES)[number];

export const WEEKDAY_LABELS: Record<WeekdayCode, string> = {
  SU: "Sunday",
  MO: "Monday",
  TU: "Tuesday",
  WE: "Wednesday",
  TH: "Thursday",
  FR: "Friday",
  SA: "Saturday",
};

/** The repeat unit; `none` is the single firing at the start. */
export const REPEAT_UNITS = ["none", "day", "week", "month", "year"] as const;
export type RepeatUnit = (typeof REPEAT_UNITS)[number];

export const UNIT_LABELS: Record<RepeatUnit, string> = {
  none: "Does not repeat",
  day: "Day",
  week: "Week",
  month: "Month",
  year: "Year",
};

const FREQUENCY_BY_UNIT: Record<Exclude<RepeatUnit, "none">, string> = {
  day: "DAILY",
  week: "WEEKLY",
  month: "MONTHLY",
  year: "YEARLY",
};

export type MonthlyChoice = { on: "day"; day: number } | { on: "weekday"; ordinal: number; weekday: WeekdayCode };

export type EndsChoice = { mode: "never" } | { mode: "on"; date: string } | { mode: "after"; count: number };

export interface RecurrenceState {
  unit: RepeatUnit;
  /** Every N units; always at least one. */
  interval: number;
  /** Weekly chips, Sunday first, never empty. */
  weekdays: WeekdayCode[];
  /** Monthly choice; the day form defaults to the start's day of month. */
  monthly: MonthlyChoice;
  /** Series end; the on-date form ends at the end of that day, UTC. */
  ends: EndsChoice;
}

export interface ParsedRecurrence {
  state: RecurrenceState;
  /** False when the stored rule uses anything the widget cannot model. */
  expressible: boolean;
}

/** Default occurrence count offered for an "after N" series. */
export const DEFAULT_AFTER_COUNT = 10;

const pad2 = (value: number): string => String(value).padStart(2, "0");

interface StartParts {
  weekday: WeekdayCode;
  dayOfMonth: number;
}

/** Wall-clock parts of a datetime-local value, falling back to today. */
function startParts(startsAt: string): StartParts {
  const parsed = new Date(startsAt);
  const date = Number.isNaN(parsed.getTime()) ? new Date() : parsed;
  const weekday = WEEKDAY_CODES[(date.getDay() + 7) % 7] as WeekdayCode;
  return { weekday, dayOfMonth: date.getDate() };
}

/** The date one year after the start, as the on-date default. */
export function defaultEndDate(startsAt: string): string {
  const parsed = new Date(startsAt);
  const date = Number.isNaN(parsed.getTime()) ? new Date() : parsed;
  date.setFullYear(date.getFullYear() + 1);
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

/** Ordinal of a day inside its weekday: day 17 is the 3rd of its weekday. */
export function ordinalForDay(day: number): number {
  return Math.min(5, Math.max(1, Math.ceil(day / 7)));
}

function defaultState(startsAt: string): RecurrenceState {
  const start = startParts(startsAt);
  return {
    unit: "day",
    interval: 1,
    weekdays: [start.weekday],
    monthly: { on: "day", day: start.dayOfMonth },
    ends: { mode: "never" },
  };
}

/** A fresh series (daily) for a given start. */
export function defaultRecurrence(startsAt: string): RecurrenceState {
  return defaultState(startsAt);
}

/**
 * The monthly choice to offer when the start day changes: the day form
 * follows the start, the weekday form keeps the start's weekday.
 */
export function monthlyForStart(startsAt: string, current: MonthlyChoice): MonthlyChoice {
  const start = startParts(startsAt);
  if (current.on === "weekday") {
    return { on: "weekday", ordinal: ordinalForDay(start.dayOfMonth), weekday: start.weekday };
  }
  return { on: "day", day: start.dayOfMonth };
}

function isWeekdayCode(value: string): value is WeekdayCode {
  return (WEEKDAY_CODES as readonly string[]).includes(value);
}

function parseWeekdays(value: string): WeekdayCode[] | null {
  const codes: WeekdayCode[] = [];
  for (const raw of value.split(",")) {
    const code = raw.trim().toUpperCase();
    if (!isWeekdayCode(code) || codes.includes(code)) return null;
    codes.push(code);
  }
  return WEEKDAY_CODES.filter((code) => codes.includes(code));
}

function parseMonthlyDay(value: string | undefined, start: StartParts): MonthlyChoice | null {
  if (value === undefined) return { on: "day", day: start.dayOfMonth };
  if (!/^\d{1,2}$/.test(value)) return null;
  const day = Number(value);
  if (day < 1 || day > 31) return null;
  return { on: "day", day };
}

function parseMonthlyWeekday(value: string): MonthlyChoice | null {
  const match = /^(-?\d{1,2})([A-Z]{2})$/.exec(value.toUpperCase());
  if (match === null) return null;
  const ordinal = Number(match[1]);
  const weekday = match[2];
  if (weekday === undefined || !isWeekdayCode(weekday)) return null;
  if (![1, 2, 3, 4, 5, -1].includes(ordinal)) return null;
  return { on: "weekday", ordinal, weekday };
}

/** `YYYYMMDD` or the end-of-day `YYYYMMDDT235959Z` to a date input value. */
function parseUntil(value: string): string | null {
  const match = /^(\d{4})(\d{2})(\d{2})(?:T235959Z)?$/.exec(value.toUpperCase());
  if (match === null) return null;
  const [year, month, day] = match.slice(1) as [string, string, string];
  const numericMonth = Number(month);
  const numericDay = Number(day);
  if (numericMonth < 1 || numericMonth > 12 || numericDay < 1 || numericDay > 31) return null;
  return `${year}-${month}-${day}`;
}

/**
 * Read a stored rule into widget state. An empty rule is the single-shot
 * form; an unreadable rule still yields a usable default state so callers
 * can render, with `expressible` false to keep the raw textarea in charge.
 */
export function parseRecurrence(rule: string, startsAt: string): ParsedRecurrence {
  const state = defaultState(startsAt);
  const text = rule.trim();
  if (text === "") return { state: { ...state, unit: "none" }, expressible: true };
  if (/^RRULE:/i.test(text)) return { state, expressible: false };

  const parts = new Map<string, string>();
  for (const segment of text.split(";")) {
    if (segment.trim() === "") return { state, expressible: false };
    const equals = segment.indexOf("=");
    if (equals <= 0) return { state, expressible: false };
    const key = segment.slice(0, equals).trim().toUpperCase();
    const value = segment.slice(equals + 1).trim();
    if (value === "" || parts.has(key)) return { state, expressible: false };
    parts.set(key, value.toUpperCase());
  }

  const frequency = parts.get("FREQ");
  const unit = (Object.keys(FREQUENCY_BY_UNIT) as Array<Exclude<RepeatUnit, "none">>).find(
    (candidate) => FREQUENCY_BY_UNIT[candidate] === frequency
  );
  if (frequency === undefined || unit === undefined) return { state, expressible: false };

  const allowed = new Set(["FREQ", "INTERVAL", "BYDAY", "BYMONTHDAY", "COUNT", "UNTIL"]);
  for (const key of parts.keys()) if (!allowed.has(key)) return { state, expressible: false };

  const rawInterval = parts.get("INTERVAL");
  if (rawInterval !== undefined && !/^\d+$/.test(rawInterval)) return { state, expressible: false };
  const interval = rawInterval === undefined ? 1 : Number(rawInterval);
  if (interval < 1) return { state, expressible: false };

  const start = startParts(startsAt);
  let weekdays: WeekdayCode[] = [start.weekday];
  let monthly: MonthlyChoice = { on: "day", day: start.dayOfMonth };

  const byDay = parts.get("BYDAY");
  const byMonthDay = parts.get("BYMONTHDAY");
  if (unit === "week") {
    if (byMonthDay !== undefined) return { state, expressible: false };
    if (byDay !== undefined) {
      const parsed = parseWeekdays(byDay);
      if (parsed === null) return { state, expressible: false };
      weekdays = parsed;
    }
  } else if (unit === "month") {
    if (byDay !== undefined) {
      const parsed = parseMonthlyWeekday(byDay);
      if (parsed === null || byMonthDay !== undefined) return { state, expressible: false };
      monthly = parsed;
    } else {
      const parsed = parseMonthlyDay(byMonthDay, start);
      if (parsed === null) return { state, expressible: false };
      monthly = parsed;
    }
  } else if (byDay !== undefined || byMonthDay !== undefined) {
    return { state, expressible: false };
  }

  const count = parts.get("COUNT");
  const until = parts.get("UNTIL");
  if (count !== undefined && until !== undefined) return { state, expressible: false };
  let ends: EndsChoice = { mode: "never" };
  if (count !== undefined) {
    if (!/^\d+$/.test(count) || Number(count) < 1) return { state, expressible: false };
    ends = { mode: "after", count: Number(count) };
  }
  if (until !== undefined) {
    const date = parseUntil(until);
    if (date === null) return { state, expressible: false };
    ends = { mode: "on", date };
  }
  return finish(unit, interval, weekdays, monthly, ends);
}

function finish(
  unit: Exclude<RepeatUnit, "none">,
  interval: number,
  weekdays: WeekdayCode[],
  monthly: MonthlyChoice,
  ends: EndsChoice = { mode: "never" }
): ParsedRecurrence {
  return { state: { unit, interval, weekdays, monthly, ends }, expressible: true };
}

function dateForUntil(date: string): string | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (match === null) return null;
  return `${match[1]}${match[2]}${match[3]}T235959Z`;
}

function serializedEnds(ends: EndsChoice): string {
  if (ends.mode === "on") {
    const until = dateForUntil(ends.date);
    return until === null ? "" : `;UNTIL=${until}`;
  }
  if (ends.mode === "after") {
    const count = Math.max(1, Math.floor(ends.count));
    return `;COUNT=${count}`;
  }
  return "";
}

/**
 * Turn widget state back into the stored bare rule. Intervals below one
 * clamp to one and are omitted at one; the empty string is the single
 * firing at the start.
 */
export function serializeRecurrence(state: RecurrenceState): string {
  if (state.unit === "none") return "";
  const parts = [`FREQ=${FREQUENCY_BY_UNIT[state.unit]}`];
  const interval = Number.isFinite(state.interval) ? Math.max(1, Math.floor(state.interval)) : 1;
  if (interval > 1) parts.push(`INTERVAL=${interval}`);
  if (state.unit === "week") {
    const days = WEEKDAY_CODES.filter((code) => state.weekdays.includes(code));
    parts.push(`BYDAY=${(days.length === 0 ? [startOfWeekFallback(state)] : days).join(",")}`);
  }
  if (state.unit === "month") {
    if (state.monthly.on === "day") {
      parts.push(`BYMONTHDAY=${Math.min(31, Math.max(1, Math.floor(state.monthly.day)))}`);
    } else {
      parts.push(`BYDAY=${state.monthly.ordinal}${state.monthly.weekday}`);
    }
  }
  return `${parts.join(";")}${serializedEnds(state.ends)}`;
}

/** The chips refuse an empty set, so this only guards hand-built states. */
function startOfWeekFallback(state: RecurrenceState): WeekdayCode {
  return state.weekdays[0] ?? "MO";
}

/** Switch the unit while keeping the shared count and ends. */
export function withUnit(state: RecurrenceState, unit: RepeatUnit): RecurrenceState {
  return { ...state, unit };
}

/** Switch the monthly form, seeding it from whatever the other form holds. */
export function withMonthlyMode(state: RecurrenceState, mode: MonthlyChoice["on"]): RecurrenceState {
  const monthly = state.monthly;
  if (mode === "weekday") {
    if (monthly.on === "weekday") return state;
    const weekday = state.weekdays[0] ?? "MO";
    return { ...state, monthly: { on: "weekday", ordinal: ordinalForDay(monthly.day), weekday } };
  }
  if (monthly.on === "day") return state;
  // A week-relative ordinal maps back into the first day of that week;
  // "last" has no fixed date, so it lands in the fourth week.
  const day = monthly.ordinal < 1 ? 28 : monthly.ordinal * 7 - 6;
  return { ...state, monthly: { on: "day", day: Math.min(31, day) } };
}

/** Toggling weekly days never leaves the set empty. */
export function withWeekdayToggled(state: RecurrenceState, weekday: WeekdayCode): RecurrenceState {
  if (state.weekdays.includes(weekday)) {
    if (state.weekdays.length === 1) return state;
    return { ...state, weekdays: state.weekdays.filter((code) => code !== weekday) };
  }
  return { ...state, weekdays: WEEKDAY_CODES.filter((code) => code === weekday || state.weekdays.includes(code)) };
}
