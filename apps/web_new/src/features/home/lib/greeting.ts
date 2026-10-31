// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Home greeting helpers (SHELL-004, SHELL-005; written from
// parity/specs/shell.md §2 and the greeting oracle). The day part follows
// the hour in the user's timezone, the salutation falls back to English
// for any unknown or malformed locale, and the date line keeps one fixed
// English shape whatever the locale or timezone holds.

export type DayPart = "morning" | "afternoon" | "evening";

/** Morning before noon, afternoon before six, evening after. */
export function selectDayPart(hour: number): DayPart {
  if (hour < 12) return "morning";
  if (hour < 18) return "afternoon";
  return "evening";
}

// The open-source build ships English strings only, so this table holds
// one language on purpose: every other tag — shipped or malformed —
// resolves through the fallback below, and the heading never blanks.
const ENGLISH_SALUTATIONS: Record<DayPart, string> = {
  morning: "Good morning",
  afternoon: "Good afternoon",
  evening: "Good evening",
};

const SALUTATIONS: Record<string, Record<DayPart, string>> = {
  en: ENGLISH_SALUTATIONS,
};

/** The salutation for a profile language tag, English unless known. */
export function selectSalutation(language: string | null | undefined, dayPart: DayPart): string {
  const tag = (language ?? "").trim().toLowerCase();
  const base = tag.replace(/[-_].*$/, "");
  return (SALUTATIONS[base] ?? ENGLISH_SALUTATIONS)[dayPart];
}

/** The greeting heading: salutation plus name, or the salutation alone. */
export function formatGreetingHeading(salutation: string, firstName: string | null | undefined): string {
  const name = (firstName ?? "").trim();
  return name === "" ? salutation : `${salutation}, ${name}`;
}

/**
 * A timezone the formatter accepts, or undefined for the system local
 * zone. Broken zones are rejected by the API, so this is only a backstop:
 * the UI degrades to local time instead of throwing.
 */
function resolveTimeZone(timeZone: string | null | undefined): string | undefined {
  if (timeZone === null || timeZone === undefined || timeZone.trim() === "") return undefined;
  try {
    new Intl.DateTimeFormat("en-US", { timeZone });
    return timeZone;
  } catch {
    return undefined;
  }
}

function zoneOption(timeZone: string | null | undefined): { timeZone: string } | Record<string, never> {
  const zone = resolveTimeZone(timeZone);
  return zone === undefined ? {} : { timeZone: zone };
}

/** The hour of an instant in a zone (0-23), system local when unknown. */
export function hourInTimeZone(now: Date, timeZone: string | null | undefined): number {
  const text = new Intl.DateTimeFormat("en-US", {
    hour: "numeric",
    hourCycle: "h23",
    ...zoneOption(timeZone),
  }).format(now);
  return Number.parseInt(text, 10);
}

/**
 * The fixed greeting sub-line: weekday, short month, day and clock, always
 * in English shape (e.g. "Tuesday, Oct 14 3:05 PM"). The zone comes from
 * the user record with the same local fallback as the day part.
 */
export function formatHomeDateLine(now: Date, timeZone: string | null | undefined): string {
  const parts = new Intl.DateTimeFormat("en-US", {
    weekday: "long",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    hour12: true,
    ...zoneOption(timeZone),
  }).formatToParts(now);
  const pick = (type: Intl.DateTimeFormatPartTypes): string => parts.find((part) => part.type === type)?.value ?? "";
  return `${pick("weekday")}, ${pick("month")} ${pick("day")} ${pick("hour")}:${pick("minute")} ${pick("dayPeriod")}`
    .replace(/\s+/g, " ")
    .trim();
}
