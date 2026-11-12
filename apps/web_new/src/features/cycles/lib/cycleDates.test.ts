// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-timezone spans: compact ranges, the DST-aware offset chip
// with its zero-offset suppression, the viewer tooltip, and the shared
// today/coverage readings (CYC-044). Every case names its zones, so the
// suite passes under any machine timezone.

import { describe, expect, it } from "vitest";

import {
  cycleCoversToday,
  cycleSpanModel,
  formatCycleSpanText,
  formatOffsetChip,
  formatZoneDay,
  instantToProjectDate,
  instantZoneDay,
  projectOffsetChip,
  projectToday,
  zoneOffsetMinutes,
} from "./cycleDates.js";

const NY = "America/New_York";
/** Midday UTC stays the same calendar day in New York and UTC. */
const SEP_01 = "2026-09-01T12:00:00Z";
const SEP_18 = "2026-09-18T12:00:00Z";
const OCT_18 = "2026-10-18T12:00:00Z";
const DEC_28_2025 = "2025-12-28T12:00:00Z";
const JAN_04 = "2026-01-04T12:00:00Z";

describe("zoneOffsetMinutes", () => {
  it("reads the DST-aware offset at the instant", () => {
    expect(zoneOffsetMinutes(NY, new Date("2026-09-01T12:00:00Z"))).toBe(-240);
    expect(zoneOffsetMinutes(NY, new Date("2026-12-15T12:00:00Z"))).toBe(-300);
    expect(zoneOffsetMinutes("Asia/Kolkata", new Date(SEP_01))).toBe(330);
    expect(zoneOffsetMinutes("UTC", new Date(SEP_01))).toBe(0);
  });

  it("returns null for an unknown zone", () => {
    expect(zoneOffsetMinutes("Mars/Olympus", new Date(SEP_01))).toBe(null);
  });
});

describe("formatOffsetChip", () => {
  it("renders the signed UTC offset", () => {
    expect(formatOffsetChip(-240)).toBe("UTC -04:00");
    expect(formatOffsetChip(-300)).toBe("UTC -05:00");
    expect(formatOffsetChip(330)).toBe("UTC +05:30");
    expect(formatOffsetChip(60)).toBe("UTC +01:00");
  });

  it("suppresses a zero offset", () => {
    expect(formatOffsetChip(0)).toBe(null);
  });
});

describe("projectOffsetChip", () => {
  it("anchors the chip at the given instant", () => {
    expect(projectOffsetChip(NY, new Date("2026-09-01T12:00:00Z"))).toBe("UTC -04:00");
    expect(projectOffsetChip(NY, new Date("2026-12-15T12:00:00Z"))).toBe("UTC -05:00");
  });

  it("suppresses zero offsets whatever the zone is named", () => {
    expect(projectOffsetChip("UTC", new Date(SEP_01))).toBe(null);
    // London carries a zero offset in winter without being named UTC.
    expect(projectOffsetChip("Europe/London", new Date("2026-12-15T12:00:00Z"))).toBe(null);
  });

  it("suppresses unknown zones", () => {
    expect(projectOffsetChip("Mars/Olympus", new Date(SEP_01))).toBe(null);
  });
});

describe("instantZoneDay", () => {
  it("reads the calendar day across the zone boundary", () => {
    expect(instantZoneDay("2026-09-01T00:30:00Z", NY)).toEqual({ year: 2026, month: 8, day: 31 });
    expect(instantZoneDay("2026-09-01T00:30:00Z", "UTC")).toEqual({ year: 2026, month: 9, day: 1 });
  });

  it("returns null for a broken instant", () => {
    expect(instantZoneDay("not-a-date", NY)).toBe(null);
  });

  it("falls back to system time for an unknown zone", () => {
    const expected = new Intl.DateTimeFormat("en-US", { year: "numeric", month: "numeric", day: "numeric" })
      .formatToParts(new Date(SEP_01))
      .reduce<Record<string, string>>((acc, part) => ({ ...acc, [part.type]: part.value }), {});
    expect(instantZoneDay(SEP_01, "Mars/Olympus")).toEqual({
      year: Number(expected["year"]),
      month: Number(expected["month"]),
      day: Number(expected["day"]),
    });
  });
});

describe("formatZoneDay", () => {
  it("renders one side with a padded day", () => {
    expect(formatZoneDay({ year: 2026, month: 10, day: 5 })).toBe("Oct 05, 2026");
    expect(formatZoneDay({ year: 2026, month: 1, day: 15 })).toBe("Jan 15, 2026");
  });
});

describe("instantToProjectDate", () => {
  it("renders the date-only reading in the zone", () => {
    expect(instantToProjectDate("2026-09-01T00:30:00Z", NY)).toBe("2026-08-31");
    expect(instantToProjectDate(SEP_01, NY)).toBe("2026-09-01");
  });

  it("returns null for a broken instant", () => {
    expect(instantToProjectDate("not-a-date", NY)).toBe(null);
  });
});

describe("projectToday", () => {
  it("reads today across the zone boundary", () => {
    const now = new Date("2026-10-10T00:30:00Z");
    expect(projectToday(NY, now)).toBe("2026-10-09");
    expect(projectToday("UTC", now)).toBe("2026-10-10");
  });

  it("falls back to system time for an unknown zone", () => {
    const now = new Date("2026-10-10T00:30:00Z");
    const expected = new Intl.DateTimeFormat("en-US", {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    })
      .formatToParts(now)
      .reduce<Record<string, string>>((acc, part) => ({ ...acc, [part.type]: part.value }), {});
    expect(projectToday("Mars/Olympus", now)).toBe(`${expected["year"]}-${expected["month"]}-${expected["day"]}`);
  });
});

