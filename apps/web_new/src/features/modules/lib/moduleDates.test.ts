// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar-day readings for module dates (§4.1, §3.5, §3.6): days never
// shift across zones, malformed values read as empty, ranges collapse the
// shared month/year, and the reader-zone note appears only when the zones
// differ and both are known.

import { afterEach, describe, expect, it } from "vitest";

import {
  formatModuleDay,
  formatModuleOffsetChip,
  formatModuleRange,
  MODULE_USER_ZONE_HEADING,
  moduleDateText,
  moduleDayAnchor,
  moduleInstantDay,
  moduleInstantReading,
  moduleOffsetChip,
  moduleRangeClick,
  moduleRangeDurationDays,
  moduleRangeReading,
  moduleZoneOffsetMinutes,
  parseModuleDay,
  userZoneDiffers,
} from "./moduleDates.js";

const originalResolved = Intl.DateTimeFormat.prototype.resolvedOptions;

afterEach(() => {
  Intl.DateTimeFormat.prototype.resolvedOptions = originalResolved;
});

describe("parseModuleDay", () => {
  it("parses a calendar day", () => {
    expect(parseModuleDay("2026-10-15")).toEqual({ year: 2026, month: 10, day: 15 });
    expect(parseModuleDay(" 2026-01-02 ")).toEqual({ year: 2026, month: 1, day: 2 });
  });

  it("rejects absent, malformed and impossible days", () => {
    expect(parseModuleDay(null)).toBeNull();
    expect(parseModuleDay(undefined)).toBeNull();
    expect(parseModuleDay("")).toBeNull();
    expect(parseModuleDay("2026-1-5")).toBeNull();
    expect(parseModuleDay("15/10/2026")).toBeNull();
    expect(parseModuleDay("2026-13-01")).toBeNull();
    expect(parseModuleDay("2026-02-31")).toBeNull();
    expect(parseModuleDay("2026-00-10")).toBeNull();
  });

  it("accepts a leap day only in a leap year", () => {
    expect(parseModuleDay("2028-02-29")).toEqual({ year: 2028, month: 2, day: 29 });
    expect(parseModuleDay("2027-02-29")).toBeNull();
  });
});

describe("formatModuleDay", () => {
  it("reads the day, month and year", () => {
    expect(formatModuleDay({ year: 2026, month: 10, day: 5 })).toBe("Oct 05, 2026");
    expect(formatModuleDay({ year: 2025, month: 12, day: 31 })).toBe("Dec 31, 2025");
  });
});

describe("formatModuleRange", () => {
  it("collapses a range inside one month", () => {
    expect(formatModuleRange({ start: "2026-10-15", target: "2026-10-22" })).toBe("Oct 15 - 22, 2026");
  });

  it("names both months inside one year", () => {
    expect(formatModuleRange({ start: "2026-10-30", target: "2026-11-04" })).toBe("Oct 30 - Nov 04, 2026");
  });

  it("repeats both years when the range crosses one", () => {
    expect(formatModuleRange({ start: "2026-12-30", target: "2027-01-04" })).toBe("Dec 30, 2026 - Jan 04, 2027");
  });

  it("reads one side as one day and neither side as nothing", () => {
    expect(formatModuleRange({ start: "2026-10-15", target: null })).toBe("Oct 15, 2026");
    expect(formatModuleRange({ start: null, target: "2026-10-22" })).toBe("Oct 22, 2026");
    expect(formatModuleRange({ start: null, target: null })).toBeNull();
    expect(formatModuleRange({ start: "bogus", target: null })).toBeNull();
  });
});

describe("moduleRangeDurationDays", () => {
  it("counts both ends", () => {
    expect(moduleRangeDurationDays({ start: "2026-10-01", target: "2026-10-03" })).toBe(3);
    expect(moduleRangeDurationDays({ start: "2026-10-03", target: "2026-10-01" })).toBe(-1);
    expect(moduleRangeDurationDays({ start: "2026-01-01", target: "2027-01-01" })).toBe(366);
  });

  it("reads null when a side is missing or malformed", () => {
    expect(moduleRangeDurationDays({ start: "2026-10-01", target: null })).toBeNull();
    expect(moduleRangeDurationDays({ start: "nope", target: "2026-10-01" })).toBeNull();
  });
});

describe("moduleDateText", () => {
  it("reads the day or the caller's placeholder", () => {
    expect(moduleDateText("2026-10-15", "Start date")).toBe("Oct 15, 2026");
    expect(moduleDateText(null, "Start date")).toBe("Start date");
    expect(moduleDateText("bad", "End date")).toBe("End date");
  });
});

