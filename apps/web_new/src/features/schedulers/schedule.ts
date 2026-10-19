// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Schedule editing rules for parity/specs/agents.md §2 (AGT-006–009).
// The install dialogs and the later edit dialog share one schedule value,
// one local/UTC round trip and one plain-language renderer. The RRULE
// text stays the source of truth: the sentence is a readability aid, and
// an unrecognised rule renders exactly as stored rather than inventing a
// meaning.

/** The zone the server falls back to when an install names none. */
export const DEFAULT_TZID = "UTC";

/** Zone the browser reports, or UTC where that is unavailable. */
export function browserTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || DEFAULT_TZID;
  } catch {
    return DEFAULT_TZID;
  }
}

const pad2 = (value: number): string => String(value).padStart(2, "0");

function toLocalInput(date: Date): string {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}T${pad2(date.getHours())}:${pad2(
    date.getMinutes()
  )}`;
}

/** Tomorrow at 09:00 in the browser's zone, as a datetime-local value. */
export function defaultStartLocal(now: Date = new Date()): string {
  const start = new Date(now.getTime());
  start.setDate(start.getDate() + 1);
  start.setHours(9, 0, 0, 0);
  return toLocalInput(start);
}

/** datetime-local wall time (browser zone) to the UTC instant the API stores. */
export function localStartToIso(local: string): string {
  if (local.trim() === "") return "";
  const parsed = new Date(local);
  if (Number.isNaN(parsed.getTime())) return "";
  return parsed.toISOString();
}

/** UTC instant from the API back to a datetime-local value. */
export function isoToLocalStart(iso: string): string {
  const parsed = new Date(iso);
  if (Number.isNaN(parsed.getTime())) return "";
  return toLocalInput(parsed);
}

export interface ScheduleValue {
  /** datetime-local wall time in the browser's zone. */
  startsAt: string;
  /** IANA zone stored with the install. */
  timeZone: string;
  /** RFC 5545 rule; empty fires once at the start. */
  rrule: string;
  /** Project framing appended to the base prompt at run time. */
  projectContext: string;
  /** Whether the install fires at all. */
  enabled: boolean;
}

export type ScheduleErrors = Partial<Record<keyof ScheduleValue, string>>;

/** A fresh schedule: tomorrow 09:00 local, daily, enabled. */
export function emptySchedule(now: Date = new Date()): ScheduleValue {
  return {
    startsAt: defaultStartLocal(now),
    timeZone: browserTimeZone(),
    rrule: "FREQ=DAILY",
    projectContext: "",
    enabled: true,
  };
}

/** The only required schedule field; the server validates the rule itself. */
export function validateSchedule(value: ScheduleValue): ScheduleErrors {
  const errors: ScheduleErrors = {};
  if (value.startsAt.trim() === "") {
    errors.startsAt = "Start time is required.";
  } else if (localStartToIso(value.startsAt) === "") {
    errors.startsAt = "Enter a valid start time.";
  }
  return errors;
}

export function hasScheduleErrors(errors: ScheduleErrors): boolean {
  return Object.keys(errors).length > 0;
}

/** The binding fields a schedule contributes to a create request. */
export interface SchedulePayload {
  dtstart: string;
  tzid: string;
  rrule: string;
  extra_context: string;
  enabled: boolean;
}

export function schedulePayload(value: ScheduleValue): SchedulePayload {
  return {
    dtstart: localStartToIso(value.startsAt),
    tzid: value.timeZone || DEFAULT_TZID,
    rrule: value.rrule.trim(),
    extra_context: value.projectContext.trim(),
    enabled: value.enabled,
  };
}

/** Zone options: everything the runtime knows, UTC first. */
export function timeZoneOptions(): string[] {
  const intl = Intl as typeof Intl & { supportedValuesOf?: (key: "timeZone") => string[] };
  let zones: string[] = [];
  if (typeof intl.supportedValuesOf === "function") {
    try {
      zones = intl.supportedValuesOf("timeZone");
    } catch {
      zones = [];
    }
  }
  const seen = new Set<string>([DEFAULT_TZID, ...zones]);
  return [...seen].sort((left, right) => {
    if (left === DEFAULT_TZID) return -1;
    if (right === DEFAULT_TZID) return 1;
    return left.localeCompare(right);
  });
}

const WEEKDAYS: Record<string, string> = {
  MO: "Monday",
  TU: "Tuesday",
  WE: "Wednesday",
  TH: "Thursday",
  FR: "Friday",
  SA: "Saturday",
  SU: "Sunday",
};

const FREQUENCY_NOUNS: Record<string, [string, string]> = {
  MINUTELY: ["minute", "minutes"],
  HOURLY: ["hour", "hours"],
  DAILY: ["day", "days"],
  WEEKLY: ["week", "weeks"],
  MONTHLY: ["month", "months"],
  YEARLY: ["year", "years"],
};

function weekdaySentence(days: string[]): string | null {
  const names = days.map((day) => WEEKDAYS[day.toUpperCase()]);
  if (names.some((name) => name === undefined)) return null;
  if (days.length === 5 && ["MO", "TU", "WE", "TH", "FR"].every((day) => days.includes(day))) {
    return "Every weekday";
  }
  if (names.length === 1) return `Every ${names[0]}`;
  if (names.length === 2) return `Every ${names[0]} and ${names[1]}`;
  return `Every ${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

function clockSentence(parts: Map<string, string>): string {
  const hour = parts.get("BYHOUR");
  if (hour === undefined) return "";
  const minute = parts.get("BYMINUTE") ?? "0";
  return ` at ${pad2(Number(hour)).padStart(2, "0")}:${pad2(Number(minute)).padStart(2, "0")}`;
}

function untilSentence(parts: Map<string, string>): string {
  const until = parts.get("UNTIL");
  if (until === undefined) return "";
  const stamp = until.replace(/T.*$/, "").replace(/Z$/, "");
  const match = /^(\d{4})(\d{2})(\d{2})$/.exec(stamp);
  if (match === null) return "";
  return ` until ${match[1]}-${match[2]}-${match[3]}`;
}

function countSentence(parts: Map<string, string>): string {
  const count = parts.get("COUNT");
  if (count === undefined) return "";
  const numeric = Number(count);
  return Number.isFinite(numeric) && numeric > 0 ? ` for ${numeric} ${numeric === 1 ? "time" : "times"}` : "";
}

/**
 * Plain-language schedule text. Empty rules describe the single firing at
 * the start; rules this renderer cannot read come back verbatim so the
 * caller always shows what is stored.
 */
export function describeRrule(rrule: string): string {
  const rule = rrule.trim().replace(/^RRULE:/i, "");
  if (rule === "") return "Fires once at the start time";
  const parts = new Map<string, string>();
  for (const segment of rule.split(";")) {
    if (segment.trim() === "") continue;
    const [rawKey, rawValue] = segment.split("=");
    if (rawKey === undefined || rawValue === undefined) return rule;
    parts.set(rawKey.trim().toUpperCase(), rawValue.trim().toUpperCase());
  }
  const freq = parts.get("FREQ");
  if (freq === undefined) return rule;
  if (freq === "SECONDLY") return rule;
  const nouns = FREQUENCY_NOUNS[freq];
  if (nouns === undefined) return rule;

  const interval = Number(parts.get("INTERVAL") ?? "1");
  if (!Number.isFinite(interval) || interval < 1) return rule;
  const cadence = interval === 1 ? `Every ${nouns[0]}` : `Every ${interval} ${interval === 1 ? nouns[0] : nouns[1]}`;

  const days = parts.get("BYDAY");
  const dayText = days === undefined ? null : weekdaySentence(days.split(","));
  if (days !== undefined && dayText === null) return rule;

  const monthDays = parts.get("BYMONTHDAY");
  if (monthDays !== undefined && !/^-?\d+(,-?\d+)*$/.test(monthDays)) return rule;

  const head = dayText ?? cadence;
  const onMonthDay = monthDays === undefined ? "" : ` on day ${monthDays.replace(/,/g, ", ")}`;
  return `${head}${onMonthDay}${clockSentence(parts)}${countSentence(parts)}${untilSentence(parts)}`;
}