describe("cycleCoversToday", () => {
  const now = new Date("2026-10-10T12:00:00Z");

  it("covers today inside a dated window", () => {
    expect(cycleCoversToday("2026-09-01T12:00:00Z", "2026-12-31T12:00:00Z", NY, now)).toBe(true);
    expect(cycleCoversToday("2026-10-10T12:00:00Z", "2026-10-10T12:00:00Z", NY, now)).toBe(true);
  });

  it("misses outside the window", () => {
    expect(cycleCoversToday("2026-11-01T12:00:00Z", "2026-12-31T12:00:00Z", NY, now)).toBe(false);
    expect(cycleCoversToday("2026-01-01T12:00:00Z", "2026-09-01T12:00:00Z", NY, now)).toBe(false);
  });

  it("leaves a missing end open", () => {
    expect(cycleCoversToday("2026-09-01T12:00:00Z", null, NY, now)).toBe(true);
    expect(cycleCoversToday(null, "2026-12-31T12:00:00Z", NY, now)).toBe(true);
    expect(cycleCoversToday("2026-11-01T12:00:00Z", null, NY, now)).toBe(false);
    expect(cycleCoversToday(null, "2026-09-01T12:00:00Z", NY, now)).toBe(false);
  });

  it("never covers with neither date", () => {
    expect(cycleCoversToday(null, null, NY, now)).toBe(false);
  });

  it("compares in the project zone, not UTC", () => {
    // 00:30 UTC is still Oct 9 in New York: a cycle ending Oct 9 covers
    // "today" for the project while UTC already reads Oct 10.
    const boundary = new Date("2026-10-10T00:30:00Z");
    expect(cycleCoversToday(null, "2026-10-09T12:00:00Z", NY, boundary)).toBe(true);
    expect(cycleCoversToday(null, "2026-10-09T12:00:00Z", "UTC", boundary)).toBe(false);
  });
});

describe("formatCycleSpanText", () => {
  it("renders a same-month range with the month and year once", () => {
    expect(formatCycleSpanText(SEP_01, SEP_18, NY)).toBe("Sep 01 - 18, 2026");
  });

  it("renders a cross-month range with the year once", () => {
    expect(formatCycleSpanText(SEP_01, OCT_18, NY)).toBe("Sep 01 - Oct 18, 2026");
  });

  it("renders a cross-year range with both years", () => {
    expect(formatCycleSpanText(DEC_28_2025, JAN_04, NY)).toBe("Dec 28, 2025 - Jan 04, 2026");
  });

  it("renders a one-sided span as the single date", () => {
    expect(formatCycleSpanText(SEP_01, null, NY)).toBe("Sep 01, 2026");
    expect(formatCycleSpanText(null, OCT_18, NY)).toBe("Oct 18, 2026");
  });

  it("renders no text without dates", () => {
    expect(formatCycleSpanText(null, null, NY)).toBe(null);
  });

  it("shifts the days into the project zone", () => {
    expect(formatCycleSpanText("2026-09-01T00:30:00Z", "2026-09-18T00:30:00Z", NY)).toBe("Aug 31 - Sep 17, 2026");
  });
});

describe("cycleSpanModel", () => {
  it("carries the chip and the viewer tooltip when zones differ", () => {
    const model = cycleSpanModel(SEP_01, "2026-12-31T12:00:00Z", NY, "UTC");
    expect(model.text).toBe("Sep 01 - Dec 31, 2026");
    expect(model.chip).toBe("UTC -04:00");
    expect(model.chip).toMatch(/^UTC [+-]/);
    expect(model.tooltip).toContain("your timezone");
    expect(model.tooltip).toContain("2026");
  });

  it("keeps the chip but drops the tooltip in the same zone", () => {
    const model = cycleSpanModel(SEP_01, "2026-12-31T12:00:00Z", NY, NY);
    expect(model.text).toBe("Sep 01 - Dec 31, 2026");
    expect(model.chip).toBe("UTC -04:00");
    expect(model.tooltip).toBe(null);
  });

  it("shows no chip or tooltip on a zero offset", () => {
    const model = cycleSpanModel(SEP_01, "2026-12-31T12:00:00Z", "UTC", NY);
    expect(model.text).toBe("Sep 01 - Dec 31, 2026");
    expect(model.chip).toBe(null);
    expect(model.tooltip).toBe(null);
  });

  it("renders nothing dateless", () => {
    expect(cycleSpanModel(null, null, NY, "UTC")).toEqual({ text: null, chip: null, tooltip: null });
  });

  it("degrades an unknown project zone to chipless system text", () => {
    const model = cycleSpanModel(SEP_01, null, "Mars/Olympus", "UTC");
    expect(model.text).toBe(formatCycleSpanText(SEP_01, null, undefined));
    expect(model.chip).toBe(null);
    expect(model.tooltip).toBe(null);
  });

  it("drops the tooltip for an unknown viewer zone", () => {
    const model = cycleSpanModel(SEP_01, null, NY, "Mars/Olympus");
    expect(model.chip).toBe("UTC -04:00");
    expect(model.tooltip).toBe(null);
  });

  it("defaults the viewer to the browser zone", () => {
    const model = cycleSpanModel(SEP_01, null, NY);
    const system = Intl.DateTimeFormat().resolvedOptions().timeZone;
    expect(model.text).toBe("Sep 01, 2026");
    expect(model.chip).toBe("UTC -04:00");
    if (system === NY) expect(model.tooltip).toBe(null);
    else expect(model.tooltip).toContain("your timezone");
  });
});