describe("userZoneDiffers", () => {
  it("compares the project zone with the reader's", () => {
    expect(userZoneDiffers("Asia/Tokyo", "Asia/Tokyo")).toBe(false);
    expect(userZoneDiffers("Asia/Tokyo", "America/Los_Angeles")).toBe(true);
  });

  it("reads the browser zone when the caller names none", () => {
    Intl.DateTimeFormat.prototype.resolvedOptions = function resolved(this: Intl.DateTimeFormat) {
      return { ...originalResolved.call(this), timeZone: "Europe/Berlin" };
    } as typeof Intl.DateTimeFormat.prototype.resolvedOptions;
    expect(userZoneDiffers("Europe/Berlin")).toBe(false);
    expect(userZoneDiffers("Asia/Tokyo")).toBe(true);
  });

  it("never claims a difference it cannot establish", () => {
    expect(userZoneDiffers(null, "Asia/Tokyo")).toBe(false);
    expect(userZoneDiffers(undefined, "Asia/Tokyo")).toBe(false);
    expect(userZoneDiffers("   ", "Asia/Tokyo")).toBe(false);
    expect(userZoneDiffers("Asia/Tokyo", "")).toBe(false);
    Intl.DateTimeFormat.prototype.resolvedOptions = function resolved(this: Intl.DateTimeFormat) {
      const { timeZone: _omitted, ...rest } = originalResolved.call(this);
      return rest;
    } as typeof Intl.DateTimeFormat.prototype.resolvedOptions;
    expect(userZoneDiffers("Asia/Tokyo")).toBe(false);
  });

  it("reads no browser zone when the engine cannot name one", () => {
    Intl.DateTimeFormat.prototype.resolvedOptions = function resolved(): Intl.ResolvedDateTimeFormatOptions {
      throw new Error("no zone");
    } as typeof Intl.DateTimeFormat.prototype.resolvedOptions;
    expect(userZoneDiffers("Asia/Tokyo")).toBe(false);
  });
});

describe("moduleZoneOffsetMinutes", () => {
  it("reads a DST-aware offset at the anchored instant", () => {
    expect(moduleZoneOffsetMinutes("America/New_York", new Date("2026-01-15T12:00:00Z"))).toBe(-300);
    expect(moduleZoneOffsetMinutes("America/New_York", new Date("2026-07-15T12:00:00Z"))).toBe(-240);
    expect(moduleZoneOffsetMinutes("Asia/Kolkata", new Date("2026-07-15T12:00:00Z"))).toBe(330);
    expect(moduleZoneOffsetMinutes("UTC", new Date("2026-07-15T12:00:00Z"))).toBe(0);
  });

  it("returns null for an unknown zone, a blank zone or a broken instant", () => {
    expect(moduleZoneOffsetMinutes("Nowhere/City", new Date("2026-07-15T12:00:00Z"))).toBeNull();
    expect(moduleZoneOffsetMinutes(null, new Date("2026-07-15T12:00:00Z"))).toBeNull();
    expect(moduleZoneOffsetMinutes("  ", new Date("2026-07-15T12:00:00Z"))).toBeNull();
    expect(moduleZoneOffsetMinutes("UTC", new Date("not a date"))).toBeNull();
  });
});

describe("formatModuleOffsetChip", () => {
  it("reads the signed hour and minute, suppressing a zero offset", () => {
    expect(formatModuleOffsetChip(-240)).toBe("UTC -04:00");
    expect(formatModuleOffsetChip(330)).toBe("UTC +05:30");
    expect(formatModuleOffsetChip(345)).toBe("UTC +05:45");
    expect(formatModuleOffsetChip(0)).toBeNull();
    expect(formatModuleOffsetChip(Number.NaN)).toBeNull();
  });
});

describe("moduleOffsetChip", () => {
  it("combines the zone reading with the formatter", () => {
    expect(moduleOffsetChip("America/New_York", new Date("2026-01-15T12:00:00Z"))).toBe("UTC -05:00");
    expect(moduleOffsetChip("UTC", new Date("2026-01-15T12:00:00Z"))).toBeNull();
    expect(moduleOffsetChip("Nowhere/City", new Date("2026-01-15T12:00:00Z"))).toBeNull();
  });
});

