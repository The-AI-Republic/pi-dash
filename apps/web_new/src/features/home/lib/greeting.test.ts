// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  formatGreetingHeading,
  formatHomeDateLine,
  hourInTimeZone,
  selectDayPart,
  selectSalutation,
  type DayPart,
} from "./greeting.js";

// The oracle's fixed date shape (greeting.spec.ts): weekday, short month,
// day and clock. Behavior the new app must keep, quoted from the parity
// contract rather than the old app.
const DATE_SHAPE = /[A-Za-z]+, [A-Za-z]{3} \d{1,2} \d{1,2}:\d{2}/;

const DAY_PARTS: readonly DayPart[] = ["morning", "afternoon", "evening"];

describe("selectDayPart", () => {
  it.each([
    [0, "morning"],
    [11, "morning"],
    [12, "afternoon"],
    [17, "afternoon"],
    [18, "evening"],
    [23, "evening"],
  ])("maps hour %i to %s", (hour, expected) => {
    expect(selectDayPart(hour)).toBe(expected);
  });
});

describe("selectSalutation", () => {
  it("greets in English for English tags", () => {
    expect(selectSalutation("en", "morning")).toBe("Good morning");
    expect(selectSalutation("en-US", "afternoon")).toBe("Good afternoon");
    expect(selectSalutation("en_GB", "evening")).toBe("Good evening");
    expect(selectSalutation("EN", "morning")).toBe("Good morning");
  });

  it.each(DAY_PARTS)("falls back to English for an unshipped locale (%s)", (dayPart) => {
    expect(selectSalutation("de", dayPart)).toBe(selectSalutation("en", dayPart));
  });

  it.each(DAY_PARTS)("never blanks on a malformed locale (%s)", (dayPart) => {
    const salutation = selectSalutation("xx-broken!!!", dayPart);
    expect(salutation.trim().length).toBeGreaterThan(0);
    expect(salutation).toBe(selectSalutation("en", dayPart));
  });

  it.each(DAY_PARTS)("falls back to English when the language is missing (%s)", (dayPart) => {
    expect(selectSalutation(null, dayPart)).toBe(selectSalutation("en", dayPart));
    expect(selectSalutation(undefined, dayPart)).toBe(selectSalutation("en", dayPart));
    expect(selectSalutation("   ", dayPart)).toBe(selectSalutation("en", dayPart));
  });
});

describe("formatGreetingHeading", () => {
  it("names the user beside the salutation", () => {
    expect(formatGreetingHeading("Good morning", "Ada")).toBe("Good morning, Ada");
  });

  it("greets plainly without a name", () => {
    expect(formatGreetingHeading("Good evening", "")).toBe("Good evening");
    expect(formatGreetingHeading("Good evening", "   ")).toBe("Good evening");
    expect(formatGreetingHeading("Good evening", null)).toBe("Good evening");
    expect(formatGreetingHeading("Good evening", undefined)).toBe("Good evening");
  });
});

describe("hourInTimeZone", () => {
  // 2026-10-14T15:04:00Z is a Wednesday: 15:04 UTC, 11:04 in New York.
  const instant = new Date("2026-10-14T15:04:00Z");

  it("reads the hour inside the given zone", () => {
    expect(hourInTimeZone(instant, "UTC")).toBe(15);
    expect(hourInTimeZone(instant, "America/New_York")).toBe(11);
  });

  it("reads midnight as hour zero", () => {
    expect(hourInTimeZone(new Date("2026-10-14T00:00:00Z"), "UTC")).toBe(0);
  });

  it("degrades to the system zone for broken zones", () => {
    expect(hourInTimeZone(instant, "Mars/Olympus_Mons")).toBe(hourInTimeZone(instant, undefined));
    expect(hourInTimeZone(instant, "")).toBe(hourInTimeZone(instant, undefined));
    expect(hourInTimeZone(instant, null)).toBe(hourInTimeZone(instant, undefined));
  });
});

describe("formatHomeDateLine", () => {
  it("keeps the fixed English shape in the user's zone", () => {
    expect(formatHomeDateLine(new Date("2026-10-14T15:04:00Z"), "UTC")).toBe("Wednesday, Oct 14 3:04 PM");
    expect(formatHomeDateLine(new Date("2026-10-14T15:04:00Z"), "America/New_York")).toBe("Wednesday, Oct 14 11:04 AM");
  });

  it("matches the oracle shape whatever the zone holds", () => {
    const instant = new Date("2026-10-14T15:04:00Z");
    expect(formatHomeDateLine(instant, "UTC")).toMatch(DATE_SHAPE);
    expect(formatHomeDateLine(instant, "Mars/Olympus_Mons")).toMatch(DATE_SHAPE);
    expect(formatHomeDateLine(instant, null)).toMatch(DATE_SHAPE);
    expect(formatHomeDateLine(instant, undefined)).toMatch(DATE_SHAPE);
  });

  it("degrades a broken zone to the system zone", () => {
    const instant = new Date("2026-10-14T15:04:00Z");
    expect(formatHomeDateLine(instant, "Mars/Olympus_Mons")).toBe(formatHomeDateLine(instant, undefined));
  });
});