describe("moduleDayAnchor", () => {
  it("anchors a calendar day at midday in the project zone", () => {
    expect(moduleDayAnchor({ year: 2026, month: 1, day: 15 }, "America/New_York")?.toISOString()).toBe(
      "2026-01-15T17:00:00.000Z"
    );
    expect(moduleDayAnchor({ year: 2026, month: 7, day: 15 }, "America/New_York")?.toISOString()).toBe(
      "2026-07-15T16:00:00.000Z"
    );
  });

  it("settles the day's own offset on a DST transition day", () => {
    const springForward = moduleDayAnchor({ year: 2026, month: 3, day: 8 }, "America/New_York");
    expect(springForward?.toISOString()).toBe("2026-03-08T16:00:00.000Z");
    expect(moduleInstantDay(springForward!, "America/New_York")).toEqual({ year: 2026, month: 3, day: 8 });
    expect(moduleOffsetChip("America/New_York", springForward!)).toBe("UTC -04:00");
  });

  it("returns null without a usable zone", () => {
    expect(moduleDayAnchor({ year: 2026, month: 1, day: 15 }, null)).toBeNull();
    expect(moduleDayAnchor({ year: 2026, month: 1, day: 15 }, "Nowhere/City")).toBeNull();
  });

  it("keeps the first reading when the engine fails the second", () => {
    const original = Intl.DateTimeFormat.prototype.formatToParts;
    let calls = 0;
    Intl.DateTimeFormat.prototype.formatToParts = function formatToParts(
      this: Intl.DateTimeFormat,
      at?: number | Date
    ) {
      calls += 1;
      if (calls > 1) throw new Error("engine refused");
      return original.call(this, at);
    } as typeof Intl.DateTimeFormat.prototype.formatToParts;
    try {
      expect(moduleDayAnchor({ year: 2026, month: 10, day: 15 }, "America/New_York")?.toISOString()).toBe(
        "2026-10-15T16:00:00.000Z"
      );
    } finally {
      Intl.DateTimeFormat.prototype.formatToParts = original;
    }
  });
});

describe("moduleInstantDay", () => {
  it("reads an instant's day in a zone, degrading an unknown zone to the engine zone", () => {
    expect(moduleInstantDay("2026-01-15T02:00:00Z", "America/New_York")).toEqual({ year: 2026, month: 1, day: 14 });
    expect(moduleInstantDay(new Date("2026-01-15T02:00:00Z"), "UTC")).toEqual({ year: 2026, month: 1, day: 15 });
    expect(moduleInstantDay("nope", "UTC")).toBeNull();
    const engineZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    const unknown = moduleInstantDay("2026-01-15T02:00:00Z", "Nowhere/City");
    expect(unknown).not.toBeNull();
    expect(unknown).toEqual(moduleInstantDay("2026-01-15T02:00:00Z", engineZone));
    expect(moduleInstantDay("2026-01-15T02:00:00Z", null)).toEqual(unknown);
  });
});

describe("moduleInstantReading", () => {
  it("states the project day and a differing reader's day", () => {
    const reading = moduleInstantReading("2026-01-15T02:00:00Z", "America/New_York", "Asia/Tokyo");
    expect(reading.text).toBe("Jan 14, 2026");
    expect(reading.chip).toBe("UTC -05:00");
    expect(reading.userText).toBe("Jan 15, 2026");
    expect(reading.note).toContain(MODULE_USER_ZONE_HEADING);
    expect(reading.note).toContain("America/New_York");
  });

  it("suppresses the reader equivalent and note when the zones agree", () => {
    const reading = moduleInstantReading("2026-01-15T02:00:00Z", "America/New_York", "America/New_York");
    expect(reading.text).toBe("Jan 14, 2026");
    expect(reading.userText).toBeNull();
    expect(reading.note).toBeNull();
  });

  it("keeps the day and chip but drops the equivalent for a zero offset", () => {
    const reading = moduleInstantReading("2026-01-15T02:00:00Z", "UTC", "UTC");
    expect(reading).toEqual({ text: "Jan 15, 2026", chip: null, userText: null, note: null });
  });

  it("keeps the day but drops the note for a zero or unknown offset", () => {
    expect(moduleInstantReading("2026-01-15T02:00:00Z", "UTC", "Asia/Tokyo")).toEqual({
      text: "Jan 15, 2026",
      chip: null,
      userText: null,
      note: null,
    });
    const degraded = moduleInstantReading("2026-01-15T02:00:00Z", "Nowhere/City", "Asia/Tokyo");
    expect(degraded.text).not.toBeNull();
    expect(degraded).toMatchObject({ chip: null, userText: null, note: null });
  });

  it("falls back to the viewer's browser zone when none is passed", () => {
    Intl.DateTimeFormat.prototype.resolvedOptions = function resolved(this: Intl.DateTimeFormat) {
      return { ...originalResolved.call(this), timeZone: "Asia/Tokyo" };
    } as typeof Intl.DateTimeFormat.prototype.resolvedOptions;
    const reading = moduleInstantReading("2026-01-15T02:00:00Z", "America/New_York");
    expect(reading.userText).toBe("Jan 15, 2026");
    expect(reading.note).toContain("In your timezone (Asia/Tokyo)");
  });

  it("returns empty readings for absent or broken instants", () => {
    expect(moduleInstantReading(null, "UTC", "Asia/Tokyo")).toEqual({
      text: null,
      chip: null,
      userText: null,
      note: null,
    });
    expect(moduleInstantReading("nope", "UTC", "Asia/Tokyo")).toEqual({
      text: null,
      chip: null,
      userText: null,
      note: null,
    });
  });
});

describe("moduleRangeReading", () => {
  it("reads the calendar range with the project chip and viewer equivalent", () => {
    const reading = moduleRangeReading({ start: "2026-10-15", target: "2026-10-22" }, "America/New_York", "Asia/Tokyo");
    expect(reading.text).toBe("Oct 15 - 22, 2026");
    expect(reading.chip).toBe("UTC -04:00");
    expect(reading.userText).toBe("Oct 15 - 22, 2026");
    expect(reading.note).toBe(
      "Dates shown in America/New_York (UTC -04:00). In your timezone (Asia/Tokyo): Oct 15 - 22, 2026."
    );
  });

  it("anchors a single-sided range and reads a westward viewer's day", () => {
    const reading = moduleRangeReading({ start: "2026-01-15", target: null }, "Europe/Berlin", "America/Los_Angeles");
    expect(reading.text).toBe("Jan 15, 2026");
    expect(reading.chip).toBe("UTC +01:00");
    expect(reading.userText).toBe("Jan 15, 2026");
  });

  it("suppresses the equivalent when the zones agree", () => {
    const reading = moduleRangeReading(
      { start: "2026-10-15", target: "2026-10-22" },
      "America/New_York",
      "America/New_York"
    );
    expect(reading.chip).toBe("UTC -04:00");
    expect(reading.userText).toBeNull();
    expect(reading.note).toBeNull();
  });

  it("suppresses the note for a zero offset, an unknown zone or an empty range", () => {
    expect(moduleRangeReading({ start: "2026-10-15", target: null }, "UTC", "Asia/Tokyo").note).toBeNull();
    expect(moduleRangeReading({ start: "2026-10-15", target: null }, "Nowhere/City", "Asia/Tokyo")).toEqual({
      text: "Oct 15, 2026",
      chip: null,
      userText: null,
      note: null,
    });
    expect(moduleRangeReading({ start: null, target: null }, "UTC", "Asia/Tokyo")).toEqual({
      text: null,
      chip: null,
      userText: null,
      note: null,
    });
    expect(moduleRangeReading({ start: "bad", target: null }, "UTC", "Asia/Tokyo")).toEqual({
      text: null,
      chip: null,
      userText: null,
      note: null,
    });
  });

  it("falls back to the viewer's browser zone when none is passed", () => {
    Intl.DateTimeFormat.prototype.resolvedOptions = function resolved(this: Intl.DateTimeFormat) {
      return { ...originalResolved.call(this), timeZone: "Asia/Tokyo" };
    } as typeof Intl.DateTimeFormat.prototype.resolvedOptions;
    const reading = moduleRangeReading({ start: "2026-10-15", target: null }, "America/New_York");
    expect(reading.userText).toBe("Oct 15, 2026");
    expect(reading.note).toContain(MODULE_USER_ZONE_HEADING);
  });
});

describe("moduleRangeClick", () => {
  it("anchors the start on an empty selection", () => {
    expect(moduleRangeClick({ start: null, end: null }, "2026-11-04")).toEqual({
      start: "2026-11-04",
      end: null,
    });
    expect(moduleRangeClick({ start: "", end: "" }, "2026-11-04")).toEqual({
      start: "2026-11-04",
      end: null,
    });
  });

  it("extends a lone start to a later day", () => {
    expect(moduleRangeClick({ start: "2026-11-04", end: null }, "2026-11-11")).toEqual({
      start: "2026-11-04",
      end: "2026-11-11",
    });
  });

  it("re-anchors a lone start to an earlier day", () => {
    expect(moduleRangeClick({ start: "2026-11-04", end: null }, "2026-10-30")).toEqual({
      start: "2026-10-30",
      end: null,
    });
  });

  it("clears both sides when the lone day is clicked again", () => {
    expect(moduleRangeClick({ start: "2026-11-04", end: null }, "2026-11-04")).toEqual({
      start: null,
      end: null,
    });
    expect(moduleRangeClick({ start: "2026-11-04", end: "" }, "2026-11-04")).toEqual({
      start: null,
      end: null,
    });
  });

  it("re-anchors a complete range to the clicked day", () => {
    expect(moduleRangeClick({ start: "2026-11-04", end: "2026-11-11" }, "2026-11-04")).toEqual({
      start: "2026-11-04",
      end: null,
    });
    expect(moduleRangeClick({ start: "2026-11-04", end: "2026-11-11" }, "2026-12-01")).toEqual({
      start: "2026-12-01",
      end: null,
    });
  });
});
